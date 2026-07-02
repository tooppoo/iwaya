# Use Session-Scoped PATH Shims for Transparent Command Proxying

- Status: Proposed
- Created: 2026-07-02T00:55:00Z

## Context

iwaya aims to let users run commands such as `gh` with a familiar CLI experience while avoiding persistent credentials in devcontainers, shells, or command-specific login state.

An explicit wrapper such as `iwaya run -- gh pr list` is clear, but it is not ergonomic enough for day-to-day use. It makes iwaya feel like an extra prefix rather than a safer way to run ordinary commands.

A full terminal emulator or shell implementation would be too broad. Terminal rendering, fonts, input methods, prompt handling, panes, and PTY UI behavior are orthogonal to iwaya's security boundary.

The desired model is a shell session that is visibly managed by iwaya, while ordinary command usage remains close to normal:

```sh
iwaya enter
gh pr list
gh issue create
git status
```

Related issues:

- #3
- #4
- #5
- #8

## Decision

iwaya will provide an `iwaya enter` command that starts an iwaya-managed shell session.

Inside that session, iwaya prepends a session-local shim directory to `PATH`. Managed commands are intercepted by generated shims in that directory.

Example session layout:

```txt
~/.cache/iwaya/sessions/<session-id>/bin/gh
~/.cache/iwaya/sessions/<session-id>/bin/git
```

The managed shell may receive iwaya-specific metadata such as:

```sh
IWAYA_SESSION=1
IWAYA_SESSION_ID=<session-id>
IWAYA_POLICY_FILE=<path>
```

It must not receive command secrets such as `GH_TOKEN` as session-wide environment variables.

A managed command shim delegates to iwaya core. iwaya core resolves policy, obtains required secrets from external secret managers, and executes the real command with process-local injection.

For example, `gh pr list` inside an iwaya-managed shell may run as:

```txt
user shell
  GH_TOKEN absent
  PATH starts with session shim directory

session gh shim
  asks iwaya core to evaluate policy

real gh process
  GH_TOKEN present only if policy authorizes it
```

The shell startup must make the managed state visible. At minimum, `iwaya enter` should print a banner or equivalent status message showing that the session is managed by iwaya.

A prompt prefix may be added later, but prompt modification is not required for the initial implementation because it can conflict with shell themes and prompt frameworks.

## Non-Goals

iwaya will not implement a terminal emulator.

iwaya will not parse or reimplement all shell syntax.

iwaya will not permanently modify the user's global `PATH`.

iwaya will not export secrets to the whole managed shell session.

iwaya will not claim that a session-scoped shim prevents all malicious scripts from invoking allowed commands.

## Alternatives Considered

### Explicit wrapper as the main UX

`iwaya run -- <command>` has clear mechanics and is useful for testing, debugging, and non-interactive execution.

It was not selected as the primary interactive UX because it is less convenient than ordinary command usage. It also weakens iwaya's intended product value: users should be able to keep using familiar commands while receiving narrower secret exposure by default.

### Global PATH shim

A global shim could make commands transparently managed everywhere.

This was not selected because it would be harder to reason about when iwaya is active. It would also make debugging and command resolution more surprising. A session-scoped shim keeps the effect local to an explicitly entered managed shell.

### Full shell or terminal application

A full shell or terminal application could intercept user input more directly.

This was not selected because shell parsing and terminal behavior are not iwaya's core responsibilities. Shell constructs such as aliases, functions, command substitution, pipes, redirects, and `sh -c` make full interpretation difficult and security-sensitive.

### Background daemon as the initial architecture

A daemon could manage confirmation state, audit logs, and shared sessions across terminals.

This was not selected for the initial architecture because it increases the attack surface and implementation complexity. A daemon may be added later, but it must not expose a secret retrieval API.

## Consequences

### Positive Consequences

- Users can run familiar commands inside an iwaya-managed shell.
- iwaya's effect is scoped to the shell launched by `iwaya enter`.
- Other shells, terminals, CLIs, and TUIs are not globally disrupted.
- Shell command parsing can remain the user's shell responsibility.
- The session can visibly indicate that iwaya management is active.
- This model should work naturally in WSL2 as a Linux CLI session, without requiring a Windows-native terminal emulator.

### Negative Consequences

- iwaya must implement real-command resolution carefully to avoid recursive shim execution.
- Users must understand when they are inside or outside an iwaya-managed session.
- Scripts executed inside the session can still invoke managed commands.
- Deny policy can reduce obvious dangerous operations, but it cannot provide complete sandboxing.
- Shell compatibility must be tested across at least the initially supported shells.

### Neutral Consequences

- `iwaya run -- <command>` may still exist as a lower-level primitive.
- Prompt modification can be deferred until the banner and `iwaya status` experience is evaluated.
- Devcontainer support can be layered on as an execution backend after the local session model is stable.
