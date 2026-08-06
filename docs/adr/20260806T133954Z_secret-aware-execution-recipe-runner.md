# Define iwaya as a Secret-Aware Execution Recipe Runner

- Status: Accepted
- Created: 2026-08-06T13:39:54Z

## Context

iwaya was previously defined as a policy-aware command proxy. That model accepted an arbitrary command invocation, classified it as managed or unmanaged, and evaluated allow and deny rules against the command name, an argument pattern, the repository context, and the execution backend kind. Secrets were injected only when a matching allow rule produced an injection mapping.

That model carries an authorization surface that iwaya cannot honor. Deciding whether an arbitrary invocation may run requires reasoning about what the target program will do with the arguments it receives, and iwaya does not analyze target-CLI semantics. Argument-pattern rules therefore look like an access-control mechanism while providing only pattern matching. The classification vocabulary compounds the problem: unmanaged pass-through is not an authorization result, yet it appears in the same evaluation flow as allow and deny, so the absence of a management claim reads as a permission.

The behavior users actually need is smaller. A user selects a pre-configured execution recipe by name, iwaya resolves the executable and argument vector that the recipe fixed, resolves only the secrets that the recipe mapped, adds them to the child environment, executes the command, and passes through the process I/O and exit status.

Framing the tool this way removes the questions iwaya was never able to answer. There is no arbitrary invocation to authorize, because the invocation is written in advance by the recipe author. There is no injection mapping to derive, because the recipe fixes it. What remains is a delivery decision that the recipe author made explicitly, and that the secret manager still authorizes independently.

Container execution motivated the original design but does not need a place in the model. `docker exec` and `podman exec` are executables with argument vectors, so a recipe can name them the same way it names a local command or script. An execution backend abstraction and a container-specific domain model add structure that the recipe already expresses.

This decision needs an ADR because it changes iwaya's architectural identity, its CLI contract, the scope of what it will execute, its secret-delivery mechanism, and the wording of its security boundary.

Related issue:

- #17

Related ADRs:

- [Treat iwaya as a Mitigation Boundary, Not a Sandbox](20260710T170955Z_mitigation-boundary-not-sandbox.md) remains accepted, and this ADR does not supersede it. Narrowing execution to named recipes does not make iwaya a sandbox, so its boundary, its non-goals, and its requirement for complementary layers all continue to apply. This ADR does replace the individual mitigations that ADR enumerates — pass-through of unmatched commands, explicit deny rules for command patterns, and isolation contributed by an execution backend — because those mechanisms no longer exist under the recipe model.
- [Implement iwaya in Go](20260719T024300Z_implements-iwaya-in-go.md) is unaffected by this decision.

This ADR supersedes:

- [Define iwaya as a Policy-Aware Command Proxy](20260702T005400Z_policy-aware-command-proxy.md)
- [Use Explicit Command Execution for v0](20260718T211800Z_explicit-command-execution-for-v0.md)
- [Distinguish Managed Commands from Unmanaged Pass-Through Commands](20260719T024911Z_managed-commands-default-deny.md)

## Decision

iwaya is a secret-aware execution recipe runner. It executes pre-configured, named execution recipes, and injects secrets for the duration of a single execution.

iwaya must not be a policy-aware command proxy, and must not be described as one.

### Execution recipes define what may run

An execution recipe is a named definition that fixes what will be executed and which secrets will be delivered to it. A recipe defines at least a name, an executable, a fixed argument vector, whether additional arguments are accepted, and the mapping from environment variable names to secret references.

A recipe must define a command as an executable and an argument vector. iwaya must not accept a shell command string and must not start a shell implicitly to evaluate one.

The configuration syntax and field names are not decided here.

### Recipe execution is the primary entrypoint

The invocation form is:

```sh
iwaya exec <recipe> -- [additional arguments...]
```

For example:

```sh
iwaya exec claude -- --resume
```

An invocation must identify a recipe by name. An unknown recipe name must produce an error, and must not execute anything.

iwaya must not provide arbitrary command proxying or unmanaged pass-through. There is no execution path for a command that no recipe defines, so the managed and unmanaged classification, the allow and deny rules, policy no-match, the argument-pattern evaluator, and repository-context authorization are all removed rather than replaced.

An invocation must not be able to request a secret reference or an injection mapping. Only the recipe defines which secrets are delivered and under which environment variable names.

### Resolution precedes secret resolution

iwaya must resolve and validate the entire recipe before resolving any secret. A recipe that cannot be resolved or validated must fail without contacting the secret manager, because retrieval itself is observable to the secret manager or to an intermediary.

iwaya must resolve only the secret references that the selected recipe declares.

### Secret delivery is limited to the child environment

In v0, the only secret delivery mechanism is injection into the child process environment.

A raw secret value must not be expanded into the executable, the argument vector, the working directory, a non-secret interpolation, or any other recipe field. In particular, a raw secret must not be embedded in a command-line argument, where it would be visible to any process that can read the process table.

A recipe may name a transport command such as `docker exec`. What iwaya guarantees there is only the rule above: no raw secret value is expanded into an argument vector. Constructing the transport so that the value travels through the transport process environment is the recipe author's responsibility. The fixed argument vector may name the variable to forward, as `docker exec -e NAME` does, but it must not carry that variable's value.

The environment variable names declared by the recipe, together with the secret references they map to, are the complete injection mapping. Nothing else constitutes one.

### Recipe-managed variables override the invoking environment

When a recipe-managed environment variable name already exists in the invoking environment, the value resolved from the secret manager must overwrite it. A recipe-managed variable must never inherit the same-named value from the parent environment.

If secret resolution fails, iwaya must not execute the command, and must not fall back to the same-named value in the invoking environment. Silent fallback would run the command against a credential that the recipe did not select, which is harder to diagnose than a failure and may be a different principal entirely.

A diagnostic may identify the secret reference or the environment variable name that failed. It must not contain a raw secret value.

### Additional arguments are a scope decision by the recipe author

A recipe may allow additional arguments. When it does, the arguments supplied at the invocation are appended after the fixed argument vector.

When a recipe does not accept additional arguments, an invocation that supplies them must produce an error, and must not execute anything or resolve any secret. iwaya must not silently discard them, because that would run a command the user did not ask for while a secret is live.

iwaya does not analyze the target CLI's subcommand semantics or option interactions. A fixed command prefix therefore does not, in general, constrain what the target program can be asked to do. A recipe that allows additional arguments delivers its secrets to whatever the target CLI can reach through them.

Recipe authors must evaluate the fixed argument vector and the additional-argument allowance together as the secret-delivery scope they are granting.

### Execution is transparent apart from injection

iwaya must pass stdin, stdout, and stderr through to the child process, must exit with the child's exit status, and must forward signals to the child as far as the platform allows.

Secret injection is the only difference a caller should observe. A recipe is a way to run a command with the secrets it needs, not a wrapper that changes how that command behaves in a script or a terminal.

### Containers are recipe content, not a core concept

iwaya core must not recognize containers. A container command is an executable and an argument vector like any other, so local processes, `docker exec`, and `podman exec` are all expressed by the same recipe model.

iwaya must not define an execution backend abstraction or a container-specific domain model.

### Secret exposure is execution-scoped

iwaya must describe secret exposure as execution-scoped or invocation-scoped. The term `process-local` must not be used, because it suggests that exactly one process holds the secret, which is not what iwaya provides.

The accurate statement is that secrets are resolved only for the execution of the selected recipe, that the child process iwaya starts receives them, and that descendants of that child inherit them under ordinary operating-system rules. When a recipe uses a transport command, both the transport process and the destination process may receive the secret.

### The boundary that does not change

iwaya remains a mitigation layer and not a sandbox, as recorded in [Treat iwaya as a Mitigation Boundary, Not a Sandbox](20260710T170955Z_mitigation-boundary-not-sandbox.md).

A recipe definition establishes a configured execution scope and secret-delivery scope. It does not replace the secret manager, which remains responsible for deciding whether the invoking user may retrieve a given secret. Both must permit a delivery for it to happen.

iwaya must not export a raw secret to the invoking shell, must not persist a raw secret in its configuration, logs, caches, or credential state, and must not expose an API, subcommand, or output mode that returns a raw secret value.

iwaya does not prevent a command that received a secret from printing, storing, forwarding, or leaking it.

## Non-Goals

This decision does not:

- fix the configuration syntax or field names for recipes
- define a workflow engine, including pipelines, sequential execution of multiple commands, and conditional branching
- reintroduce an argument pattern language, allow and deny policy, a managed shell session, or `PATH` shims
- sandbox the process that receives a secret
- make iwaya a secret manager or a general-purpose secret retrieval tool

## Alternatives Considered

### Keep the policy-aware command proxy and narrow the policy language

Under this model, iwaya would continue to accept arbitrary invocations, and the argument-pattern language would be restricted until it could be reasoned about reliably.

This was not selected because the difficulty is not the expressiveness of the pattern language. Authorizing an arbitrary invocation requires knowing what the target program will do with its arguments, and no pattern language supplies that. Restricting the syntax would leave a mechanism that still reads as access control while providing only matching.

### Keep recipes but retain unmanaged pass-through for unrecognized commands

Under this model, an invocation that matched no recipe would execute without managed secrets, preserving the previous fallback.

This was not selected because it keeps the failure mode that motivated the change. A mistyped recipe name would run something rather than report an error, and iwaya would retain an execution path it makes no claim over. Reporting an unknown recipe is both safer and easier to diagnose.

### Allow the invocation to select secrets or injection mappings

Under this model, a recipe would define the command, and the invocation would choose which secrets to inject, for example through a flag.

This was not selected because it returns iwaya to deciding whether a requested injection is permitted, which is the authorization problem this decision removes. Fixing the mapping in the recipe keeps the delivery scope reviewable in configuration rather than negotiated per invocation.

### Deliver secrets through files or a local socket in v0

Under this model, iwaya would write secrets to a temporary file or serve them over a socket, avoiding environment inheritance by descendant processes.

This was not selected for v0 because these mechanisms introduce lifetime, permission, and cleanup requirements of their own, and because the target CLIs iwaya is built for read credentials from environment variables. Environment injection is the mechanism whose exposure is easiest to state accurately. A later decision may add another mechanism.

### Model container execution as an execution backend

Under this model, iwaya would keep a backend abstraction, and container execution would be a backend kind alongside local process execution.

This was not selected because a recipe already expresses container execution as an executable and an argument vector. A backend abstraction would add an extension point, per-backend semantics, and a container-specific domain model without changing what iwaya does.

## Consequences

### Positive Consequences

- What may run and which secrets it receives are both fixed in reviewable configuration rather than decided per invocation.
- iwaya no longer presents an authorization mechanism whose guarantees it cannot honor.
- The security boundary is stated in terms of a configured delivery scope, which is a claim iwaya can keep.
- The vocabulary shrinks: managed and unmanaged commands, policy no-match, unmanaged pass-through, and execution backends are gone rather than redefined.
- Container execution requires no core support, so local, Docker, and Podman recipes are written the same way.
- Describing exposure as execution-scoped states the descendant and transport-process inheritance that `process-local` obscured.

### Negative Consequences

- Every command a user wants to run through iwaya must be defined as a recipe first, which is a larger configuration step than wrapping an ad-hoc command.
- A recipe that allows additional arguments grants secret delivery to whatever the target CLI can reach through them, and iwaya cannot narrow that for the author.
- Ad-hoc and exploratory use is no longer supported, so a one-off command requires either a recipe or a different tool.
- Overwriting a recipe-managed environment variable can surprise a user who intentionally exported that variable before invoking iwaya.

### Neutral Consequences

- `iwaya exec` remains the entrypoint name, but its operand becomes a recipe name rather than a command.
- Restricting v0 delivery to environment injection does not rule out another mechanism later.
- Recipes are per-execution definitions; whether they compose or share configuration is left open.
