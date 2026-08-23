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
// The item-level allows below are temporary: the proxy execution path
// (issue #31) is built incrementally, and nothing consumes a phantom yet.
// They are per item, not module-wide, so an item that stays unused once
// the proxy path lands is still flagged.
#[allow(dead_code)]
pub struct Phantom(String);

#[allow(dead_code)]
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

    /// Reconstructs the phantom the supervisor minted, from the value it
    /// transferred to the proxy process. The minting side calls
    /// [`Phantom::generate`]; the proxy (matching) side calls this with the
    /// same value so it can recognise the credential the target was given.
    pub fn from_transferred(value: String) -> Phantom {
        Phantom(value)
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
    /// every phantom's length is fixed and public. Rust makes no timing
    /// guarantee for the bare OR-fold — an optimizer may rewrite an
    /// equality reduction into an early exit — so the accumulator passes
    /// through `black_box` each step; that barrier is what upholds the
    /// every-byte claim under optimization.
    pub fn matches_presented(&self, presented: &str) -> bool {
        let ours = self.0.as_bytes();
        let theirs = presented.as_bytes();
        if ours.len() != theirs.len() {
            return false;
        }
        ours.iter()
            .zip(theirs)
            .fold(0u8, |acc, (a, b)| std::hint::black_box(acc | (a ^ b)))
            == 0
    }
}

/// The OS entropy source failed; without it no phantom is trustworthy, so
/// the invocation must not proceed to proxy-backed delivery.
#[derive(Debug)]
#[allow(dead_code)]
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
    use test_case::test_case;

    use super::*;

    fn generated_suffix() -> String {
        let phantom = Phantom::generate().unwrap();
        phantom
            .expose_to_target_env()
            .strip_prefix("iwaya-phantom-")
            .expect("every phantom carries the documented prefix")
            .to_string()
    }

    #[test]
    fn generates_the_documented_prefix() {
        let phantom = Phantom::generate().unwrap();
        assert!(phantom.expose_to_target_env().starts_with("iwaya-phantom-"));
    }

    #[test]
    fn generates_a_64_character_suffix() {
        assert_eq!(generated_suffix().len(), 64);
    }

    #[test]
    fn generates_a_hexadecimal_suffix() {
        let suffix = generated_suffix();
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()), "{suffix}");
    }

    #[test]
    fn generates_a_lowercase_suffix() {
        let suffix = generated_suffix();
        assert!(suffix.chars().all(|c| !c.is_ascii_uppercase()), "{suffix}");
    }

    #[test]
    fn every_generation_is_unrelated() {
        let first = Phantom::generate().unwrap();
        let second = Phantom::generate().unwrap();
        assert_ne!(first.expose_to_target_env(), second.expose_to_target_env());
        assert!(!first.matches_presented(second.expose_to_target_env()));
    }

    #[test]
    fn matches_its_own_value() {
        let phantom = Phantom::generate().unwrap();
        assert!(phantom.matches_presented(phantom.expose_to_target_env()));
    }

    // Without this test, a byte comparison that stops before the last byte
    // would go unnoticed: every other mismatching value in this module
    // differs from the phantom long before its final byte.
    #[test]
    fn rejects_a_same_length_value_differing_in_one_byte() {
        let phantom = Phantom::generate().unwrap();
        let mut altered = phantom.expose_to_target_env().to_string().into_bytes();
        let last = altered.last_mut().unwrap();
        *last = if *last == b'0' { b'1' } else { b'0' };
        assert!(!phantom.matches_presented(&String::from_utf8(altered).unwrap()));
    }

    // Without this test, removing the length guard would go unnoticed:
    // `zip` stops at the shorter side, so the extended value would wrongly
    // match while every same-length test still passes.
    #[test_case(&|_| "iwaya-phantom-".to_string() ; "prefix alone")]
    #[test_case(&|v| v[..v.len() - 1].to_string() ; "truncated by one")]
    #[test_case(&|v| format!("{v}0") ; "extended by one")]
    #[test_case(&|_| String::new() ; "empty")]
    fn rejects_a_value_of_different_length(mutate: &dyn Fn(&str) -> String) {
        let phantom = Phantom::generate().unwrap();
        let presented = mutate(phantom.expose_to_target_env());
        assert!(!phantom.matches_presented(&presented));
    }
}
