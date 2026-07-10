# Managed Session Architecture

## Purpose

An iwaya-managed session provides the primary interactive environment for the policy-aware command proxy.

Running `iwaya` without a subcommand starts a managed shell. Inside that shell, configured command names are intercepted through session-scoped `PATH` shims. Commands without applicable policy continue through the selected execution backend without managed secret injection.

This document defines how the managed shell is hosted by an existing terminal environment. Command policy and secret-injection behavior are defined in [Command Proxy Architecture](./command-proxy.md).

The durable decisions behind this document are recorded in:

- [Use Session-Scoped PATH Shims for Transparent Command Proxying](../adr/20260702T005500Z_session-scoped-path-shims.md)
- [Keep Managed Sessions Independent of Terminal Emulators and Multiplexers](../adr/20260710T173700Z_terminal-independent-managed-sessions.md)
- [Treat iwaya as a Mitigation Boundary, Not a Sandbox](../adr/20260710T170955Z_mitigation-boundary-not-sandbox.md)

## Hosting Model

iwaya is hosted by the user's existing terminal environment. It is not itself a terminal emulator or terminal multiplexer.

The recommended topology is:

```txt
terminal emulator
└── optional existing terminal multiplexer
    └── user shell
        └── iwaya
            └── managed shell
                ├── unmanaged command
                └── managed command shim
                    └── command proxy
                        └── real command
```

Examples of valid outer environments include an ordinary terminal emulator, an integrated development environment terminal, and a pane in an already-running terminal multiplexer.

No product-specific terminal or multiplexer API is part of the core contract.

## Session Startup

Running `iwaya` without a subcommand starts the primary interactive session.

The launcher performs the following conceptual steps:

1. resolve the shell to start according to the supported shell contract
2. create a unique managed-session identifier
3. create session-scoped resources, including the shim directory
4. generate shims for configured managed commands
5. construct the managed-shell environment
6. prepend the shim directory to `PATH`
7. make the managed state visible through a banner or equivalent output
8. start the interactive shell as a child process
9. wait for the managed shell to exit
10. release session-scoped resources when safe to do so

The exact storage location is platform-specific. A conceptual Unix-like layout is:

```txt
~/.cache/iwaya/sessions/<session-id>/
└── bin/
    ├── gh
    ├── git
    └── other-managed-command
```

## Environment Construction

The launcher starts from the ordinary user environment and applies a narrow set of changes.

It may add metadata such as:

```sh
IWAYA_SESSION=1
IWAYA_SESSION_ID=<session-id>
IWAYA_POLICY_FILE=<path>
```

It prepends the session shim directory to `PATH`:

```txt
PATH=<session-shim-directory>:<original-path>
```

The managed-shell environment must not contain command secrets such as `GH_TOKEN` solely because the session is active.

Secrets are resolved only after policy authorization and are injected only into the selected child-process execution.

## Inherited Terminal State

Unless a documented shell or platform compatibility rule requires otherwise, the managed shell should inherit:

- standard input
- standard output
- standard error
- controlling TTY or PTY
- current working directory
- terminal-related environment such as `TERM` and `COLORTERM`
- existing multiplexer environment such as `TMUX` or `ZELLIJ`
- locale and ordinary user environment

The core launcher must not replace the terminal's rendering, input, clipboard, resize, or window-management behavior.

## Standard I/O and TTY Ownership

iwaya is an ordinary foreground process in the launching shell. The managed shell is an ordinary interactive child process using the inherited terminal.

iwaya should not allocate a new terminal merely to implement core managed-session behavior when the launching environment already provides one.

The managed shell and its foreground jobs retain normal terminal behavior, including direct interaction through the inherited standard streams.

Platform and shell implementations must preserve ordinary exit status, signal, and interactive behavior as far as the supported environment allows.

## Signals and Job Control

Signal forwarding and job-control behavior are shell- and platform-sensitive.

The architectural expectation is:

- the managed shell remains the interactive process responsible for shell job control
- iwaya does not reinterpret shell syntax or job-control commands
- termination and interrupt signals must not be swallowed silently by the launcher
- the launcher must report or propagate managed-shell termination consistently

Exact signal and process-group behavior belongs to the shell and platform compatibility contract tracked by issue #8.

## Managed-State Visibility

Users must be able to identify that the active shell is managed by iwaya.

The initial implementation may print a startup banner containing non-secret metadata such as:

```txt
iwaya session active
session: <session-id>
policy: <policy-file>
managed commands: gh, git
```

Prompt modification is optional. It must not be required for core behavior because prompt frameworks and shell configuration vary significantly.

A status command may expose the same non-secret state after startup.

## Terminal Emulator Independence

iwaya does not depend on terminal-emulator-specific APIs or configuration.

A terminal emulator is expected only to provide ordinary terminal behavior supported by the host operating system and selected shell.

Product-specific enhancements may be implemented later, but they must remain optional and must not alter command policy semantics.

Starting a new terminal emulator from inside an iwaya-managed shell is not recommended for v0. The resulting terminal process or window may detach from the lifecycle and inherited terminal assumptions of the managed session.

## Terminal Multiplexer Independence

iwaya may be started inside an existing terminal multiplexer pane.

```txt
existing tmux or zellij session
└── pane shell
    └── iwaya
        └── managed shell
```

iwaya does not create, attach, detach, rename, or terminate multiplexer sessions as part of core behavior.

Starting a new persistent multiplexer server inside an iwaya-managed shell is not recommended for v0. Such a server may outlive the original managed shell while retaining:

- a `PATH` entry for the session shim directory
- `IWAYA_SESSION_ID`
- `IWAYA_POLICY_FILE`
- other session-scoped metadata

After the managed session ends, those references may no longer be valid. Supporting persistent descendants requires a separate durable resource and lifecycle design.

This topology is not prohibited. It is outside the recommended and guaranteed v0 lifecycle.

## Shell Compatibility Boundary

Terminal independence does not imply shell independence.

Shells differ in:

- executable discovery
- interactive startup flags
- startup-file behavior
- process groups and job control
- signal handling
- environment syntax
- prompt integration

The set of supported shells must be documented and tested separately. Unsupported shells must produce an actionable diagnostic instead of silently receiving partially supported behavior.

## Platform Compatibility Boundary

Platforms differ in:

- process creation
- filesystem layout
- executable lookup
- signal support
- TTY or console behavior
- cache and temporary directory conventions

Linux, WSL2, macOS, and native Windows are separate compatibility targets even when they use similar shells.

See [Compatibility](../compatibility.md) for the support model.

## Session Resource Lifecycle

Session-scoped resources are associated with the managed-shell lifecycle.

The initial lifecycle boundary is the process started by `iwaya` and the resources required by that process. iwaya does not guarantee that arbitrary detached descendants terminate when the managed shell exits.

Resource cleanup must avoid deleting active resources while the managed shell still depends on them. Conversely, cleanup must not retain sensitive or stale session state indefinitely.

Raw secret values must never be written into session resource files.

Persistent descendant and reattachment behavior, if required later, must be designed explicitly rather than inferred from the v0 managed shell.

## Architectural Invariants

Implementations must preserve the following invariants:

1. Core behavior does not depend on a terminal-emulator-specific API.
2. Core behavior does not depend on a terminal-multiplexer-specific API.
3. iwaya does not implement terminal rendering or multiplexer session management.
4. The managed shell uses ordinary inherited stdio and terminal semantics.
5. iwaya does not permanently modify terminal, multiplexer, or shell configuration.
6. Session state is scoped through process environment and session resources.
7. Command secrets are not placed in the managed-shell environment.
8. Terminal compatibility is separate from shell and platform compatibility.
9. Optional terminal integrations do not redefine command policy semantics.
10. Nested terminal emulators and persistent multiplexers are not part of the recommended v0 topology.

## Related Work

- Issue #3 tracks implementation of the managed shell and session-scoped shims.
- Issue #8 tracks supported shells and WSL2 compatibility.
- [Command Proxy Architecture](./command-proxy.md) defines policy evaluation and process-local secret injection.
