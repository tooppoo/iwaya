//! Credential-aware reverse proxy for proxy-backed secret delivery: it
//! validates an invocation-scoped phantom credential and forwards the
//! request — with the raw value injected — to that `proxy-secret`'s fixed
//! upstream (docs/adr/20260820T162206Z_proxy-backed-secret-delivery.md).
//!
//! This is deliberately not a general HTTP proxy. The caller cannot select
//! the upstream: the request target must be origin-form, the outbound
//! authority always comes from the configured upstream, framing headers are
//! re-derived rather than forwarded, and upstream redirects are returned to
//! the client unfollowed so the raw credential is never re-sent to a
//! redirect-selected origin.

use std::fmt;
use std::sync::Arc;
use std::thread;

use tiny_http::{Header, Response, Server, StatusCode};

use crate::phantom::Phantom;
use crate::secret::Secret;

// The item-level allows in this module are temporary: the proxy execution
// mode (issue #31) is built incrementally, and nothing outside the tests
// consumes the proxy yet. They are per item so anything that stays unused
// once that mode lands is still flagged.

/// One `proxy-secret` entry, resolved and armed for one invocation.
#[allow(dead_code)]
pub struct ProxyRoute {
    pub header_name: String,
    pub template: String,
    pub phantom: Phantom,
    pub raw_value: Secret,
    pub upstream: String,
}

/// A loopback-only reverse proxy serving every `proxy-secret` of one
/// invocation from a single ephemeral port.
#[allow(dead_code)]
pub struct ReverseProxy {
    server: Server,
    routes: Arc<Vec<ProxyRoute>>,
    agent: ureq::Agent,
}

#[allow(dead_code)]
impl ReverseProxy {
    /// Binds `127.0.0.1:0`: loopback only, ephemeral port, never a
    /// host-published or wildcard address.
    pub fn bind_loopback(routes: Vec<ProxyRoute>) -> Result<ReverseProxy, BindError> {
        let server = Server::http("127.0.0.1:0").map_err(|source| BindError {
            message: source.to_string(),
        })?;
        let config = ureq::Agent::config_builder()
            // A redirect must reach the client unchanged: following it here
            // would re-send the injected credential to an origin the
            // upstream response chose.
            .max_redirects(0)
            // An upstream 4xx/5xx is a response to forward, not an error.
            .http_status_as_error(false)
            .build();
        Ok(ReverseProxy {
            server,
            routes: Arc::new(routes),
            agent: ureq::Agent::new_with_config(config),
        })
    }

    /// The ephemeral port the proxy selected; the supervisor reports it to
    /// the target through the configured `base-url-env`.
    pub fn port(&self) -> u16 {
        self.server
            .server_addr()
            .to_ip()
            .map(|addr| addr.port())
            .expect("a loopback listener has an IP address")
    }

    /// Serves requests until the process ends. Each request runs on its own
    /// thread so one streaming response cannot block other callers.
    pub fn serve(&self) {
        for request in self.server.incoming_requests() {
            let routes = Arc::clone(&self.routes);
            let agent = self.agent.clone();
            thread::spawn(move || handle(request, &routes, &agent));
        }
    }
}

/// The proxy could not open its loopback listener; the invocation cannot
/// offer proxy-backed delivery without it.
#[derive(Debug)]
pub struct BindError {
    message: String,
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot bind the loopback proxy listener: {}", self.message)
    }
}

/// A request the proxy refuses without contacting any upstream. Bodies are
/// fixed strings: a rejection must never echo request data, which could
/// contain a credential attempt.
#[allow(dead_code)]
enum Rejection {
    /// The request itself cannot be forwarded: a non-origin-form target, an unusable method, a credential header presented more than once, or an outbound request that could not be constructed from it.
    BadRequest(&'static str),
    /// No configured `proxy-secret` recognised the request: no credential
    /// header carried a phantom that validated against any route.
    NoMatch,
    /// The request's credential headers validated against more than one
    /// configured `proxy-secret`, so the upstream to inject toward is
    /// undecidable.
    Ambiguous,
    /// The request selected exactly one route, but the upstream call itself
    /// failed (connection, TLS, or transport error).
    Upstream(ureq::Error),
}

#[allow(dead_code)]
impl Rejection {
    fn status(&self) -> u16 {
        match self {
            Rejection::BadRequest(_) | Rejection::Ambiguous => 400,
            Rejection::NoMatch => 401,
            Rejection::Upstream(_) => 502,
        }
    }

    fn body(&self) -> String {
        match self {
            Rejection::BadRequest(reason) => format!("iwaya-proxy: {reason}"),
            Rejection::NoMatch => "iwaya-proxy: no proxied credential matched".to_string(),
            Rejection::Ambiguous => {
                "iwaya-proxy: the request matched more than one proxied credential".to_string()
            }
            Rejection::Upstream(_) => "iwaya-proxy: the upstream request failed".to_string(),
        }
    }
}

#[allow(dead_code)]
fn handle(mut request: tiny_http::Request, routes: &[ProxyRoute], agent: &ureq::Agent) {
    match forward(&mut request, routes, agent) {
        Ok(upstream) => respond_with_upstream(request, upstream),
        Err(rejection) => {
            if let Rejection::Upstream(error) = &rejection {
                // The error names the configured upstream at worst; it never
                // carries the raw credential.
                eprintln!("iwaya-proxy: error: upstream request failed: {error}");
            }
            let response = Response::from_string(rejection.body())
                .with_status_code(StatusCode(rejection.status()));
            let _ = request.respond(response);
        }
    }
}

#[allow(dead_code)]
fn forward(
    request: &mut tiny_http::Request,
    routes: &[ProxyRoute],
    agent: &ureq::Agent,
) -> Result<ureq::http::Response<ureq::Body>, Rejection> {
    let target = request.url().to_string();
    if !is_origin_form(&target) {
        return Err(Rejection::BadRequest(
            "the request target must be origin-form",
        ));
    }

    let header_pairs: Vec<(String, String)> = request
        .headers()
        .iter()
        .map(|header| {
            (
                header.field.as_str().as_str().to_ascii_lowercase(),
                header.value.as_str().to_string(),
            )
        })
        .collect();

    let route = select_route(routes, &header_pairs)?;

    let method: ureq::http::Method = request
        .method()
        .as_str()
        .parse()
        .map_err(|_| Rejection::BadRequest("the request method is not usable"))?;

    let mut builder = ureq::http::Request::builder()
        .method(method)
        // Only path and query come from the caller; scheme and authority
        // are always the configured upstream's.
        .uri(format!("{}{}", route.upstream, target));
    for (name, value) in &header_pairs {
        if forwards_request_header(name, routes) {
            builder = builder.header(name.as_str(), value.as_str());
        }
    }
    let injected = route
        .template
        .replacen("{}", route.raw_value.expose_to_upstream_header(), 1);
    builder = builder.header(route.header_name.as_str(), injected);

    // Match tiny_http's own reader selection: a body exists only when a
    // positive Content-Length was given, or a Transfer-Encoding was. A bare
    // bodyless GET/HEAD reports `body_length() == None` too, so keying off
    // "not Some(0)" would wrap it in an empty chunked upload — framing that
    // strict upstreams reject. Re-derived framing for a bodyless request is
    // no body framing, not an empty stream.
    let has_body = match request.body_length() {
        Some(length) => length > 0,
        None => header_pairs
            .iter()
            .any(|(name, _)| name == "transfer-encoding"),
    };
    let sent = if has_body {
        let mut reader = request.as_reader();
        let body = ureq::SendBody::from_reader(&mut reader);
        let outbound = builder
            .body(body)
            .map_err(|_| Rejection::BadRequest("the request could not be constructed"))?;
        agent.run(outbound)
    } else {
        let outbound = builder
            .body(())
            .map_err(|_| Rejection::BadRequest("the request could not be constructed"))?;
        agent.run(outbound)
    };
    sent.map_err(Rejection::Upstream)
}

/// Exactly one route must match. A credential header that appears more than
/// once is a malformed presentation, not a failed match: it cannot be
/// attributed to one value, and treating "any occurrence matches" as a
/// match would let extra caller-chosen occurrences ride along on an
/// authenticated request. Such a request is rejected as a bad request
/// before any phantom comparison, so it never reaches an upstream.
#[allow(dead_code)]
fn select_route<'r>(
    routes: &'r [ProxyRoute],
    headers: &[(String, String)],
) -> Result<&'r ProxyRoute, Rejection> {
    let mut matched = Vec::new();
    for route in routes {
        let values: Vec<&str> = headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case(&route.header_name))
            .map(|(_, value)| value.as_str())
            .collect();
        match values.as_slice() {
            [value] if route.phantom.matches_presented(value) => matched.push(route),
            [_] | [] => {}
            _ => {
                return Err(Rejection::BadRequest(
                    "a proxied credential header appears more than once",
                ));
            }
        }
    }
    match matched.as_slice() {
        [route] => Ok(route),
        [] => Err(Rejection::NoMatch),
        _ => Err(Rejection::Ambiguous),
    }
}

/// Origin-form only (RFC 9112 §3.2.1): an absolute URI, an authority form,
/// or a scheme-relative path could carry a caller-chosen origin into the
/// upstream URL. Both `//host` and `/\host` are rejected: WHATWG URL
/// parsing (browsers, many CDN/edge stacks, Node) treats `\` as `/` for
/// special schemes, so a layer behind the upstream could read a forwarded
/// `/\host` path as the same scheme-relative authority `//host` denotes.
#[allow(dead_code)]
fn is_origin_form(target: &str) -> bool {
    target.starts_with('/') && !target.starts_with("//") && !target.starts_with("/\\")
}

/// Returns true when a caller's request header is passed through to the upstream, and false when the proxy must own it instead.
///
/// The proxy owns, and therefore does not forward, the following headers.
/// Hop-by-hop headers (RFC 9110 §7.6.1) belong to the client-proxy connection, not to the upstream request.
/// Host and the framing headers are re-derived — Host from the configured upstream, framing from the actual body.
/// Expect is withheld because the proxy runs the upstream body leg itself and must not promise the caller's 100-continue on the upstream's behalf.
/// Every configured credential header is withheld too: the matched one is re-added with the raw value, and a non-matched one must not smuggle a caller-chosen value upstream in a credential position.
#[allow(dead_code)]
fn forwards_request_header(name: &str, routes: &[ProxyRoute]) -> bool {
    if is_hop_by_hop(name) {
        return false;
    }
    if matches!(name, "host" | "content-length" | "expect") {
        return false;
    }
    !routes
        .iter()
        .any(|route| route.header_name.eq_ignore_ascii_case(name))
}

#[allow(dead_code)]
fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

#[allow(dead_code)]
fn respond_with_upstream(request: tiny_http::Request, upstream: ureq::http::Response<ureq::Body>) {
    let status = upstream.status().as_u16();
    let mut headers = Vec::new();
    let mut content_length: Option<usize> = None;
    for (name, value) in upstream.headers() {
        let lowered = name.as_str().to_ascii_lowercase();
        if lowered == "content-length" {
            // tiny_http derives framing from the data length it is given.
            content_length = value.to_str().ok().and_then(|v| v.parse().ok());
            continue;
        }
        if is_hop_by_hop(&lowered) {
            continue;
        }
        if let Ok(header) = Header::from_bytes(name.as_str().as_bytes(), value.as_bytes()) {
            headers.push(header);
        }
    }
    let (_, body) = upstream.into_parts();
    // Streamed through: the body is copied to the client as it arrives from
    // the upstream, which keeps event streams usable.
    let reader = body.into_reader();
    let _ = request.respond(Response::new(
        StatusCode(status),
        headers,
        reader,
        content_length,
        None,
    ));
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread;

    use test_case::test_case;

    use super::*;

    struct Received {
        path: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    enum Behavior {
        Echo,
        Redirect(&'static str),
    }

    struct Upstream {
        port: u16,
        requests: Arc<Mutex<Vec<Received>>>,
    }

    impl Upstream {
        fn start(behavior: Behavior) -> Upstream {
            let server = Server::http("127.0.0.1:0").unwrap();
            let port = server.server_addr().to_ip().unwrap().port();
            let requests: Arc<Mutex<Vec<Received>>> = Arc::default();
            let recorded = Arc::clone(&requests);
            thread::spawn(move || {
                for mut request in server.incoming_requests() {
                    let mut body = String::new();
                    let _ = request.as_reader().read_to_string(&mut body);
                    recorded.lock().unwrap().push(Received {
                        path: request.url().to_string(),
                        headers: request
                            .headers()
                            .iter()
                            .map(|h| {
                                (
                                    h.field.as_str().as_str().to_ascii_lowercase(),
                                    h.value.as_str().to_string(),
                                )
                            })
                            .collect(),
                        body,
                    });
                    let response = match behavior {
                        Behavior::Echo => Response::from_string("hello-from-upstream"),
                        Behavior::Redirect(location) => Response::from_string("moved")
                            .with_status_code(StatusCode(302))
                            .with_header(
                                Header::from_bytes("Location".as_bytes(), location.as_bytes())
                                    .unwrap(),
                            ),
                    };
                    let _ = request.respond(response);
                }
            });
            Upstream { port, requests }
        }

        fn request_count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }

        fn last(&self) -> Received {
            self.requests.lock().unwrap().pop().unwrap()
        }
    }

    struct Running {
        port: u16,
        phantom_value: String,
    }

    fn start_proxy(upstream_port: u16) -> Running {
        let phantom = Phantom::generate().unwrap();
        let phantom_value = phantom.expose_to_target_env().to_string();
        let routes = vec![ProxyRoute {
            header_name: "x-api-key".to_string(),
            template: "Bearer {}".to_string(),
            phantom,
            raw_value: Secret::new("raw-secret-value".to_string()),
            upstream: format!("http://127.0.0.1:{upstream_port}"),
        }];
        let proxy = ReverseProxy::bind_loopback(routes).unwrap();
        let port = proxy.port();
        thread::spawn(move || proxy.serve());
        Running {
            port,
            phantom_value,
        }
    }

    fn client() -> ureq::Agent {
        let config = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .build();
        ureq::Agent::new_with_config(config)
    }

    #[test]
    fn forwards_to_the_upstream_with_the_credential_injected_and_path_preserved() {
        let upstream = Upstream::start(Behavior::Echo);
        let proxy = start_proxy(upstream.port);

        client()
            .get(format!(
                "http://127.0.0.1:{}/v1/messages?limit=1",
                proxy.port
            ))
            .header("x-api-key", &proxy.phantom_value)
            .call()
            .unwrap();

        let received = upstream.last();
        assert_eq!(received.path, "/v1/messages?limit=1");
        assert!(received.headers.contains(&(
            "x-api-key".to_string(),
            "Bearer raw-secret-value".to_string()
        )));
    }

    #[test]
    fn forwards_to_the_upstream_under_the_upstream_host() {
        let upstream = Upstream::start(Behavior::Echo);
        let proxy = start_proxy(upstream.port);

        client()
            .get(format!("http://127.0.0.1:{}/", proxy.port))
            .header("x-api-key", &proxy.phantom_value)
            .call()
            .unwrap();

        let received = upstream.last();
        assert!(
            received
                .headers
                .contains(&("host".to_string(), format!("127.0.0.1:{}", upstream.port)))
        );
    }

    #[test]
    fn returns_the_upstream_response_body_to_the_client() {
        let upstream = Upstream::start(Behavior::Echo);
        let proxy = start_proxy(upstream.port);

        let body = client()
            .get(format!("http://127.0.0.1:{}/", proxy.port))
            .header("x-api-key", &proxy.phantom_value)
            .call()
            .unwrap()
            .body_mut()
            .read_to_string()
            .unwrap();

        assert_eq!(body, "hello-from-upstream");
    }

    #[test]
    fn forwards_a_bodyless_get_to_the_upstream_without_body_framing() {
        let upstream = Upstream::start(Behavior::Echo);
        let proxy = start_proxy(upstream.port);

        client()
            .get(format!("http://127.0.0.1:{}/", proxy.port))
            .header("x-api-key", &proxy.phantom_value)
            .call()
            .unwrap();

        // Body framing is carried entirely by the Transfer-Encoding and
        // Content-Length headers, so "no body framing" is precisely the
        // absence of both from the forwarded request.
        let names: Vec<String> = upstream
            .last()
            .headers
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert!(!names.iter().any(|n| n == "transfer-encoding"), "{names:?}");
        assert!(!names.iter().any(|n| n == "content-length"), "{names:?}");
    }

    #[test]
    fn streams_the_request_body_to_the_upstream() {
        let upstream = Upstream::start(Behavior::Echo);
        let proxy = start_proxy(upstream.port);

        client()
            .post(format!("http://127.0.0.1:{}/v1/messages", proxy.port))
            .header("x-api-key", &proxy.phantom_value)
            .send("ping-body")
            .unwrap();

        assert_eq!(upstream.last().body, "ping-body");
    }

    #[test]
    fn rejects_a_missing_credential_without_contacting_the_upstream() {
        let upstream = Upstream::start(Behavior::Echo);
        let proxy = start_proxy(upstream.port);

        let response = client()
            .get(format!("http://127.0.0.1:{}/", proxy.port))
            .call()
            .unwrap();

        assert_eq!(response.status().as_u16(), 401);
        assert_eq!(upstream.request_count(), 0);
    }

    #[test]
    fn rejects_a_wrong_credential_without_contacting_the_upstream() {
        let upstream = Upstream::start(Behavior::Echo);
        let proxy = start_proxy(upstream.port);

        let response = client()
            .get(format!("http://127.0.0.1:{}/", proxy.port))
            .header("x-api-key", "iwaya-phantom-not-the-right-value")
            .call()
            .unwrap();

        assert_eq!(response.status().as_u16(), 401);
        assert_eq!(upstream.request_count(), 0);
    }

    #[test]
    fn rejects_a_duplicated_credential_header_without_contacting_the_upstream() {
        let upstream = Upstream::start(Behavior::Echo);
        let proxy = start_proxy(upstream.port);

        let outbound = ureq::http::Request::builder()
            .method("GET")
            .uri(format!("http://127.0.0.1:{}/", proxy.port))
            .header("x-api-key", &proxy.phantom_value)
            .header("x-api-key", "second-occurrence")
            .body(())
            .unwrap();
        let response = client().run(outbound).unwrap();

        // A credential header that cannot be attributed to one value is a
        // malformed request, not a failed match.
        assert_eq!(response.status().as_u16(), 400);
        assert_eq!(upstream.request_count(), 0);
    }

    #[test]
    fn returns_an_upstream_redirect_to_the_client_unfollowed() {
        let upstream = Upstream::start(Behavior::Redirect("http://redirect-target.example/next"));
        let proxy = start_proxy(upstream.port);

        let response = client()
            .get(format!("http://127.0.0.1:{}/", proxy.port))
            .header("x-api-key", &proxy.phantom_value)
            .call()
            .unwrap();

        assert_eq!(response.status().as_u16(), 302);
        assert_eq!(
            response.headers().get("location").unwrap(),
            "http://redirect-target.example/next"
        );
    }

    #[test]
    fn rejects_an_absolute_form_target_without_contacting_the_upstream() {
        let upstream = Upstream::start(Behavior::Echo);
        let proxy = start_proxy(upstream.port);

        // The threat is a process inside the target container hand-crafting a request that names its own origin in the request-target, to make the proxy send the injected credential to `evil.example` instead of the configured upstream.
        // No ordinary HTTP client emits an absolute-form target, so the request is written to the socket raw.
        use std::io::{BufRead, BufReader, Write};
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", proxy.port)).unwrap();
        write!(
            stream,
            "GET http://evil.example/steal HTTP/1.1\r\nHost: 127.0.0.1\r\nx-api-key: {}\r\nConnection: close\r\n\r\n",
            proxy.phantom_value
        )
        .unwrap();
        let mut status_line = String::new();
        BufReader::new(&stream).read_line(&mut status_line).unwrap();

        assert!(status_line.contains("400"), "{status_line}");
        assert_eq!(upstream.request_count(), 0);
    }

    // Ambiguous here means the request carried valid phantoms for two
    // different routes at once, so which route's upstream and credential to
    // use is undecidable and the proxy rejects rather than guessing.
    #[test]
    fn rejects_an_ambiguous_credential_match_without_contacting_the_upstream() {
        let upstream = Upstream::start(Behavior::Echo);
        let first = Phantom::generate().unwrap();
        let second = Phantom::generate().unwrap();
        let (first_value, second_value) = (
            first.expose_to_target_env().to_string(),
            second.expose_to_target_env().to_string(),
        );
        let route = |header: &str, phantom: Phantom| ProxyRoute {
            header_name: header.to_string(),
            template: "{}".to_string(),
            phantom,
            raw_value: Secret::new("raw".to_string()),
            upstream: format!("http://127.0.0.1:{}", upstream.port),
        };
        let proxy =
            ReverseProxy::bind_loopback(vec![route("x-first", first), route("x-second", second)])
                .unwrap();
        let port = proxy.port();
        thread::spawn(move || proxy.serve());

        let response = client()
            .get(format!("http://127.0.0.1:{port}/"))
            .header("x-first", &first_value)
            .header("x-second", &second_value)
            .call()
            .unwrap();

        assert_eq!(response.status().as_u16(), 400);
        assert_eq!(upstream.request_count(), 0);
    }

    // Without these checks, a caller-supplied value could override what the proxy must derive itself (authority, framing), ride along on the client-proxy connection, or occupy a credential position.
    #[test_case("host")]
    #[test_case("content-length")]
    #[test_case("transfer-encoding")]
    #[test_case("connection")]
    #[test_case("x-api-key" ; "a configured credential header")]
    fn refuses_to_forward_a_proxy_owned_request_header(name: &str) {
        let routes = vec![ProxyRoute {
            header_name: "x-api-key".to_string(),
            template: "{}".to_string(),
            phantom: Phantom::generate().unwrap(),
            raw_value: Secret::new("raw".to_string()),
            upstream: "http://127.0.0.1:1".to_string(),
        }];
        assert!(!forwards_request_header(name, &routes));
    }

    #[test]
    fn forwards_an_ordinary_request_header() {
        assert!(forwards_request_header("accept", &[]));
        assert!(forwards_request_header("anthropic-version", &[]));
    }

    #[test_case("/v1/messages", true ; "origin form")]
    #[test_case("/", true ; "root")]
    #[test_case("//evil.example/path", false ; "scheme relative")]
    #[test_case("/\\evil.example/path", false ; "backslash scheme relative")]
    #[test_case("http://evil.example/", false ; "absolute form")]
    fn accepts_only_origin_form_targets(target: &str, accepted: bool) {
        assert_eq!(is_origin_form(target), accepted);
    }
}
