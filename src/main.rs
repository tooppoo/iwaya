//! `iwaya exec --context <context> <command> -- [args...]` is the only
//! entrypoint (docs/v0-scope.md). The flow is validate, resolve, construct,
//! execute, in that order, and a failure at any stage executes nothing
//! (docs/design/docker-execution.md, "Execution Order").
//!
//! The output format and the exit-code categories are open decisions in
//! docs/v0-scope.md. Until they are settled, iwaya's own failure diagnostics
//! are single lines on stderr, usage errors and help are rendered by clap
//! (which exits with 2, the provisional usage category), and the exit codes
//! below are provisional; they distinguish the failure stages the model
//! requires to stay distinguishable. One exception to the usage category:
//! a bare `iwaya` is a request for orientation, not a mistake, so it renders
//! the same help `--help` renders, on stdout with exit 0.

mod bws;
mod config;
mod phantom;
mod proxy;
mod run;
mod secret;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};

use config::{CommandId, ContextId, Provider, SecretName};

const EXIT_CONFIG: u8 = 3;
const EXIT_UNKNOWN_SELECTION: u8 = 4;
const EXIT_RESOLUTION: u8 = 5;
const EXIT_EXECUTION: u8 = 6;

#[derive(Parser)]
#[command(name = "iwaya", version)]
enum Cli {
    /// Run a configured command inside a configured Docker execution context
    Exec {
        /// The Docker execution context to run in; never inferred
        #[arg(long)]
        context: String,
        /// The command policy to execute
        command: String,
        /// Appended unchanged to the target command
        #[arg(last = true)]
        args: Vec<String>,
    },
}

// Debug is safe here: no variant carries a `Secret`, and `Secret` itself has
// no `Debug` for a wrapper to derive one through.
#[derive(Debug)]
enum Failure {
    Config(config::ConfigError),
    UnknownContext(ContextId),
    UnknownCommand(CommandId),
    // Temporary while proxy-secret delivery is built incrementally (issue
    // #31): the configuration contract is in place before the execution
    // path. Shares the configuration exit code because the corrective
    // action is configuration-side.
    ProxySecretUnsupported(CommandId),
    Resolution(bws::ResolveError),
    Execution(run::ExecError),
}

impl Failure {
    fn exit_code(&self) -> u8 {
        match self {
            Failure::Config(_) | Failure::ProxySecretUnsupported(_) => EXIT_CONFIG,
            Failure::UnknownContext(_) | Failure::UnknownCommand(_) => EXIT_UNKNOWN_SELECTION,
            Failure::Resolution(_) => EXIT_RESOLUTION,
            Failure::Execution(_) => EXIT_EXECUTION,
        }
    }

    fn message(&self) -> String {
        match self {
            Failure::Config(e) => e.to_string(),
            Failure::UnknownContext(id) => {
                format!("unknown context '{id}': no configured context has this identifier")
            }
            Failure::UnknownCommand(id) => {
                format!("unknown command '{id}': no configured command policy has this identifier")
            }
            Failure::ProxySecretUnsupported(id) => {
                format!(
                    "command '{id}' declares 'proxy-secret', which this version of iwaya does not execute yet; nothing was executed"
                )
            }
            Failure::Resolution(e) => format!("secret resolution failed, nothing was executed: {e}"),
            Failure::Execution(e) => e.to_string(),
        }
    }
}

fn main() -> ExitCode {
    let parsed = Cli::try_parse().unwrap_or_else(|error| {
        // A bare `iwaya` behaves exactly like `iwaya --help`: help on
        // stdout, exit 0. Every other malformed invocation stays a clap
        // usage error on stderr with exit 2.
        if matches!(
            error.kind(),
            ErrorKind::MissingSubcommand | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        ) {
            let _ = Cli::command().print_help();
            std::process::exit(0);
        }
        error.exit()
    });
    let Cli::Exec { context, command, args } = parsed;
    let invocation = Invocation {
        context: ContextId::new(&context),
        command: CommandId::new(&command),
        args,
    };

    // `exec_and_never_return` replaces this process on success, so reaching
    // a return value at all means a failure to report.
    let failure = exec_and_never_return(invocation);
    eprintln!("iwaya: error: {}", failure.message());
    ExitCode::from(failure.exit_code())
}

struct Invocation {
    context: ContextId,
    command: CommandId,
    args: Vec<String>,
}

/// `IWAYA_CONFIG` overrides the location; the default follows the XDG base
/// directory convention.
fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("IWAYA_CONFIG") {
        return PathBuf::from(path);
    }
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_default();
    config_home.join("iwaya").join("config.kdl")
}

fn exec_and_never_return(invocation: Invocation) -> Failure {
    let configuration = match config::load(&config_path()) {
        Ok(configuration) => configuration,
        Err(e) => return Failure::Config(e),
    };

    let Some(context) = configuration.context(&invocation.context) else {
        return Failure::UnknownContext(invocation.context);
    };
    let Some(policy) = configuration.policy(&invocation.command) else {
        return Failure::UnknownCommand(invocation.command);
    };

    // Running the command anyway would break the guarantee that the declared
    // injection mapping is complete: the process would start without the
    // credential its policy promises.
    if !policy.proxy_secrets.is_empty() {
        return Failure::ProxySecretUnsupported(invocation.command);
    }

    // Every declared secret resolves before anything executes, and only
    // declared secrets are resolved. One failure aborts the invocation: a
    // partially populated environment is never handed to the container.
    let mut names_by_provider: Vec<(&config::ProviderId, Vec<&SecretName>)> = Vec::new();
    for secret in &policy.secrets {
        match names_by_provider.iter_mut().find(|(id, _)| *id == &secret.provider) {
            Some((_, names)) => names.push(&secret.secret_name),
            None => names_by_provider.push((&secret.provider, vec![&secret.secret_name])),
        }
    }

    let mut resolved: HashMap<&config::ProviderId, HashMap<SecretName, secret::Secret>> =
        HashMap::new();
    for (provider_id, names) in names_by_provider {
        // Validation guarantees the reference resolves; see config::validate.
        let Some(Provider::Bws(provider)) = configuration.provider(provider_id) else {
            unreachable!("validated configuration references provider '{provider_id}'");
        };
        match bws::resolve(provider, &names) {
            Ok(values) => {
                resolved.insert(provider_id, values);
            }
            Err(e) => return Failure::Resolution(e),
        }
    }

    let environment = policy
        .secrets
        .iter()
        .map(|spec| {
            let value = resolved
                .get(&spec.provider)
                .and_then(|values| values.get(&spec.secret_name))
                .cloned()
                .expect("every declared secret was resolved");
            (spec.env_name.clone(), value)
        })
        .collect();

    Failure::Execution(run::exec_runtime(context, policy, environment, &invocation.args))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once(&"iwaya").chain(args))
    }

    fn parsed_exec(args: &[&str]) -> (String, String, Vec<String>) {
        let Cli::Exec { context, command, args } = parse(args).unwrap();
        (context, command, args)
    }

    #[test]
    fn parses_the_documented_invocations() {
        let (context, command, args) = parsed_exec(&["exec", "--context", "iwaya", "claude"]);
        assert_eq!(context, "iwaya");
        assert_eq!(command, "claude");
        assert_eq!(args, Vec::<String>::new());

        let (context, command, args) =
            parsed_exec(&["exec", "--context", "git-kura", "claude", "--", "--resume"]);
        assert_eq!(context, "git-kura");
        assert_eq!(command, "claude");
        assert_eq!(args, ["--resume"]);
    }

    #[test]
    fn everything_after_the_separator_is_appended_unchanged() {
        let (_, _, args) = parsed_exec(&[
            "exec", "--context", "c", "cmd", "--", "--context", "other", "--", "-x",
        ]);
        assert_eq!(args, ["--context", "other", "--", "-x"]);
    }

    #[test]
    fn rejects_malformed_invocations() {
        // At the clap layer a missing subcommand is still an error; main
        // renders that one case as `--help` output (e2e/usage.repor).
        assert!(parse(&[]).is_err(), "a missing subcommand is a parse error");
        assert!(parse(&["run"]).is_err(), "unknown subcommand");
        assert!(parse(&["exec", "cmd"]).is_err(), "'--context' is required");
        assert!(parse(&["exec", "--context"]).is_err(), "'--context' requires a value");
        assert!(
            parse(&["exec", "--context", "a", "--context", "b", "cmd"]).is_err(),
            "'--context' more than once"
        );
        assert!(parse(&["exec", "--context", "c"]).is_err(), "a command operand is required");
        assert!(parse(&["exec", "--context", "c", "-v", "cmd"]).is_err(), "unknown option");
        assert!(
            parse(&["exec", "--context", "c", "cmd", "extra"]).is_err(),
            "target-command arguments must follow '--'"
        );
    }
}
