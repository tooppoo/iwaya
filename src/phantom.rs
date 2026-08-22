//! Invocation-scoped phantom credentials for proxy-backed secret delivery.
//!
//! A phantom is the bearer capability a target process receives instead of a
//! raw secret: one phantom per `proxy-secret` per invocation, valid only
//! while that invocation's proxy runs
//! (docs/adr/20260820T162206Z_proxy-backed-secret-delivery.md).

use std::fmt;
use std::fmt::Write;

/// 32 bytes (256 bits) of entropy keeps guessing infeasible even though a
/// phantom is reachable by every process in the target container's network
/// namespace for the invocation's lifetime.
const ENTROPY_BYTES: usize = 32;

/// A fixed recognizable prefix serves two consumers: the proxy can reject a
/// non-phantom credential value before comparing entropy, and a phantom that
/// leaks into a log or file is identifiable as an expired iwaya artifact
/// rather than mistaken for a real provider credential.
const PREFIX: &str = "iwaya-phantom-";

/// An invocation-scoped bearer credential. Like `secret::Secret` it has no
/// `Debug`/`Display`, so a phantom cannot ride along in formatted
/// diagnostics; unlike a `Secret`, its value is deliberately delivered to
/// the target process environment.
pub struct Phantom(String);

impl Phantom {
    /// Draws fresh OS entropy, so every call — across `proxy-secret`
    /// entries and across invocations — yields an unrelated value.
    pub fn generate() -> Result<Phantom, GenerateError> {
        let mut bytes = [0u8; ENTROPY_BYTES];
        getrandom::fill(&mut bytes).map_err(|source| GenerateError { source })?;
        let mut value = String::with_capacity(PREFIX.len() + ENTROPY_BYTES * 2);
        value.push_str(PREFIX);
        for byte in bytes {
            // Hex keeps the value shell-safe, header-safe, and free of
            // padding, at an acceptable length for an environment variable.
            write!(value, "{byte:02x}").expect("writing to a String cannot fail");
        }
        Ok(Phantom(value))
    }

    /// The value injected into the target process environment under the
    /// `proxy-secret` environment variable name. This is the only accessor
    /// that hands out the phantom itself; validation goes through
    /// [`Phantom::matches_presented`] instead.
    pub fn expose_to_target_env(&self) -> &str {
        &self.0
    }

    /// Whether a credential value presented to the proxy is this phantom.
    ///
    /// The comparison examines every byte regardless of where the first
    /// mismatch occurs, so response timing does not tell a probing caller
    /// how much of a guess was right. Only the length check short-circuits:
    /// every phantom's length is fixed and public.
    pub fn matches_presented(&self, presented: &str) -> bool {
        let ours = self.0.as_bytes();
        let theirs = presented.as_bytes();
        if ours.len() != theirs.len() {
            return false;
        }
        ours.iter()
            .zip(theirs)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
}

/// The OS entropy source failed; without it no phantom is trustworthy, so
/// the invocation must not proceed to proxy-backed delivery.
#[derive(Debug)]
pub struct GenerateError {
    source: getrandom::Error,
}

impl fmt::Display for GenerateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot generate a phantom credential: no usable OS entropy source: {}",
            self.source
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_the_documented_shape() {
        let phantom = Phantom::generate().unwrap();
        let value = phantom.expose_to_target_env();
        let suffix = value.strip_prefix("iwaya-phantom-").expect("prefix");
        assert_eq!(suffix.len(), 64);
        assert!(
            suffix.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "{suffix}"
        );
    }

    #[test]
    fn every_generation_is_unrelated() {
        let first = Phantom::generate().unwrap();
        let second = Phantom::generate().unwrap();
        assert_ne!(first.expose_to_target_env(), second.expose_to_target_env());
        assert!(!first.matches_presented(second.expose_to_target_env()));
    }

    #[test]
    fn matches_only_its_own_value() {
        let phantom = Phantom::generate().unwrap();
        let value = phantom.expose_to_target_env().to_string();
        assert!(phantom.matches_presented(&value));

        // Same length, one byte off: the constant-time path must reject it.
        let mut altered = value.clone().into_bytes();
        let last = altered.last_mut().unwrap();
        *last = if *last == b'0' { b'1' } else { b'0' };
        assert!(!phantom.matches_presented(&String::from_utf8(altered).unwrap()));

        // Prefix alone, truncations, and extensions are length mismatches.
        assert!(!phantom.matches_presented("iwaya-phantom-"));
        assert!(!phantom.matches_presented(&value[..value.len() - 1]));
        assert!(!phantom.matches_presented(&format!("{value}0")));
        assert!(!phantom.matches_presented(""));
    }
}
