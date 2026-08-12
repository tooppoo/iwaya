//! Loads and validates the three configuration layers defined in
//! docs/design/configuration.md: secret providers, Docker execution contexts,
//! and command policies.
//!
//! Validation covers the whole configuration, not only the entries an
//! invocation selects, so a defect is reported when it is introduced
//! (docs/design/docker-execution.md, "Validation Precedes Secret Resolution").

use std::fmt;
use std::path::Path;

use kdl::{KdlDocument, KdlNode};

macro_rules! identifier_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: &str) -> Self {
                $name(value.to_string())
            }

            // Not every identifier type has a borrowing call site yet.
            #[allow(dead_code)]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

identifier_type!(
    /// Identifies a provider instance, uniquely across all provider types.
    ProviderId
);
identifier_type!(
    /// Identifies a Docker execution context.
    ContextId
);
identifier_type!(
    /// Identifies a command policy; it is also the command name executed
    /// inside the container.
    CommandId
);
identifier_type!(
    /// Names a secret within the provider that supplies it.
    SecretName
);
identifier_type!(
    /// Names the environment variable a resolved value is injected as.
    EnvName
);

pub struct Config {
    pub providers: Vec<Provider>,
    pub contexts: Vec<DockerContext>,
    pub policies: Vec<CommandPolicy>,
}

impl Config {
    pub fn provider(&self, id: &ProviderId) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id() == id)
    }

    pub fn context(&self, id: &ContextId) -> Option<&DockerContext> {
        self.contexts.iter().find(|c| &c.id == id)
    }

    pub fn policy(&self, id: &CommandId) -> Option<&CommandPolicy> {
        self.policies.iter().find(|p| &p.id == id)
    }
}

/// A provider type is a node name in the `providers` block, so adding one is
/// a configuration-model change rather than a new field.
pub enum Provider {
    Bws(BwsProvider),
}

impl Provider {
    pub fn id(&self) -> &ProviderId {
        match self {
            Provider::Bws(p) => &p.id,
        }
    }
}

pub struct BwsProvider {
    pub id: ProviderId,
    pub project: String,
    pub access_token: ExecAcquisition,
}

/// `exec` is the only acquisition type the model defines. It invokes the
/// executable directly, without a shell.
pub struct ExecAcquisition {
    pub program: String,
    pub args: Vec<String>,
}

pub struct DockerContext {
    pub id: ContextId,
    pub runtime: String,
    pub user: String,
    pub workdir: String,
    pub container_name: String,
}

pub struct CommandPolicy {
    pub id: CommandId,
    pub secrets: Vec<SecretSpec>,
}

pub struct SecretSpec {
    pub env_name: EnvName,
    pub provider: ProviderId,
    pub secret_name: SecretName,
}

/// A configuration that does not parse and a configuration that parses but
/// violates the model call for different corrective actions, so they are
/// distinct variants.
#[derive(Debug)]
pub enum ConfigError {
    Read { path: String, source: std::io::Error },
    Parse { path: String, message: String },
    Invalid { path: String, message: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Read { path, source } => {
                write!(f, "cannot read configuration file '{path}': {source}")
            }
            ConfigError::Parse { path, message } => {
                write!(f, "configuration file '{path}' does not parse: {message}")
            }
            ConfigError::Invalid { path, message } => {
                write!(f, "configuration file '{path}' is invalid: {message}")
            }
        }
    }
}

pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let shown_path = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: shown_path.clone(),
        source,
    })?;
    parse(&text).map_err(|message| match message {
        ParseFailure::Syntax(message) => ConfigError::Parse {
            path: shown_path.clone(),
            message,
        },
        ParseFailure::Model(message) => ConfigError::Invalid {
            path: shown_path.clone(),
            message,
        },
    })
}

#[derive(Debug)]
enum ParseFailure {
    Syntax(String),
    Model(String),
}

fn invalid<T>(message: impl Into<String>) -> Result<T, ParseFailure> {
    Err(ParseFailure::Model(message.into()))
}

fn parse(text: &str) -> Result<Config, ParseFailure> {
    let document: KdlDocument = text
        .parse()
        .map_err(|e: kdl::KdlError| ParseFailure::Syntax(e.to_string()))?;

    let root = match document.nodes() {
        [single] if single.name().value() == "iwaya" => single,
        _ => return invalid("expected exactly one 'iwaya' root node"),
    };

    match root.get("version").and_then(|v| v.as_integer()) {
        Some(1) => {}
        Some(other) => return invalid(format!("unsupported configuration version {other}")),
        None => return invalid("the 'iwaya' root node requires an integer 'version' property"),
    }

    let mut config = Config {
        providers: Vec::new(),
        contexts: Vec::new(),
        policies: Vec::new(),
    };

    for section in children(root) {
        match section.name().value() {
            "providers" => {
                for node in children(section) {
                    config.providers.push(parse_provider(node)?);
                }
            }
            "contexts" => {
                for node in children(section) {
                    config.contexts.push(parse_context(node)?);
                }
            }
            "policies" => {
                for node in children(section) {
                    config.policies.push(parse_policy(node)?);
                }
            }
            other => return invalid(format!("unknown section '{other}' in the 'iwaya' node")),
        }
    }

    validate(&config)?;
    Ok(config)
}

fn children(node: &KdlNode) -> &[KdlNode] {
    node.children().map(KdlDocument::nodes).unwrap_or(&[])
}

fn positional_strings(node: &KdlNode) -> Result<Vec<&str>, ParseFailure> {
    node.entries()
        .iter()
        .filter(|e| e.name().is_none())
        .map(|e| match e.value().as_string() {
            Some(s) => Ok(s),
            None => invalid(format!(
                "node '{}' has a non-string argument",
                node.name().value()
            )),
        })
        .collect()
}

fn single_string_argument<'a>(node: &'a KdlNode, owner: &str) -> Result<&'a str, ParseFailure> {
    match positional_strings(node)?.as_slice() {
        [value] => Ok(value),
        _ => invalid(format!(
            "'{}' in {owner} requires exactly one string argument",
            node.name().value()
        )),
    }
}

fn property<'a>(node: &'a KdlNode, name: &str, owner: &str) -> Result<&'a str, ParseFailure> {
    match node.get(name).and_then(|v| v.as_string()) {
        Some(value) => Ok(value),
        None => invalid(format!(
            "'{}' in {owner} requires a string '{name}' property",
            node.name().value()
        )),
    }
}

fn parse_provider(node: &KdlNode) -> Result<Provider, ParseFailure> {
    match node.name().value() {
        "bws" => parse_bws_provider(node).map(Provider::Bws),
        other => invalid(format!("unknown provider type '{other}'")),
    }
}

fn parse_bws_provider(node: &KdlNode) -> Result<BwsProvider, ParseFailure> {
    let arguments = positional_strings(node)?;
    let [id] = arguments.as_slice() else {
        return invalid("a 'bws' provider requires exactly one identifier argument");
    };
    let owner = format!("provider '{id}'");

    let mut project = None;
    let mut access_token = None;
    for child in children(node) {
        match child.name().value() {
            "project" => {
                if project.is_some() {
                    return invalid(format!("{owner} declares 'project' more than once"));
                }
                project = Some(single_string_argument(child, &owner)?.to_string());
            }
            "access-token" => {
                if access_token.is_some() {
                    return invalid(format!("{owner} declares 'access-token' more than once"));
                }
                access_token = Some(parse_access_token(child, &owner)?);
            }
            other => return invalid(format!("unknown setting '{other}' in {owner}")),
        }
    }

    let Some(project) = project else {
        return invalid(format!("{owner} requires a 'project' setting"));
    };
    let Some(access_token) = access_token else {
        return invalid(format!("{owner} requires exactly one 'access-token' block"));
    };

    Ok(BwsProvider {
        id: ProviderId::new(id),
        project,
        access_token,
    })
}

fn parse_access_token(node: &KdlNode, owner: &str) -> Result<ExecAcquisition, ParseFailure> {
    let [acquisition] = children(node) else {
        return invalid(format!(
            "'access-token' in {owner} requires exactly one acquisition node"
        ));
    };
    match acquisition.name().value() {
        "exec" => {
            let arguments = positional_strings(acquisition)?;
            let [program, args @ ..] = arguments.as_slice() else {
                return invalid(format!(
                    "'exec' in {owner} requires an executable argument"
                ));
            };
            Ok(ExecAcquisition {
                program: program.to_string(),
                args: args.iter().map(|a| a.to_string()).collect(),
            })
        }
        other => invalid(format!(
            "unsupported acquisition type '{other}' in {owner}; 'exec' is the only acquisition type"
        )),
    }
}

fn parse_context(node: &KdlNode) -> Result<DockerContext, ParseFailure> {
    if node.name().value() != "docker" {
        return invalid(format!("unknown context type '{}'", node.name().value()));
    }
    let arguments = positional_strings(node)?;
    let [id] = arguments.as_slice() else {
        return invalid("a 'docker' context requires exactly one identifier argument");
    };
    let owner = format!("context '{id}'");

    let mut runtime = None;
    let mut user = None;
    let mut workdir = None;
    let mut container_name = None;
    for child in children(node) {
        let target = match child.name().value() {
            "runtime" => &mut runtime,
            "user" => &mut user,
            "workdir" => &mut workdir,
            "container-name" => &mut container_name,
            other => return invalid(format!("unknown setting '{other}' in {owner}")),
        };
        if target.is_some() {
            return invalid(format!(
                "{owner} declares '{}' more than once",
                child.name().value()
            ));
        }
        *target = Some(single_string_argument(child, &owner)?.to_string());
    }

    let runtime = runtime.unwrap_or_else(|| "docker".to_string());
    // A runtime carrying arguments, redirection, or command separators would
    // extend the invocation beyond the model's fixed argv shape, so it is a
    // configuration error (docs/design/configuration.md, "Docker Execution
    // Contexts"). Quotes and expansion characters are rejected with them:
    // they only make sense in a shell command string, which a runtime is not.
    if runtime.is_empty()
        || runtime
            .chars()
            .any(|c| c.is_whitespace() || ";|&<>'\"`$\\\0".contains(c))
    {
        return invalid(format!(
            "{owner} has a 'runtime' that is not a single executable"
        ));
    }

    let require = |value: Option<String>, field: &str| match value {
        Some(v) => Ok(v),
        None => invalid(format!("{owner} requires a '{field}' setting")),
    };

    Ok(DockerContext {
        id: ContextId::new(id),
        runtime,
        user: require(user, "user")?,
        workdir: require(workdir, "workdir")?,
        container_name: require(container_name, "container-name")?,
    })
}

fn parse_policy(node: &KdlNode) -> Result<CommandPolicy, ParseFailure> {
    if node.name().value() != "command" {
        return invalid(format!("unknown policy type '{}'", node.name().value()));
    }
    let arguments = positional_strings(node)?;
    let [id] = arguments.as_slice() else {
        return invalid("a 'command' policy requires exactly one identifier argument");
    };
    let owner = format!("command '{id}'");

    let mut secrets = Vec::new();
    for child in children(node) {
        if child.name().value() != "secret" {
            return invalid(format!(
                "unknown entry '{}' in {owner}",
                child.name().value()
            ));
        }
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

    Ok(CommandPolicy {
        id: CommandId::new(id),
        secrets,
    })
}

fn validate(config: &Config) -> Result<(), ParseFailure> {
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

        let mut env_names = std::collections::HashSet::new();
        for secret in &policy.secrets {
            // '=' would turn the generated `--env NAME` into the forbidden
            // `--env NAME=VALUE` shape; '-' would read as an option.
            let name = secret.env_name.as_str();
            if name.is_empty()
                || name.starts_with('-')
                || name.contains('=')
                || name.contains('\0')
            {
                return invalid(format!(
                    "command '{}' declares '{}', which is not an environment variable name",
                    policy.id, secret.env_name
                ));
            }
            if !env_names.insert(&secret.env_name) {
                return invalid(format!(
                    "command '{}' declares environment variable '{}' more than once",
                    policy.id, secret.env_name
                ));
            }
            if config.provider(&secret.provider).is_none() {
                return invalid(format!(
                    "command '{}' references unknown provider '{}' for '{}'",
                    policy.id, secret.provider, secret.env_name
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASELINE: &str = r#"
/- kdl-version 2

iwaya version=1 {
  providers {
    bws "bws-default" {
      project "philomagi.dev"

      access-token {
        exec "pass" "show" "bws/access-token"
      }
    }
  }

  contexts {
    docker "iwaya" {
      runtime "podman"
      user "vscode"
      workdir "/workspaces/iwaya"
      container-name "iwaya-dev"
    }

    docker "git-kura" {
      runtime "podman"
      user "vscode"
      workdir "/workspaces/git-kura"
      container-name "git-kura-dev"
    }
  }

  policies {
    command "claude" {
      secret \
        "ANTHROPIC_AUTH_TOKEN" \
        provider="bws-default" \
        secret-name="ANTHROPIC_AUTH_TOKEN"
    }

    command "gh" {
      secret \
        "GH_TOKEN" \
        provider="bws-default" \
        secret-name="GH_TOKEN"
    }
  }
}
"#;

    fn invalid_message(text: &str) -> String {
        match parse(text) {
            Err(ParseFailure::Model(message)) => message,
            Err(ParseFailure::Syntax(message)) => {
                panic!("expected a model error, got a syntax error: {message}")
            }
            Ok(_) => panic!("expected an error"),
        }
    }

    #[test]
    fn parses_the_baseline_example() {
        let config = parse(BASELINE).unwrap();

        let Provider::Bws(bws) = config.provider(&ProviderId::new("bws-default")).unwrap();
        assert_eq!(bws.project, "philomagi.dev");
        assert_eq!(bws.access_token.program, "pass");
        assert_eq!(bws.access_token.args, ["show", "bws/access-token"]);

        let context = config.context(&ContextId::new("iwaya")).unwrap();
        assert_eq!(context.runtime, "podman");
        assert_eq!(context.user, "vscode");
        assert_eq!(context.workdir, "/workspaces/iwaya");
        assert_eq!(context.container_name, "iwaya-dev");

        let policy = config.policy(&CommandId::new("claude")).unwrap();
        assert_eq!(policy.secrets.len(), 1);
        assert_eq!(policy.secrets[0].env_name, EnvName::new("ANTHROPIC_AUTH_TOKEN"));
        assert_eq!(policy.secrets[0].provider, ProviderId::new("bws-default"));
        assert_eq!(
            policy.secrets[0].secret_name,
            SecretName::new("ANTHROPIC_AUTH_TOKEN")
        );
    }

    #[test]
    fn runtime_defaults_to_docker() {
        let config = parse(
            r#"iwaya version=1 {
                 contexts {
                   docker "c" { user "u"; workdir "/w"; container-name "n" }
                 }
               }"#,
        )
        .unwrap();
        assert_eq!(config.context(&ContextId::new("c")).unwrap().runtime, "docker");
    }

    #[test]
    fn rejects_a_runtime_that_is_not_a_single_executable() {
        let message = invalid_message(
            r#"iwaya version=1 {
                 contexts {
                   docker "c" { runtime "podman --remote"; user "u"; workdir "/w"; container-name "n" }
                 }
               }"#,
        );
        assert!(message.contains("single executable"), "{message}");
    }

    #[test]
    fn rejects_a_context_missing_a_required_field() {
        let message = invalid_message(
            r#"iwaya version=1 {
                 contexts { docker "c" { user "u"; workdir "/w" } }
               }"#,
        );
        assert!(message.contains("container-name"), "{message}");
    }

    #[test]
    fn rejects_a_bws_provider_without_an_access_token() {
        let message = invalid_message(
            r#"iwaya version=1 {
                 providers { bws "b" { project "p" } }
               }"#,
        );
        assert!(message.contains("access-token"), "{message}");
    }

    #[test]
    fn rejects_an_exec_acquisition_without_an_executable() {
        let message = invalid_message(
            r#"iwaya version=1 {
                 providers { bws "b" { project "p"; access-token { exec } } }
               }"#,
        );
        assert!(message.contains("executable"), "{message}");
    }

    #[test]
    fn rejects_an_unsupported_acquisition_type() {
        let message = invalid_message(
            r#"iwaya version=1 {
                 providers { bws "b" { project "p"; access-token { file "/token" } } }
               }"#,
        );
        assert!(message.contains("acquisition"), "{message}");
    }

    #[test]
    fn rejects_a_runtime_containing_a_command_separator() {
        let message = invalid_message(
            r#"iwaya version=1 {
                 contexts {
                   docker "c" { runtime "podman;rm"; user "u"; workdir "/w"; container-name "n" }
                 }
               }"#,
        );
        assert!(message.contains("single executable"), "{message}");
    }

    #[test]
    fn rejects_a_container_name_that_reads_as_an_option() {
        let message = invalid_message(
            r#"iwaya version=1 {
                 contexts {
                   docker "c" { user "u"; workdir "/w"; container-name "--privileged" }
                 }
               }"#,
        );
        assert!(message.contains("container-name"), "{message}");
    }

    #[test]
    fn rejects_a_command_identifier_that_reads_as_an_option() {
        let message = invalid_message(
            r#"iwaya version=1 {
                 policies { command "--help" { } }
               }"#,
        );
        assert!(message.contains("command name"), "{message}");
    }

    #[test]
    fn rejects_an_environment_variable_name_carrying_a_value() {
        let message = invalid_message(
            r#"iwaya version=1 {
                 providers { bws "b" { project "p"; access-token { exec "true" } } }
                 policies {
                   command "c" { secret "FOO=bar" provider="b" secret-name="x" }
                 }
               }"#,
        );
        assert!(
            message.contains("not an environment variable name"),
            "{message}"
        );
    }

    #[test]
    fn rejects_argv_bound_values_containing_a_nul_byte() {
        let config = Config {
            providers: vec![],
            contexts: vec![DockerContext {
                id: ContextId::new("c"),
                runtime: "docker".to_string(),
                user: "u".to_string(),
                workdir: "/w\0".to_string(),
                container_name: "n".to_string(),
            }],
            policies: vec![],
        };
        match validate(&config) {
            Err(ParseFailure::Model(message)) => {
                assert!(message.contains("NUL"), "{message}")
            }
            _ => panic!("expected a model error"),
        }
    }

    #[test]
    fn rejects_duplicate_identifiers() {
        let message = invalid_message(
            r#"iwaya version=1 {
                 contexts {
                   docker "c" { user "u"; workdir "/w"; container-name "n" }
                   docker "c" { user "u"; workdir "/w"; container-name "n" }
                 }
               }"#,
        );
        assert!(message.contains("duplicate context"), "{message}");
    }

    #[test]
    fn rejects_duplicate_environment_variable_names_within_a_policy() {
        let message = invalid_message(
            r#"iwaya version=1 {
                 providers { bws "b" { project "p"; access-token { exec "true" } } }
                 policies {
                   command "c" {
                     secret "A" provider="b" secret-name="x"
                     secret "A" provider="b" secret-name="y"
                   }
                 }
               }"#,
        );
        assert!(message.contains("more than once"), "{message}");
    }

    #[test]
    fn rejects_a_reference_to_an_unknown_provider() {
        let message = invalid_message(
            r#"iwaya version=1 {
                 policies {
                   command "c" { secret "A" provider="nope" secret-name="x" }
                 }
               }"#,
        );
        assert!(message.contains("unknown provider 'nope'"), "{message}");
    }

    #[test]
    fn rejects_a_missing_version() {
        let message = invalid_message("iwaya { }");
        assert!(message.contains("version"), "{message}");
    }
}
