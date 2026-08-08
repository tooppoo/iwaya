# iwaya

## Concept

iwaya is a policy-aware command proxy designed to reduce accidental secret exposure during command execution.

It resolves and injects secrets for explicitly invoked commands according to the configured policies, while limiting secret handling to the lifetime and scope of those commands.

iwaya is a mitigation layer, not a security sandbox. It does not guarantee containment of malicious commands or prevent every possible form of secret exfiltration.

See [Security Model and Limitations](docs/design/security-model.md) for the security boundary and non-goals.

## Quick Start

The basic installation is:

```sh
# TODO: install
```

The basic command interface is:

```sh
iwaya exec -- <command> [args...]
```

For example:

```sh
iwaya exec -- your-command --flag value
```

Installation and configuration are documented alongside the implementation they describe.

## Basic Usage

Commands are executed explicitly through `iwaya exec`:

```sh
iwaya exec -- <command> [args...]
```

See the [Docker Execution Context and Command Policy Model](docs/design/docker-execution.md) for the configuration layers, the execution order, and the secret-delivery constraints, and [v0 Scope](docs/v0-scope.md) for how far the current release implements them.

## Index for Documents

Start from [the documentation index](docs/README.md), which explains what to read for a given question.

### Design and security

* [Security Model and Limitations](docs/design/security-model.md)
* [Docker Execution Context and Command Policy Model](docs/design/docker-execution.md)
* [v0 Scope](docs/v0-scope.md)
* [Architecture Decision Records](docs/adr/README.md)
