# Add Proxy-Backed Secret Delivery with Phantom Credentials

- Status: Accepted
- Created: 2026-08-20T16:22:06Z

## Context

Direct `secret` delivery places the resolved raw value in the target process environment. Anything running in the target container can read that value, copy it, and keep using it after the invocation ends. For a development container that runs tools the user does not fully audit — package postinstall scripts, editor extensions, AI agents — this makes credential theft and credential persistence the dominant residual risk of the current model.

Most of the credentials iwaya delivers are API credentials for a fixed, well-known upstream, and the clients that use them can be pointed at a different base URL through an environment variable such as `ANTHROPIC_BASE_URL` or `OPENAI_BASE_URL`. That combination allows a delivery mode in which the target process never holds the raw credential at all, while the command keeps working unchanged.

This needs an ADR because it changes the secret-delivery model, the configuration schema, and the process model — [the Docker-context runner ADR](20260806T192918Z_docker-context-secret-injection-runner.md) defined environment injection as the delivery mechanism and process replacement as the execution model, and this decision adds a second delivery mode that follows neither.

Related issue:

- #31

Related ADRs:

- [Define iwaya as a Docker-Context Secret Injection Runner](20260806T192918Z_docker-context-secret-injection-runner.md) remains accepted. Direct `secret` delivery keeps its model unchanged; this ADR adds a delivery mode beside it rather than replacing it.
- [Treat iwaya as a Mitigation Boundary, Not a Sandbox](20260710T170955Z_mitigation-boundary-not-sandbox.md) remains accepted and applies to the new mode: proxy-backed delivery mitigates credential extraction and persistence, not misuse of the upstream API, and iwaya remains a mitigation layer rather than an authorization system.

## Decision

iwaya adds a `proxy-secret` command-policy node beside `secret`. For a `proxy-secret`, the target process receives only a per-invocation phantom credential, and an iwaya-run credential-aware reverse proxy replaces that phantom with the raw value when forwarding requests to one fixed upstream.

### The phantom credential is an invocation-scoped bearer capability

Each `proxy-secret` in each invocation must receive its own cryptographically random phantom credential. The proxy must validate the phantom before injecting the raw value, must keep the phantom-to-secret association in memory only, and must invalidate it when the invocation ends. The phantom is not a placeholder the proxy accepts unconditionally: without it, any process that can reach the proxy could use the credential.

### The upstream is fixed in configuration

Each `proxy-secret` names one upstream origin. The caller must not be able to select or override the origin the raw value is sent to — not through the request target, the `Host` header, or an upstream redirect. The proxy is a credential-aware reverse proxy only, not a general HTTP proxy.

### iwaya supervises instead of replacing itself

Proxy-backed invocations run under an iwaya supervisor that manages an ephemeral sidecar proxy container and the target runtime process, because process replacement leaves no process to run the proxy. One sidecar per invocation handles all of that invocation's `proxy-secret` entries. The sidecar shares the target container's network namespace and binds to loopback only, so the target reaches it without host port publication, dedicated bridge networks, or host-gateway routing.

### Raw secrets reach the proxy through an ephemeral channel

Resolved raw values are transferred to the already-running proxy process over a non-persistent startup channel such as stdin, and exist afterward only in proxy process memory. They must not appear in the proxy image, build context, container-runtime argv, container environment, or filesystem, because each of those persists beyond the invocation or is observable outside it.

### The proxy image is iwaya-owned and built locally

The proxy image recipe is embedded in iwaya, keyed to the iwaya version, built lazily on first proxy use, and built without registry or network access. Users do not configure a proxy image, Dockerfile, or build context, and `iwaya init` does not generate one into the project.

## Alternatives Considered

### A delivery flag on `secret`

A `delivery="proxy"` property on the existing `secret` node was rejected. The two modes share almost no settings — proxy delivery requires an upstream, a base-URL variable, and a header rewrite rule that direct delivery must not carry — and a one-word flag would make the security-relevant difference between "the process holds the raw value" and "the process never sees it" easy to miss when reviewing a policy.

### A general HTTP proxy, CONNECT proxy, or TLS interception

Routing all target traffic through iwaya was rejected. It would require trust manipulation inside the target container (CA installation), would put iwaya in the position of an authorization layer it cannot honor, and mitigates nothing the credential-aware reverse proxy does not already mitigate for the fixed-upstream case that motivates the feature.

### A placeholder the proxy accepts unconditionally

Having the proxy inject the raw value into any request that reaches it was rejected. The proxy is reachable by every process in the target container's network namespace, so an unauthenticated proxy would widen credential access instead of narrowing it. The phantom credential keeps use of the raw value tied to what the invocation delivered.

### Host-published proxy ports or per-invocation bridge networks

Reaching the proxy through a host port, a temporary bridge network, or `host.docker.internal` was rejected. Each exposes the proxy beyond the target container or depends on container-to-host routing that differs across Docker and Podman; sharing the target's network namespace keeps connectivity inside the runtime abstraction the target already uses.

### Application-specific client adapters

Supporting clients that cannot be redirected through a base-URL environment variable was rejected for the initial model. Per-service adapters grow without bound and embed service knowledge iwaya otherwise avoids; the mode targets only base-URL-configurable clients.

### A published proxy image

Distributing the proxy as a registry image was rejected. It would add a registry dependency and a supply-chain surface to every proxy-backed invocation, while the embedded recipe keeps the proxy's provenance identical to the iwaya binary the user already trusts.

## Non-Goals

Proxy-backed delivery does not add endpoint-level allow/deny rules, HTTP method restrictions, rate limiting, audit logging, network allowlists, OAuth or token refresh, or runtime user approval. While an invocation runs, the target process can use the credential through the proxy; the mode narrows where the raw value exists, not what the upstream API is asked to do.

## Consequences

### Positive Consequences

- The raw credential never enters the target process or target-container environment, so in-container theft and post-invocation persistence of proxy-managed secrets are prevented rather than merely discouraged.
- The phantom credential dies with the invocation, so anything the target process leaks or stores is worthless afterward.
- Direct `secret` delivery is untouched, so existing configurations keep their behavior and their simpler process model.

### Negative Consequences

- iwaya stops being a thin exec wrapper for proxy-backed invocations: it must supervise a sidecar container and a runtime process, forward signals and exit status, and clean up on failure — a substantially larger lifecycle surface than process replacement.
- The proxy sits in the request path, so upstream interactions gain a hop, streaming behavior must be preserved deliberately, and proxy defects become invocation failures.
- Only clients that honor a configurable base URL are supported; everything else must keep using direct delivery.

### Neutral Consequences

- The raw value still exists transiently on the host during resolution and in proxy process memory during use; the boundary moves, it does not disappear.
- The proxy configuration lives directly on `proxy-secret` with no reusable proxy/route abstraction until concrete reuse requirements exist.
