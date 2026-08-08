# Define iwaya as a Docker-Context Secret Injection Runner

- Status: Accepted
- Created: 2026-08-06T19:29:18Z

## Context

iwaya was previously defined as a policy-aware command proxy. That model accepted an arbitrary command invocation, classified it as managed or unmanaged, and evaluated allow and deny rules against the command name, an argument pattern, the repository context, and the execution backend kind. Secrets were injected only when a matching allow rule produced an injection mapping.

That model carries an authorization surface iwaya cannot honor. Deciding whether an arbitrary invocation may run requires reasoning about what the target program will do with the arguments it receives, and iwaya does not analyze target-CLI semantics. Argument-pattern rules therefore look like an access-control mechanism while providing only pattern matching. The classification vocabulary compounds the problem: unmanaged pass-through is not an authorization result, yet it appears in the same evaluation flow as allow and deny, so the absence of a management claim reads as a permission.

The behavior users actually need is smaller. Development containers are where the credentials are wanted, and the host is where the secret provider is reachable. A user picks a container to work in, picks a command that needs a credential, and expects that credential to exist inside the container for that one execution and nowhere else. The alternative most users fall back to — `gh auth login` inside the container, or a `.env` file mounted into it — leaves the credential behind after the command finishes.

Two things vary independently in that workflow. The container varies because a developer works across several checkouts at once, and the command varies because different tools need different credentials. Fixing the pair in configuration would force a definition per combination, which grows multiplicatively and duplicates container settings across entries that differ only in which secret they carry.

Container execution is therefore not an incidental backend that happens to be available. It is the thing the model is about, and treating it as one implementation of a generic execution abstraction adds an extension point that v0 has no second implementation for.

This decision needs an ADR because it changes iwaya's architectural identity, its CLI contract, the scope of what it will execute, its configuration model, its secret-delivery mechanism, and the wording of its security boundary.

Related issue:

- #17

Related ADRs:

- [Treat iwaya as a Mitigation Boundary, Not a Sandbox](20260710T170955Z_mitigation-boundary-not-sandbox.md) remains accepted, and this ADR does not supersede it. Restricting execution to configured contexts and command policies does not make iwaya a sandbox, so its boundary, its non-goals, and its requirement for complementary layers all continue to apply. Of the mitigations that ADR enumerates, this decision replaces the pass-through of unmatched commands, the explicit deny rules for command patterns, and the description of injection as reaching only the selected child process; the rest remain in force.
- [Implement iwaya in Go](20260719T024300Z_implements-iwaya-in-go.md) is unaffected by this decision.

This ADR supersedes:

- [Define iwaya as a Policy-Aware Command Proxy](20260702T005400Z_policy-aware-command-proxy.md)
- [Use Explicit Command Execution for v0](20260718T211800Z_explicit-command-execution-for-v0.md)
- [Distinguish Managed Commands from Unmanaged Pass-Through Commands](20260719T024911Z_managed-commands-default-deny.md)

## Decision

iwaya is a Docker-context secret injection runner. It executes a configured command inside an explicitly selected Docker-compatible container, and delivers to that execution only the secrets the selected command policy declares.

iwaya must not be a policy-aware arbitrary command proxy, and must not be described as one.

### Both the context and the command are selected explicitly

The invocation form is:

```sh
iwaya exec --context <context> <command> -- [args...]
```

For example:

```sh
iwaya exec --context iwaya claude
iwaya exec --context git-kura claude -- --resume
```

`--context` must be required. iwaya must not infer a context from the working directory, the repository, or a default entry, because a silently chosen container would decide where a credential is delivered.

The command operand must name a configured command policy. An unknown context name and an unknown command name must each produce an error, and must not execute anything or resolve any secret.

iwaya must not provide arbitrary command proxying or unmanaged pass-through. There is no execution path for a command that no policy defines, so the managed and unmanaged classification, the allow and deny rules, policy no-match, the argument-pattern evaluator, and repository-context authorization are all removed rather than replaced.

Arguments for the target command must be separated by `--`. Everything after `--` must be appended unchanged to the end of the target command, and iwaya must not interpret it. An invocation must not be able to name a provider, a secret name, or an environment mapping.

### Configuration has three independent layers

Configuration must be divided into secret providers, Docker execution contexts, and command policies. Each layer answers one question: a provider defines how a secret is obtained, a context defines where a command runs, and a command policy defines which secrets a command receives.

```kdl
/- kdl-version 2

iwaya version=1 {
  providers {
    bws "bws-default" {
      project "philomagi.dev"
    }
  }

  contexts {
    docker "iwaya" {
      runtime "podman"
      user "vscode"
      workdir "/workspaces/iwaya"
      container-name "iwaya-dev"
    }
  }

  policies {
    command "claude" {
      secret \
        "ANTHROPIC_AUTH_TOKEN" \
        provider="bws-default" \
        secret-name="ANTHROPIC_AUTH_TOKEN"
    }
  }
}
```

The three layers sit under an `iwaya` root node carrying a configuration `version`, which is what makes the version available to validate before anything below it is read.

A provider must not select contexts or commands. A context must not declare secrets. A command policy must not declare container settings. Keeping the layers separate is what allows one container definition and one command definition to be written once each rather than once per combination.

v0 must define exactly one context type, `docker`, meaning the Docker-compatible `exec` model. Its `runtime` must default to `docker` and may be set to another Docker-compatible command such as `podman`. iwaya must treat `runtime` as a single executable, and must not evaluate it as a shell command string.

A command policy's identifier must be both the name used at the invocation and the command name executed inside the container. The declared environment variable names, together with the provider and secret name supplying each, must be the complete injection mapping.

### The context and the command combine at run time

The pairing of a context with a command policy must be decided by the invocation, not fixed in configuration. Every configured context must be usable with every configured command policy.

v0 must not provide per-context command restrictions. A restriction of that kind would read as an authorization rule, and iwaya would be trusted for a containment property it does not provide. What a user may run in a container is already governed by the container itself and by the credentials the secret provider will release to them.

### Validation precedes secret resolution, and resolution precedes execution

iwaya must validate the configuration and the selected entries before resolving any secret. Validation must cover at least the configuration root and version, the uniqueness of provider, context, and command identifiers, the existence of the selected context and command policy, the required fields of the Docker context, the uniqueness of environment variable names within a command policy, and the existence of every provider a command policy references.

If validation fails, iwaya must not resolve a secret and must not execute the runtime command. Retrieval itself is observable to the provider, so a request that will not be used must not be made.

After validation succeeds, iwaya must resolve every secret the selected command policy declares, and only those. If any one of them fails to resolve, iwaya must not execute the runtime command.

### Injection forwards variable names, never values

iwaya must set the resolved secrets in the environment of the Docker-compatible runtime process it starts, and must forward them into the container by name.

For the invocation `iwaya exec --context iwaya claude -- --resume`, and the configuration above, the constructed argv is:

```text
podman
exec
--interactive
--tty
--env
ANTHROPIC_AUTH_TOKEN
--user
vscode
--workdir
/workspaces/iwaya
iwaya-dev
claude
--resume
```

The construction rule is:

```text
runtime
+ exec
+ --interactive / --tty
+ --env <environment-name> for each secret in the command policy
+ --user <user>
+ --workdir <workdir>
+ <container-name>
+ <command>
+ user arguments
```

iwaya must use the `--env NAME` form and must never use `--env NAME=VALUE`. A raw secret value must not appear anywhere in the runtime argv, where it would be visible to any process that can read the host process table.

iwaya must not generate an `--env` option for an environment variable that the selected command policy does not declare.

v0 targets interactive development CLIs, so iwaya must always pass `--interactive` and `--tty`. Making TTY and stdin behavior configurable is deferred until a concrete need appears.

### Policy-managed variables override the parent environment

When a policy-managed environment variable name already exists in the invoking environment, the value resolved from the provider must overwrite it. A policy-managed variable must never inherit the same-named value from the parent environment.

When secret resolution fails, iwaya must not fall back to the same-named value in the invoking environment, and must not execute the command. Silent fallback would run the command against a credential the policy did not select, which is harder to diagnose than a failure and may be a different principal entirely.

A diagnostic may identify the provider identifier, the secret name, or the environment variable name that failed. It must not contain a raw secret value.

### Execution is transparent apart from injection

iwaya must pass stdin, stdout, and stderr through to the runtime process, must exit with that process's exit status, and must forward signals to it as far as the platform and the container runtime allow.

Apart from secret injection and the forced `--interactive` and `--tty` behavior, a caller should observe no difference.

### The boundary that does not change

iwaya remains a mitigation layer and not a sandbox, as recorded in [Treat iwaya as a Mitigation Boundary, Not a Sandbox](20260710T170955Z_mitigation-boundary-not-sandbox.md).

Configuration establishes where secrets may be delivered. It does not replace the secret provider, which remains responsible for deciding whether the invoking user may retrieve a given secret. Both must permit a delivery for it to happen.

iwaya must not export a raw secret to the invoking shell, must not persist a raw secret in its configuration, logs, caches, or credential state, and must not expose an API, subcommand, or output mode that returns a raw secret value.

A delivered secret is inherited by the runtime process, by the process inside the container, and by that process's descendants, under ordinary operating-system and container-runtime rules. iwaya must not be described as confining a secret to a single process. It does not prevent a command that received a secret from printing, storing, forwarding, or leaking it.

## Non-Goals

This decision does not:

- implement `iwaya exec`, the configuration parser, the validation rules, or any provider
- introduce a local process context or a generic execution backend abstraction
- support container technologies other than Docker-compatible runtimes
- support container ID selection, variables, interpolation, or runtime-specific option pass-through
- separate the command identifier from the executable run inside the container
- define a noninteractive execution mode or make TTY and stdin behavior configurable
- reintroduce allow and deny rules, an argument pattern language, or repository-context authorization
- reintroduce an iwaya-managed shell session, session-scoped or global `PATH` shims, or automatic command interception, all of which remain incompatible with requiring an explicit context and command at the call site
- define a workflow engine, including pipelines, sequential execution of multiple commands, and conditional branching
- sandbox the process that receives a secret
- make iwaya a secret manager or a general-purpose secret retrieval tool

## Alternatives Considered

### Keep the policy-aware command proxy and narrow the policy language

Under this model, iwaya would continue to accept arbitrary invocations, and the argument-pattern language would be restricted until it could be reasoned about reliably.

This was not selected because the difficulty is not the expressiveness of the pattern language. Authorizing an arbitrary invocation requires knowing what the target program will do with its arguments, and no pattern language supplies that. Restricting the syntax would leave a mechanism that still reads as access control while providing only matching.

### Define named execution recipes that fix the context and the command together

Under this model, one configuration entry would name a command, its arguments, its container, and its secrets, and the invocation would select that entry by name.

This was not selected because the container and the command vary independently. A developer working across several checkouts would need one entry per container and command pair, and each entry would repeat the container settings. Separating contexts from command policies keeps each fact written once, and makes the pairing an explicit choice at the call site rather than a configuration artifact.

### Model container execution as one execution backend among several

Under this model, iwaya would keep a backend abstraction, and Docker execution would be a backend kind alongside local process execution.

This was not selected because v0 has no second backend to justify the abstraction. A generic context model would also have to reduce Docker's execution parameters to a lowest common denominator, and `user`, `workdir`, and `container-name` have no local equivalent. A local context can be added later as its own decision, with its own field set.

### Let the invocation select secrets, or let a context restrict commands

Under the first variant, a flag would choose which secrets to inject. Under the second, a context would list the commands permitted in it.

Neither was selected. Choosing secrets at the invocation returns iwaya to deciding whether a requested injection is permitted, which is the authorization problem this decision removes. A per-context command restriction presents as containment, and iwaya cannot enforce it: the same container is reachable through the runtime directly, without iwaya.

### Pass secrets with `--env NAME=VALUE`

Under this model, iwaya would place resolved values directly in the runtime argv, which is the shortest path from a resolved secret to a container environment.

This was rejected because the value would then be visible to any process that can read the host process table, and would be captured by any shell history, process accounting, or audit tooling that records argv. Setting the value in the runtime process environment and forwarding only the name costs nothing and avoids that exposure.

## Consequences

### Positive Consequences

- Where a secret may be delivered is fixed in reviewable configuration, and which container receives it is visible at the call site.
- iwaya no longer presents an authorization mechanism whose guarantees it cannot honor.
- Containers and commands are each defined once, so adding a checkout or a credential-using tool is a single entry rather than a combination.
- The vocabulary shrinks: managed and unmanaged commands, policy no-match, unmanaged pass-through, and execution backends are gone rather than redefined.
- Restricting v0 to the Docker-compatible `exec` model keeps the configuration fields concrete rather than reduced to a common denominator.
- Forwarding names rather than values keeps raw secrets out of the host process table.

### Negative Consequences

- Every command a user wants to run through iwaya must have a command policy, and every container must have a context, which is a larger configuration step than wrapping an ad-hoc command.
- `--context` on every invocation is verbose, and the correct context is not inferred even when the working directory makes it obvious.
- Users without a container workflow cannot use v0 at all, because no local context exists.
- Any configured command can be run in any configured container, so a context does not limit which credentials can be carried into it.
- Always passing `--interactive` and `--tty` makes v0 unsuitable for CI and other noninteractive callers.
- Overwriting a policy-managed environment variable can surprise a user who intentionally exported that variable before invoking iwaya.

### Neutral Consequences

- `iwaya exec` remains the entrypoint name, but it now takes a context and a configured command rather than an arbitrary command line.
- Defaulting `runtime` to `docker` while allowing `podman` treats Podman as a compatible command rather than a separate integration.
- Restricting v0 to Docker-compatible execution does not rule out a local context later.
- The configuration syntax shown here is a baseline example; the parser and its diagnostics are decided separately.
