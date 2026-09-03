//! Constructs the runtime invocation and executes it, as defined in
//! docs/design/docker-execution.md ("Invocation Construction", "Environment
//! Injection Constraints", "Process Behavior").

use std::fmt;
use std::os::unix::process::CommandExt;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, ExitStatus};
use std::thread;

use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook::iterator::Signals;

use crate::config::{CommandPolicy, DockerContext, EnvName};
use crate::secret::Secret;

/// Signals a foreground process is normally expected to act on: an
/// interactive Ctrl-C (`SIGINT`) or Ctrl-\ (`SIGQUIT`), a `docker stop` /
/// service shutdown (`SIGTERM`), and a terminal hangup (`SIGHUP`).
///
/// The relay is load-bearing for a signal aimed at iwaya's pid — above all
/// `SIGTERM` from `docker stop` or a service manager — which the kernel
/// delivers to iwaya alone, never to the supervised child.
/// The terminal-generated members (`SIGINT`/`SIGQUIT`/`SIGHUP`) already
/// reach the child through the shared process group, so relaying them is a
/// harmless second delivery, kept for when they are sent to iwaya's pid
/// directly (`kill -INT <iwaya-pid>`) rather than typed at the terminal.
const FORWARDED_SIGNALS: [i32; 4] = [SIGINT, SIGTERM, SIGHUP, SIGQUIT];

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
    // A proxy-backed secret injects two variables, neither carrying a raw
    // value: the phantom under the credential name, and the loopback proxy
    // URL under `base-url-env` (docs/design/configuration.md,
    // "Proxy-Backed Secret Delivery").
    for proxy_secret in &policy.proxy_secrets {
        argv.push("--env".to_string());
        argv.push(proxy_secret.env_name.to_string());
        argv.push("--env".to_string());
        argv.push(proxy_secret.base_url_env.to_string());
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
    apply_injected_environment(&mut command, &environment);
    let source = command.exec();
    ExecError {
        runtime: context.runtime.clone(),
        source,
    }
}

/// Registers each resolved value as an explicit environment entry on the
/// command. `Command::env` overrides a same-named variable inherited from
/// the invoking environment at spawn time, as the injection constraints
/// require. Shared by both execution paths so the delivery contract cannot
/// drift between them.
fn apply_injected_environment(command: &mut Command, environment: &[(EnvName, Secret)]) {
    for (name, value) in environment {
        command.env(name.as_str(), value.expose_to_subprocess_env());
    }
}

/// Runs the runtime as a child of iwaya instead of replacing iwaya with it,
/// so iwaya stays alive alongside it — the supervision a proxy-backed
/// invocation needs, where a sidecar proxy must outlive the target's start
/// (docs/adr/20260820T162206Z_proxy-backed-secret-delivery.md, "Process
/// model"). Returns the exit code to propagate, or an error if the runtime
/// could not be started.
///
/// The direct-`secret` path keeps using `exec_runtime`; this path exists for
/// proxy-backed delivery, which is not yet wired into execution (issue #31).
// Unread until the proxy execution mode wires it in (issue #31).
#[allow(dead_code)]
pub fn supervise_runtime(
    context: &DockerContext,
    policy: &CommandPolicy,
    environment: Vec<(EnvName, Secret)>,
    user_args: &[String],
) -> Result<u8, ExecError> {
    let argv = build_argv(context, policy, user_args);
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    apply_injected_environment(&mut command, &environment);
    supervise(command, &context.runtime)
}

/// Spawns a prepared command with iwaya's own stdin/stdout/stderr (the
/// default for a spawned child), relays foreground signals to it, waits for
/// it, and reduces its status to an exit code. Split from
/// `supervise_runtime` so the wait-and-propagate behavior is testable
/// without constructing a Docker-shaped invocation.
#[allow(dead_code)]
fn supervise(mut command: Command, runtime: &str) -> Result<u8, ExecError> {
    let error = |source| ExecError {
        runtime: runtime.to_string(),
        source,
    };
    // Register the handlers before the child exists so a signal arriving
    // during spawn is queued for relay rather than taking iwaya's default
    // action (which for most of these is to terminate iwaya and orphan the
    // child).
    let mut signals = Signals::new(FORWARDED_SIGNALS).map_err(error)?;
    let handle = signals.handle();

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(source) => {
            handle.close();
            return Err(error(source));
        }
    };

    // Relay each caught signal to the child on a separate thread. The child
    // manages its own descendants, so the signal goes to its pid alone, not
    // to iwaya's process group (which would include iwaya itself).
    let pid = child.id();
    let relay = thread::spawn(move || {
        for signal in &mut signals {
            forward_signal(pid, signal);
        }
    });

    let result = child.wait();
    // Stop the relay: `handle.close()` ends the signal iterator, so the
    // thread returns and can be joined.
    handle.close();
    let _ = relay.join();

    match result {
        Ok(status) => Ok(exit_code_of(status)),
        Err(source) => {
            // `wait` failed but the child may still be running, and dropping
            // a `Child` does not stop its process. A supervisor must not
            // leave the runtime (and its `exec`-ed target) orphaned, so
            // terminate and reap it best-effort before reporting the
            // original failure.
            let _ = child.kill();
            let _ = child.wait();
            Err(error(source))
        }
    }
}

/// Sends one signal to a child process by pid. `libc::kill` is the only way
/// to relay an arbitrary signal to another process; std offers none. Errors
/// are ignored on purpose: the one that occurs in practice is the child
/// having already exited (`ESRCH`), which needs no action.
fn forward_signal(pid: u32, signal: i32) {
    // SAFETY: `kill` is an async-signal-safe syscall with no memory effects;
    // passing a pid and signal number cannot violate any Rust invariant.
    unsafe {
        libc::kill(pid as libc::pid_t, signal);
    }
}

/// Follows the shell convention so the propagated code is the one a caller
/// already expects: a normal exit yields its own code, and a signal-killed
/// child yields 128 + the signal number.
#[allow(dead_code)]
fn exit_code_of(status: ExitStatus) -> u8 {
    match status.code() {
        Some(code) => code as u8,
        None => 128u8.wrapping_add(status.signal().unwrap_or(0) as u8),
    }
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;
    use crate::config::{
        CommandId, ContextId, InjectHeader, ProviderId, ProxySecretSpec, SecretName, SecretSpec,
    };

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

    // Without this test, a proxy-backed policy could reach the runtime with
    // neither of its two variables listed, and the container would silently
    // not receive the phantom or the proxy URL.
    #[test]
    fn lists_both_proxy_secret_env_names_in_the_argv() {
        let context = DockerContext {
            id: ContextId::new("iwaya"),
            runtime: "docker".to_string(),
            user: "vscode".to_string(),
            workdir: "/w".to_string(),
            container_name: "app".to_string(),
        };
        let policy = CommandPolicy {
            id: CommandId::new("claude"),
            secrets: vec![],
            proxy_secrets: vec![ProxySecretSpec {
                env_name: EnvName::new("ANTHROPIC_AUTH_TOKEN"),
                provider: ProviderId::new("bws-default"),
                secret_name: SecretName::new("ANTHROPIC_AUTH_TOKEN"),
                upstream: "https://api.anthropic.com".to_string(),
                base_url_env: EnvName::new("ANTHROPIC_BASE_URL"),
                inject_header: InjectHeader {
                    name: "x-api-key".to_string(),
                    template: "{}".to_string(),
                },
            }],
        };

        let argv = build_argv(&context, &policy, &[]);

        let env_names: Vec<&str> = argv
            .windows(2)
            .filter(|pair| pair[0] == "--env")
            .map(|pair| pair[1].as_str())
            .collect();
        assert_eq!(env_names, ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL"]);
    }

    // Several distinct codes so a stubbed `Ok(<constant>)` cannot pass.
    #[test_case(0)]
    #[test_case(3)]
    #[test_case(42)]
    #[test_case(255)]
    fn propagates_a_normal_child_exit_code(code: u8) {
        let mut command = Command::new("sh");
        command.args(["-c", &format!("exit {code}")]);
        assert_eq!(supervise(command, "sh").unwrap(), code);
    }

    #[test]
    fn maps_a_signal_killed_child_to_128_plus_the_signal() {
        let mut command = Command::new("sh");
        // SIGTERM is 15, so the shell convention yields 143.
        command.args(["-c", "kill -TERM $$"]);
        assert_eq!(supervise(command, "sh").unwrap(), 143);
    }

    #[test]
    fn forwards_the_foreground_signals() {
        // The set a foreground process is expected to act on. Pinned so a
        // later edit cannot silently drop one (e.g. SIGTERM, which delivers
        // `docker stop`).
        assert_eq!(FORWARDED_SIGNALS, [SIGINT, SIGTERM, SIGHUP, SIGQUIT]);
    }

    #[test]
    fn forward_signal_delivers_to_the_child_process() {
        // Signals the child directly (not this test process), so no handler
        // is installed on the harness and the delivery path is exercised in
        // isolation. `sleep` has no SIGTERM handler, so it dies from the
        // signal and the status carries the signal number.
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        forward_signal(child.id(), SIGTERM);
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(SIGTERM));
    }

    #[test]
    fn passes_arguments_through_to_the_child() {
        let dir = std::env::temp_dir().join(format!("iwaya-supervise-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("arg.txt");
        let _ = std::fs::remove_file(&marker);

        let mut command = Command::new("sh");
        command.args([
            "-c",
            "printf %s \"$1\" > \"$2\"",
            "sh",
            "forwarded-value",
            marker.to_str().unwrap(),
        ]);
        assert_eq!(supervise(command, "sh").unwrap(), 0);
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "forwarded-value");

        let _ = std::fs::remove_file(&marker);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn reports_a_runtime_that_cannot_start() {
        let command = Command::new("no-such-runtime-zz-iwaya");
        let error = supervise(command, "no-such-runtime-zz-iwaya").unwrap_err();
        assert_eq!(error.runtime, "no-such-runtime-zz-iwaya");
    }

    // Guards the shared injection contract: every resolved value is
    // registered as an explicit command environment entry, which
    // `Command::env` applies over any inherited same-named variable at
    // spawn. Inspecting the built command (rather than the process
    // environment) keeps this free of the data race that mutating the global
    // environment in a threaded test harness would introduce.
    #[test]
    fn registers_each_resolved_value_as_an_overriding_env_entry() {
        let mut command = Command::new("true");
        let environment = vec![
            (EnvName::new("TOKEN"), Secret::new("resolved".to_string())),
            (EnvName::new("OTHER"), Secret::new("second".to_string())),
        ];

        apply_injected_environment(&mut command, &environment);

        let injected: Vec<(String, Option<String>)> = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_str().unwrap().to_string(),
                    value.map(|v| v.to_str().unwrap().to_string()),
                )
            })
            .collect();
        assert!(injected.contains(&("TOKEN".to_string(), Some("resolved".to_string()))));
        assert!(injected.contains(&("OTHER".to_string(), Some("second".to_string()))));
    }
}
