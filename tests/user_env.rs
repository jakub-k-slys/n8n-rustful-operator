//! Unit coverage for `env::build_user_env` — the precedence/dedup logic behind
//! `secureCookie` and `extraEnv`. Pure function, no cluster needed (the BDD
//! suite in `features/` exercises the wired-up behaviour against kind).

use n8n_rustful_operator::{EnvVar, EnvVarSource, SecretKeyRef, env::build_user_env};
use serde_json::json;

fn ev(name: &str, value: &str) -> EnvVar {
    EnvVar {
        name: name.to_string(),
        value: Some(value.to_string()),
        value_from: None,
    }
}

fn ev_secret(name: &str, secret: &str, key: &str) -> EnvVar {
    EnvVar {
        name: name.to_string(),
        value: None,
        value_from: Some(EnvVarSource {
            secret_ref: SecretKeyRef {
                name: secret.to_string(),
                key: key.to_string(),
            },
        }),
    }
}

#[test]
fn secure_cookie_sugar_renders_env() {
    assert_eq!(
        build_user_env(Some(false), &[], &[]),
        vec![json!({ "name": "N8N_SECURE_COOKIE", "value": "false" })]
    );
}

#[test]
fn unset_secure_cookie_emits_nothing() {
    assert!(build_user_env(None, &[], &[]).is_empty());
}

#[test]
fn extra_env_overrides_secure_cookie_and_dedups() {
    // cluster extraEnv wins over the secureCookie sugar; no duplicate names.
    let cluster = [ev("N8N_SECURE_COOKIE", "true"), ev("N8N_PROXY_HOPS", "1")];
    let out = build_user_env(Some(false), &cluster, &[]);
    assert_eq!(
        out,
        vec![
            json!({ "name": "N8N_SECURE_COOKIE", "value": "true" }),
            json!({ "name": "N8N_PROXY_HOPS", "value": "1" }),
        ]
    );
}

#[test]
fn role_extra_env_overrides_cluster() {
    let cluster = [ev("FOO", "cluster")];
    let role = [ev("FOO", "role")];
    assert_eq!(
        build_user_env(None, &cluster, &role),
        vec![json!({ "name": "FOO", "value": "role" })]
    );
}

#[test]
fn value_from_renders_secret_key_ref() {
    let cluster = [ev_secret("ANTHROPIC_API_KEY", "n8n-secret", "ANTHROPIC_API_KEY")];
    assert_eq!(
        build_user_env(None, &cluster, &[]),
        vec![json!({
            "name": "ANTHROPIC_API_KEY",
            "valueFrom": { "secretKeyRef": { "name": "n8n-secret", "key": "ANTHROPIC_API_KEY" } }
        })]
    );
}

#[test]
fn role_value_from_overrides_cluster_value() {
    // a per-role secret ref wins over a cluster-level inline value of the same name
    let cluster = [ev("TOKEN", "inline")];
    let role = [ev_secret("TOKEN", "s", "tok")];
    assert_eq!(
        build_user_env(None, &cluster, &role),
        vec![json!({
            "name": "TOKEN",
            "valueFrom": { "secretKeyRef": { "name": "s", "key": "tok" } }
        })]
    );
}
