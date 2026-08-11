//! Resolves secrets through the `bws` CLI, as defined in
//! docs/design/configuration.md ("BWS Access Token", "BWS Secret Resolution").
//!
//! The acquired access token reaches the `bws` subprocess only as
//! `BWS_ACCESS_TOKEN` in its environment, never as an argv value, and it is
//! never inherited from the invoking environment. Diagnostics from this module
//! name identifiers and locations, never a token or a resolved value.

use std::collections::HashMap;
use std::fmt;
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::config::{BwsProvider, SecretName};
use crate::secret::Secret;

pub enum ResolveError {
    AcquisitionStart {
        provider: String,
        program: String,
        source: std::io::Error,
    },
    AcquisitionFailed {
        provider: String,
        program: String,
        status: String,
    },
    AcquisitionEmpty {
        provider: String,
        program: String,
    },
    CliStart {
        provider: String,
        source: std::io::Error,
    },
    CliFailed {
        provider: String,
        action: String,
        status: String,
        stderr: String,
    },
    CliOutputUnreadable {
        provider: String,
        action: String,
        message: String,
    },
    UnknownProject {
        provider: String,
        project: String,
    },
    UnknownSecret {
        provider: String,
        project: String,
        secret_name: String,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::AcquisitionStart { provider, program, source } => write!(
                f,
                "provider '{provider}': cannot start access-token command '{program}': {source}"
            ),
            ResolveError::AcquisitionFailed { provider, program, status } => write!(
                f,
                "provider '{provider}': access-token command '{program}' failed ({status})"
            ),
            ResolveError::AcquisitionEmpty { provider, program } => write!(
                f,
                "provider '{provider}': access-token command '{program}' produced an empty value"
            ),
            ResolveError::CliStart { provider, source } => write!(
                f,
                "provider '{provider}': cannot start the 'bws' CLI: {source}"
            ),
            ResolveError::CliFailed { provider, action, status, stderr } => write!(
                f,
                "provider '{provider}': 'bws' failed to {action} ({status}): {stderr}"
            ),
            ResolveError::CliOutputUnreadable { provider, action, message } => write!(
                f,
                "provider '{provider}': cannot read 'bws' output while trying to {action}: {message}"
            ),
            ResolveError::UnknownProject { provider, project } => write!(
                f,
                "provider '{provider}': project '{project}' was not found in the BWS account"
            ),
            ResolveError::UnknownSecret { provider, project, secret_name } => write!(
                f,
                "provider '{provider}': secret '{secret_name}' was not found in project '{project}'"
            ),
        }
    }
}

#[derive(Deserialize)]
struct BwsProject {
    id: String,
    name: String,
}

/// `value` is a raw secret; the struct stays private, derives no formatting
/// trait, and is wrapped into `Secret` immediately after deserialization.
#[derive(Deserialize)]
struct BwsSecret {
    key: String,
    value: String,
}

/// Resolves every requested secret from one provider, acquiring the access
/// token first. Acquisition failure means no secret is resolved from this
/// provider (docs/design/configuration.md, "BWS Access Token").
pub fn resolve(
    provider: &BwsProvider,
    names: &[&SecretName],
) -> Result<HashMap<SecretName, Secret>, ResolveError> {
    let token = acquire_access_token(provider)?;

    let projects: Vec<BwsProject> =
        bws_json(provider, &token, &["project", "list"], "list projects")?;
    let project = projects
        .into_iter()
        .find(|p| p.name == provider.project)
        .ok_or_else(|| ResolveError::UnknownProject {
            provider: provider.id.to_string(),
            project: provider.project.clone(),
        })?;

    let secrets: Vec<BwsSecret> = bws_json(
        provider,
        &token,
        &["secret", "list", &project.id],
        "list secrets",
    )?;
    let by_key: HashMap<String, Secret> = secrets
        .into_iter()
        .map(|s| (s.key, Secret::new(s.value)))
        .collect();

    let mut resolved = HashMap::new();
    for name in names {
        let value = by_key
            .get(name.as_str())
            .cloned()
            .ok_or_else(|| ResolveError::UnknownSecret {
                provider: provider.id.to_string(),
                project: provider.project.clone(),
                secret_name: name.to_string(),
            })?;
        resolved.insert((*name).clone(), value);
    }
    Ok(resolved)
}

/// Runs the configured acquisition command directly, without a shell. stdin
/// and stderr stay attached to the caller's terminal so an interactive
/// credential store (a GPG pinentry, for example) can prompt; only stdout is
/// captured, because stdout is the token.
fn acquire_access_token(provider: &BwsProvider) -> Result<Secret, ResolveError> {
    let acquisition = &provider.access_token;
    let output = Command::new(&acquisition.program)
        .args(&acquisition.args)
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|child| child.wait_with_output())
        .map_err(|source| ResolveError::AcquisitionStart {
            provider: provider.id.to_string(),
            program: acquisition.program.clone(),
            source,
        })?;

    if !output.status.success() {
        return Err(ResolveError::AcquisitionFailed {
            provider: provider.id.to_string(),
            program: acquisition.program.clone(),
            status: output.status.to_string(),
        });
    }

    // The command's stdout supplies the token, and it must never appear in a
    // diagnostic, so a token that is not UTF-8 is reported without content.
    let stdout = String::from_utf8(output.stdout).map_err(|_| ResolveError::AcquisitionFailed {
        provider: provider.id.to_string(),
        program: acquisition.program.clone(),
        status: "stdout is not valid UTF-8".to_string(),
    })?;
    let token = strip_one_trailing_line_ending(&stdout);

    if token.is_empty() {
        return Err(ResolveError::AcquisitionEmpty {
            provider: provider.id.to_string(),
            program: acquisition.program.clone(),
        });
    }
    Ok(Secret::new(token.to_string()))
}

/// Exactly one trailing `\n` or `\r\n` is removed; other stdout content is
/// preserved (docs/design/configuration.md, "BWS Access Token").
fn strip_one_trailing_line_ending(stdout: &str) -> &str {
    stdout
        .strip_suffix("\r\n")
        .or_else(|| stdout.strip_suffix('\n'))
        .unwrap_or(stdout)
}

fn bws_json<T: serde::de::DeserializeOwned>(
    provider: &BwsProvider,
    token: &Secret,
    args: &[&str],
    action: &str,
) -> Result<Vec<T>, ResolveError> {
    let output = Command::new("bws")
        .args(args)
        .args(["--output", "json"])
        // Always the acquired value, overwriting a same-named variable in the
        // invoking environment; never inherited, never a fallback.
        .env("BWS_ACCESS_TOKEN", token.expose_to_subprocess_env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| ResolveError::CliStart {
            provider: provider.id.to_string(),
            source,
        })?;

    if !output.status.success() {
        return Err(ResolveError::CliFailed {
            provider: provider.id.to_string(),
            action: action.to_string(),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    // stdout carries resolved secret values, so a parse failure reports only
    // the parser's position message, never the content.
    serde_json::from_slice(&output.stdout).map_err(|e| ResolveError::CliOutputUnreadable {
        provider: provider.id.to_string(),
        action: action.to_string(),
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::strip_one_trailing_line_ending;

    #[test]
    fn strips_exactly_one_trailing_line_ending() {
        assert_eq!(strip_one_trailing_line_ending("token\n"), "token");
        assert_eq!(strip_one_trailing_line_ending("token\r\n"), "token");
        assert_eq!(strip_one_trailing_line_ending("token\n\n"), "token\n");
        assert_eq!(strip_one_trailing_line_ending("token"), "token");
        assert_eq!(strip_one_trailing_line_ending(""), "");
    }
}
