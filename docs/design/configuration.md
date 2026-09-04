# Configuration Model

This document defines the configuration layers iwaya reads before it validates, resolves secrets, and executes: secret providers, the credentials some of them require, Docker execution contexts, and command policies.

It is normative. Where it states a requirement, an implementation that violates it is incorrect.

How this configuration is validated, consumed, and turned into a running command is defined in [Docker Execution Context and Command Policy Model](docker-execution.md). The security boundary a resolved secret and a provider credential operate within is defined in [Security Model and Limitations](security-model.md). Neither is restated here.

## Scope

This document owns the provider, context, and command-policy schema, including the credential a provider type may require before it can resolve a secret.

It does not define validation order, secret resolution, invocation construction, or error behavior; see [Docker Execution Context and Command Policy Model](docker-execution.md) for those.

The configuration syntax shown here is a baseline example rather than a syntax reference. Once a parser exists, the field-level contract belongs in a generated configuration reference derived from it.

## The Three Configuration Layers

Configuration is divided into three layers, each answering one question.

| Layer | Question | Defines |
|---|---|---|
| Secret provider | How is a secret obtained? | The provider type and its provider-specific settings |
| Docker execution context | Where does a command run? | The Docker-compatible runtime and the container to execute in |
| Command policy | Which secrets does a command receive? | The environment variable names and the secrets that supply them |

The layers are independent by construction. A provider must not select contexts or commands, a context must not declare secrets, and a command policy must not declare container settings. This is what allows one container and one command to be defined once each, rather than once per combination of the two.

### Secret Providers

A provider defines how a secret is obtained. It does not decide which command receives it.

```kdl
providers {
  bws "bws-default" {
    project "philomagi.dev"
  }
}
```

The node name is the provider type. The first argument is the provider identifier, which must be unique across all provider instances, including instances of different types. Settings a provider type requires belong inside that provider's block.

Each provider type declares what it requires. A general parameter model covering providers that do not exist yet must not be designed in advance.

The example above omits the `bws` provider's required `access-token` block to show the shape shared by every provider type; see [Provider Credentials](#provider-credentials) below for the settings `bws` itself requires.

#### Provider Credentials

A provider type may require a credential of its own before it can resolve any secret. A provider credential authenticates the provider's client to its backend. It is not among the values the provider resolves, and it is never delivered to a command policy.

A provider type that requires a credential declares how the credential is acquired inside its own configuration block. The acquisition method is represented by a child node name, following the same node-name-as-type convention used elsewhere in this model. A credential-acquisition abstraction covering acquisition types that do not exist yet must not be designed in advance.

Where a provider credential may exist at run time, and how that differs from a resolved user secret, is defined in [Provider Credentials](security-model.md#provider-credentials).

##### BWS Access Token

The `bws` provider requires an access token before it can resolve any secret. The token is declared with an `access-token` block:

```kdl
providers {
  bws "bws-default" {
    project "philomagi.dev"

    access-token {
      exec "pass" "show" "bws/access-token"
    }
  }
}
```

A `bws` provider requires exactly one `access-token` block, and `access-token` requires exactly one acquisition node. `exec` is the only acquisition type this model defines; an unsupported acquisition node name is a configuration error.

`exec` requires an executable as its first argument and accepts zero or more following argv entries. It invokes the executable directly, without a shell, so shell syntax, interpolation, redirection, and command separators in its arguments are not interpreted. An `exec` node without an executable argument is a configuration error.

The example above uses `pass` to show the shape of an acquisition command. `pass`, GPG, and a pre-existing `BWS_ACCESS_TOKEN` in the invoking environment are illustrations, not requirements: the `bws` provider does not assume a specific credential store, and it does not read an access token from the invoking environment.

On acquisition, a successful command's stdout supplies the access-token value. One trailing line ending (`\n` or `\r\n`) is removed from stdout before it is used; other stdout content is preserved. An empty value after that removal, a failure to start the command, and a non-zero exit status are each an access-token acquisition failure.

If access-token acquisition fails, BWS secret resolution is not attempted.

##### BWS Secret Resolution

The `bws` provider resolves a secret by invoking the `bws` CLI as a subprocess. The acquired access token is supplied to that subprocess as `BWS_ACCESS_TOKEN` in its environment, and only that way: it is never passed through `--access-token` or any other argv value, because a raw value in argv is visible to any process that can read the host process table.

`BWS_ACCESS_TOKEN` in the `bws` subprocess's environment always takes the acquired value, overwriting a same-named variable already present in the invoking environment. It is never inherited from, and never falls back to, that value.

### Docker Execution Contexts

A context defines the container a command runs in.

```kdl
contexts {
  docker "iwaya" {
    runtime "podman"
    user "vscode"
    workdir "/workspaces/iwaya"
    container-name "iwaya-dev"
  }
}
```

The node name `docker` denotes the Docker-compatible `exec` model. A context type is a node name in this block, so adding one is a configuration-model change rather than a new field.

| Field | Required | Meaning |
|---|---|---|
| first argument | yes | The context identifier, which must be unique |
| `runtime` | no | The Docker-compatible command to invoke. Defaults to `docker` |
| `user` | yes | The user the command runs as inside the container |
| `workdir` | yes | The working directory inside the container |
| `container-name` | yes | The name of the target container |

`user` and `workdir` are required because the constructed argv always carries `--user` and `--workdir`. No invocation shape omits either one, so a context missing them is a configuration error rather than a request to let the container decide.

`runtime` must be treated as a single executable. It must not be evaluated as a shell command string, so a value containing arguments, redirection, or command separators is a configuration error rather than a way to extend the invocation.

### Command Policies

A command policy defines which secrets a command receives, and under which environment variable names.

```kdl
policies {
  command "claude" {
    secret \
      "ANTHROPIC_AUTH_TOKEN" \
      provider="bws-default" \
      secret-name="ANTHROPIC_AUTH_TOKEN"
  }
}
```

| Element | Meaning |
|---|---|
| `command` first argument | The command identifier, which must be unique |
| `secret` first argument | The environment variable name the value is injected as |
| `provider` | The identifier of the provider that supplies the value |
| `secret-name` | The name of the secret within that provider |

The command identifier serves two roles at once: it is the name the user types at the invocation, and it is the command name executed inside the container.

The declared environment variable names, together with the provider and secret name supplying each, are the complete injection mapping. An invocation cannot name a provider, a secret name, or an environment mapping, so the delivery scope is fixed in configuration and reviewable there.

A command policy is not an allow or deny rule. Matching machinery of that kind — argument patterns, rule priority, fallback, repository matching — has no place in the model, because none of it would constrain what the target command can do with the credential it received.

#### Proxy-Backed Secret Delivery

A `secret` delivers the resolved raw value directly into the target process environment. A `proxy-secret` never does: the target process receives a per-invocation phantom credential, and the raw value is used only by an iwaya-run reverse proxy toward one fixed upstream. Why this delivery mode exists, its phantom-credential and proxy model, and the boundary it does and does not provide are recorded in [Add Proxy-Backed Secret Delivery with Phantom Credentials](../adr/20260820T162206Z_proxy-backed-secret-delivery.md); this document owns only the configuration shape.

```kdl
policies {
  command "claude" {
    proxy-secret "ANTHROPIC_AUTH_TOKEN" {
      provider "bws-default"
      secret-name "ANTHROPIC_AUTH_TOKEN"

      upstream "https://api.anthropic.com"
      base-url-env "ANTHROPIC_BASE_URL"
      inject-header "x-api-key" "{}"
    }
  }
}
```

| Element | Required | Meaning |
|---|---|---|
| `proxy-secret` first argument | yes | The environment variable name the phantom credential is injected as; the raw value is never injected under it |
| `provider` | yes | The identifier of the provider that supplies the raw value |
| `secret-name` | yes | The name of the secret within that provider |
| `upstream` | yes | The fixed origin the proxy forwards requests to |
| `base-url-env` | yes | The environment variable name through which the target client is pointed at the proxy |
| `inject-header` | yes | The name of the HTTP header the proxy rewrites, followed by the template the raw value is sent as |

Each of the following violations is a configuration error:

* `upstream` that is not an `http(s)://host[:port]` origin — no path, query, userinfo, or fragment.
* An `inject-header` template without exactly one `{}` placeholder, or one leaving printable ASCII.
* An `inject-header` name that is a header the proxy itself controls: `Host`, `Content-Length`, `Transfer-Encoding`, or `Connection`.
* A collision among the environment variable names one policy injects — the first argument of every `secret`, the first argument of every `proxy-secret`, and every `base-url-env` value share a single uniqueness scope. A collision is never an ordering rule or a last-write-wins choice.

A policy declaring a `proxy-secret` executes only through the sidecar-supervised proxy path ([the proxy-backed delivery ADR](../adr/20260820T162206Z_proxy-backed-secret-delivery.md)): the target receives the phantom credential and the proxy URL, never the raw value, and a `proxy-secret` never silently degrades into direct delivery or into a command running without its declared credential.

### Baseline Example

```kdl
/- kdl-version 2

iwaya version=1 {
  providers {
    bws "bws-default" {
      project "philomagi.dev"

      access-token {
        exec "pass" "show" "bws/access-token"
      }
    }
  }

  contexts {
    docker "iwaya" {
      runtime "podman"
      user "vscode"
      workdir "/workspaces/iwaya"
      container-name "iwaya-dev"
    }

    docker "git-kura" {
      runtime "podman"
      user "vscode"
      workdir "/workspaces/git-kura"
      container-name "git-kura-dev"
    }
  }

  policies {
    command "claude" {
      secret \
        "ANTHROPIC_AUTH_TOKEN" \
        provider="bws-default" \
        secret-name="ANTHROPIC_AUTH_TOKEN"
    }

    command "gh" {
      secret \
        "GH_TOKEN" \
        provider="bws-default" \
        secret-name="GH_TOKEN"
    }
  }
}
```

The three layers sit under an `iwaya` root node carrying a configuration `version`, so the version is available before anything below it is interpreted.

This configuration is reused in [Invocation Construction](docker-execution.md#invocation-construction) to walk through a complete invocation.

## Related Documents

- [Docker Execution Context and Command Policy Model](docker-execution.md) validates this configuration, resolves the secrets it declares, and executes the resulting command.
- [Security Model and Limitations](security-model.md) defines what this model does and does not protect against.
- [Architecture Decision Records](../adr/README.md) record why these choices were made.
