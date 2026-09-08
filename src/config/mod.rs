//! Loads and validates the three configuration layers defined in
//! docs/design/configuration.md: secret providers, Docker execution contexts,
//! and command policies.
//!
//! Validation covers the whole configuration, not only the entries an
//! invocation selects, so a defect is reported when it is introduced
//! (docs/design/docker-execution.md, "Validation Precedes Secret Resolution").
//!
//! Each configuration layer parses in its own submodule. This module owns the
//! document frame (root node, version, section dispatch), the identifier
//! types, the shared KDL helpers, and the error types.

mod context;
mod policy;
mod provider;
mod validate;

#[cfg(test)]
mod tests;

pub use context::DockerContext;
pub(crate) use validate::is_http_origin;
pub use policy::{CommandPolicy, ProxySecretSpec};
pub use provider::{BwsProvider, Provider};
// SecretSpec and InjectHeader are named only by tests today, and
// ExecAcquisition is not yet named outside its defining module; they stay
// re-exported so the configuration types remain one flat namespace for the
// rest of the crate.
#[allow(unused_imports)]
pub use policy::{InjectHeader, SecretSpec};
#[allow(unused_imports)]
pub use provider::ExecAcquisition;

use std::fmt;
use std::path::Path;

use kdl::{KdlDocument, KdlNode};

use context::parse_context;
use policy::parse_policy;
use provider::parse_provider;
use validate::validate;

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
