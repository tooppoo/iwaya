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
mod proxy_image;
mod run;
mod secret;
mod sidecar;
mod transfer;

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
    /// Run the credential-aware proxy, reading its secret transfer on stdin.
    /// Hidden: this is the process the sidecar image runs, driven by the
    /// supervisor, not a command a user invokes directly.
    #[command(hide = true)]
    Proxy,
}

// Debug is safe here: no variant carries a `Secret`, and `Secret` itself has
// no `Debug` for a wrapper to derive one through.
#[derive(Debug)]
enum Failure {
    Config(config::ConfigError),
    UnknownContext(ContextId),
    UnknownCommand(CommandId),
    Resolution(bws::ResolveError),
    Provision(phantom::GenerateError),
    ProxyImage(proxy_image::ProxyImageError),
    Sidecar(sidecar::SidecarError),
    Execution(run::ExecError),
}

impl Failure {
    fn exit_code(&self) -> u8 {
        match self {
            Failure::Config(_) => EXIT_CONFIG,
            Failure::UnknownContext(_) | Failure::UnknownCommand(_) => EXIT_UNKNOWN_SELECTION,
            Failure::Resolution(_) => EXIT_RESOLUTION,
            // Provisioning, image preparation, and sidecar startup are
            // stages of constructing the execution, so they share its
            // provisional category.
            Failure::Provision(_)
            | Failure::ProxyImage(_)
            | Failure::Sidecar(_)
            | Failure::Execution(_) => EXIT_EXECUTION,
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
            Failure::Resolution(e) => format!("secret resolution failed, nothing was executed: {e}"),
            Failure::Provision(e) => {
                format!("proxy-secret provisioning failed, nothing was executed: {e}")
            }
            Failure::ProxyImage(e) => {
                format!("proxy image preparation failed, nothing was executed: {e}")
            }
            Failure::Sidecar(e) => {
                format!("proxy sidecar startup failed, nothing was executed: {e}")
            }
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
    match parsed {
        Cli::Exec { context, command, args } => run_exec(context, command, args),
        Cli::Proxy => run_proxy(),
    }
}

fn run_exec(context: String, command: String, args: Vec<String>) -> ExitCode {
    let invocation = Invocation {
        context: ContextId::new(&context),
        command: CommandId::new(&command),
        args,
    };

    // A direct-`secret` execution replaces this process, so `Ok` can only
    // come from a supervised proxy-backed execution, carrying the exit code
    // the target produced.
    match execute(invocation) {
        Ok(code) => ExitCode::from(code),
        Err(failure) => {
            eprintln!("iwaya: error: {}", failure.message());
            ExitCode::from(failure.exit_code())
        }
    }
}

/// Serves the credential-aware proxy until the process is terminated. The
/// secret transfer arrives on stdin and the readiness line on stdout; the
/// supervisor manages this process's lifetime, so `serve` returning at all
/// means the listener stopped and there is nothing left to do.
fn run_proxy() -> ExitCode {
    match proxy::run_proxy_mode(std::io::stdin().lock(), std::io::stdout().lock()) {
        Ok(proxy) => {
            proxy.serve();
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("iwaya: error: {e}");
            ExitCode::from(EXIT_EXECUTION)
        }
    }
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

fn execute(invocation: Invocation) -> Result<u8, Failure> {
    let configuration = match config::load(&config_path()) {
        Ok(configuration) => configuration,
        Err(e) => return Err(Failure::Config(e)),
    };

    let Some(context) = configuration.context(&invocation.context) else {
        return Err(Failure::UnknownContext(invocation.context));
    };
    let Some(policy) = configuration.policy(&invocation.command) else {
        return Err(Failure::UnknownCommand(invocation.command));
    };

    // Every declared secret — direct and proxy-backed — resolves before
    // anything executes, and only declared secrets are resolved. One failure
    // aborts the invocation: a partially populated environment is never
    // handed to the container.
    let mut names_by_provider: Vec<(&config::ProviderId, Vec<&SecretName>)> = Vec::new();
    let declared = policy
        .secrets
        .iter()
        .map(|secret| (&secret.provider, &secret.secret_name))
        .chain(
            policy
                .proxy_secrets
                .iter()
                .map(|secret| (&secret.provider, &secret.secret_name)),
        );
    for (provider, name) in declared {
        match names_by_provider.iter_mut().find(|(id, _)| *id == provider) {
            Some((_, names)) => names.push(name),
            None => names_by_provider.push((provider, vec![name])),
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
            Err(e) => return Err(Failure::Resolution(e)),
        }
    }

    let resolved_value = |provider: &config::ProviderId, name: &SecretName| {
        resolved
            .get(provider)
            .and_then(|values| values.get(name))
            .cloned()
            .expect("every declared secret was resolved")
    };

    let environment = policy
        .secrets
        .iter()
        .map(|spec| (spec.env_name.clone(), resolved_value(&spec.provider, &spec.secret_name)))
        .collect();

    if policy.proxy_secrets.is_empty() {
        // `exec_runtime` replaces this process on success, so reaching the
        // return at all means a failure to report.
        return Err(Failure::Execution(run::exec_runtime(
            context,
            policy,
            environment,
            &invocation.args,
        )));
    }

    // The proxy-backed order is fixed: provision, prepare the image, start
    // the sidecar, and only then start the target — the target must never
    // run before the proxy is ready
    // (docs/adr/20260820T162206Z_proxy-backed-secret-delivery.md).
    let mut provisioned = Vec::with_capacity(policy.proxy_secrets.len());
    for spec in &policy.proxy_secrets {
        let raw_value = resolved_value(&spec.provider, &spec.secret_name);
        match transfer::ProvisionedProxySecret::provision(spec, raw_value) {
            Ok(secret) => provisioned.push(secret),
            Err(e) => return Err(Failure::Provision(e)),
        }
    }
    let image = match proxy_image::ensure_proxy_image(&context.runtime) {
        Ok(image) => image,
        Err(e) => return Err(Failure::ProxyImage(e)),
    };
    let sidecar = match sidecar::Sidecar::start(
        &context.runtime,
        &image,
        &context.container_name,
        &transfer::transfer_line(&provisioned),
    ) {
        Ok(sidecar) => sidecar,
        Err(e) => return Err(Failure::Sidecar(e)),
    };
    let proxy_environment = transfer::target_environment(&provisioned, sidecar.port());
    // `sidecar` stays alive across the supervision and drops afterward on
    // every path, so the container is removed exactly when the invocation —
    // successful or not — is over.
    run::supervise_runtime(
        context,
        policy,
        environment,
        &proxy_environment,
        &invocation.args,
    )
    .map_err(Failure::Execution)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once(&"iwaya").chain(args))
    }

    fn parsed_exec(args: &[&str]) -> (String, String, Vec<String>) {
        match parse(args).unwrap() {
            Cli::Exec { context, command, args } => (context, command, args),
            Cli::Proxy => panic!("expected an exec invocation"),
        }
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
