//! The `providers` section: how a secret is obtained
//! (docs/design/configuration.md, "Secret Providers").

use kdl::KdlNode;

use super::{
    ParseFailure, ProviderId, children, invalid, positional_strings, single_string_argument,
};

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

pub(super) fn parse_provider(node: &KdlNode) -> Result<Provider, ParseFailure> {
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
