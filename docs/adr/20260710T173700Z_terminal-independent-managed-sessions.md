# Keep Managed Sessions Independent of Terminal Emulators and Multiplexers

- Status: Accepted
- Created: 2026-07-10T17:37:00Z

## Context

iwaya starts a managed shell session and uses session-scoped `PATH` shims to proxy configured commands. The command proxy must remain usable from ordinary terminal environments without requiring users to adopt a terminal emulator or terminal multiplexer selected by iwaya.

Terminal emulators provide rendering, input, clipboard, resize handling, and a PTY. Terminal multiplexers may provide panes, windows, detach and attach behavior, and persistent terminal sessions. These responsibilities are separate from iwaya's policy evaluation and process-local secret injection.

Making a specific terminal emulator or multiplexer part of the core architecture would introduce unrelated configuration, lifecycle, and compatibility requirements. It would also prevent iwaya from working uniformly in environments such as Linux terminals, WSL2, macOS terminals, VS Code terminals, existing tmux sessions, and future terminal implementations.

Related issues:

- #3
- #8

Related ADRs:

- [Define iwaya as a Policy-Aware Command Proxy](20260702T005400Z_policy-aware-command-proxy.md)
- [Use Session-Scoped PATH Shims for Transparent Command Proxying](20260702T005500Z_session-scoped-path-shims.md)
- [Treat iwaya as a Mitigation Boundary, Not a Sandbox](20260710T170955Z_mitigation-boundary-not-sandbox.md)

## Decision

iwaya-managed sessions must not depend on a specific terminal emulator or terminal multiplexer.

iwaya must operate as an ordinary process hosted by the user's existing terminal, optional existing terminal multiplexer, and supported shell environment.

The managed-session launcher must use ordinary platform process semantics, environment inheritance, `PATH`, standard input, standard output, standard error, and controlling TTY or PTY behavior.

The launcher should preserve the following values from the launching environment unless a documented compatibility rule requires otherwise:

- standard input, output, and error
- controlling TTY or PTY
- current working directory
- terminal-related environment such as `TERM` and `COLORTERM`
- existing multiplexer metadata such as `TMUX` or `ZELLIJ`
- locale and ordinary user environment

The launcher may add iwaya session metadata and prepend the session shim directory to `PATH`. It must not place command secrets in the managed-session environment.

iwaya core must not require:

- terminal-emulator-specific APIs
- terminal-multiplexer-specific APIs
- pane, tab, window, or session management
- terminal configuration changes
- multiplexer configuration changes
- persistent shell startup-file modifications
- terminal-specific shell integration

Terminal-specific, multiplexer-specific, prompt, or status integrations may be provided as optional extensions. Core command proxy behavior must remain available without them.

### Recommended process topology

The supported and recommended topology is:

```txt
terminal emulator
└── optional existing terminal multiplexer
    └── user shell
        └── iwaya
            └── iwaya-managed shell
                └── commands
```

Using iwaya from an existing terminal or multiplexer session is the primary usage model.

### Nested terminal and multiplexer use

Starting a new terminal emulator from inside an iwaya-managed session is not a core use case and is not recommended. A nested terminal may detach from the lifecycle and environment assumptions of the managed shell.

Starting a persistent terminal multiplexer server from inside an iwaya-managed session is also not recommended for v0. A persistent multiplexer may retain `PATH` and iwaya session metadata after the original managed shell exits. Its processes may then reference session-scoped shim resources whose lifecycle has ended.

iwaya does not need to prohibit either operation. This is a compatibility and lifecycle limitation, not a sandbox restriction.

### Compatibility boundaries

Terminal emulator and terminal multiplexer independence does not imply compatibility with every shell or operating system.

Shell compatibility and platform compatibility must be defined and tested separately. A terminal environment is compatible when it provides ordinary process, environment, stdio, and TTY or PTY behavior supported by an explicitly supported shell and platform.

## Non-Goals

iwaya does not:

- implement a terminal emulator
- implement a terminal multiplexer
- manage panes, tabs, windows, or multiplexer sessions
- replace the user's shell
- guarantee support for every shell or platform
- guarantee that persistent descendant processes terminate with the managed shell
- guarantee continued operation of session shims after the managed-session lifecycle ends

## Alternatives Considered

### Provide iwaya as a terminal emulator

This would make the iwaya-managed state directly visible and could give iwaya complete control over terminal windows and process launch.

It was rejected because rendering, input methods, fonts, clipboard handling, and window management are unrelated to the command proxy security boundary. It would substantially expand the project while reducing compatibility with existing terminal workflows.

### Require a specific terminal multiplexer

A required multiplexer could provide persistent sessions and a stable place for status or approval UI.

It was rejected because it would force a specific session model on users, exclude environments that do not use that multiplexer, and couple iwaya session lifecycle to an external server process.

### Require shell startup integration

Startup-file integration could expose session state in prompts and intercept commands without an explicit managed shell launcher.

It was rejected as a core requirement because it would require persistent changes to shell configuration and introduce shell-specific behavior into the core architecture. Such integration may remain optional.

### Support nested terminal and multiplexer processes as a primary topology

This would treat terminal emulators and persistent multiplexer servers started inside iwaya as fully supported descendants.

It was not selected for v0 because those processes may outlive the managed session while retaining session-scoped `PATH` and metadata. Supporting that topology requires a separate durable session resource and lifecycle design.

## Consequences

### Positive Consequences

- Users may choose their existing terminal emulator and terminal multiplexer.
- iwaya remains usable in ordinary terminal environments, including WSL2.
- Core behavior is testable through normal process and TTY semantics.
- Terminal and prompt integrations can evolve without changing policy or secret-injection semantics.
- The managed-session lifecycle remains explicit and process-scoped.

### Negative Consequences

- iwaya cannot rely on terminal-specific UI for persistent status or approvals.
- Shell and platform compatibility must be tested separately.
- Nested terminal or persistent multiplexer use cannot be fully supported without additional lifecycle design.
- Users must understand that a managed session is not a persistent terminal workspace.

### Neutral Consequences

- Existing terminal multiplexer sessions may host iwaya normally.
- Terminal-specific integrations may be added later as optional adapters.
- Persistent managed workspaces, if required later, must be designed separately from the v0 managed shell lifecycle.
