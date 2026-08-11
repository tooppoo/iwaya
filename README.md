# iwaya

## Concept

iwaya runs a configured command inside a selected development container, delivering only the secrets that command's policy declares.

You choose the container and the command at the call site. iwaya resolves the declared secrets and forwards them into the container for that one execution. iwaya itself writes the raw value nowhere: not into container login state, not into your shell, not into its own configuration, logs, or caches. What the command does with the value once it has it is outside iwaya's control.

It exists so that you do not have to leave a credential behind yourself: no `gh auth login` inside the container, no token exported into your shell, no `.env` file mounted in.

iwaya is a mitigation layer, not a security sandbox. It does not guarantee containment of malicious commands or prevent every possible form of secret exfiltration.

See [Security Model and Limitations](docs/design/security-model.md) for the boundary it claims and the protection it does not provide.

## Quick Start

The basic installation is:

```sh
# TODO: install
```

The basic command interface is:

```sh
iwaya exec --context <context> <command> -- [args...]
```

For example:

```sh
iwaya exec --context iwaya claude
iwaya exec --context git-kura claude -- --resume
```

Installation and configuration are documented alongside the implementation they describe.

## Basic Usage

Both the container and the command are named explicitly:

* `--context` selects a configured Docker execution context, and is required. iwaya never guesses which container a credential goes to.
* The operand names a configured command policy, which fixes the secrets that command receives.
* Arguments after `--` are appended unchanged to the target command.
* An unknown context or an unknown command is an error, and nothing runs. iwaya executes only what is configured.

Configuration is split so that a container and a command are each defined once, rather than once per combination. See the [Configuration Model](docs/design/configuration.md) for the configuration layers, and the [Docker Execution Context and Command Policy Model](docs/design/docker-execution.md) for the execution order and the secret-delivery constraints.

## Index for Documents

Start from [the documentation index](docs/README.md), which explains what to read for a given question.

### Design and security

* [Security Model and Limitations](docs/design/security-model.md)
* [Configuration Model](docs/design/configuration.md)
* [Docker Execution Context and Command Policy Model](docs/design/docker-execution.md)
* [Architecture Decision Records](docs/adr/README.md)
