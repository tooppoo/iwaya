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

/// One policy body under a fixed provider, so proxy-secret tests state
/// only what they vary.
fn policy_config(policy_body: &str) -> String {
    format!(
        r#"iwaya version=1 {{
             providers {{ bws "b" {{ project "p"; access-token {{ exec "true" }} }} }}
             policies {{ command "c" {{ {policy_body} }} }}
           }}"#
    )
}

fn proxy_secret(upstream: &str, base_url_env: &str, inject_header: &str) -> String {
    format!(
        r#"proxy-secret "A" {{
             provider "b"
             secret-name "x"
             upstream "{upstream}"
             base-url-env "{base_url_env}"
             inject-header {inject_header}
           }}"#
    )
}

#[test]
fn parses_the_documented_proxy_secret() {
    let config = parse(&policy_config(
        r#"proxy-secret "ANTHROPIC_AUTH_TOKEN" {
             provider "b"
             secret-name "ANTHROPIC_AUTH_TOKEN"
             upstream "https://api.anthropic.com"
             base-url-env "ANTHROPIC_BASE_URL"
             inject-header "x-api-key" "{}"
           }"#,
    ))
    .unwrap();

    let policy = config.policy(&CommandId::new("c")).unwrap();
    assert!(policy.secrets.is_empty());
    let [proxy] = policy.proxy_secrets.as_slice() else {
        panic!("expected exactly one proxy-secret");
    };
    assert_eq!(proxy.env_name, EnvName::new("ANTHROPIC_AUTH_TOKEN"));
    assert_eq!(proxy.provider, ProviderId::new("b"));
    assert_eq!(proxy.secret_name, SecretName::new("ANTHROPIC_AUTH_TOKEN"));
    assert_eq!(proxy.upstream, "https://api.anthropic.com");
    assert_eq!(proxy.base_url_env, EnvName::new("ANTHROPIC_BASE_URL"));
    assert_eq!(proxy.inject_header.name, "x-api-key");
    assert_eq!(proxy.inject_header.template, "{}");
}

#[test]
fn rejects_a_proxy_secret_without_an_environment_variable_name() {
    let message = invalid_message(&policy_config(r#"proxy-secret { provider "b" }"#));
    assert!(
        message.contains("exactly one environment variable name argument"),
        "{message}"
    );
}

#[test]
fn rejects_a_proxy_secret_missing_a_required_setting() {
    let message = invalid_message(&policy_config(
        r#"proxy-secret "A" {
             provider "b"
             secret-name "x"
             base-url-env "B"
             inject-header "x-api-key" "{}"
           }"#,
    ));
    assert!(message.contains("upstream"), "{message}");
}

#[test]
fn rejects_a_proxy_secret_missing_an_inject_header() {
    let message = invalid_message(&policy_config(
        r#"proxy-secret "A" {
             provider "b"
             secret-name "x"
             upstream "https://u.example"
             base-url-env "B"
           }"#,
    ));
    assert!(message.contains("inject-header"), "{message}");
}

#[test]
fn rejects_a_proxy_secret_declaring_a_setting_more_than_once() {
    let message = invalid_message(&policy_config(
        r#"proxy-secret "A" {
             provider "b"
             provider "b"
             secret-name "x"
             upstream "https://u.example"
             base-url-env "B"
             inject-header "x-api-key" "{}"
           }"#,
    ));
    assert!(message.contains("more than once"), "{message}");
}

#[test]
fn rejects_an_unknown_setting_in_a_proxy_secret() {
    let message = invalid_message(&policy_config(
        r#"proxy-secret "A" {
             provider "b"
             secret-name "x"
             upstream "https://u.example"
             base-url-env "B"
             inject-header "x-api-key" "{}"
             follow-redirects "yes"
           }"#,
    ));
    assert!(message.contains("unknown setting 'follow-redirects'"), "{message}");
}

#[test]
fn rejects_an_inject_header_without_a_template() {
    let message = invalid_message(&policy_config(&proxy_secret(
        "https://u.example",
        "B",
        r#""x-api-key""#,
    )));
    assert!(message.contains("template argument"), "{message}");
}

#[test]
fn rejects_an_inject_header_name_that_is_not_a_header_name() {
    let message = invalid_message(&policy_config(&proxy_secret(
        "https://u.example",
        "B",
        r#""x api key" "{}""#,
    )));
    assert!(message.contains("HTTP header name"), "{message}");
}

#[test]
fn rejects_an_inject_header_template_without_exactly_one_placeholder() {
    for template in [r#""x-api-key" "token""#, r#""x-api-key" "{} {}""#] {
        let message = invalid_message(&policy_config(&proxy_secret(
            "https://u.example",
            "B",
            template,
        )));
        assert!(message.contains("exactly one '{}' placeholder"), "{message}");
    }
}

#[test]
fn rejects_an_inject_header_template_containing_a_control_character() {
    let message = invalid_message(&policy_config(&proxy_secret(
        "https://u.example",
        "B",
        r#""x-api-key" "{}\n""#,
    )));
    assert!(message.contains("HTTP header value"), "{message}");
}

#[test]
fn accepts_an_upstream_origin_with_a_port() {
    for upstream in [
        "https://u.example:8443",
        "http://127.0.0.1:8080",
        "https://[::1]:8080",
    ] {
        parse(&policy_config(&proxy_secret(
            upstream,
            "B",
            r#""x-api-key" "{}""#,
        )))
        .unwrap();
    }
}

#[test]
fn rejects_an_upstream_that_is_not_an_origin() {
    for upstream in [
        "u.example",
        "https://u.example/v1",
        "https://",
        "https://user@u.example",
        "ftp://u.example",
        "https://:8080",
        "https://u.example:not-a-port",
        "https://u.example:80:90",
        "https://u.example:99999",
        "https://[::1",
    ] {
        let message = invalid_message(&policy_config(&proxy_secret(
            upstream,
            "B",
            r#""x-api-key" "{}""#,
        )));
        assert!(message.contains("http(s) origin"), "{message}");
    }
}

#[test]
fn rejects_an_inject_header_naming_a_header_the_proxy_controls() {
    for name in ["host", "Content-Length", "transfer-encoding", "Connection"] {
        let message = invalid_message(&policy_config(&proxy_secret(
            "https://u.example",
            "B",
            &format!(r#""{name}" "{{}}""#),
        )));
        assert!(message.contains("the proxy itself controls"), "{message}");
    }
}

#[test]
fn parses_a_policy_mixing_secret_and_proxy_secret() {
    let body = format!(
        r#"secret "S" provider="b" secret-name="s"
           {}"#,
        proxy_secret("https://u.example", "B", r#""x-api-key" "{}""#)
    );
    let config = parse(&policy_config(&body)).unwrap();
    let policy = config.policy(&CommandId::new("c")).unwrap();
    assert_eq!(policy.secrets.len(), 1);
    assert_eq!(policy.secrets[0].env_name, EnvName::new("S"));
    assert_eq!(policy.proxy_secrets.len(), 1);
    assert_eq!(policy.proxy_secrets[0].env_name, EnvName::new("A"));
}

#[test]
fn rejects_a_base_url_env_that_is_not_an_environment_variable_name() {
    let message = invalid_message(&policy_config(&proxy_secret(
        "https://u.example",
        "B=1",
        r#""x-api-key" "{}""#,
    )));
    assert!(
        message.contains("not an environment variable name"),
        "{message}"
    );
}

#[test]
fn rejects_a_base_url_env_that_collides_with_a_secret_env_name() {
    let body = format!(
        r#"secret "B" provider="b" secret-name="y"
           {}"#,
        proxy_secret("https://u.example", "B", r#""x-api-key" "{}""#)
    );
    let message = invalid_message(&policy_config(&body));
    assert!(message.contains("more than once"), "{message}");
}

#[test]
fn rejects_a_base_url_env_that_collides_with_its_own_credential_name() {
    let message = invalid_message(&policy_config(&proxy_secret(
        "https://u.example",
        "A",
        r#""x-api-key" "{}""#,
    )));
    assert!(message.contains("more than once"), "{message}");
}

#[test]
fn rejects_a_proxy_secret_referencing_an_unknown_provider() {
    let message = invalid_message(&policy_config(
        r#"proxy-secret "A" {
             provider "nope"
             secret-name "x"
             upstream "https://u.example"
             base-url-env "B"
             inject-header "x-api-key" "{}"
           }"#,
    ));
    assert!(message.contains("unknown provider 'nope'"), "{message}");
}
