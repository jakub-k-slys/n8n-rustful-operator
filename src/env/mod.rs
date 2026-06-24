pub mod database;
pub mod redis;
pub mod storage;

use crate::spec::{EnvVar, NetworkingSpec, SecretKeyRef};
use serde_json::{Value, json};

pub fn env_str(name: &str, value: impl Into<Value>) -> Value {
    json!({
        "name": name,
        "value": value.into().to_string().trim_matches('"').to_string()
    })
}

pub fn env_secret(name: &str, sec: &SecretKeyRef) -> Value {
    json!({
        "name": name,
        "valueFrom": { "secretKeyRef": { "name": sec.name, "key": sec.key } }
    })
}

/// Render the user-controlled env for a role into container-env JSON: the
/// operator `defaults` (e.g. host-derived URLs), the `secureCookie` sugar, the
/// cluster-wide `extraEnv`, and the per-role `extraEnv`, layered low→high.
/// Later layers win on a name clash, and the result is de-duplicated by name
/// (so the container never gets two entries for the same variable, and any
/// default can be overridden via `extraEnv`). Each entry renders as either an
/// inline `value` or a `valueFrom.secretKeyRef`. For a `Single`, pass an empty
/// `role` slice.
pub fn build_user_env(
    defaults: &[EnvVar],
    secure_cookie: Option<bool>,
    cluster: &[EnvVar],
    role: &[EnvVar],
) -> Vec<Value> {
    let cookie = secure_cookie.map(|v| EnvVar {
        name: "N8N_SECURE_COOKIE".to_string(),
        value: Some(v.to_string()),
        value_from: None,
    });
    let layered = defaults.iter().chain(cookie.iter()).chain(cluster).chain(role);
    let mut order: Vec<&str> = Vec::new();
    let mut latest: std::collections::HashMap<&str, &EnvVar> = std::collections::HashMap::new();
    for e in layered {
        if !latest.contains_key(e.name.as_str()) {
            order.push(&e.name);
        }
        latest.insert(&e.name, e);
    }
    order
        .into_iter()
        .map(|name| render_user_env(latest[name]))
        .collect()
}

/// One `EnvVar` → container-env JSON. A `valueFrom` entry becomes a
/// `secretKeyRef`; otherwise the inline `value` (defaulting to empty) is used.
fn render_user_env(e: &EnvVar) -> Value {
    match &e.value_from {
        Some(src) => env_secret(&e.name, &src.secret_ref),
        None => json!({ "name": e.name, "value": e.value.clone().unwrap_or_default() }),
    }
}

/// `https` when the networking terminates TLS (an Ingress with a TLS secret, or
/// any HTTPRoute — Gateways conventionally terminate TLS), otherwise `http`.
pub fn protocol_for(net: Option<&NetworkingSpec>) -> &'static str {
    match net {
        Some(n) if n.http_route.is_some() => "https",
        Some(n) if n.ingress.as_ref().is_some_and(|i| i.tls_secret_name.is_some()) => "https",
        _ => "http",
    }
}

/// Host-derived n8n URL env (`N8N_HOST`, `N8N_PROTOCOL`, `WEBHOOK_URL`,
/// `N8N_EDITOR_BASE_URL`) used as the lowest `build_user_env` layer so a role
/// generates correct webhook/OAuth URLs without hand-written `extraEnv`. Empty
/// when no `host` is set; every entry is overridable via `extraEnv`.
pub fn host_env(host: Option<&str>, protocol: &str) -> Vec<EnvVar> {
    let Some(h) = host else { return Vec::new() };
    let base = format!("{protocol}://{h}");
    let var = |name: &str, value: String| EnvVar {
        name: name.to_string(),
        value: Some(value),
        value_from: None,
    };
    vec![
        var("N8N_HOST", h.to_string()),
        var("N8N_PROTOCOL", protocol.to_string()),
        var("WEBHOOK_URL", format!("{base}/")),
        var("N8N_EDITOR_BASE_URL", base),
    ]
}
