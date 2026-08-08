# Implement iwaya in Rust

- Status: Accepted
- Created: 2026-08-08T17:17:32Z

## Context

[Implement iwaya in Go](20260719T024300Z_implements-iwaya-in-go.md) chose Go when iwaya was expected to grow a container execution backend of its own. The work it anticipated was Dev Container discovery, Docker Engine API compatibility, process creation inside a running container, stdio attachment, TTY allocation and resize, signal handling, and exit-status propagation. Go was selected because an official Docker Go SDK covers that surface and because Go's process and concurrency APIs suit stream handling. The ADR said so directly: growth in Docker and container integration complexity was a reason to keep Go, not to leave it.

[Define iwaya as a Docker-Context Secret Injection Runner](20260806T192918Z_docker-context-secret-injection-runner.md) removed that work. It recorded that the Go decision was unaffected, but the design it established leaves iwaya with no Engine API client, no Dev Container integration, and no stream or TTY implementation of its own. iwaya constructs an argv for a Docker-compatible executable, sets the resolved secrets in that process's environment, starts it, passes stdio through, and exits with its status. The execution flow is linear:

```text
load configuration
  -> validate and compile
  -> select context and command policy
  -> resolve declared secrets
  -> construct runtime argv and environment
  -> execute the Docker-compatible runtime
```

Container integration is now a single process launch against a command-line interface, which every candidate language can perform from its standard library. The advantage that decided the previous ADR is no longer an advantage the implementation can use.

What remains is a set of values that must not be confused with one another:

- provider identifier
- context identifier
- command identifier
- secret identifier and secret name
- resolved secret value
- environment variable name
- diagnostic string
- runtime argv element

Almost all of them are strings at run time, they are passed through the same linear flow, and the cost of confusing two of them is not uniform. Substituting a context identifier for a command identifier produces a validation error that a test will catch. Letting a resolved secret value reach a diagnostic string or an argv element breaches the boundary the Docker-context ADR states, and does so silently, in exactly the code paths that run when something has already gone wrong.

This decision needs an ADR because the previous ADR's own reconsideration clause is not what triggered it. That clause asked whether the core domain model had become hard to express in Go, and explicitly refused Docker complexity as grounds for a change. What happened instead was unenumerated: the Docker integration disappeared, and with it the reason Go was preferred. The comparison is therefore made from zero rather than argued against the old clause. No implementation exists yet, so the change costs documentation only and no code is migrated.

This ADR supersedes:

- [Implement iwaya in Go](20260719T024300Z_implements-iwaya-in-go.md)

## Decision

iwaya must be implemented in Rust.

The main executable, configuration loading and validation, provider integrations, secret resolution, argv and environment construction, and process execution must be written in Rust unless a later ADR supersedes this decision.

The values listed above must be represented by distinct types rather than by a shared string type with a naming convention. Where a value has a closed set of forms, such as the context type or the provider kind, it must be represented by an enum, so that adding a form surfaces every site that must handle it.

### The reason is the domain boundary, not performance

Rust must not be justified by execution speed, memory use, or startup time. iwaya's cost at run time is dominated by the provider call and by the container process it starts, and neither is affected by the language of the wrapper. If this decision is revisited, performance must not be presented as the reason it was made.

### The resolved secret value is the boundary that motivates the choice

A resolved secret value must be represented by a type that does not implement the ordinary display and debug formatting traits. Placing it in a diagnostic, a log line, an error message, or an argv element must be a compile error rather than something a reviewer is expected to notice.

Reading the underlying value must require an explicit, named operation, and the call sites must remain few enough to enumerate. Under the current design there is one: setting the environment of the Docker-compatible runtime process.

The Docker-context ADR already forbids a raw secret in the runtime argv, in logs, in configuration, in caches, and in any output mode. That prohibition should be carried by the type, so that it does not have to be restated and re-checked at every site that handles a secret.

### The Docker-compatible runtime is invoked as a command

iwaya must invoke the runtime as an executable, as the Docker-context ADR requires, and must not adopt a Docker or Podman client library. The Engine API is not part of iwaya's model, and the absence of an official Rust SDK for it is therefore not a cost this decision incurs.

If a future decision needs the Engine API, that is a change to the execution model and requires its own ADR. It must not be derived from the language choice.

## Non-Goals

Adopting Rust must not be read as introducing or requiring:

- an asynchronous runtime
- a generic execution framework or backend abstraction
- a plugin system
- a Docker or Podman SDK crate
- a secret lifecycle abstraction with leases, caching, expiration, or rotation

The initial implementation should stay close to the standard library, and should add a dependency when a concrete need appears rather than in anticipation of one. The CLI parser, the KDL parser, the secret wrapper, the process execution API, and the module structure are decided in the implementation issues, not here.

## Alternatives Considered

### Remain in Go

Go remains a reasonable language for this program, and keeping it would avoid writing this ADR at all.

It was not selected because the reason it was chosen no longer applies, and because the work that is left is the work Go supports least directly. The values above would be separated by unexported types, constructors, package boundaries, and tests, which is the arrangement the Go ADR itself prescribed and recorded as a negative consequence. A secret wrapper in Go can redact its own formatting, but the underlying string stays reachable within its package, every type has a usable zero value, and no arrangement makes misuse a compile error across the whole program. Since no Go code exists, retaining Go would preserve a decision rather than an investment.

### Use a managed-runtime language such as TypeScript or Python

Both have mature ecosystems and would express this linear flow with little ceremony.

Neither was selected because the type boundary does not survive into execution. TypeScript's branded types are erased at compile time, and a secret wrapper is interpolated into a template string or a log call without complaint. Python's annotations are not enforced unless a separate checker runs. Both would also require iwaya to ship a runtime or a bundled interpreter for a command whose entire job is to start another process.

### Use Zig or C

Both produce a small self-contained binary and would invoke the runtime command directly.

Neither was selected. Neither offers the type-level distinctions this decision is made for, and both add manual memory management as a defect class unrelated to the problem. The libraries iwaya needs for configuration parsing and provider integration are also thinner there than in Rust.

### Implement a Rust core with a Go helper for container work

The Go ADR held this open as a reconsideration candidate, so it was re-examined here.

It was not selected because there is nothing left for the helper to do. Its purpose was to hold the Docker Go SDK and the stream and TTY handling, and the Docker-context ADR removed both. A second toolchain, a cross-language protocol, and a second distribution artifact would be introduced to wrap one `exec` invocation.

## Consequences

### Positive Consequences

- Passing a resolved secret to a formatter, a logger, or an argv element can be made a compile error rather than a review finding.
- The identifiers in the configuration model can be distinguished by type instead of by naming convention.
- Adding a context type or a provider kind surfaces every site that must handle it, rather than relying on a default branch to stay correct.
- iwaya ships as a self-contained binary, with no runtime for the user to install.
- The language decision now rests on the current design rather than on a superseded one.

### Negative Consequences

- Rust asks more of a contributor than Go does, and reading the code requires familiarity with ownership and borrowing.
- Compile times are longer than Go's, which slows the edit-and-test loop for a program this small.
- The crate ecosystem for what iwaya needs, including KDL parsing and secret provider clients, is smaller than the Go equivalent, and some provider integrations may have to be written against a CLI or an HTTP API rather than an official SDK.
- Ownership and borrowing add friction to code that is otherwise a straight line, and this decision accepts that friction for the sake of the boundaries above.
- If the Engine API is ever required, there is no official Rust SDK, and the previous ADR's argument for Go would become relevant again.

### Neutral Consequences

- The Go ADR is kept as history. Its status changes, and its rationale and decision are left as written.
- No code is migrated, because none was written under the previous decision.
- The specific crates, module structure, and process execution API remain undecided.
- This decision does not change the CLI contract, the configuration model, or the security boundary, all of which stay as the Docker-context ADR states them.
