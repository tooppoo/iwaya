# Forward Signals to the Supervised Runtime with signal-hook

- Status: Accepted
- Created: 2026-08-23T06:43:29Z

## Context

The direct `secret` path replaces iwaya with the runtime via `exec`, so the kernel delivers signals straight to the runtime and iwaya has nothing to forward. The proxy-backed path cannot do that: iwaya must stay alive to run the sidecar proxy, so it supervises the runtime as a child ([Add Proxy-Backed Secret Delivery](20260820T162206Z_proxy-backed-secret-delivery.md), "Process model"). A supervised child does not receive the terminal's signals directly — an interactive Ctrl-C, a `docker stop` (`SIGTERM`), or a terminal hangup reaches iwaya first. Without forwarding, Ctrl-C would kill iwaya and orphan the runtime, changing the externally observable behavior the direct path has today.

Rust's standard library offers no safe way to install a signal handler or to send a signal to another process, so implementing forwarding requires choosing how to reach the OS signal facilities. This needs an ADR because it adds a runtime dependency and settles whether iwaya writes its own `unsafe` async-signal-safe handler.

Related issue:

- #31

Related ADRs:

- [Add Proxy-Backed Secret Delivery](20260820T162206Z_proxy-backed-secret-delivery.md) requires the supervisor to "forward relevant signals correctly"; this ADR records how.
- [Use a Blocking HTTP Stack for the Reverse Proxy](20260822T162745Z_blocking-http-stack-for-proxy.md) established the thread-per-connection, no-async posture this forwarding fits into (a dedicated relay thread, not an async task).
- [Implement iwaya in Rust](20260808T171732Z_implement-iwaya-in-rust.md) sets the dependency posture: add a dependency when a concrete need appears, and prefer the smaller one.

## Decision

Signal forwarding uses `signal-hook`. A `signal_hook::iterator::Signals` instance registers `SIGINT`, `SIGTERM`, `SIGHUP`, and `SIGQUIT` before the child is spawned, and a dedicated relay thread iterates the caught signals and sends each to the child's pid. The child manages its own descendants, so the signal targets the child pid alone, not iwaya's process group. Sending the signal uses `libc::kill`, the only available primitive for relaying an arbitrary signal to another process.

The relay ends when the child exits: closing the `Signals` handle stops the iterator, and the relay thread is joined. iwaya does not adopt an async runtime for this.

## Alternatives Considered

### A hand-rolled `libc::sigaction` handler

Registering the handler directly with `libc` would add only `libc` (already in the tree). It was rejected because a correct signal handler must be async-signal-safe and must hand the event to normal code without a data race — in practice the self-pipe or atomic-flag pattern, plus correct handling of re-entrancy and `EINTR`. That is exactly the fiddly, `unsafe`, easy-to-get-subtly-wrong code `signal-hook` exists to encapsulate and has hardened. For a supervisor whose whole job is to not lose control of the child, hand-rolling the handler is the higher-risk choice for a marginal dependency saving. `libc` is still used, but only for the well-understood `kill` call.

### Adopting an async runtime (tokio signal handling)

`tokio::signal` handles this cleanly, but pulling in an async runtime solely for signal forwarding contradicts the blocking, no-async posture set for the proxy stack and would reshape far more of the binary than this feature warrants.

## Consequences

### Positive Consequences

- A supervised runtime receives Ctrl-C, `docker stop`, and hangups as if iwaya were not between it and the terminal, preserving the direct path's observable behavior.
- The `unsafe`, async-signal-safety-critical part of signal handling is delegated to a hardened library; iwaya's own `unsafe` is limited to a single `kill` call with no memory effects.

### Negative Consequences

- The dependency tree gains `signal-hook` (and its small `signal-hook-registry`/`errno` support crates), and `libc` becomes a direct dependency.
- A relay thread and the pre-spawn handler registration add lifecycle to the supervision path that the `exec` path does not have.

### Neutral Consequences

- The forwarded set is fixed to the four foreground signals; signals such as `SIGWINCH` (terminal resize) are out of scope until a concrete need appears.
- Forwarding targets the child pid, not a process group; if a future runtime needs group-wide delivery, that is a separate decision.
