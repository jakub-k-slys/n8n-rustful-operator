use crate::spec::{CommunityNodesConfig, EnvVar};
use serde_json::{Value, json};

/// Map `CommunityNodesConfig` to n8n's community-package env. `packages` switch
/// on declarative management (`N8N_COMMUNITY_PACKAGES` + `…_MANAGED_BY_ENV`);
/// `sharedStorage` is volume wiring and emits no env here. Returned as `EnvVar`s
/// so it rides the low-priority `build_user_env` layer (overridable by extraEnv).
pub fn build_community_env(cn: &CommunityNodesConfig) -> Vec<EnvVar> {
    let lit = |name: &str, value: String| EnvVar {
        name: name.to_string(),
        value: Some(value),
        value_from: None,
    };
    let mut out = Vec::new();
    if let Some(enabled) = cn.enabled {
        out.push(lit("N8N_COMMUNITY_PACKAGES_ENABLED", enabled.to_string()));
    }
    if !cn.packages.is_empty() {
        let arr: Vec<Value> = cn
            .packages
            .iter()
            .map(|p| {
                let mut o = serde_json::Map::new();
                o.insert("name".to_string(), json!(p.name));
                if let Some(v) = &p.version {
                    o.insert("version".to_string(), json!(v));
                }
                if let Some(c) = &p.checksum {
                    o.insert("checksum".to_string(), json!(c));
                }
                Value::Object(o)
            })
            .collect();
        out.push(lit("N8N_COMMUNITY_PACKAGES", Value::Array(arr).to_string()));
        out.push(lit("N8N_COMMUNITY_PACKAGES_MANAGED_BY_ENV", "true".to_string()));
    }
    if let Some(r) = cn.reinstall_missing {
        out.push(lit("N8N_REINSTALL_MISSING_PACKAGES", r.to_string()));
    }
    out
}
