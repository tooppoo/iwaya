# Use Explicit Command Execution for v0

- Status: Superseded by [20260806T192918Z_docker-context-secret-injection-runner.md](20260806T192918Z_docker-context-secret-injection-runner.md)
- Created: 2026-07-18T21:18:00Z

## Context

iwaya is a policy-aware command proxy that resolves secrets after policy evaluation and injects them only into the selected child process. Because iwaya mediates credential use, users must be able to distinguish clearly between commands executed through iwaya and commands executed directly by the surrounding shell.

The previously accepted design used an iwaya-managed shell session with session-scoped `PATH` shims. That design preserved ordinary command syntax, but it also changed executable resolution implicitly. The mechanism resembles ordinary `PATH` interception, introduces shell- and lifecycle-sensitive behavior, and makes it easier to overstate the scope of iwaya's control.

The v0 implementation should validate the command proxy, policy evaluation, secret resolution, process-local injection, and child-process execution boundary before adding transparent shell integration. Ergonomic costs should be evaluated from actual use rather than assumed in advance.

Related issue:

- #3

Related ADRs:

- [Define iwaya as a Policy-Aware Command Proxy](20260702T005400Z_policy-aware-command-proxy.md)
- [Use Session-Scoped PATH Shims for Transparent Command Proxying](20260702T005500Z_session-scoped-path-shims.md)
- [Treat iwaya as a Mitigation Boundary, Not a Sandbox](20260710T170955Z_mitigation-boundary-not-sandbox.md)
- [Define iwaya as a Docker-Context Secret Injection Runner](20260806T192918Z_docker-context-secret-injection-runner.md) supersedes this decision.

## Decision

v0 must use explicit command execution as its primary and only command-proxy entrypoint.

The command form is:

```sh
iwaya exec -- <command> [args...]
```

The `--` separator is required. Arguments before `--` belong to iwaya. The command name and all arguments after `--` belong to the target command.

Only a command explicitly invoked through `iwaya exec --` is managed by iwaya. A command invoked directly from the surrounding shell is outside iwaya's execution boundary and proceeds according to the shell and operating system's ordinary command resolution.

The explicit entrypoint must delegate to the same policy-aware command proxy model defined for iwaya core:

1. construct and resolve the command invocation
2. evaluate policy
3. deny execution, pass through without managed secrets, or determine an injection mapping
4. resolve only the authorized secrets
5. execute the selected child process with process-local injection
6. preserve the relevant exit status, standard streams, signals, and interactive behavior supported by the execution backend

v0 must not provide:

- an iwaya-managed shell session
- session-scoped `PATH` shims
- global `PATH` shims
- automatic command interception
- required shell startup integration
- a claim that commands not prefixed with `iwaya exec --` are governed by iwaya policy

A session mode may be reconsidered in v0.1.x or later if actual use shows that the explicit prefix creates material usability problems, such as frequent repetition, accidental omission, or incompatibility with interactive workflows. Reconsideration must be recorded as a separate decision and must compare available integration mechanisms rather than assuming that `PATH` shims are required.

This decision improves transparency and reduces the initial attack surface, but it does not make iwaya a sandbox or guarantee that an authorized child process handles injected secrets safely.

## Alternatives Considered

### Session-scoped PATH shims in v0

Under this model, `iwaya` would start a managed shell and prepend generated command shims to that shell's `PATH`.

This was not selected for v0 because executable interception is implicit, command resolution varies across shells, and session resource lifecycle introduces additional security-sensitive behavior before the core proxy boundary has been validated. The ergonomic benefit remains plausible, but it is not yet supported by operational evidence.

### Optional explicit execution and session mode in v0

Providing both modes would let users choose between transparency and convenience.

This was not selected because two primary invocation paths would expand the implementation and test matrix. It would also make it harder to determine whether failures originate in the core command proxy or in session integration.

### User-defined aliases or shell functions

Users may define a local abbreviation for `iwaya exec --` without iwaya implementing session management.

This is not part of the v0 contract, but it is compatible with the explicit execution model. Such abbreviations remain user-controlled and must not be presented as equivalent to a policy-enforced managed session.

## Consequences

### Positive Consequences

- Users can identify iwaya-managed execution directly from the command line.
- v0 does not alter `PATH` or executable resolution implicitly.
- The initial security-sensitive implementation surface is smaller.
- Core policy, secret resolution, and process execution behavior can be tested without shell-session integration.
- Commands outside iwaya's boundary are easier to distinguish and document.
- A future session mode can reuse the explicit execution core as a frontend rather than redefining policy semantics.

### Negative Consequences

- Users must type `iwaya exec --` for every managed command.
- Users may accidentally run a command directly and therefore without iwaya-managed secret injection or policy evaluation.
- Existing scripts and interactive habits require an explicit wrapper when they need iwaya-managed credentials.
- The v0 interface is less transparent than the previously planned managed-session experience.

### Neutral Consequences

- Directly invoked shell commands remain ordinary unmanaged commands.
- User-defined aliases may reduce typing but are outside the supported security boundary.
- Session mode remains a possible later feature, not a committed roadmap item.
- The policy-aware command proxy and non-sandbox security boundary remain unchanged.
