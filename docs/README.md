# iwaya Documentation

This directory holds the durable documentation for iwaya. The project overview and entry points are in [the repository README](../README.md).

## Where to Start

**If you want to know whether iwaya protects your credentials**, read [Security Model and Limitations](design/security-model.md) first. It states the boundary and, more importantly, what iwaya does not protect against.

**If you want to know when a command is allowed to run**, read [Policy Reference](design/policy.md). It defines managed and unmanaged commands, and the default-deny rule for managed ones.

**If you are implementing or changing iwaya**, read [Command Proxy Architecture](design/command-proxy.md) for the component structure and invariants, then the relevant records in [Architecture Decision Records](adr/README.md) for why those choices were made.

**If you are about to make a decision that future contributors will question**, read [the ADR guide](adr/README.md) before writing code.

## Sections

### [Design](design/README.md)

Architecture, authorization semantics, and the security boundary. These documents are normative for implementation, and they describe how the system is divided and what must remain true.

### [Architecture Decision Records](adr/README.md)

The rationale behind each durable decision, including alternatives that were rejected and trade-offs that were accepted. ADRs are historical records: an accepted ADR is not rewritten when the design later changes, so a superseded ADR still describes what was decided at the time. Consult the design documents for current behavior, and ADRs for the reasoning.

### Guides and generated references

User-facing guides and the generated CLI and configuration references are added alongside the implementation they describe. Facts that can be derived from the implementation — command-line options, configuration syntax, the argument pattern language — are generated rather than hand-written, so that they cannot drift from the behavior they document.

## Conventions

Documentation is written in English.

Substantive information has exactly one authoritative home. Where another document already covers something, these documents link to it rather than restating it. When editing, prefer extending the authoritative document over adding a second explanation elsewhere.
