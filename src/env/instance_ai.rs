use crate::spec::AssistantSpec;
use std::collections::BTreeMap;

/// Contents of `Secret <target>-instance-ai`, pulled into the n8n container
/// with `envFrom`. `model_api_key` and `brave_api_key` are already-resolved
/// values copied from the user's Secrets.
pub fn build_instance_ai_data(
    spec: &AssistantSpec,
    sandbox_url: &str,
    sandbox_api_key: &str,
    model_api_key: Option<&str>,
    brave_api_key: Option<&str>,
) -> BTreeMap<String, String> {
    let mut d = BTreeMap::new();
    let mut set = |k: &str, v: &str| {
        d.insert(k.to_string(), v.to_string());
    };
    set("N8N_ENABLED_MODULES", "instance-ai");
    set("N8N_INSTANCE_AI_MODEL", &spec.model.name);
    set("N8N_INSTANCE_AI_SANDBOX_ENABLED", "true");
    set("N8N_INSTANCE_AI_SANDBOX_PROVIDER", "n8n-sandbox");
    set("N8N_INSTANCE_AI_SANDBOX_IMAGE", &spec.sandbox.sandbox_image);
    set("N8N_INSTANCE_AI_SANDBOX_API_URL", sandbox_url);
    set("N8N_SANDBOX_SERVICE_URL", sandbox_url);
    set("N8N_SANDBOX_SERVICE_API_KEY", sandbox_api_key);
    if let Some(u) = &spec.model.url {
        set("N8N_INSTANCE_AI_MODEL_URL", u);
    }
    if let Some(k) = model_api_key {
        set("N8N_INSTANCE_AI_MODEL_API_KEY", k);
    }
    // Deliberately unprefixed — the n8n docs require this exact name.
    if let Some(k) = brave_api_key {
        set("INSTANCE_AI_BRAVE_SEARCH_API_KEY", k);
    }
    if let Some(s) = &spec.search
        && let Some(sx) = &s.searxng
    {
        set("N8N_INSTANCE_AI_SEARXNG_URL", &sx.url);
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{
        AssistantSpec, BraveConfig, ModelConfig, SandboxConfig, SearchConfig, SearxngConfig, SecretKeyRef,
        TargetRef,
    };

    fn spec() -> AssistantSpec {
        AssistantSpec {
            target_ref: TargetRef {
                kind: "Cluster".into(),
                name: "n8n".into(),
            },
            model: ModelConfig {
                name: "anthropic/claude-opus-4-8".into(),
                api_key_secret: Some(SecretKeyRef {
                    name: "n8n-ai".into(),
                    key: "ANTHROPIC_API_KEY".into(),
                }),
                url: None,
            },
            search: None,
            sandbox: SandboxConfig::default(),
        }
    }

    const URL: &str = "http://n8n-cluster-n8n-sandbox-api.n8n-sandbox.svc:8080";

    #[test]
    fn wires_the_module_sandbox_and_model() {
        let d = build_instance_ai_data(&spec(), URL, "k3y", Some("sk-ant-x"), None);
        assert_eq!(d["N8N_ENABLED_MODULES"], "instance-ai");
        assert_eq!(d["N8N_INSTANCE_AI_MODEL"], "anthropic/claude-opus-4-8");
        assert_eq!(d["N8N_INSTANCE_AI_MODEL_API_KEY"], "sk-ant-x");
        assert_eq!(d["N8N_INSTANCE_AI_SANDBOX_ENABLED"], "true");
        assert_eq!(d["N8N_INSTANCE_AI_SANDBOX_PROVIDER"], "n8n-sandbox");
        assert_eq!(
            d["N8N_INSTANCE_AI_SANDBOX_IMAGE"],
            "ghcr.io/n8n-io/n8n-sandbox-service-sandbox:latest"
        );
        assert_eq!(d["N8N_INSTANCE_AI_SANDBOX_API_URL"], URL);
        assert_eq!(d["N8N_SANDBOX_SERVICE_URL"], URL);
        assert_eq!(d["N8N_SANDBOX_SERVICE_API_KEY"], "k3y");
    }

    #[test]
    fn omits_the_model_key_entry_when_there_is_none() {
        let mut s = spec();
        s.model.api_key_secret = None;
        s.model.url = Some("http://lmstudio:1234/v1".into());
        let d = build_instance_ai_data(&s, URL, "k3y", None, None);
        assert!(!d.contains_key("N8N_INSTANCE_AI_MODEL_API_KEY"));
        assert_eq!(d["N8N_INSTANCE_AI_MODEL_URL"], "http://lmstudio:1234/v1");
    }

    #[test]
    fn brave_key_keeps_its_unprefixed_name() {
        let mut s = spec();
        s.search = Some(SearchConfig {
            brave: Some(BraveConfig {
                api_key_secret: SecretKeyRef {
                    name: "n8n-ai".into(),
                    key: "BRAVE_API_KEY".into(),
                },
            }),
            searxng: None,
        });
        let d = build_instance_ai_data(&s, URL, "k3y", Some("sk"), Some("BSA-x"));
        assert_eq!(d["INSTANCE_AI_BRAVE_SEARCH_API_KEY"], "BSA-x");
        assert!(!d.contains_key("N8N_INSTANCE_AI_BRAVE_SEARCH_API_KEY"));
    }

    #[test]
    fn searxng_url_is_passed_through() {
        let mut s = spec();
        s.search = Some(SearchConfig {
            brave: None,
            searxng: Some(SearxngConfig {
                url: "http://searxng.search.svc:8080".into(),
            }),
        });
        let d = build_instance_ai_data(&s, URL, "k3y", Some("sk"), None);
        assert_eq!(d["N8N_INSTANCE_AI_SEARXNG_URL"], "http://searxng.search.svc:8080");
    }
}
