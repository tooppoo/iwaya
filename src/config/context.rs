//! The `contexts` section: where a command runs
//! (docs/design/configuration.md, "Docker Execution Contexts").

use kdl::KdlNode;

use super::{
    ContextId, ParseFailure, children, invalid, positional_strings, single_string_argument,
};

pub struct DockerContext {
    pub id: ContextId,
    pub runtime: String,
    pub user: String,
    pub workdir: String,
    pub container_name: String,
}

pub(super) fn parse_context(node: &KdlNode) -> Result<DockerContext, ParseFailure> {
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
