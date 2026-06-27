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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::CommunityPackage;

    #[test]
    fn packages_enable_declarative_management() {
        let cn = CommunityNodesConfig {
            enabled: Some(true),
            packages: vec![
                CommunityPackage {
                    name: "n8n-nodes-foo".into(),
                    version: Some("1.2.3".into()),
                    checksum: None,
                },
                CommunityPackage {
                    name: "n8n-nodes-bar".into(),
                    version: None,
                    checksum: None,
                },
            ],
            shared_storage: None,
            reinstall_missing: None,
        };
        let env = build_community_env(&cn);
        let by_name = |n: &str| env.iter().find(|e| e.name == n).and_then(|e| e.value.as_deref());
        assert_eq!(by_name("N8N_COMMUNITY_PACKAGES_ENABLED"), Some("true"));
        assert_eq!(by_name("N8N_COMMUNITY_PACKAGES_MANAGED_BY_ENV"), Some("true"));
        assert_eq!(
            by_name("N8N_COMMUNITY_PACKAGES"),
            Some(r#"[{"name":"n8n-nodes-foo","version":"1.2.3"},{"name":"n8n-nodes-bar"}]"#)
        );
        // no reinstall when using declarative packages
        assert!(by_name("N8N_REINSTALL_MISSING_PACKAGES").is_none());
    }

    #[test]
    fn shared_storage_emits_no_env() {
        // sharedStorage is volume wiring; without packages/reinstall it yields nothing
        let cn = CommunityNodesConfig {
            enabled: None,
            packages: vec![],
            shared_storage: Some(crate::spec::SharedStorage {
                size: "5Gi".into(),
                storage_class_name: None,
                access_mode: "ReadWriteMany".into(),
            }),
            reinstall_missing: None,
        };
        assert!(build_community_env(&cn).is_empty());
    }
}
