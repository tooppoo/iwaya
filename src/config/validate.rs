//! Whole-configuration validation, run after parsing
//! (docs/design/docker-execution.md, "Validation Precedes Secret Resolution").

use super::{CommandPolicy, Config, EnvName, ParseFailure, ProxySecretSpec, invalid};

pub(super) fn validate(config: &Config) -> Result<(), ParseFailure> {
    let mut provider_ids = std::collections::HashSet::new();
    for provider in &config.providers {
        if !provider_ids.insert(provider.id()) {
            return invalid(format!("duplicate provider identifier '{}'", provider.id()));
        }
    }

    let mut context_ids = std::collections::HashSet::new();
    for context in &config.contexts {
        if !context_ids.insert(&context.id) {
            return invalid(format!("duplicate context identifier '{}'", context.id));
        }
        // The container name fills the first positional argv slot after the
        // option pairs, so a leading '-' would be consumed by the runtime as
        // an exec option — an arbitrary option passed through configuration,
        // which the invocation-construction rule forbids.
        if context.container_name.is_empty() || context.container_name.starts_with('-') {
            return invalid(format!(
                "context '{}' has a 'container-name' that is not a container name",
                context.id
            ));
        }
        // These fields become argv elements, where a NUL byte cannot exist;
        // it would surface as an exec failure instead of the configuration
        // error it is.
        for (field, value) in [
            ("user", &context.user),
            ("workdir", &context.workdir),
            ("container-name", &context.container_name),
        ] {
            if value.contains('\0') {
                return invalid(format!(
                    "context '{}' has a '{field}' containing a NUL byte",
                    context.id
                ));
            }
        }
    }

    let mut command_ids = std::collections::HashSet::new();
    for policy in &config.policies {
        if !command_ids.insert(&policy.id) {
            return invalid(format!("duplicate command identifier '{}'", policy.id));
        }
        // The command identifier is executed inside the container and also
        // occupies a positional argv slot; a leading '-' would read as an
        // option to the runtime rather than a command name, and a NUL byte
        // cannot exist in an argv element.
        if policy.id.as_str().is_empty()
            || policy.id.as_str().starts_with('-')
            || policy.id.as_str().contains('\0')
        {
            return invalid(format!(
                "command '{}' has an identifier that is not a command name",
                policy.id
            ));
        }

        // Every environment variable a policy injects lands in the same
        // target environment, so `secret` names, `proxy-secret` names, and
        // `base-url-env` names share one uniqueness scope; a collision is a
        // configuration error, never an ordering rule.
        let mut env_names = std::collections::HashSet::new();
        let mut declare_env_name = |env_name: &EnvName| -> Result<(), ParseFailure> {
            // '=' would turn the generated `--env NAME` into the forbidden
            // `--env NAME=VALUE` shape; '-' would read as an option.
            let name = env_name.as_str();
            if name.is_empty()
                || name.starts_with('-')
                || name.contains('=')
                || name.contains('\0')
            {
                return invalid(format!(
                    "command '{}' declares '{}', which is not an environment variable name",
                    policy.id, env_name
                ));
            }
            if !env_names.insert(env_name.clone()) {
                return invalid(format!(
                    "command '{}' declares environment variable '{}' more than once",
                    policy.id, env_name
                ));
            }
            Ok(())
        };
        for secret in &policy.secrets {
            declare_env_name(&secret.env_name)?;
        }
        for proxy in &policy.proxy_secrets {
            declare_env_name(&proxy.env_name)?;
            declare_env_name(&proxy.base_url_env)?;
        }

        for secret in &policy.secrets {
            if config.provider(&secret.provider).is_none() {
                return invalid(format!(
                    "command '{}' references unknown provider '{}' for '{}'",
                    policy.id, secret.provider, secret.env_name
                ));
            }
        }
        for proxy in &policy.proxy_secrets {
            validate_proxy_secret(config, policy, proxy)?;
        }
    }

    Ok(())
}

fn validate_proxy_secret(
    config: &Config,
    policy: &CommandPolicy,
    proxy: &ProxySecretSpec,
) -> Result<(), ParseFailure> {
    if config.provider(&proxy.provider).is_none() {
        return invalid(format!(
            "command '{}' references unknown provider '{}' for '{}'",
            policy.id, proxy.provider, proxy.env_name
        ));
    }

    if !is_http_origin(&proxy.upstream) {
        return invalid(format!(
            "command '{}' has an 'upstream' for '{}' that is not an http(s) origin",
            policy.id, proxy.env_name
        ));
    }

    let name = &proxy.inject_header.name;
    let is_token_char =
        |c: char| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c);
    if name.is_empty() || !name.chars().all(is_token_char) {
        return invalid(format!(
            "command '{}' has an 'inject-header' for '{}' whose name is not an HTTP header name",
            policy.id, proxy.env_name
        ));
    }
    // The proxy always derives the outbound authority from the configured
    // upstream and owns message framing itself, so a credential configured
    // to arrive in one of these headers could never be forwarded; rejecting
    // it here keeps that contradiction a configuration error.
    let proxy_owned = ["host", "content-length", "transfer-encoding", "connection"];
    if proxy_owned.contains(&name.to_ascii_lowercase().as_str()) {
        return invalid(format!(
            "command '{}' has an 'inject-header' for '{}' that names '{}', a header the proxy itself controls",
            policy.id, proxy.env_name, name
        ));
    }

    let template = &proxy.inject_header.template;
    if template.matches("{}").count() != 1 {
        return invalid(format!(
            "command '{}' has an 'inject-header' template for '{}' that does not contain exactly one '{{}}' placeholder",
            policy.id, proxy.env_name
        ));
    }
    // The template becomes a header value carrying the raw secret, so
    // anything outside printable ASCII — above all CR and LF — would open
    // header injection at the configuration layer.
    if !template.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
        return invalid(format!(
            "command '{}' has an 'inject-header' template for '{}' containing a character that cannot appear in an HTTP header value",
            policy.id, proxy.env_name
        ));
    }

    Ok(())
}

/// `http(s)://host[:port]` and nothing else. The upstream contributes only
/// the scheme and the authority of proxied requests; path and query always
/// come from the request itself, so a value carrying its own path, query,
/// userinfo, or fragment would be silently ignored — a configuration error
/// rather than a merge rule.
///
/// Shared with the proxy's transfer validation: the supervisor sends the
/// proxy the same upstream it read from configuration, and both sides must
/// agree on what an acceptable origin is.
pub(crate) fn is_http_origin(upstream: &str) -> bool {
    let Some(authority) = upstream
        .strip_prefix("https://")
        .or_else(|| upstream.strip_prefix("http://"))
    else {
        return false;
    };
    let (host_is_valid, port) = if let Some(rest) = authority.strip_prefix('[') {
        // A bracketed IPv6 literal is the one host form carrying ':'.
        let Some((host, after)) = rest.split_once(']') else {
            return false;
        };
        let host_is_valid = !host.is_empty()
            && host
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.');
        let port = match after.strip_prefix(':') {
            Some(port) => Some(port),
            None if after.is_empty() => None,
            None => return false,
        };
        (host_is_valid, port)
    } else {
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        // Rejecting ':' here is what limits a name-form authority to one
        // optional port; 'h:80:90' leaves ':80' in the host.
        let host_is_valid = !host.is_empty()
            && !host
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || "/?#@\\:[]".contains(c));
        (host_is_valid, port)
    };
    host_is_valid
        && match port {
            None => true,
            Some(port) => {
                port.chars().all(|c| c.is_ascii_digit())
                    && port.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n))
            }
        }
}
