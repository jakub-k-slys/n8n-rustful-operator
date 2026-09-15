use crate::spec::common::{PersistenceConfig, ResourceRequirements, SecretKeyRef};
use crate::spec::pod::PodConfig;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub static ASSISTANT_FINALIZER: &str = "assistants.n8n.slys.dev";

/// `Assistant` configures the n8n Assistant (`instance-ai`) module for a
/// `Cluster` or `Single` in the same namespace, and owns the self-hosted
/// sandbox stack the module requires.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(
    kind = "Assistant",
    group = "n8n.slys.dev",
    version = "v1",
    namespaced,
    shortname = "n8na",
    plural = "assistants",
    status = "AssistantStatus"
)]
pub struct AssistantSpec {
    /// The n8n instance this assistant configures. Must be in the same
    /// namespace — the generated Secret is owned by this CR, and owner
    /// references cannot cross namespaces.
    #[serde(rename = "targetRef")]
    pub target_ref: TargetRef,
    pub model: ModelConfig,
    /// Optional web search. Brave takes priority over SearXNG when both are set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchConfig>,
    #[serde(default)]
    pub sandbox: SandboxConfig,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
pub struct TargetRef {
    /// `Cluster` or `Single`.
    pub kind: String,
    pub name: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
pub struct ModelConfig {
    /// `provider/model`, e.g. `anthropic/claude-opus-4-8`. Providers:
    /// `anthropic`, `openai`, `openrouter`.
    pub name: String,
    /// Provider API key. Required unless `url` points at an endpoint that
    /// needs none.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "apiKeySecret")]
    pub api_key_secret: Option<SecretKeyRef>,
    /// Custom OpenAI-compatible endpoint (`N8N_INSTANCE_AI_MODEL_URL`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SearchConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brave: Option<BraveConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub searxng: Option<SearxngConfig>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct BraveConfig {
    #[serde(rename = "apiKeySecret")]
    pub api_key_secret: SecretKeyRef,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SearxngConfig {
    /// URL of an externally-hosted SearXNG with the JSON API enabled.
    pub url: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SandboxConfig {
    /// Namespace for the sandbox stack. Defaults to the CR's namespace. Must
    /// carry `pod-security.kubernetes.io/enforce: privileged`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Tag for both `n8n-sandbox-service-*` images.
    #[serde(default = "default_sandbox_version")]
    pub version: String,
    /// Image the runner starts per execution.
    #[serde(default = "default_sandbox_image", rename = "sandboxImage")]
    pub sandbox_image: String,
    #[serde(default)]
    pub api: SandboxRoleConfig,
    #[serde(default)]
    pub runner: SandboxRunnerConfig,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            namespace: None,
            version: default_sandbox_version(),
            sandbox_image: default_sandbox_image(),
            api: SandboxRoleConfig::default(),
            runner: SandboxRunnerConfig::default(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
pub struct SandboxRoleConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod: Option<PodConfig>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SandboxRunnerConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod: Option<PodConfig>,
    /// Backing store for the runner's inner Docker (`/var/lib/docker`).
    #[serde(default, rename = "dockerStorage")]
    pub docker_storage: DockerStorage,
}

impl Default for SandboxRunnerConfig {
    fn default() -> Self {
        Self {
            resources: None,
            pod: None,
            docker_storage: DockerStorage::default(),
        }
    }
}

/// `/var/lib/docker` for the runner. An `emptyDir` with a size limit by
/// default; set `persistence` to keep the pulled sandbox image across restarts.
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct DockerStorage {
    /// `emptyDir.sizeLimit`. Ignored when `persistence` is set.
    #[serde(default = "default_docker_size")]
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistence: Option<PersistenceConfig>,
}

impl Default for DockerStorage {
    fn default() -> Self {
        Self {
            size: default_docker_size(),
            persistence: None,
        }
    }
}

fn default_sandbox_version() -> String {
    "1.2.0".to_string()
}
fn default_sandbox_image() -> String {
    "ghcr.io/n8n-io/n8n-sandbox-service-sandbox:latest".to_string()
}
fn default_docker_size() -> String {
    "20Gi".to_string()
}

impl AssistantSpec {
    /// Namespace the sandbox stack lives in: the override, or the CR's own.
    pub fn sandbox_namespace(&self, cr_ns: &str) -> String {
        self.sandbox
            .namespace
            .clone()
            .unwrap_or_else(|| cr_ns.to_string())
    }
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct AssistantStatus {
    pub ready: bool,
    #[serde(rename = "certsReady")]
    pub certs_ready: bool,
    #[serde(rename = "apiReady")]
    pub api_ready: bool,
    #[serde(rename = "runnerReady")]
    pub runner_ready: bool,
    #[serde(skip_serializing_if = "Option::is_none", rename = "targetSecret")]
    pub target_secret: Option<String>,
    /// The sandbox namespace the stack was actually built in. Pinned once the
    /// stack exists — `apply` refuses a later `spec.sandbox.namespace` change
    /// rather than orphaning the old privileged stack (it has no
    /// ownerReference and isn't cleaned up by any namespace but this one).
    #[serde(skip_serializing_if = "Option::is_none", rename = "sandboxNamespace")]
    pub sandbox_namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_json() -> serde_json::Value {
        serde_json::json!({
            "targetRef": { "kind": "Cluster", "name": "n8n" },
            "model": {
                "name": "anthropic/claude-opus-4-8",
                "apiKeySecret": { "name": "n8n-ai", "key": "ANTHROPIC_API_KEY" }
            }
        })
    }

    #[test]
    fn minimal_spec_applies_documented_defaults() {
        let s: AssistantSpec = serde_json::from_value(spec_json()).unwrap();
        assert_eq!(s.target_ref.kind, "Cluster");
        assert_eq!(s.sandbox.version, "1.2.0");
        assert_eq!(
            s.sandbox.sandbox_image,
            "ghcr.io/n8n-io/n8n-sandbox-service-sandbox:latest"
        );
        assert_eq!(s.sandbox.runner.docker_storage.size, "20Gi");
        assert!(s.sandbox.namespace.is_none());
        assert!(s.search.is_none());
    }

    #[test]
    fn sandbox_namespace_defaults_to_the_cr_namespace() {
        let s: AssistantSpec = serde_json::from_value(spec_json()).unwrap();
        assert_eq!(s.sandbox_namespace("n8n-cluster"), "n8n-cluster");
    }

    #[test]
    fn sandbox_namespace_honours_the_override() {
        let mut v = spec_json();
        v["sandbox"] = serde_json::json!({ "namespace": "n8n-sandbox" });
        let s: AssistantSpec = serde_json::from_value(v).unwrap();
        assert_eq!(s.sandbox_namespace("n8n-cluster"), "n8n-sandbox");
    }
}
