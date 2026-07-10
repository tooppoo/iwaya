# Command Proxy Architecture

## Purpose

iwaya is a policy-aware command proxy for running ordinary development commands with narrowly scoped secrets.

The core design goal is to let a user enter an iwaya-managed shell session and continue using familiar commands while avoiding persistent raw secrets in the shell, container, repository, or command-specific login state.

iwaya is not a secret manager and is not a sandbox.

The durable decisions behind this architecture are recorded in:

- [Define iwaya as a Policy-Aware Command Proxy](../adr/20260702T005400Z_policy-aware-command-proxy.md)
- [Use Session-Scoped PATH Shims for Transparent Command Proxying](../adr/20260702T005500Z_session-scoped-path-shims.md)
- [Treat iwaya as a Mitigation Boundary, Not a Sandbox](../adr/20260710T170955Z_mitigation-boundary-not-sandbox.md)

## Scope

This document defines the architecture from managed-session startup through command execution:

1. start an iwaya-managed shell session
2. intercept configured command names through session-scoped PATH shims
3. construct a command invocation
4. evaluate policy
5. either deny, pass through, or determine a secret injection mapping
6. resolve required secrets from external secret managers
7. execute the command through a selected backend

This document does not define:

- the complete configuration file syntax
- the exact pattern language used for argument matching
- the complete `iwaya --help` contract
- a sandbox or hostile-code containment model
- secret-manager storage, encryption, authentication, or rotation

## Interactive Entry Point

Running `iwaya` without a subcommand starts an iwaya-managed shell session.

```sh
iwaya
gh pr list
git status
```

The managed session must be visibly distinguishable from an ordinary shell session. The initial implementation may use a startup banner. Prompt modification is optional because it may conflict with user prompt frameworks.

Help and explicit subcommand behavior are handled separately in issue #10. In particular, `iwaya --help` must not start a managed session.

## High-Level Structure

```txt
user
  |
  | iwaya
  v
managed shell session
  - no command secrets in session environment
  - PATH starts with a session-local shim directory
  |
  | managed command, for example: gh pr list
  v
command shim
  |
  | command name + args + session context
  v
iwaya core
  1. resolve command invocation
  2. evaluate policy
  3. deny, pass through, or determine injection mapping
  4. resolve required secrets
  5. select execution backend
  |
  v
execution backend
  |
  v
real child process
  - receives only policy-authorized injected secrets
```

## Components

### Managed Session Launcher

The session launcher starts the user's shell with iwaya session metadata and a session-local shim directory prepended to `PATH`.

The launcher may provide metadata such as:

```sh
IWAYA_SESSION=1
IWAYA_SESSION_ID=<session-id>
IWAYA_POLICY_FILE=<path>
```

The launcher must not place command secrets such as `GH_TOKEN` in the session-wide environment.

The launcher must not permanently modify the user's global `PATH`.

### Session-Scoped Command Shims

A managed command has an executable shim in the session-local shim directory.

Example:

```txt
~/.cache/iwaya/sessions/<session-id>/bin/gh
```

The shim forwards the command name, argument list, and available session context to iwaya core. It does not resolve or inject secrets itself.

The real-command lookup must avoid recursively selecting the shim again.

Commands without a shim continue to be resolved by the user's shell and existing `PATH` rules.

### iwaya Core

iwaya core coordinates policy evaluation, secret resolution, and backend execution.

Its responsibilities are:

- observe the command invocation
- evaluate policy
- determine the injection mapping produced by the matched policy
- select an execution backend
- resolve referenced secrets through configured external integrations
- construct a child-process environment
- execute the real command
- redact secrets from logs and diagnostics

It must not expose a general-purpose API or command that prints, exports, or returns raw secrets.

### Policy Evaluator

The policy evaluator receives a resolved command invocation and returns one of three outcomes:

```txt
Deny
PassThrough
AllowWithInjection(mapping)
```

`PassThrough` means that the command is delegated to the selected backend without managed secret injection.

`AllowWithInjection(mapping)` means that the matched policy determines the environment variable names and secret references required by the child process.

### Secret Resolver

The secret resolver obtains values from an external secret manager or configured provider integration only after policy has authorized injection.

External secret managers remain responsible for:

- storage
- encryption
- authentication
- rotation
- manager-side authorization
- manager-side audit behavior

iwaya must not persist the resolved value in its configuration, cache, logs, credential store, or managed-session environment.

### Execution Backend

An execution backend performs the real command execution in a target environment.

Expected backend kinds include:

- local process execution
- devcontainer execution

Backends are extension points. They do not define iwaya's core identity or policy semantics.

Each backend receives:

- the resolved real command
- the argument list
- the working directory and relevant repository context
- the child-process environment after authorized injection

Each backend must preserve the child-process-only injection boundary. It must not promote injected secrets into a persistent backend-wide or session-wide environment.

## Command Invocation Model

Policy is evaluated against the command invocation that has been resolved immediately before delegation to an execution backend.

A command invocation contains:

```txt
CommandInvocation
  resolved command name
  argument list
  current working directory
  repository context
  execution backend kind
  requested profile or policy scope, if explicitly provided
```

The exact representation is an implementation detail, but these fields form the architectural input to policy evaluation.

The injection mapping is not part of the command invocation. It is produced by the matched policy.

This separation prevents a circular model in which the requested secret injection would itself determine whether that injection is authorized.

## Policy Model

The minimum policy model can match on:

- command
- argument pattern
- current working directory or repository context
- backend kind
- requested profile or policy scope, when explicitly provided

A policy outcome may specify:

- allow or deny
- injected environment variable name
- secret source reference

### Precedence

Policy precedence is:

1. explicit deny
2. matching allow rule
3. no-match pass-through

An explicit deny rule takes precedence over a matching allow rule.

### No-Match Behavior

When no policy matches, iwaya delegates the command to the selected backend without managed secret injection.

No-match is not an implicit denial.

A command or command pattern is denied only when policy explicitly constrains and denies it.

This keeps the managed shell usable for ordinary commands that do not require iwaya-managed secrets.

## Execution Flow

### Allowed Command with Secret Injection

```txt
1. The user runs a managed command inside the iwaya session.
2. The shim sends the command request to iwaya core.
3. iwaya resolves the command invocation.
4. The policy evaluator finds a matching allow rule.
5. The matched policy produces an injection mapping.
6. iwaya resolves only the referenced secrets.
7. iwaya creates the child-process environment.
8. The selected backend executes the real command.
9. The secret exists only within the authorized child-process execution boundary.
```

Example conceptual mapping:

```txt
command: gh
args: ["pr", "list"]
backend: local

matched policy result:
  GH_TOKEN <- bws://github/read-only-token
```

### Explicitly Denied Command

```txt
1. The user runs a managed command.
2. The policy evaluator finds an explicit deny rule.
3. iwaya does not resolve any secret.
4. iwaya does not execute the real command.
5. iwaya returns a non-secret diagnostic describing the denial.
```

### Unmatched Command

```txt
1. The user runs a command for which no policy matches.
2. iwaya resolves the real command and selected backend.
3. iwaya does not resolve any managed secret.
4. iwaya executes the command without managed secret injection.
```

## Secret Lifecycle

The intended lifecycle is:

```txt
external secret manager
  -> resolve after policy authorization
  -> hold only as required for child-process construction
  -> inject into the child process
  -> release iwaya-owned references after process setup or completion
```

Raw secret values must not be written to:

- repository files
- iwaya configuration
- logs or diagnostics
- persistent caches
- shell history
- the managed-session environment
- command-specific persistent login state created by iwaya

Process-local injection reduces exposure but cannot control what the receiving process does after injection.

## Security Boundary

iwaya is a mitigation boundary, not a sandbox.

It is designed to reduce:

- persistent secret storage
- session-wide secret exposure
- unnecessary secret delivery
- manual credential selection errors

It does not guarantee containment of malicious commands or scripts.

After a child process receives a secret, that process may print, save, forward, or leak it. Stronger protection must come from additional layers such as operating-system permissions, container or virtual-machine isolation, sandboxing, narrowly scoped credentials, secret-manager authorization, and code review.

A backend may provide isolation, but backend isolation is not guaranteed by iwaya core.

## Architectural Invariants

Implementations must preserve the following invariants:

1. iwaya does not persist raw secret values.
2. iwaya does not expose a secret retrieval API.
3. managed-session environments do not contain command secrets.
4. injection mappings are produced by policy, not supplied as authorization facts by the command invocation.
5. explicit deny takes precedence over allow.
6. no-match execution receives no managed secrets.
7. injected secrets are scoped to the authorized child-process execution.
8. execution backends do not redefine core policy semantics.
9. iwaya is not represented as a sandbox.

## Related Work

- Issue #2 defines and tracks this architecture.
- Issue #10 defines top-level and subcommand help behavior after adopting `iwaya` as the default session entrypoint.
