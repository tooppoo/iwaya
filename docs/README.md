# iwaya Documentation

This directory holds the durable documentation for iwaya. The project overview and entry points are in [the repository README](../README.md).

## Where to Start

**If you want to know whether iwaya protects your credentials**, read [Security Model and Limitations](design/security-model.md) first. It states the boundary and, more importantly, what iwaya does not protect against.

**If you want to know what iwaya will run and which secrets it delivers**, read [Docker Execution Context and Command Policy Model](design/docker-execution.md). It defines the three configuration layers, how an invocation selects a container and a command, and the order in which iwaya validates, resolves secrets, and executes.

**If you are implementing or changing iwaya**, read that same document for the invariants an implementation must preserve, then the relevant records in [Architecture Decision Records](adr/README.md) for why those choices were made.

**If you want to know whether a capability exists yet**, read [v0 Scope](v0-scope.md). The design documents describe the durable model; that one records how far the current release implements it, what is deliberately absent, and which decisions are still open.

**If you are about to make a decision that future contributors will question**, read [the ADR guide](adr/README.md) before writing code.

## Sections

### [Design](design/README.md)

Architecture, the execution and secret-delivery model, and the security boundary. These documents are normative for implementation, and they describe how the system is divided and what must remain true.

### [v0 Scope](v0-scope.md)

The version-specific layer over the design documents: current constraints, deliberate omissions, and open decisions. Unlike the other sections, it is expected to shrink and eventually disappear, so it must not be used as a home for anything durable.

### [Architecture Decision Records](adr/README.md)

The rationale behind each durable decision, including alternatives that were rejected and trade-offs that were accepted. ADRs are historical records: an accepted ADR is not rewritten when the design later changes, so a superseded ADR still describes what was decided at the time. Consult the design documents for current behavior, and ADRs for the reasoning.

### Guides and generated references

User-facing guides and the generated CLI and configuration references are added alongside the implementation they describe. Facts that can be derived from the implementation — command-line options and the configuration syntax — are generated rather than hand-written, so that they cannot drift from the behavior they document.

## Conventions

Documentation is written in English.

Substantive information has exactly one authoritative home. Where another document already covers something, these documents link to it rather than restating it. When editing, prefer extending the authoritative document over adding a second explanation elsewhere.
