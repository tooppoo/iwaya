# iwaya

## Concept

iwaya runs a configured command inside a selected development container, with the secrets that command needs and nothing else.

You choose the container and the command at the call site. iwaya resolves only the secrets the command's policy declares, forwards them into the container for that one execution, and never writes them into the container, your shell, or its own state.

It exists so that a credential does not have to be left behind: no `gh auth login` inside the container, no token exported into your shell, no `.env` file mounted in.

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

Configuration has three layers: the providers a secret is fetched from, the containers a command can run in, and the secrets each command receives. See the [Docker Execution Context and Command Policy Model](docs/design/docker-execution.md) for the configuration layers, the execution order, and the secret-delivery constraints.

## Index for Documents

Start from [the documentation index](docs/README.md), which explains what to read for a given question.

### Design and security

* [Security Model and Limitations](docs/design/security-model.md)
* [Docker Execution Context and Command Policy Model](docs/design/docker-execution.md)
* [Architecture Decision Records](docs/adr/README.md)
