# iwaya

## Concept

iwaya is a policy-aware command proxy designed to reduce accidental secret exposure during command execution.

It resolves and injects secrets for explicitly invoked commands according to the configured policies, while limiting secret handling to the lifetime and scope of those commands.

iwaya is a mitigation layer, not a security sandbox. It does not guarantee containment of malicious commands or prevent every possible form of secret exfiltration.

See [Security Model and Limitations](docs/security-model.md) for the security boundary and non-goals.

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

See [Getting Started](docs/getting-started.md) for installation, initial configuration, and a complete walkthrough.

## Basic Usage

Commands are executed explicitly through `iwaya exec`:

```sh
iwaya exec -- <command> [args...]
```

In v0:

* arguments after `--` identify the command and its arguments;
* only the explicitly invoked command is handled by iwaya;
* subprocesses started by that command are not automatically intercepted.

See the [CLI Reference](docs/cli.md) for the complete command-line interface and the [Policy Reference](docs/policy.md) for command matching and secret handling semantics.

## Index for Documents

### Guides and references

* [Getting Started](docs/getting-started.md)
* [CLI Reference](docs/cli.md)
* [Configuration Reference](docs/configuration.md)
* [Policy Reference](docs/policy.md)

### Design and security

* [Security Model and Limitations](docs/security-model.md)
* [Command Proxy Architecture](docs/architecture/command-proxy.md)
* [Architecture Decision Records](docs/adr/README.md)
