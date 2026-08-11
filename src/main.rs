//! `iwaya exec --context <context> <command> -- [args...]` is the only
//! entrypoint (docs/v0-scope.md). The flow is validate, resolve, construct,
//! execute, in that order, and a failure at any stage executes nothing
//! (docs/design/docker-execution.md, "Execution Order").
//!
//! The output format and the exit-code categories are open decisions in
//! docs/v0-scope.md. Until they are settled, diagnostics are single lines on
//! stderr and the exit codes below are provisional; they distinguish the
//! failure stages the model requires to stay distinguishable.

mod bws;
mod config;
mod run;
mod secret;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use config::{CommandId, ContextId, Provider, SecretName};

const USAGE: &str = "usage: iwaya exec --context <context> <command> [-- args...]";

const EXIT_USAGE: u8 = 2;
const EXIT_CONFIG: u8 = 3;
const EXIT_UNKNOWN_SELECTION: u8 = 4;
const EXIT_RESOLUTION: u8 = 5;
const EXIT_EXECUTION: u8 = 6;

enum Failure {
    Usage(String),
    Config(config::ConfigError),
    UnknownContext(ContextId),
    UnknownCommand(CommandId),
    Resolution(bws::ResolveError),
    Execution(run::ExecError),
}

impl Failure {
    fn exit_code(&self) -> u8 {
        match self {
            Failure::Usage(_) => EXIT_USAGE,
            Failure::Config(_) => EXIT_CONFIG,
            Failure::UnknownContext(_) | Failure::UnknownCommand(_) => EXIT_UNKNOWN_SELECTION,
            Failure::Resolution(_) => EXIT_RESOLUTION,
            Failure::Execution(_) => EXIT_EXECUTION,
        }
    }

    fn message(&self) -> String {
        match self {
            Failure::Usage(detail) => format!("{detail}\n{USAGE}"),
            Failure::Config(e) => e.to_string(),
            Failure::UnknownContext(id) => {
                format!("unknown context '{id}': no configured context has this identifier")
            }
            Failure::UnknownCommand(id) => {
                format!("unknown command '{id}': no configured command policy has this identifier")
            }
            Failure::Resolution(e) => format!("secret resolution failed, nothing was executed: {e}"),
            Failure::Execution(e) => e.to_string(),
        }
    }
}

fn main() -> ExitCode {
    // `exec_and_never_return` replaces this process on success, so reaching
    // a return value at all means a failure to report.
    let failure = exec_and_never_return(std::env::args().skip(1).collect());
    eprintln!("iwaya: error: {}", failure.message());
    ExitCode::from(failure.exit_code())
}

struct Invocation {
    context: ContextId,
    command: CommandId,
    args: Vec<String>,
}

fn parse_invocation(args: Vec<String>) -> Result<Invocation, Failure> {
    let usage = |detail: &str| Failure::Usage(detail.to_string());

    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("exec") => {}
        Some(other) => return Err(usage(&format!("unknown subcommand '{other}'"))),
        None => return Err(usage("a subcommand is required")),
    }

    let mut context = None;
    let mut command = None;
    let mut rest = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--context" => match args.next() {
                Some(value) if context.is_none() => context = Some(value),
                Some(_) => return Err(usage("'--context' is given more than once")),
                None => return Err(usage("'--context' requires a value")),
            },
            "--" => {
                rest = args.collect();
                break;
            }
            other if other.starts_with('-') => {
                return Err(usage(&format!("unknown option '{other}'")))
            }
            _ if command.is_none() => command = Some(arg),
            _ => return Err(usage("more than one command operand; put command arguments after '--'")),
        }
    }

    Ok(Invocation {
        context: ContextId::new(&context.ok_or_else(|| usage("'--context' is required"))?),
        command: CommandId::new(&command.ok_or_else(|| usage("a command operand is required"))?),
        args: rest,
    })
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

fn exec_and_never_return(args: Vec<String>) -> Failure {
    let invocation = match parse_invocation(args) {
        Ok(invocation) => invocation,
        Err(failure) => return failure,
    };

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
