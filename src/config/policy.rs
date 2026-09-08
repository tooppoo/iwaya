//! The `policies` section: which secrets a command receives, and how each is
//! delivered (docs/design/configuration.md, "Command Policies").

use kdl::KdlNode;

use super::{
    CommandId, EnvName, ParseFailure, ProviderId, SecretName, children, invalid,
    positional_strings, property, single_string_argument,
};

pub struct CommandPolicy {
    pub id: CommandId,
    pub secrets: Vec<SecretSpec>,
    pub proxy_secrets: Vec<ProxySecretSpec>,
}

pub struct SecretSpec {
    pub env_name: EnvName,
    pub provider: ProviderId,
    pub secret_name: SecretName,
}

/// Proxy-mediated delivery: the target process receives a phantom credential
/// under `env_name`, and the raw value is used only by an iwaya-run proxy
/// toward `upstream` (docs/design/configuration.md, "Proxy-Backed Secret
/// Delivery").
pub struct ProxySecretSpec {
    pub env_name: EnvName,
    pub provider: ProviderId,
    pub secret_name: SecretName,
    pub upstream: String,
    pub base_url_env: EnvName,
    pub inject_header: InjectHeader,
}

/// The header the proxy rewrites: the phantom credential arrives under
/// `name`, and the raw value is sent as `template` with its one `{}`
/// placeholder substituted.
pub struct InjectHeader {
    pub name: String,
    pub template: String,
}

pub(super) fn parse_policy(node: &KdlNode) -> Result<CommandPolicy, ParseFailure> {
    if node.name().value() != "command" {
        return invalid(format!("unknown policy type '{}'", node.name().value()));
    }
    let arguments = positional_strings(node)?;
    let [id] = arguments.as_slice() else {
        return invalid("a 'command' policy requires exactly one identifier argument");
    };
    let owner = format!("command '{id}'");

    let mut secrets = Vec::new();
    let mut proxy_secrets = Vec::new();
    for child in children(node) {
        match child.name().value() {
            "secret" => {
                let secret_arguments = positional_strings(child)?;
                let [env_name] = secret_arguments.as_slice() else {
                    return invalid(format!(
                        "a 'secret' in {owner} requires exactly one environment variable name argument"
                    ));
                };
                secrets.push(SecretSpec {
                    env_name: EnvName::new(env_name),
                    provider: ProviderId::new(property(child, "provider", &owner)?),
                    secret_name: SecretName::new(property(child, "secret-name", &owner)?),
                });
            }
            "proxy-secret" => proxy_secrets.push(parse_proxy_secret(child, &owner)?),
            other => return invalid(format!("unknown entry '{other}' in {owner}")),
        }
    }

    Ok(CommandPolicy {
        id: CommandId::new(id),
        secrets,
        proxy_secrets,
    })
}

fn parse_proxy_secret(node: &KdlNode, policy_owner: &str) -> Result<ProxySecretSpec, ParseFailure> {
    let arguments = positional_strings(node)?;
    let [env_name] = arguments.as_slice() else {
        return invalid(format!(
            "a 'proxy-secret' in {policy_owner} requires exactly one environment variable name argument"
        ));
    };
    let owner = format!("proxy-secret '{env_name}' in {policy_owner}");

    let mut provider = None;
    let mut secret_name = None;
    let mut upstream = None;
    let mut base_url_env = None;
    let mut inject_header = None;
    for child in children(node) {
        let setting = child.name().value();
        // 'inject-header' is the one setting with two arguments, so it does
        // not go through the shared single-argument path below.
        if setting == "inject-header" {
            if inject_header.is_some() {
                return invalid(format!("{owner} declares 'inject-header' more than once"));
            }
            let header_arguments = positional_strings(child)?;
            let [name, template] = header_arguments.as_slice() else {
                return invalid(format!(
                    "'inject-header' in {owner} requires a header name argument and a template argument"
                ));
            };
            inject_header = Some(InjectHeader {
                name: name.to_string(),
                template: template.to_string(),
            });
            continue;
        }
        let target = match setting {
            "provider" => &mut provider,
            "secret-name" => &mut secret_name,
            "upstream" => &mut upstream,
            "base-url-env" => &mut base_url_env,
            other => return invalid(format!("unknown setting '{other}' in {owner}")),
        };
        if target.is_some() {
            return invalid(format!("{owner} declares '{setting}' more than once"));
        }
        *target = Some(single_string_argument(child, &owner)?.to_string());
    }

    let require = |value: Option<String>, field: &str| match value {
        Some(v) => Ok(v),
        None => invalid(format!("{owner} requires a '{field}' setting")),
    };

    let Some(inject_header) = inject_header else {
        return invalid(format!("{owner} requires an 'inject-header' setting"));
    };

    Ok(ProxySecretSpec {
        env_name: EnvName::new(env_name),
        provider: ProviderId::new(&require(provider, "provider")?),
        secret_name: SecretName::new(&require(secret_name, "secret-name")?),
        upstream: require(upstream, "upstream")?,
        base_url_env: EnvName::new(&require(base_url_env, "base-url-env")?),
        inject_header,
    })
}
