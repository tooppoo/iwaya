# Define iwaya as a Policy-Aware Command Proxy

- Status: Proposed
- Created: 2026-07-02T00:54:00Z

## Context

iwaya was initially considered as a devcontainer-oriented tool for injecting development secrets into commands executed inside containers.

That framing is too narrow. The core problem is not container connectivity itself. The core problem is running commands with narrowly scoped secrets without requiring users to manually select, export, or persist credentials in shells, containers, or command-specific login state.

Using `gh auth login` inside a devcontainer can persist GitHub credentials inside the container. Mounting host SSH credentials into a container can weaken the host credential boundary. Manually separating tokens by purpose is safer, but it is operationally expensive for normal CLI use.

This decision needs an ADR because it defines iwaya's architectural identity, security boundary, and future extension model.

Related issues:

- #2
- #6
- #7

## Decision

iwaya will be designed as a policy-aware command proxy.

iwaya receives or intercepts a command execution request, evaluates policy, resolves required secrets from external secret managers, and executes the authorized child process with process-local secret injection.

iwaya must not be a secret manager.

iwaya must not persist secret values in its own configuration, logs, cache, credential store, or session environment.

iwaya must not expose a secret retrieval API such as `get secret`, `print secret`, or `export secret`. Secret resolution is allowed only as part of an authorized command execution.

The external secret manager remains responsible for:

- secret storage
- encryption
- secret manager authentication
- secret rotation
- manager-side access control
- manager-side audit behavior

iwaya is responsible for:

- command and argument observation
- policy matching
- execution backend selection
- secret reference resolution through a configured provider command or integration
- child-process-only injection
- log and diagnostic redaction
- deny behavior for policy-prohibited commands

Execution backends, such as local process execution and devcontainer execution, are extension points. They do not define iwaya's core identity.

## Non-Goals

iwaya is not a sandbox.

iwaya does not claim to prevent all malicious scripts or commands from leaking secrets after an authorized child process receives them.

iwaya does not claim to prevent a trusted command from printing, saving, forwarding, or otherwise mishandling a secret.

iwaya does not replace 1Password, Bitwarden Secrets Manager, OS keychains, GitHub Apps, deploy keys, or other credential authorities.

## Alternatives Considered

### Devcontainer-specific secret injector

This would keep the original framing: iwaya exists mainly to connect to devcontainers and inject secrets into commands executed there.

This was not selected because it makes container connectivity appear to be the core value. The same command-proxy model is useful for local shells, WSL2, Docker, SSH, and future execution backends.

### Explicit command wrapper only

An explicit wrapper such as `iwaya run -- gh pr list` has a clear security boundary and is useful for testing and debugging.

This was not selected as the primary product model because it does not preserve normal CLI ergonomics. Users would need to remember to prefix ordinary commands, which weakens the intended experience.

It may still exist as a lower-level interface or test path.

### Secret retrieval helper

A tool that prints or exports secrets would be simpler to implement.

This was rejected because it moves iwaya toward being a secret extraction tool. It would make it easier to expose secrets to shells, logs, scripts, and persistent state, which is contrary to iwaya's purpose.

## Consequences

### Positive Consequences

- iwaya's core model becomes independent of containers.
- Local process execution can be implemented before devcontainer support.
- Secret storage and rotation remain delegated to existing secret managers.
- The same policy model can be reused across multiple execution backends.
- The security boundary is easier to state: iwaya runs commands with scoped secret injection; it does not hand out secrets.

### Negative Consequences

- iwaya must implement policy matching carefully enough to avoid becoming a broad token-forwarding tool.
- Command and argument matching become part of the security-sensitive design surface.
- Once a child process receives a secret, iwaya cannot fully control what that process does with it.
- The product must document its non-sandbox boundary clearly to avoid overclaiming safety.

### Neutral Consequences

- Devcontainer support remains important, but it is an execution backend rather than the core architecture.
- `iwaya run -- <command>` may still be useful for tests, debugging, and CI-like usage, even if it is not the primary interactive UX.
