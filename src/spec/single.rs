use crate::spec::{
    common::{EncryptionKeySpec, EnvVar, PersistenceConfig, ResourceRequirements, ServiceConfig},
    database::DatabaseSpec,
    logging::LoggingConfig,
    networking::NetworkingSpec,
    pod::PodConfig,
    smtp::SmtpConfig,
};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub static SINGLE_FINALIZER: &str = "singles.n8n.slys.dev";

/// `Single` is a Kubernetes-native description of a standalone n8n deployment.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(
    kind = "Single",
    group = "n8n.slys.dev",
    version = "v1",
    namespaced,
    shortname = "n8n",
    plural = "singles",
    status = "SingleStatus"
)]
pub struct SingleSpec {
    /// Container image to deploy (e.g. `n8nio/n8n:1.70.0`).
    #[serde(default = "default_image")]
    pub image: String,
    #[serde(default = "default_replicas")]
    pub replicas: i32,
    /// Externally-facing hostname. Required when `networking` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub networking: Option<NetworkingSpec>,
    /// N8N_ENCRYPTION_KEY source. Auto-generated if omitted.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "encryptionKey")]
    pub encryption_key: Option<EncryptionKeySpec>,
    /// Database backend (sqlite default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<DatabaseSpec>,
    /// PVC at `/home/node/.n8n` so binary data and the sqlite file persist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistence: Option<PersistenceConfig>,
    /// Sets `N8N_SECURE_COOKIE`. Omit for the n8n default (true). An `extraEnv`
    /// entry of the same name overrides this.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "secureCookie")]
    pub secure_cookie: Option<bool>,
    /// Extra env passed straight to the n8n container.
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "extraEnv")]
    pub extra_env: Vec<EnvVar>,
    /// Names of Secrets used to pull the container image (private registries).
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "imagePullSecrets")]
    pub image_pull_secrets: Vec<String>,
    /// Container CPU/memory requests and limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,
    /// Pod-level scheduling and metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod: Option<PodConfig>,
    /// SMTP notification settings (sets `N8N_EMAIL_MODE`/`N8N_SMTP_*`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smtp: Option<SmtpConfig>,
    /// Logging, diagnostics and metrics toggles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingConfig>,
}

pub fn default_image() -> String {
    "n8nio/n8n:latest".to_string()
}
fn default_replicas() -> i32 {
    1
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct SingleStatus {
    pub ready: bool,
    pub replicas: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "encryptionKeySecret")]
    pub encryption_key_secret: Option<String>,
}
