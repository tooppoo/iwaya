# v0 Scope

This document collects what is true of iwaya only for v0: the constraints that narrow the current release, the capabilities deliberately left out of it, and the decisions still open.

It exists so that the design documents can describe the durable model without version-specific qualifications running through them. Everything here is expected to change. When a constraint is lifted or a decision is settled, the entry is removed from this document, and the durable consequence, if any, moves into [the design documents](design/README.md) and a new record in [Architecture Decision Records](adr/README.md).

This document is normative for the current release. An implementation that exceeds one of these constraints is incorrect until the entry is lifted, in the same way that violating a design document is incorrect.

Nothing here weakens a design document, and nothing here is durable. Where this document and a design document appear to disagree, the design document states the model and this one states how far the current release implements it.

## Execution

v0 is limited to Docker-compatible execution. A context's `runtime` accepts `docker`, `podman`, or another command implementing the same `exec` interface, and that is the whole of the execution model. `docker` is the only context type.

Every invocation carries `--interactive` and `--tty`, because v0 targets interactive development CLIs. This makes v0 unsuitable for CI and other noninteractive callers.

The entrypoint is `iwaya exec --context <context> <command> -- [args...]`, and it is the only one. There is no managed shell session, no `PATH` shim, and no automatic command interception; that prohibition is recorded in [Define iwaya as a Docker-Context Secret Injection Runner](adr/20260806T192918Z_docker-context-secret-injection-runner.md).

## Configuration

BWS is the worked example for the provider layer. Other provider types are expected, and the parameter model for them is deliberately not designed in advance.

## Deliberately Absent

Each of the following is absent for now rather than ruled out. Each requires its own decision before it is added, and none of them is a planned feature.

- a local process context, or any generic execution backend abstraction
- container technologies that do not implement the Docker-compatible `exec` interface
- selecting a container by ID rather than by name
- variables and interpolation anywhere in configuration
- separating the command identifier from the executable run inside the container
- noninteractive execution, or configurable TTY and stdin behavior
- a secret delivery mechanism other than the container environment

Capabilities the model excludes outright, rather than for now, are not listed here. Allow and deny rules, argument patterns, repository-context authorization, per-context command restrictions, and arbitrary runtime option pass-through are excluded by [the execution model](design/docker-execution.md) itself, with the reasons stated there.

## Open Decisions

These must be settled before the CLI is implemented. They are not deferred features; they are gaps in the current specification.

- the output formats iwaya supports, and the structure of machine-readable output
- the exit-code categories, and the stable error codes within them
- how iwaya's own failure exit codes coexist with passing through the exit status of the process it ran

## Related Documents

- [Docker Execution Context and Command Policy Model](design/docker-execution.md) defines the model these constraints narrow.
- [Security Model and Limitations](design/security-model.md) defines the boundary, which is not v0-specific.
