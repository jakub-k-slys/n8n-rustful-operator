//! Unit coverage for `env::build_user_env` — the precedence/dedup logic behind
//! `secureCookie` and `extraEnv`. Pure function, no cluster needed (the BDD
//! suite in `features/` exercises the wired-up behaviour against kind).

use n8n_rustful_operator::{EnvVar, env::build_user_env};
use serde_json::json;

fn ev(name: &str, value: &str) -> EnvVar {
    EnvVar {
        name: name.to_string(),
        value: value.to_string(),
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
