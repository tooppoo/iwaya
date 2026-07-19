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

In v0:

* arguments after `--` identify the command and its arguments;
* only the explicitly invoked command is handled by iwaya;
* subprocesses started by that command are not automatically intercepted.

A command is managed by iwaya only when it is registered in the configuration. A managed command is default-deny: it runs only when an allow rule matches. An unregistered command passes through without managed policy evaluation or managed secrets.

See the [Policy Reference](docs/design/policy.md) for command matching and secret handling semantics.

## Index for Documents

Start from [the documentation index](docs/README.md), which explains what to read for a given question.

### Design and security

* [Security Model and Limitations](docs/design/security-model.md)
* [Policy Reference](docs/design/policy.md)
* [Command Proxy Architecture](docs/design/command-proxy.md)
* [Architecture Decision Records](docs/adr/README.md)
