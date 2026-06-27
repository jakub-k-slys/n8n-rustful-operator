pub mod community;
pub mod database;
pub mod logging;
pub mod redis;
pub mod smtp;
pub mod storage;

use crate::spec::{Cluster, EnvVar, NetworkingSpec, SecretKeyRef};
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
        var("N8N_EDITOR_BASE_URL", base),
    ]
}

/// `WEBHOOK_URL` for a host (`protocol://host/`). Unlike the per-role host vars,
/// this is the externally-reachable webhook base and must be the same on every
/// role so the URLs n8n registers and displays all agree.
pub fn webhook_url_env(host: &str, protocol: &str) -> EnvVar {
    EnvVar {
        name: "WEBHOOK_URL".to_string(),
        value: Some(format!("{protocol}://{host}/")),
        value_from: None,
    }
}

/// Cluster-wide `WEBHOOK_URL`: the dedicated webhook role's host when it has
/// one, otherwise the main host. Applied to all roles. `None` when neither host
/// is set.
pub fn cluster_webhook_url(c: &Cluster) -> Option<EnvVar> {
    let (host, net) = match c.spec.webhooks.as_ref().filter(|w| w.host.is_some()) {
        Some(w) => (w.host.as_deref(), w.networking.as_ref()),
        None => (c.spec.main.host.as_deref(), c.spec.main.networking.as_ref()),
    };
    host.map(|h| webhook_url_env(h, protocol_for(net)))
}

/// The operator-derived env defaults shared by a cluster role: the role's
/// host-derived URLs, the cluster-wide SMTP / logging / community-node settings,
/// and the cluster-wide webhook URL. Built immutably; pass to `build_user_env`
/// where `extraEnv` can still override any entry.
pub fn cluster_role_defaults(c: &Cluster, host: Option<&str>, net: Option<&NetworkingSpec>) -> Vec<EnvVar> {
    [
        host_env(host, protocol_for(net)),
        c.spec.smtp.as_ref().map(smtp::build_smtp_env).unwrap_or_default(),
        c.spec
            .logging
            .as_ref()
            .map(logging::build_logging_env)
            .unwrap_or_default(),
        c.spec
            .community_nodes
            .as_ref()
            .map(community::build_community_env)
            .unwrap_or_default(),
        cluster_webhook_url(c).into_iter().collect(),
    ]
    .concat()
}
