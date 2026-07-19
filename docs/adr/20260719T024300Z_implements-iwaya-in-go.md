# Implement iwaya in Go

* Status: Accepted
* Created: 2026-07-19T02:00:08Z

## Context

iwaya is a policy-aware command proxy that evaluates a resolved command invocation, determines whether execution is denied or allowed, resolves only the authorized secrets, and delegates the resulting execution plan to a selected backend.

Related architectural decisions are recorded in:

* [Define iwaya as a Policy-Aware Command Proxy](20260702T005400Z_policy-aware-command-proxy.md)
* [Treat iwaya as a Mitigation Boundary, Not a Sandbox](20260710T170955Z_mitigation-boundary-not-sandbox.md)
* [Use Explicit Command Execution for v0](20260718T211800Z_explicit-command-execution-for-v0.md)

The v0 implementation must support Linux and WSL2. It must support local command execution and is expected to support container-based execution, beginning with Dev Container integration.

Container execution introduces substantial integration complexity, including:

* Dev Container discovery and configuration resolution
* Docker Engine API compatibility
* process creation inside an existing container
* process-local environment injection
* stdin, stdout, and stderr attachment
* TTY allocation and resize handling
* signal and cancellation behavior
* target exit-status propagation

Go has strong standard-library support for process execution and concurrency, and Docker provides an official Go SDK. These properties reduce the implementation and maintenance risk of the expected container backend.

Rust was also considered because its enums, ownership model, and type system can express closed domain states and secret lifecycles more strictly. However, the currently expected iwaya core is primarily a linear pipeline:

```text
load and compile configuration
→ resolve command invocation and backend
→ evaluate policy
→ resolve authorized secrets
→ construct execution plan
→ execute through the selected backend
```

The current domain model is not expected to require enough state-machine or type-level complexity to outweigh the integration advantages of Go.

A hybrid implementation was also considered, in which a Rust core delegates Docker operations to a Go helper. Such an architecture is technically possible, but it would introduce a second toolchain, a cross-language protocol, multiple binaries or an FFI boundary, duplicated release concerns, and additional integration tests.

## Decision

iwaya must be implemented in Go.

The main executable, core domain model, policy evaluator, secret resolution orchestration, local execution backend, and initial container execution backends must use Go unless this decision is superseded by a later ADR.

The implementation should use explicit package and type boundaries to preserve domain invariants. In particular, the implementation must distinguish at least:

* raw decoded configuration from validated and compiled configuration
* resolved command invocations from raw CLI arguments
* policy decisions from execution results
* authorization from secret resolution
* execution plans from backend-specific SDK request types
* secret values from ordinary diagnostic strings
* core domain types from Docker or Dev Container integration types

Docker and Dev Container SDK types must not become the authoritative domain model. They must remain inside backend adapters.

### Docker and container complexity

Growth in Docker, Dev Container, process-streaming, TTY, signal, container-runtime, or backend integration complexity is not by itself a reason to replace Go or introduce Rust.

Such complexity is primarily integration and adapter complexity. Go should be retained when this area grows because its Docker ecosystem and process-concurrency model directly support that work.

The implementation may introduce additional Go packages, internal abstractions, or helper processes when needed to contain backend-specific complexity without changing the implementation language of the core.

### Domain-model complexity

The language choice must be reconsidered if the core domain model becomes materially difficult to represent, validate, or maintain safely in Go.

Relevant signals include, but are not limited to:

* multiple secret injection transports with different preparation, inheritance, and cleanup lifecycles
* compositional policy expressions, rule inheritance, rule merging, or complex conflict resolution
* persistent managed sessions with explicit lifecycle states
* secret leases, caching, expiration, rotation, or revocation
* privilege-separated client and broker processes
* complex backend capability negotiation that affects authorization semantics
* dynamic provider or backend plugins
* repeated reliance on invalid-state conventions that cannot be adequately contained by Go package boundaries and tests

An increase in the number of structs, packages, or backend implementations alone does not satisfy this condition. The relevant concern is growth in core invariants, state transitions, and mutually exclusive domain states.

If these conditions arise, the project must evaluate a hybrid architecture in a separate ADR.

A possible hybrid architecture is:

```text
Rust core
  ├─ configuration and domain model
  ├─ policy evaluation
  ├─ secret lifecycle orchestration
  └─ execution planning
       ↓ explicit internal protocol
Go container backend helper
  ├─ Dev Container integration
  ├─ Docker Go SDK
  ├─ process creation and attachment
  └─ TTY and stream handling
```

This is a reconsideration candidate, not a committed future architecture.

Adopting a hybrid architecture must require evidence that:

1. core domain complexity has become a material correctness or maintainability problem in Go;
2. the Rust domain-model benefits outweigh the additional protocol, build, distribution, and operational costs;
3. the Go container boundary can remain narrow and stable;
4. authorization and policy semantics remain authoritative in one module rather than being duplicated across languages.

A hybrid implementation must not be introduced solely because Rust is preferred stylistically or because an individual Docker integration is difficult.

## Alternatives Considered

### Implement all of iwaya in Rust

Rust provides stronger language-level tools for closed variants, ownership, state transitions, and secret wrappers.

This was not selected because the expected v0 domain model is moderate in complexity, while Docker and Dev Container integration are expected to be among the most technically complex parts of the implementation. Using Rust would require a community Docker client or direct Engine API implementation and would likely introduce an asynchronous runtime primarily for the container backend.

The additional type-system strength does not currently outweigh the Docker integration and maintenance costs.

### Implement a Rust core with a Go container helper from the beginning

This would preserve Rust for the domain model while using the official Docker Go SDK.

This was not selected because the current domain model does not justify:

* two implementation languages
* two dependency and vulnerability-management surfaces
* a versioned internal protocol
* additional binary packaging
* mixed-version compatibility handling
* cross-language integration testing
* duplicated diagnostic and secret-handling boundaries

The hybrid approach remains a reconsideration option if domain complexity materially increases.

### Implement all of iwaya in Go without explicit domain boundaries

This would minimize initial implementation effort.

This was rejected because iwaya handles authorization decisions and secret-bearing execution plans. Configuration models, policy outcomes, secret values, and backend SDK types must not be freely interchangeable. Go is selected together with explicit package boundaries, validated constructors, and tests that enforce domain invariants.

### Use FFI between Rust and Go

Go can expose C-compatible shared or archive libraries, and Rust can call them through FFI.

This was not selected because Docker execution involves long-lived streams, cancellation, TTY resizing, error propagation, and runtime-managed resources. An FFI boundary would introduce unsafe memory and lifecycle contracts and would effectively require a custom cross-language Docker facade.

If a hybrid architecture is later adopted, a separate-process boundary should be evaluated before FFI.

## Consequences

### Positive Consequences

* iwaya can use the official Docker Go SDK.
* Docker API compatibility and version negotiation can follow the supported Go ecosystem.
* Dev Container and container-process integration can be implemented without introducing a second language or internal protocol.
* Go's process, context, goroutine, and stream APIs fit the expected execution-backend work.
* v0 build, test, release, and vulnerability-management workflows remain single-language.
* The language choice remains compatible with both local and container execution backends.
* A future hybrid architecture is not prohibited if core domain complexity provides sufficient justification.

### Negative Consequences

* Go cannot express closed sum types and exhaustive pattern matching as directly as Rust.
* Some domain invariants must be enforced through unexported types, constructors, package boundaries, validation, and tests rather than by the language alone.
* Secret wrappers can reduce accidental disclosure but cannot provide Rust-like ownership or drop semantics.
* Careless use of booleans, nullable fields, generic maps, or backend SDK types could create invalid states if code-review discipline is insufficient.
* Migrating part of the core to Rust later would require a new protocol and a nontrivial architectural transition.

### Neutral Consequences

* Docker integration complexity is expected and does not invalidate the Go decision.
* A large codebase alone does not trigger language reconsideration.
* Rust remains a candidate for a future domain-oriented core, but no Rust module is currently planned.
* Any future hybrid architecture must be approved through a separate ADR.
* This decision does not select specific Go CLI, configuration, logging, or Docker helper libraries except where an official Docker Go SDK is relevant to the rationale.
