//! The supervisor side of the secret-transfer contract: provisioning each
//! `proxy-secret` with a fresh phantom, serializing the transfer document
//! the sidecar reads on stdin, and planning the environment the target
//! process receives instead of raw values.
//!
//! The wire structs live here and are shared with the proxy-side parser
//! (`proxy::parse_transfer`), so the two ends of the supervisor/proxy
//! protocol cannot drift apart.

use serde::{Deserialize, Serialize};

use crate::config::{EnvName, ProxySecretSpec};
use crate::phantom::{GenerateError, Phantom};
use crate::secret::Secret;

/// The secret-transfer document the supervisor writes to the proxy process
/// stdin, before the proxy reports readiness
/// (docs/adr/20260820T162206Z_proxy-backed-secret-delivery.md, "Secret
/// transfer into the proxy container"). Each route carries the phantom to
/// match and the raw value to inject; both travel only here, never through
/// argv, environment, or a file. The exact shape is a supervisor/proxy
/// implementation detail, not user configuration.
#[derive(Serialize, Deserialize)]
pub(crate) struct ProxyTransfer {
    pub(crate) routes: Vec<RouteTransfer>,
}

// No `Debug`: a derived one would format `secret` and `phantom`, which must
// not reach any diagnostic.
#[derive(Serialize, Deserialize)]
pub(crate) struct RouteTransfer {
    pub(crate) header_name: String,
    pub(crate) template: String,
    pub(crate) upstream: String,
    pub(crate) phantom: String,
    pub(crate) secret: String,
}

/// One `proxy-secret` of the invocation, resolved and bound to the fresh
/// phantom its target will present. Holds everything the transfer document
/// and the target environment need, so the exec path resolves and mints
/// once and derives both from the same state.
pub struct ProvisionedProxySecret {
    env_name: EnvName,
    base_url_env: EnvName,
    header_name: String,
    template: String,
    upstream: String,
    phantom: Phantom,
    raw_value: Secret,
}

impl ProvisionedProxySecret {
    /// Binds a resolved raw value to its spec under a phantom minted here:
    /// one call per `proxy-secret` per invocation is what yields the
    /// one-phantom-per-secret-per-invocation property.
    // Unwired until the exec path provisions proxy secrets (issue #42); with
    // `transfer_line` and `target_environment` these are the module's
    // allowed roots.
    #[allow(dead_code)]
    pub fn provision(spec: &ProxySecretSpec, raw_value: Secret) -> Result<ProvisionedProxySecret, GenerateError> {
        Ok(ProvisionedProxySecret {
            env_name: spec.env_name.clone(),
            base_url_env: spec.base_url_env.clone(),
            header_name: spec.inject_header.name.clone(),
            template: spec.inject_header.template.clone(),
            upstream: spec.upstream.clone(),
            phantom: Phantom::generate()?,
            raw_value,
        })
    }
}

/// Serializes the transfer document for `Sidecar::start`. The output is one
/// line: JSON string escaping keeps every raw newline out of the
/// serialization, which the sidecar's single-line delivery contract relies
/// on.
// Unwired until issue #42's exec path; see `ProvisionedProxySecret::provision`.
#[allow(dead_code)]
pub fn transfer_line(provisioned: &[ProvisionedProxySecret]) -> String {
    let document = ProxyTransfer {
        routes: provisioned
            .iter()
            .map(|secret| RouteTransfer {
                header_name: secret.header_name.clone(),
                template: secret.template.clone(),
                upstream: secret.upstream.clone(),
                phantom: secret.phantom.expose_to_transfer().to_string(),
                secret: secret.raw_value.expose_to_proxy_transfer().to_string(),
            })
            .collect(),
    };
    serde_json::to_string(&document)
        .expect("a struct of strings and vectors always serializes to JSON")
}

/// The environment the target process receives for its proxy-backed
/// secrets: the phantom under each credential name, and the loopback proxy
/// URL under each `base-url-env`. Raw values never appear here — this is
/// the entire proxy-secret surface the target sees.
// Unwired until issue #42's exec path; see `ProvisionedProxySecret::provision`.
#[allow(dead_code)]
pub fn target_environment(provisioned: &[ProvisionedProxySecret], port: u16) -> Vec<(EnvName, String)> {
    let mut environment = Vec::with_capacity(provisioned.len() * 2);
    for secret in provisioned {
        environment.push((
            secret.env_name.clone(),
            secret.phantom.expose_to_target_env().to_string(),
        ));
        environment.push((
            secret.base_url_env.clone(),
            format!("http://127.0.0.1:{port}"),
        ));
    }
    environment
}

#[cfg(test)]
mod tests {
    use crate::config::InjectHeader;
    use crate::proxy::parse_transfer;

    use super::*;

    fn spec() -> ProxySecretSpec {
        ProxySecretSpec {
            env_name: EnvName::new("ANTHROPIC_AUTH_TOKEN"),
            provider: crate::config::ProviderId::new("bws-default"),
            secret_name: crate::config::SecretName::new("ANTHROPIC_AUTH_TOKEN"),
            upstream: "https://api.anthropic.com".to_string(),
            base_url_env: EnvName::new("ANTHROPIC_BASE_URL"),
            inject_header: InjectHeader {
                name: "x-api-key".to_string(),
                template: "{}".to_string(),
            },
        }
    }

    fn provisioned() -> Vec<ProvisionedProxySecret> {
        vec![ProvisionedProxySecret::provision(&spec(), Secret::new("raw-secret-value".to_string())).unwrap()]
    }

    #[test]
    fn round_trips_the_forwarding_fields_through_the_proxy_parser() {
        let routes = parse_transfer(&transfer_line(&provisioned())).unwrap();
        let [route] = routes.as_slice() else {
            panic!("expected exactly one route");
        };
        assert_eq!(
            (
                route.header_name.as_str(),
                route.template.as_str(),
                route.upstream.as_str()
            ),
            ("x-api-key", "{}", "https://api.anthropic.com")
        );
    }

    #[test]
    fn round_trips_the_credential_material_through_the_proxy_parser() {
        let provisioned = provisioned();
        let routes = parse_transfer(&transfer_line(&provisioned)).unwrap();
        let [route] = routes.as_slice() else {
            panic!("expected exactly one route");
        };
        assert!(route.phantom.matches_presented(provisioned[0].phantom.expose_to_target_env()));
        assert_eq!(route.raw_value.expose_to_upstream_header(), "raw-secret-value");
    }

    #[test]
    fn serializes_to_a_single_line_even_with_newline_bearing_fields() {
        let mut spec = spec();
        spec.inject_header.template = "Bearer\n{}".to_string();
        let provisioned =
            vec![ProvisionedProxySecret::provision(&spec, Secret::new("raw\nvalue".to_string())).unwrap()];
        assert!(!transfer_line(&provisioned).contains('\n'));
    }

    #[test]
    fn mints_a_distinct_phantom_per_provisioning() {
        let spec = spec();
        let first = ProvisionedProxySecret::provision(&spec, Secret::new("raw".to_string())).unwrap();
        let second = ProvisionedProxySecret::provision(&spec, Secret::new("raw".to_string())).unwrap();
        assert_ne!(
            first.phantom.expose_to_target_env(),
            second.phantom.expose_to_target_env()
        );
    }

    #[test]
    fn plans_the_phantom_under_the_credential_env_name() {
        let provisioned = provisioned();
        let environment = target_environment(&provisioned, 34567);
        assert_eq!(
            environment[0],
            (
                EnvName::new("ANTHROPIC_AUTH_TOKEN"),
                provisioned[0].phantom.expose_to_target_env().to_string()
            )
        );
    }

    #[test]
    fn plans_the_loopback_proxy_url_under_the_base_url_env() {
        let environment = target_environment(&provisioned(), 34567);
        assert_eq!(
            environment[1],
            (
                EnvName::new("ANTHROPIC_BASE_URL"),
                "http://127.0.0.1:34567".to_string()
            )
        );
    }

    #[test]
    fn plans_no_entry_carrying_the_raw_value() {
        let environment = target_environment(&provisioned(), 34567);
        assert!(
            environment
                .iter()
                .all(|(_, value)| !value.contains("raw-secret-value"))
        );
    }
}
