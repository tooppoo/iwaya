# Use a Blocking HTTP Stack (tiny_http + ureq) for the Reverse Proxy

- Status: Accepted
- Created: 2026-08-22T16:27:45Z

## Context

Proxy-backed secret delivery needs a credential-aware reverse proxy inside iwaya: an HTTP server that accepts the target process's loopback requests, and an HTTP client that forwards them to the configured upstream ([Add Proxy-Backed Secret Delivery with Phantom Credentials](20260820T162206Z_proxy-backed-secret-delivery.md)). Until now iwaya had no HTTP dependency and no async runtime, so this choice adds both a server and a client stack and sets whether iwaya takes on `async`.

The workload is narrow. One proxy sidecar serves one invocation's `proxy-secret` entries; the client population is the processes inside a single development container, not internet-scale traffic. What the proxy must do well is different from what it must do at volume: reject a non-matching phantom before any upstream contact, never let a caller-chosen value select the origin a raw credential is sent to, and stream responses (including event streams) through transparently.

This needs an ADR because a runtime/dependency choice of this size is one future contributors will question, and because taking on `async` would shape code far beyond this module.

Related issue:

- #31

Related ADRs:

- [Add Proxy-Backed Secret Delivery with Phantom Credentials](20260820T162206Z_proxy-backed-secret-delivery.md) defines what the proxy must guarantee; this ADR records the stack that implements it.
- [Implement iwaya in Rust](20260808T171732Z_implement-iwaya-in-rust.md) sets the dependency posture this decision follows: add a dependency when a concrete need appears, and prefer the smaller one.

## Decision

The reverse proxy uses a blocking, thread-per-connection HTTP stack: `tiny_http` for the loopback server and `ureq` (with its `rustls` TLS) for the upstream client. iwaya does not adopt an `async` runtime.

The server binds `127.0.0.1:0` — loopback only, ephemeral port. Each accepted request is handled on its own thread, so one slow or streamed response does not block other callers. The client is configured to not follow redirects (`max_redirects(0)`) and to treat upstream 4xx/5xx as responses to forward rather than errors.

## Alternatives Considered

### hyper + tokio

The industry-standard proxy stack, with the most precise streaming and backpressure control and the most proxy prior art. Rejected for this stage: it pulls in an `async` runtime and a substantially larger dependency tree, and `async` would raise the review cost of this security-sensitive module and leak into the rest of the binary. The workload — one sidecar, a single container's clients — does not need the concurrency `hyper`/`tokio` exist to provide. It remains the natural migration target if a future need (HTTP/2 upstreams, high concurrency) appears.

### A hand-rolled `std::net` implementation

Zero HTTP dependencies, but it would mean hand-writing HTTP/1.1 framing (chunked transfer, header continuation, connection handling). For a layer whose job is to handle attacker-adjacent traffic and never misattribute a credential, hand-rolled framing is exactly where request-smuggling and parsing-mismatch bugs live. It also would not avoid a dependency for TLS, since `rustls` is still required for HTTPS upstreams. Rejected as the highest-risk option for the least benefit.

## Consequences

### Positive Consequences

- No `async` runtime enters the codebase; the proxy reads as ordinary synchronous Rust, which lowers the review cost of the module that handles raw credentials.
- The dependency tree stays comparatively small, consistent with the Rust ADR's posture.
- Redirect-not-followed and status-passthrough are set once on the shared client, so the credential-safety guarantees do not depend on per-request discipline.

### Negative Consequences

- Thread-per-connection does not scale to high concurrency. This is acceptable for one sidecar per invocation but would need revisiting if the model ever widened.
- `tiny_http` is maintained at a slow cadence; a future need it cannot meet would force the migration to `hyper` that this decision defers.

### Neutral Consequences

- HTTP/2 and native Windows are out of scope, unchanged from the rest of v0.
- Migrating to `hyper`/`tokio` later is contained: the proxy is one module behind a small surface (`ReverseProxy::bind_loopback` / `port` / `serve`).
