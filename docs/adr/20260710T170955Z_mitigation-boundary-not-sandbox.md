# Treat iwaya as a Mitigation Boundary, Not a Sandbox

- Status: Accepted
- Created: 2026-07-10T17:09:55Z

## Context

iwaya places a policy-aware boundary between secret managers and command execution. It limits when secrets are resolved and injects them only into authorized child processes instead of persisting them in shells, containers, configuration files, or command-specific login state.

This model reduces ordinary secret exposure, but it does not isolate, inspect, or neutralize every command or script executed inside an iwaya-managed session.

Session-scoped PATH shims and deny rules are mitigations. They can narrow secret delivery and reject explicitly prohibited command patterns, but they do not provide the containment guarantees of a sandbox.

This distinction must be recorded because describing iwaya as generally "safe" without a precise boundary could cause users to rely on protections that iwaya does not provide.

Related issue:

- #2

Related ADRs:

- [Define iwaya as a Policy-Aware Command Proxy](20260702T005400Z_policy-aware-command-proxy.md)
- [Use Session-Scoped PATH Shims for Transparent Command Proxying](20260702T005500Z_session-scoped-path-shims.md)
- [Separate a Disposable Environment from a Non-Disposable Secret Boundary](20260813T131228Z_disposable-environment-and-secret-boundary.md) records the premise this record's non-goals leave implicit, and excludes behavioral restriction from iwaya's scope.
- [Define iwaya as a Docker-Context Secret Injection Runner](20260806T192918Z_docker-context-secret-injection-runner.md) keeps this non-sandbox boundary in force. Of the mitigations listed below, it replaces the pass-through of unmatched commands, the explicit deny rules for command patterns, and the description of injection as reaching only the selected child process; the rest continue to apply.

## Decision

iwaya must be documented and designed as a mitigation boundary, not as a sandbox.

iwaya must not claim to completely block malicious scripts, malicious child processes, or all forms of secret leakage.

iwaya should reduce ordinary secret exposure by:

- avoiding persistent raw secrets in shells, containers, configuration, logs, caches, and command-specific login state
- resolving secrets only as part of an authorized command execution
- injecting secrets only into the selected child process
- passing unmatched commands through without managed secret injection
- allowing explicit deny rules for command patterns that require restrictions

iwaya cannot fully control how a child process uses a secret after that secret has been injected. An authorized process may print, save, forward, transform, or leak the secret.

Protection against malicious code must be provided by other layers where required, such as:

- operating-system permissions
- container or virtual-machine isolation
- sandboxing
- secret-manager-side access control
- narrowly scoped credentials
- code and dependency review
- organizational execution policy

## Non-Goals

iwaya does not attempt to:

- inspect arbitrary program semantics
- prove that an authorized command is trustworthy
- prevent every indirect invocation of an allowed command
- contain all filesystem, network, process, or kernel effects of a child process
- replace sandboxing, endpoint security, or secret-manager authorization

## Alternatives Considered

### Describe iwaya as a secure execution environment

This framing would emphasize the security benefit of process-local secret injection.

It was rejected because "secure execution environment" implies broader isolation and control than iwaya provides. It could lead users to assume that malicious commands are contained merely because they run inside an iwaya-managed session.

### Treat deny policy as a sandbox boundary

Under this model, sufficiently detailed command and argument deny rules would be presented as protection against malicious execution.

It was rejected because command-pattern matching cannot account for arbitrary scripts, aliases, interpreters, subprocesses, generated arguments, or behavior after execution begins. Deny policy remains useful as a targeted mitigation, not as a general containment mechanism.

### Avoid making an explicit security claim

The project could describe only its mechanics and leave users to infer the security boundary.

It was rejected because omission would make misuse more likely. A security-sensitive tool should state both the protection it provides and the protection it does not provide.

## Consequences

### Positive Consequences

- The project's security claims remain proportional to its actual controls.
- Users are less likely to mistake an iwaya-managed session for a sandbox.
- The v0 scope can remain focused on reducing persistent and excessive secret exposure.
- Future isolation features can be evaluated as separate layers rather than being implied by the command proxy.

### Negative Consequences

- iwaya cannot be presented as a complete defense against malicious commands.
- Users with hostile-code execution requirements must deploy and understand additional isolation layers.
- Documentation must repeatedly preserve this distinction in architecture, help, and user-facing security guidance.

### Neutral Consequences

- Explicit deny rules remain part of the policy model, but their role is targeted restriction rather than complete containment.
- Execution backends may provide stronger isolation, but backend isolation is not guaranteed by iwaya core.
