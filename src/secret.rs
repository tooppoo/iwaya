//! A resolved secret value, and the provider credential that obtains it, must
//! never reach a diagnostic, a log line, or an argv element. This type carries
//! that prohibition so it does not have to be restated at every site that
//! handles a secret; see docs/adr/20260808T171732Z_implement-iwaya-in-rust.md.
//!
//! Exposure that is one-shot by nature consumes the value, so a second read
//! of the same binding is a compile error rather than a review finding — the
//! same posture the missing `Debug`/`Display` take for formatting. Only
//! deliberately repeatable uses (a provider credential shared by several
//! provider subprocesses, the proxy's per-request header rewrite) keep
//! borrowing accessors.

use std::process::Command;

/// Deliberately implements neither `Display` nor `Debug`: formatting a secret
/// anywhere is a compile error rather than a review finding. `Clone` is not
/// derived for the same reason: an unnamed clone would silently restore the
/// double-read the consuming accessors exist to prevent, so duplication goes
/// through [`Secret::clone_for_shared_declaration`] and stays greppable.
pub struct Secret(String);

impl Secret {
    pub fn new(value: String) -> Self {
        Secret(value)
    }

    /// Duplicates the value for one declared delivery that shares a
    /// resolved secret with another declaration: two policy entries may
    /// legitimately name the same provider and secret. Every duplication is
    /// an exposure-adjacent event, so it carries an intent-bearing name
    /// instead of a derived `Clone`.
    pub fn clone_for_shared_declaration(&self) -> Secret {
        Secret(self.0.clone())
    }

    /// Registers the raw value as an environment entry on a subprocess
    /// command and consumes the secret: delivery is the value's last act,
    /// and nothing can read this binding afterwards. The raw value never
    /// leaves this type — it goes straight into the command's environment
    /// table.
    pub fn deliver_to_subprocess_env(self, command: &mut Command, name: &str) {
        command.env(name, &self.0);
    }

    /// Reads the raw value of a provider credential for the environment of
    /// a provider subprocess. Borrowing rather than consuming is deliberate:
    /// one acquired credential authenticates several provider invocations
    /// (`bws project list`, `bws secret list`). The only permitted call site
    /// is the provider subprocess construction.
    pub fn expose_to_subprocess_env(&self) -> &str {
        &self.0
    }

    /// Reads the raw value for injection into the credential header of a
    /// proxied upstream request, after the phantom credential has been
    /// validated
    /// (docs/adr/20260820T162206Z_proxy-backed-secret-delivery.md). Borrowing
    /// rather than consuming is deliberate: the proxy rewrites the header on
    /// every request for the invocation's lifetime. The value must never
    /// reach the proxy's own diagnostics or responses.
    pub fn expose_to_upstream_header(&self) -> &str {
        &self.0
    }

    /// Yields the raw value for the supervisor-to-proxy transfer document
    /// and consumes the secret
    /// (docs/adr/20260820T162206Z_proxy-backed-secret-delivery.md, "Secret
    /// transfer into the proxy container"). The only permitted call site is
    /// the transfer serialization; the document travels over the sidecar's
    /// stdin and must never reach argv, environment, files, or diagnostics.
    pub fn into_proxy_transfer(self) -> String {
        self.0
    }
}
