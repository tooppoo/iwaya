# iwaya

iwaya is a policy-aware command proxy for running ordinary development commands with narrowly scoped secrets.

It is designed to let users keep a familiar shell workflow while avoiding persistent raw secrets in shell sessions, containers, repositories, and command-specific login state.

> iwaya is currently under design and development. The examples below describe the intended interface and architecture.

## Concept

Running `iwaya` without a subcommand starts an iwaya-managed shell session.

```sh
iwaya
gh pr list
gh issue create
git status
```

Inside the managed shell:

- configured command names are intercepted through session-scoped `PATH` shims
- policy determines whether a command is denied, passed through, or executed with secret injection
- required secrets are resolved from external secret managers only after policy authorization
- secrets are injected only into the selected child process
- unmatched commands continue without managed secret injection
- command secrets are not exported to the entire shell session

Secret storage, encryption, authentication, rotation, and manager-side authorization remain the responsibility of external secret managers such as Bitwarden Secrets Manager or 1Password.

## Terminal Model

iwaya does not require a specific terminal emulator or terminal multiplexer.

The recommended topology is:

```text
terminal emulator
└── optional existing terminal multiplexer
    └── supported shell
        └── iwaya
            └── iwaya-managed shell
                └── commands
```

Use iwaya from an existing terminal or multiplexer session. Starting a new terminal emulator or a persistent terminal multiplexer server from inside an iwaya-managed shell is not recommended for v0 because the nested process may outlive session-scoped `PATH` shims and metadata.

Terminal independence does not imply support for every shell or operating system. Shell and platform compatibility are defined separately.

## Security Boundary

iwaya is a mitigation boundary, not a sandbox.

It is intended to reduce:

- persistent secret storage
- session-wide secret exposure
- unnecessary secret delivery
- manual credential-selection errors

After a child process receives a secret, iwaya cannot fully control how that process uses it. A process may print, save, forward, or leak the secret. Hostile-code containment requires other layers such as operating-system permissions, containers or virtual machines, sandboxing, narrowly scoped credentials, and code review.

## Architecture

- [Command Proxy Architecture](docs/architecture/command-proxy.md)
- [Managed Session Architecture](docs/architecture/managed-session.md)
- [Compatibility](docs/compatibility.md)
- [Architecture Decision Records](docs/adr/README.md)

Key decisions:

- [Define iwaya as a Policy-Aware Command Proxy](docs/adr/20260702T005400Z_policy-aware-command-proxy.md)
- [Use Session-Scoped PATH Shims for Transparent Command Proxying](docs/adr/20260702T005500Z_session-scoped-path-shims.md)
- [Treat iwaya as a Mitigation Boundary, Not a Sandbox](docs/adr/20260710T170955Z_mitigation-boundary-not-sandbox.md)
- [Keep Managed Sessions Independent of Terminal Emulators and Multiplexers](docs/adr/20260710T173700Z_terminal-independent-managed-sessions.md)

## Project Status

Current design and implementation work is tracked in GitHub Issues. The primary managed-session work is tracked by:

- [#3 Implement iwaya-managed shell sessions with session-scoped PATH shims](https://github.com/tooppoo/iwaya/issues/3)
- [#8 Define supported shells and WSL2 compatibility](https://github.com/tooppoo/iwaya/issues/8)
