//! Starts and stops the ephemeral proxy sidecar container of one
//! proxy-backed invocation.
//!
//! The sidecar shares the target container's network namespace and receives
//! its secret transfer through stdin before reporting readiness
//! (docs/adr/20260820T162206Z_proxy-backed-secret-delivery.md, "Proxy
//! container networking" and "Secret transfer into the proxy container").
//! One container serves every `proxy-secret` of the invocation.

use std::fmt;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};

use serde::Deserialize;

/// A running proxy sidecar. Dropping it removes the container and reaps the
/// runtime client, so every exit path of the supervisor — including early
/// failures — tears the sidecar down without separate bookkeeping.
pub struct Sidecar {
    runtime: String,
    container_name: String,
    port: u16,
    child: Child,
}

/// The one readiness line the proxy writes before serving
/// (`proxy::run_proxy_mode`).
#[derive(Deserialize)]
struct Readiness {
    port: u16,
}

/// Far above any `{"port":N}` line, far below anything worth buffering from
/// an image the supervisor must not trust.
const READINESS_LIMIT: u64 = 256;

impl Sidecar {
    /// Starts the sidecar from `image`, delivers `transfer` on its stdin,
    /// and waits for the readiness line. Returning means the proxy is
    /// listening on `port()` inside `target_container`'s network namespace;
    /// the target command must not start before that
    /// (docs/adr/20260820T162206Z_proxy-backed-secret-delivery.md).
    ///
    /// `transfer` must be a single line without the trailing newline; the
    /// serialized JSON transfer document satisfies this because JSON string
    /// escapes keep raw newlines out of the serialization.
    // Unwired until the exec path composes image, sidecar, and supervision
    // (issue #42); this allow and the one on `port` are the module's only
    // roots, so anything they stop reaching stays flagged.
    #[allow(dead_code)]
    pub fn start(
        runtime: &str,
        image: &str,
        target_container: &str,
        transfer: &str,
    ) -> Result<Sidecar, SidecarError> {
        if transfer.contains('\n') {
            // The proxy reads exactly one line; a second line would be
            // silently ignored, so a multi-line document is a supervisor
            // bug surfaced here rather than as a half-configured proxy.
            return Err(SidecarError::TransferNotSingleLine);
        }
        let container_name = ephemeral_name()?;
        let mut child = Command::new(runtime)
            .args(["run", "--rm", "--interactive"])
            // Loopback inside the target's namespace is the only path to
            // the proxy: nothing is published on the host.
            .arg("--network")
            .arg(format!("container:{target_container}"))
            .args(["--name", &container_name, image])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // The proxy's diagnostics carry no secret by design, and the
            // user's terminal is where a startup failure must show up.
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|source| SidecarError::Start {
                runtime: runtime.to_string(),
                source,
            })?;

        // The write end is dropped after the newline: the transfer is the
        // only thing the supervisor ever sends, and the proxy does not need
        // the pipe to stay open.
        let delivery = {
            let mut stdin = child.stdin.take().expect("stdin was requested as piped");
            stdin
                .write_all(transfer.as_bytes())
                .and_then(|_| stdin.write_all(b"\n"))
        };
        let stdout = child.stdout.take().expect("stdout was requested as piped");
        // From here `sidecar` is alive on every failure return, so its
        // `Drop` removes the container the failure leaves behind.
        let mut sidecar = Sidecar {
            runtime: runtime.to_string(),
            container_name,
            port: 0,
            child,
        };
        if let Err(source) = delivery {
            return Err(SidecarError::Transfer(source));
        }
        // The read is capped: readiness bytes come from the image, and a
        // defective one streaming a newline-less flood must exhaust this
        // limit — landing on the unusable-readiness rejection — rather than
        // the supervisor's memory.
        let mut readiness = String::new();
        match BufReader::new(stdout.take(READINESS_LIMIT)).read_line(&mut readiness) {
            Ok(0) => return Err(SidecarError::ExitedBeforeReady),
            Ok(_) => {}
            Err(source) => return Err(SidecarError::Readiness(source)),
        }
        let parsed: Readiness = match serde_json::from_str(readiness.trim_end()) {
            Ok(parsed) => parsed,
            // The line is not echoed: readiness carries only the port by
            // contract, but a defective image could print anything, and a
            // diagnostic must not gamble on what.
            Err(_) => return Err(SidecarError::UnusableReadiness),
        };
        sidecar.port = parsed.port;
        Ok(sidecar)
    }

    /// The loopback port the proxy listens on, valid inside the target
    /// container's network namespace.
    // Unwired until issue #42's exec path; see `start`.
    #[allow(dead_code)]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Removes the container and reaps the runtime client. `rm --force` is
    /// idempotent toward a container that already exited under `--rm`, and
    /// its outcome is deliberately ignored: teardown runs on paths that
    /// already have a more useful error to report.
    fn teardown(&mut self) {
        let _ = Command::new(&self.runtime)
            .args(["rm", "--force", &self.container_name])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = self.child.wait();
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        self.teardown();
    }
}

/// A name no concurrent invocation shares, so `--name` collisions and
/// `rm --force` cross-talk between invocations cannot happen.
fn ephemeral_name() -> Result<String, SidecarError> {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| SidecarError::Name)?;
    let mut name = String::from("iwaya-proxy-");
    for byte in random {
        write!(name, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(name)
}

/// A failure to bring the sidecar up. No variant carries transfer or
/// readiness bytes: the transfer holds raw secrets, and a defective image
/// controls what readiness contains.
#[derive(Debug)]
pub enum SidecarError {
    Name,
    TransferNotSingleLine,
    Start {
        runtime: String,
        source: std::io::Error,
    },
    Transfer(std::io::Error),
    ExitedBeforeReady,
    Readiness(std::io::Error),
    UnusableReadiness,
}

impl fmt::Display for SidecarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SidecarError::Name => write!(
                f,
                "cannot name the proxy sidecar: no usable OS entropy source"
            ),
            SidecarError::TransferNotSingleLine => {
                write!(f, "the proxy transfer document is not a single line")
            }
            SidecarError::Start { runtime, source } => {
                write!(f, "cannot start runtime '{runtime}': {source}")
            }
            SidecarError::Transfer(source) => {
                write!(f, "cannot deliver the proxy transfer: {source}")
            }
            SidecarError::ExitedBeforeReady => {
                write!(f, "the proxy sidecar exited before reporting readiness")
            }
            SidecarError::Readiness(source) => {
                write!(f, "cannot read the proxy sidecar readiness: {source}")
            }
            SidecarError::UnusableReadiness => {
                write!(f, "the proxy sidecar reported unusable readiness")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("iwaya-sidecar-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A runtime stand-in that appends its argv to `log` and follows `body`
    /// for its `run` behavior; `rm` always succeeds.
    fn fake_runtime(dir: &Path, log: &Path, body: &str) -> String {
        let path = dir.join("runtime");
        fs::write(
            &path,
            format!(
                "#!/bin/sh\necho \"$@\" >> {}\ncase \"$1\" in\n  rm) exit 0 ;;\nesac\n{body}\n",
                log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    const READY: &str = r#"read line; echo '{"port":34567}'"#;

    // `Sidecar` has no `Debug` (it has no reason to be formatted), so the
    // error side is extracted by match rather than `unwrap_err`.
    fn start_error(runtime: &str, transfer: &str) -> String {
        match Sidecar::start(runtime, "img", "target", transfer) {
            Ok(_) => panic!("expected a start error"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn starts_the_sidecar_with_the_documented_argv() {
        let dir = test_dir("argv");
        let log = dir.join("log");
        let runtime = fake_runtime(&dir, &log, READY);
        let _sidecar = Sidecar::start(&runtime, "img:tag", "target-app", "{}").unwrap();
        let calls = fs::read_to_string(&log).unwrap();
        let run = calls.lines().next().unwrap();
        assert!(
            run.starts_with(
                "run --rm --interactive --network container:target-app --name iwaya-proxy-"
            ),
            "{run}"
        );
        assert!(run.ends_with(" img:tag"), "{run}");
    }

    #[test]
    fn delivers_the_transfer_document_on_stdin() {
        let dir = test_dir("transfer");
        let log = dir.join("log");
        let received = dir.join("received");
        let body = format!(
            "read line; echo \"$line\" > {}; echo '{{\"port\":1}}'",
            received.display()
        );
        let runtime = fake_runtime(&dir, &log, &body);
        let _sidecar = Sidecar::start(&runtime, "img", "target", r#"{"routes":["r1"]}"#).unwrap();
        assert_eq!(
            fs::read_to_string(&received).unwrap().trim_end(),
            r#"{"routes":["r1"]}"#
        );
    }

    #[test]
    fn reports_the_port_the_sidecar_announced() {
        let dir = test_dir("port");
        let log = dir.join("log");
        let runtime = fake_runtime(&dir, &log, READY);
        let sidecar = Sidecar::start(&runtime, "img", "target", "{}").unwrap();
        assert_eq!(sidecar.port(), 34567);
    }

    #[test]
    fn names_each_sidecar_uniquely() {
        let dir = test_dir("names");
        let log = dir.join("log");
        let runtime = fake_runtime(&dir, &log, READY);
        let first = Sidecar::start(&runtime, "img", "target", "{}").unwrap();
        let second = Sidecar::start(&runtime, "img", "target", "{}").unwrap();
        assert_ne!(first.container_name, second.container_name);
    }

    #[test]
    fn removes_the_container_on_drop() {
        let dir = test_dir("drop");
        let log = dir.join("log");
        let runtime = fake_runtime(&dir, &log, READY);
        let sidecar = Sidecar::start(&runtime, "img", "target", "{}").unwrap();
        let name = sidecar.container_name.clone();
        drop(sidecar);
        let calls = fs::read_to_string(&log).unwrap();
        assert!(
            calls.lines().any(|line| line == format!("rm --force {name}")),
            "{calls}"
        );
    }

    #[test]
    fn removes_the_container_when_readiness_is_unusable() {
        let dir = test_dir("failure-cleanup");
        let log = dir.join("log");
        let runtime = fake_runtime(&dir, &log, r#"read line; echo not-json"#);
        start_error(&runtime, "{}");
        let calls = fs::read_to_string(&log).unwrap();
        assert!(
            calls
                .lines()
                .any(|line| line.starts_with("rm --force iwaya-proxy-")),
            "{calls}"
        );
    }

    #[test]
    fn reports_a_sidecar_that_exits_before_readiness() {
        let dir = test_dir("early-exit");
        let log = dir.join("log");
        let runtime = fake_runtime(&dir, &log, "exit 7");
        let rendered = start_error(&runtime, "{}");
        assert!(rendered.contains("before reporting readiness"), "{rendered}");
    }

    #[test]
    fn reports_unusable_readiness_without_echoing_it() {
        let dir = test_dir("bad-readiness");
        let log = dir.join("log");
        let runtime = fake_runtime(&dir, &log, r#"read line; echo leaked-readiness-bytes"#);
        let rendered = start_error(&runtime, "{}");
        assert!(rendered.contains("unusable readiness"), "{rendered}");
        assert!(!rendered.contains("leaked-readiness-bytes"), "{rendered}");
    }

    #[test]
    fn rejects_a_newline_less_readiness_flood_at_the_limit() {
        let dir = test_dir("flood");
        let log = dir.join("log");
        // 4 KiB without a newline: read_line must stop at READINESS_LIMIT
        // and reject, not buffer until the stream ends.
        let runtime = fake_runtime(
            &dir,
            &log,
            r#"read line; head -c 4096 /dev/zero | tr '\0' 'x'"#,
        );
        let rendered = start_error(&runtime, "{}");
        assert!(rendered.contains("unusable readiness"), "{rendered}");
    }

    #[test]
    fn rejects_a_multi_line_transfer_before_starting_anything() {
        let dir = test_dir("multi-line");
        let log = dir.join("log");
        let runtime = fake_runtime(&dir, &log, READY);
        let rendered = start_error(&runtime, "{}\n{}");
        assert!(rendered.contains("not a single line"), "{rendered}");
        assert!(!log.exists(), "the runtime must not have been invoked");
    }

    #[test]
    fn reports_a_runtime_that_cannot_start() {
        let rendered = start_error("no-such-runtime-zz-iwaya", "{}");
        assert!(rendered.contains("no-such-runtime-zz-iwaya"), "{rendered}");
    }
}
