//! Constructs the runtime invocation and executes it, as defined in
//! docs/design/docker-execution.md ("Invocation Construction", "Environment
//! Injection Constraints", "Process Behavior").

use std::fmt;
use std::os::unix::process::CommandExt;
use std::process::Command;

use crate::config::{CommandPolicy, DockerContext, EnvName};
use crate::secret::Secret;

/// The complete shape of what iwaya builds. An `--env` option exists only for
/// an environment variable the selected policy declares, always as a bare
/// `--env NAME`: the `--env NAME=VALUE` form would place the raw value in the
/// host process table.
pub fn build_argv(
    context: &DockerContext,
    policy: &CommandPolicy,
    user_args: &[String],
) -> Vec<String> {
    let mut argv = vec![
        context.runtime.clone(),
        "exec".to_string(),
        "--interactive".to_string(),
        "--tty".to_string(),
    ];
    for secret in &policy.secrets {
        argv.push("--env".to_string());
        argv.push(secret.env_name.to_string());
    }
    argv.push("--user".to_string());
    argv.push(context.user.clone());
    argv.push("--workdir".to_string());
    argv.push(context.workdir.clone());
    argv.push(context.container_name.clone());
    argv.push(policy.id.to_string());
    argv.extend(user_args.iter().cloned());
    argv
}

#[derive(Debug)]
pub struct ExecError {
    pub runtime: String,
    pub source: std::io::Error,
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot start runtime '{}': {}", self.runtime, self.source)
    }
}

/// Replaces the iwaya process with the runtime process, so stdin, stdout,
/// stderr, signals, and the exit status pass through without an intermediary.
/// Returns only when the runtime could not be started.
pub fn exec_runtime(
    context: &DockerContext,
    policy: &CommandPolicy,
    environment: Vec<(EnvName, Secret)>,
    user_args: &[String],
) -> ExecError {
    let argv = build_argv(context, policy, user_args);
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    for (name, value) in &environment {
        // `Command::env` overwrites a same-named variable inherited from the
        // invoking environment, as the injection constraints require.
        command.env(name.as_str(), value.expose_to_subprocess_env());
    }
    let source = command.exec();
    ExecError {
        runtime: context.runtime.clone(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CommandId, ContextId, ProviderId, SecretName, SecretSpec};

    // The documented construction walk-through for
    // `iwaya exec --context iwaya claude -- --resume` against the baseline
    // example (docs/design/docker-execution.md, "Invocation Construction").
    #[test]
    fn builds_the_documented_argv() {
        let context = DockerContext {
            id: ContextId::new("iwaya"),
            runtime: "podman".to_string(),
            user: "vscode".to_string(),
            workdir: "/workspaces/iwaya".to_string(),
            container_name: "iwaya-dev".to_string(),
        };
        let policy = CommandPolicy {
            id: CommandId::new("claude"),
            secrets: vec![SecretSpec {
                env_name: EnvName::new("ANTHROPIC_AUTH_TOKEN"),
                provider: ProviderId::new("bws-default"),
                secret_name: SecretName::new("ANTHROPIC_AUTH_TOKEN"),
            }],
            proxy_secrets: vec![],
        };

        let argv = build_argv(&context, &policy, &["--resume".to_string()]);

        assert_eq!(
            argv,
            [
                "podman",
                "exec",
                "--interactive",
                "--tty",
                "--env",
                "ANTHROPIC_AUTH_TOKEN",
                "--user",
                "vscode",
                "--workdir",
                "/workspaces/iwaya",
                "iwaya-dev",
                "claude",
                "--resume",
            ]
        );
    }
}
