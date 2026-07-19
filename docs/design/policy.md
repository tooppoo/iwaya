# Policy Reference

This document defines how iwaya classifies a command, how it authorizes an invocation, and how an authorized invocation obtains its secrets.

It defines semantics only. The concrete configuration syntax and the argument pattern language are derived from the implementation and are documented in the generated configuration reference, so they are not restated here.

The durable decisions behind these semantics are recorded in:

- [Define iwaya as a Policy-Aware Command Proxy](../adr/20260702T005400Z_policy-aware-command-proxy.md)
- [Distinguish Managed Commands from Unmanaged Pass-Through Commands](../adr/20260719T024911Z_managed-commands-default-deny.md)

## Terminology

These terms are used consistently across the project, and diagnostics should follow them:

- **managed command**: a command registered for iwaya policy management
- **unmanaged command**: a command not registered for iwaya policy management
- **policy no-match**: a managed-command invocation for which no allow or deny rule matches
- **unmanaged pass-through**: execution of an unmanaged command without managed policy or secrets

The term `no-match pass-through` must not be used for managed commands. For a managed command, no match is a denial, not a fallback.

## Classification Precedes Authorization

iwaya classifies a command as managed or unmanaged before evaluating any policy rule.

A command is managed when it is registered as a managed command in the active configuration. Registration is what establishes the management boundary: once a command name is registered, every invocation of it that enters iwaya must be evaluated by that command's policy.

A command is unmanaged when it is not registered. An unmanaged command passes through without managed policy evaluation, secret resolution, or secret injection.

These two cases are materially different, and conflating them weakens the meaning of management. Unmanaged pass-through means iwaya has no management claim over the command. It is not an authorization result, and it must not be reported as one.

## Managed Commands Are Default-Deny

A managed-command invocation has exactly two authorization outcomes: `Allow` or `Deny`.

Evaluation follows these rules:

1. A matching explicit deny rule takes precedence over any matching allow rule.
2. A matching allow rule authorizes execution, and may define an injection mapping.
3. If no allow rule matches, the invocation is denied.
4. Secret resolution occurs only after an allow result has been determined.
5. A denied invocation does not execute the target command.

Rule 3 is the default-deny property. An unexpected subcommand, a typo, or a rule that was never written fails closed rather than executing without managed secrets. Failing to receive an iwaya-managed secret is not the same as being unauthorized, because the process may hold credentials acquired outside iwaya.

An allow rule without an injection mapping is meaningful: it authorizes execution without managed secrets. This is distinct from unmanaged pass-through, because the invocation was evaluated and permitted.

## Matching Inputs

Policy is evaluated against the resolved command invocation described in [the command invocation model](command-proxy.md#command-invocation-model). The available matching dimensions are:

- the resolved command name
- the argument pattern
- the current working directory or repository context
- the execution backend kind
- the requested profile or policy scope, when explicitly provided

## Injection Mappings Are Produced, Not Requested

A policy outcome may specify an allow or deny result, the environment variable names to inject, and the secret source references that supply their values.

The injection mapping is produced by the matched policy. It is not part of the command invocation, and an invocation cannot request one.

This separation exists to avoid a circular authorization model, in which the secret injection being requested would itself determine whether that injection is authorized. Authorization must be derived from what the invocation *is*, never from what it *asks for*.

## Outcome Summary

```txt
unmanaged command
  -> pass through without managed secrets

managed command + deny match
  -> deny

managed command + allow match
  -> execute, with optional authorized injection

managed command + no allow match
  -> deny
```

## Semantics Are Independent of the Frontend

Classification and managed-command authorization must not depend on which frontend supplied the invocation.

In v0 every command supplied through `iwaya exec --` is classified by iwaya: unmanaged commands pass through, and managed commands require an allow match. A future session frontend might route only registered managed commands into iwaya and let the shell execute the rest directly. That is a difference in routing, not in authorization.

The constraint is that the same command must never become authorized or denied merely because it entered iwaya through a different mode.

## Diagnostics

A denial must be distinguishable by cause. A user needs to know whether an invocation was refused by an explicit deny rule or by the absence of an allow match, because the two call for different configuration changes.

Diagnostics must describe the outcome without disclosing resolved secret values, as required by [the security model](security-model.md#secret-lifecycle).

## Related Documents

- [Command Proxy Architecture](command-proxy.md) describes where policy evaluation sits in the execution flow.
- [Security Model and Limitations](security-model.md) describes what authorization does and does not guarantee.
