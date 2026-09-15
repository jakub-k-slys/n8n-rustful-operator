use std::collections::BTreeMap;

pub const ASSISTANT_LABEL: &str = "n8n.slys.dev/assistant";
pub const ASSISTANT_NS_LABEL: &str = "n8n.slys.dev/assistant-namespace";

/// Every object name the Assistant controller generates. The prefix carries
/// the CR's namespace so several Assistants can share one sandbox namespace.
pub struct SandboxNames {
    pub prefix: String,
    pub secrets: String,
    pub certs: String,
    pub tls_api: String,
    pub tls_runner: String,
    pub api: String,
    pub runner: String,
    pub target_secret: String,
}

impl SandboxNames {
    pub fn new(cr_ns: &str, cr_name: &str, target_name: &str) -> Self {
        let prefix = format!("{cr_ns}-{cr_name}");
        Self {
            secrets: format!("{prefix}-sandbox-secrets"),
            certs: format!("{prefix}-sandbox-certs"),
            tls_api: format!("{prefix}-sandbox-tls-api"),
            tls_runner: format!("{prefix}-sandbox-tls-runner"),
            api: format!("{prefix}-sandbox-api"),
            runner: format!("{prefix}-sandbox-runner-1"),
            target_secret: format!("{target_name}-instance-ai"),
            prefix,
        }
    }

    /// URL n8n uses to reach the sandbox API across namespaces.
    pub fn api_url(&self, sbx_ns: &str) -> String {
        format!("http://{}.{sbx_ns}.svc:8080", self.api)
    }
}

/// Labels on every object in the sandbox namespace. These objects carry no
/// ownerReference — a cross-namespace owner would make the GC treat them as
/// orphans and delete them — so the finalizer finds them by these labels.
pub fn sandbox_labels(cr_ns: &str, cr_name: &str, component: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("app.kubernetes.io/name".to_string(), "n8n-sandbox".to_string());
    m.insert("app.kubernetes.io/instance".to_string(), cr_name.to_string());
    m.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "n8n-rustful-operator".to_string(),
    );
    m.insert("app.kubernetes.io/part-of".to_string(), "n8n".to_string());
    m.insert("app.kubernetes.io/component".to_string(), component.to_string());
    m.insert(ASSISTANT_LABEL.to_string(), cr_name.to_string());
    m.insert(ASSISTANT_NS_LABEL.to_string(), cr_ns.to_string());
    m
}

/// Label selector for `delete_collection` during cleanup.
pub fn sandbox_selector(cr_ns: &str, cr_name: &str) -> String {
    format!("{ASSISTANT_LABEL}={cr_name},{ASSISTANT_NS_LABEL}={cr_ns}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::*;

    #[test]
    fn derives_every_object_name_from_the_prefix() {
        let n = SandboxNames::new("n8n-cluster", "n8n", "n8n");
        assert_eq!(n.prefix, "n8n-cluster-n8n");
        assert_eq!(n.secrets, "n8n-cluster-n8n-sandbox-secrets");
        assert_eq!(n.certs, "n8n-cluster-n8n-sandbox-certs");
        assert_eq!(n.tls_api, "n8n-cluster-n8n-sandbox-tls-api");
        assert_eq!(n.tls_runner, "n8n-cluster-n8n-sandbox-tls-runner");
        assert_eq!(n.api, "n8n-cluster-n8n-sandbox-api");
        assert_eq!(n.runner, "n8n-cluster-n8n-sandbox-runner-1");
        assert_eq!(n.target_secret, "n8n-instance-ai");
    }

    #[test]
    fn api_url_is_the_cross_namespace_service_dns() {
        let n = SandboxNames::new("n8n-cluster", "n8n", "n8n");
        assert_eq!(
            n.api_url("n8n-sandbox"),
            "http://n8n-cluster-n8n-sandbox-api.n8n-sandbox.svc:8080"
        );
    }

    #[test]
    fn labels_identify_the_owning_assistant() {
        let l = sandbox_labels("n8n-cluster", "n8n", "sandbox-api");
        assert_eq!(l["n8n.slys.dev/assistant"], "n8n");
        assert_eq!(l["n8n.slys.dev/assistant-namespace"], "n8n-cluster");
        assert_eq!(l["app.kubernetes.io/component"], "sandbox-api");
        assert_eq!(l["app.kubernetes.io/managed-by"], "n8n-rustful-operator");
    }

    #[test]
    fn selector_matches_only_this_assistant() {
        assert_eq!(
            sandbox_selector("n8n-cluster", "n8n"),
            "n8n.slys.dev/assistant=n8n,n8n.slys.dev/assistant-namespace=n8n-cluster"
        );
    }
}
