# Command Proxy Architecture

## Purpose

iwaya is a policy-aware command proxy for running ordinary development commands with narrowly scoped secrets.

The design goal is to let a user run a familiar command while avoiding persistent raw secrets in the shell, container, repository, or command-specific login state. A secret should exist only for the invocation that policy authorized to receive it.

iwaya is not a secret manager and is not a sandbox. See [Security Model and Limitations](security-model.md) for the boundary this architecture claims.

The durable decisions behind this architecture are recorded in:

- [Define iwaya as a Policy-Aware Command Proxy](../adr/20260702T005400Z_policy-aware-command-proxy.md)
- [Treat iwaya as a Mitigation Boundary, Not a Sandbox](../adr/20260710T170955Z_mitigation-boundary-not-sandbox.md)
- [Use Explicit Command Execution for v0](../adr/20260718T211800Z_explicit-command-execution-for-v0.md)
- [Distinguish Managed Commands from Unmanaged Pass-Through Commands](../adr/20260719T024911Z_managed-commands-default-deny.md)
- [Implement iwaya in Go](../adr/20260719T024300Z_implements-iwaya-in-go.md)

## Scope

This document defines the structure of the command proxy: its components, their responsibilities, and the flow from an explicit invocation through to child-process execution.

Authorization semantics are defined in [the policy reference](policy.md). The security boundary and secret lifecycle are defined in [the security model](security-model.md). This document does not restate either.

It also does not define the configuration file syntax, the argument pattern language, or the `iwaya --help` contract. Those are derived from the implementation and belong in generated references.

## Entry Point

v0 has one command-proxy entrypoint:

```sh
iwaya exec -- <command> [args...]
```

The `--` separator is required. Arguments before `--` belong to iwaya, and the command name and all arguments after `--` belong to the target command.

Only a command passed through `iwaya exec --` enters iwaya's execution boundary. See [what iwaya does not protect against](security-model.md#what-iwaya-does-not-protect-against) for what that excludes.

v0 deliberately provides no managed shell session, no `PATH` shims, and no automatic command interception. The explicit prefix makes iwaya-mediated execution visible at the call site, and it keeps executable resolution unchanged. A session mode may be reconsidered later as a frontend over this same core, which is why authorization semantics are defined independently of the frontend.

## High-Level Structure

```txt
user
  |
  | iwaya exec -- gh pr list
  v
iwaya core
  1. resolve command invocation
  2. classify: managed or unmanaged
  3. evaluate policy for managed commands
  4. resolve only the authorized secrets
  5. construct the execution plan
  |
  v
execution backend
  |
  v
real child process
  - receives only policy-authorized injected secrets
```

## Components

### iwaya Core

iwaya core coordinates classification, policy evaluation, secret resolution, and backend execution.

Its responsibilities are to resolve the command invocation, classify the command, evaluate policy, determine the injection mapping produced by the matched policy, select an execution backend, resolve referenced secrets through configured integrations, construct the child-process environment, execute the real command, and redact secrets from logs and diagnostics.

It must not expose a general-purpose API or command that prints, exports, or returns raw secrets.

### Policy Evaluator

The policy evaluator receives a resolved command invocation and returns an authorization outcome, according to [the policy reference](policy.md).

Its architectural role is to be the sole authority on authorization. No other component may decide, infer, or reinterpret that outcome, and no frontend or backend may alter it.

### Secret Resolver

The secret resolver obtains values from an external secret manager or configured provider integration, and only after policy has authorized injection.

The ordering is a requirement rather than an optimization. See [the secret lifecycle](security-model.md#secret-lifecycle) for why.

Responsibilities that remain with the external secret manager, and the constraints on holding a resolved value, are described in [the security model](security-model.md#division-of-responsibility).

### Execution Backend

An execution backend performs the real command execution in a target environment. Expected backend kinds include local process execution and container execution, beginning with Dev Container integration.

Each backend receives the resolved real command, the argument list, the working directory and relevant repository context, and the child-process environment after authorized injection.

Backends are extension points. They must not redefine policy semantics, and they must preserve the child-process-only injection boundary — an injected secret must never be promoted into a persistent backend-wide or session-wide environment.

Backend integration types must stay inside the adapter. Docker and Dev Container SDK types must not become the authoritative domain model.

## Command Invocation Model

Policy is evaluated against the command invocation resolved immediately before delegation to an execution backend.

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

The injection mapping is not part of the command invocation. It is produced by the matched policy, for the reason given in [the policy reference](policy.md#injection-mappings-are-produced-not-requested).

## Execution Flow

### Managed Command, Allowed with Injection

```txt
1. The user runs `iwaya exec -- <command> [args...]`.
2. iwaya resolves the command invocation.
3. The command is registered, so it is classified as managed.
4. The policy evaluator finds a matching allow rule.
5. The matched policy produces an injection mapping.
6. iwaya resolves only the referenced secrets.
7. iwaya constructs the child-process environment.
8. The selected backend executes the real command.
9. The secret exists only within the authorized child-process execution.
```

Example conceptual mapping:

```txt
command: gh
args: ["pr", "list"]
backend: local

matched policy result:
  GH_TOKEN <- bws://github/read-only-token
```

### Managed Command, Denied

The flow is the same whatever the cause of the denial, which [the policy reference](policy.md#managed-commands-are-default-deny) defines:

```txt
1. The user runs a managed command.
2. The policy evaluator returns Deny.
3. iwaya does not resolve any secret.
4. iwaya does not execute the real command.
5. iwaya returns a non-secret diagnostic that distinguishes the denial causes.
```

### Unmanaged Command, Pass-Through

```txt
1. The user runs a command that is not registered as managed.
2. iwaya classifies it as unmanaged and does not evaluate policy.
3. iwaya does not resolve any managed secret.
4. iwaya executes the command without managed secret injection.
```

## Process Behavior

The proxy is transparent to the caller apart from secret injection. Execution must preserve the target's exit status, standard streams, signal behavior, and the interactive behavior supported by the selected backend.

A user should be able to wrap an existing command in `iwaya exec --` without changing how the surrounding script or terminal behaves.

## Architectural Invariants

These are the structural invariants. They constrain how components are arranged and what may flow between them, rather than restating the authorization rules themselves.

1. iwaya does not persist raw secret values.
2. iwaya does not expose a secret retrieval API.
3. Classification into managed or unmanaged precedes policy evaluation.
4. The policy evaluator is the sole authority on authorization, under the rules defined in [the policy reference](policy.md#managed-commands-are-default-deny). Every other component treats its outcome as given.
5. Injection mappings are produced by policy, not supplied as authorization facts by the command invocation.
6. Secret resolution is reachable only from an allow outcome.
7. Injected secrets are scoped to the authorized child-process execution.
8. Execution backends do not redefine core policy semantics.
9. Authorization semantics do not depend on the frontend that supplied the invocation.
10. iwaya is not represented as a sandbox.

## Related Work

- Issue #2 defines and tracks this architecture.
- Issue #3 tracks the explicit execution entrypoint.
- Issue #10 defines top-level and subcommand help behavior.
