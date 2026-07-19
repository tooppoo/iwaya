# Design Documentation

These documents describe how iwaya is structured, what it authorizes, and what it guarantees. They are written for maintainers and for anyone evaluating iwaya's security claims.

They are normative for implementation. Where a document states a requirement, an implementation that violates it is incorrect, not merely unconventional.

## Reading Order

Read in this order when approaching the design for the first time. Each document assumes the boundary established by the previous one.

1. **[Security Model and Limitations](security-model.md)** — the boundary iwaya claims, the exposure it reduces, and the protection it explicitly does not provide. Read this first, because the rest of the design only makes sense against a correct understanding of what iwaya is not.

2. **[Policy Reference](policy.md)** — how a command is classified as managed or unmanaged, how a managed invocation is authorized, and how an authorized invocation obtains its secrets. This is where the default-deny rule and its rationale live.

3. **[Command Proxy Architecture](command-proxy.md)** — the components, the execution flow from an explicit invocation to a child process, and the invariants an implementation must preserve.

## Responsibility Boundaries

Each document owns a distinct question, and none restates another:

| Question | Document |
|---|---|
| What does iwaya guarantee, and what does it not? | [Security Model and Limitations](security-model.md) |
| Is this invocation authorized, and what does it receive? | [Policy Reference](policy.md) |
| Which component does what, and in what order? | [Command Proxy Architecture](command-proxy.md) |
| Why was it decided this way? | [Architecture Decision Records](../adr/README.md) |

Exact syntax and option-level contracts belong in generated references derived from the implementation, not in these documents.

## Maintaining These Documents

When behavior changes, update the design document that owns the affected question, and record the reasoning in a new ADR rather than editing an accepted one.

Design documents describe current intended behavior. When an ADR is superseded, the superseding decision must be reflected here, because a reader consulting these documents should not have to reconstruct current behavior from a chain of ADRs.
