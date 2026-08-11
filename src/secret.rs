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

    /// The only way to read the raw value. Call sites must remain few enough
    /// to enumerate, and each must set the environment of a subprocess: the
    /// Docker-compatible runtime for a policy secret, the provider subprocess
    /// for a provider credential.
    pub fn expose_to_subprocess_env(&self) -> &str {
        &self.0
    }
}
