# Compatibility

## Overview

iwaya does not require a specific terminal emulator or terminal multiplexer.

The core compatibility contract is based on ordinary process, environment, `PATH`, standard I/O, and TTY or PTY behavior. Terminal emulator compatibility is therefore different from shell and platform support.

This document describes the compatibility model. It does not claim that every named or unnamed product has already been tested.

## Compatibility Dimensions

iwaya compatibility is evaluated separately across four dimensions:

1. terminal emulator
2. terminal multiplexer
3. shell
4. operating platform

A compatible terminal environment alone does not imply that its shell or platform is supported.

## Terminal Emulators

iwaya does not integrate with or require a specific terminal emulator.

A terminal emulator is expected to provide ordinary input, output, resize, and TTY or PTY behavior through the host platform.

Examples of environments that should fit this architecture include:

- standalone terminal emulators
- Windows Terminal hosting WSL2
- Ghostty
- WezTerm
- Alacritty
- integrated development environment terminals

These examples describe the intended architecture, not a completed test matrix.

Product-specific APIs, configuration files, pane management, tab management, and shell integration are not part of the core requirement.

## Terminal Multiplexers

iwaya may be started inside an existing terminal multiplexer session.

Examples include:

- tmux
- zellij
- GNU screen

The recommended topology is:

```txt
terminal emulator
└── optional existing multiplexer
    └── supported shell
        └── iwaya
            └── managed shell
```

iwaya does not manage multiplexer sessions, panes, windows, attach behavior, or detach behavior.

Starting a persistent terminal multiplexer server from inside an iwaya-managed shell is not recommended for v0. The server may outlive the managed session while retaining session-scoped `PATH` and iwaya metadata.

## Nested Terminal Emulators

Starting a terminal emulator from inside an iwaya-managed shell is not a supported primary topology and is not recommended for v0.

The new terminal may inherit only part of the managed environment, may detach from the original process lifecycle, or may continue after session resources have been released.

iwaya does not need to block this operation. The behavior is outside the guaranteed managed-session lifecycle.

## Shells

Shell support must be defined explicitly.

The current design work tracks the following candidates:

| Shell | Status | Notes |
|---|---|---|
| bash | Planned | Candidate for initial support |
| zsh | Planned | Candidate for initial support |
| fish | Undecided | May require later shell-specific work |
| PowerShell | Undecided | Native Windows and Unix-hosted PowerShell require separate evaluation |

The implementation must not treat this table as evidence of completed support. Issue #8 is responsible for determining and verifying the v0 support set.

Shell compatibility includes:

- locating the shell executable
- starting an interactive shell
- startup-file behavior
- environment inheritance
- job control
- signal behavior
- exit status

Unsupported shells should fail with an actionable diagnostic.

## Platforms

Platform support must also be explicit.

| Platform | Status | Notes |
|---|---|---|
| Linux | Planned | Primary Unix-like target candidate |
| WSL2 | Planned | Linux process model with Windows-hosted terminal environments |
| macOS | Undecided | Expected to share many Unix semantics, but must be verified |
| Native Windows | Undecided | Console, process, path, and shell behavior require separate design |

Issue #8 tracks WSL2 and shell compatibility. Other platforms should receive separate implementation or validation work when their requirements are known.

## WSL2 Model

The expected WSL2 topology is:

```txt
Windows terminal emulator
└── WSL2 distribution
    └── supported Linux shell
        └── iwaya
            └── managed shell
```

iwaya runs as a Linux process within WSL2. It should not require a Windows terminal emulator integration.

Secret-provider execution may use a command available inside WSL2. Calling a Windows executable from WSL2 may be supported later, but it must not require iwaya core to understand Windows credential storage or implement an implicit cross-boundary secret bridge.

## Support Terminology

Documentation and release notes should distinguish:

- **architecturally compatible**: the environment satisfies the ordinary process and terminal model and has no known architectural conflict
- **tested**: the environment has been exercised manually or in automation
- **supported**: the project intentionally maintains compatibility and treats regressions as defects
- **unsupported**: the environment is outside the current compatibility contract
- **not recommended**: execution may be possible, but the project does not guarantee the lifecycle or behavior

Terminal emulator independence should not be described as “tested on every terminal.”

## Core Requirements

A supported managed-session environment must provide:

- a supported shell executable
- ordinary process creation
- environment inheritance
- executable lookup through `PATH`
- usable standard input, output, and error
- a controlling TTY, PTY, or documented equivalent for interactive use
- a platform filesystem location for session-scoped resources

## Non-Requirements

Core compatibility does not require:

- a specific terminal brand
- a specific terminal multiplexer
- terminal tabs or panes
- terminal-specific shell integration
- prompt modification
- graphical notifications
- persistent detached iwaya sessions

## Related Documents

- [Managed Session Architecture](./architecture/managed-session.md)
- [Command Proxy Architecture](./architecture/command-proxy.md)
- [Keep Managed Sessions Independent of Terminal Emulators and Multiplexers](./adr/20260710T173700Z_terminal-independent-managed-sessions.md)
