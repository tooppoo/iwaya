//! A resolved secret value, and the provider credential that obtains it, must
//! never reach a diagnostic, a log line, or an argv element. This type carries
//! that prohibition so it does not have to be restated at every site that
//! handles a secret; see docs/adr/20260808T171732Z_implement-iwaya-in-rust.md.

/// Deliberately implements neither `Display` nor `Debug`: formatting a secret
/// anywhere is a compile error rather than a review finding.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: String) -> Self {
        Secret(value)
    }

    /// Reads the raw value for subprocess environment delivery. Call sites
    /// must remain few enough to enumerate, and each must set the
    /// environment of a subprocess: the Docker-compatible runtime for a
    /// policy secret, the provider subprocess for a provider credential.
    pub fn expose_to_subprocess_env(&self) -> &str {
        &self.0
    }

    /// Reads the raw value for injection into the credential header of a
    /// proxied upstream request, after the phantom credential has been
    /// validated
    /// (docs/adr/20260820T162206Z_proxy-backed-secret-delivery.md). The
    /// only permitted call site is the reverse proxy's header rewrite; the
    /// value must never reach the proxy's own diagnostics or responses.
    pub fn expose_to_upstream_header(&self) -> &str {
        &self.0
    }

    /// Reads the raw value for the supervisor-to-proxy transfer document
    /// (docs/adr/20260820T162206Z_proxy-backed-secret-delivery.md, "Secret
    /// transfer into the proxy container"). The only permitted call site is
    /// the transfer serialization; the document travels over the sidecar's
    /// stdin and must never reach argv, environment, files, or diagnostics.
    pub fn expose_to_proxy_transfer(&self) -> &str {
        &self.0
    }
}
