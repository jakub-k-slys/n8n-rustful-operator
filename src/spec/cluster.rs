use crate::spec::{
    common::{EncryptionKeySpec, EnvVar},
    database::DatabaseSpec,
    redis::RedisConfig,
    roles::{MainConfig, WebhookConfig, WorkerConfig},
    single::default_image,
};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub static CLUSTER_FINALIZER: &str = "clusters.n8n.slys.dev";

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(
    kind = "Cluster",
    group = "n8n.slys.dev",
    version = "v1",
    namespaced,
    shortname = "n8nc",
    plural = "clusters",
    status = "ClusterStatus"
)]
pub struct ClusterSpec {
    /// Cascading default image. Each role can override.
    #[serde(default = "default_image")]
    pub image: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "encryptionKey")]
    pub encryption_key: Option<EncryptionKeySpec>,
    /// Queue mode requires a shared DB; sqlite is rejected.
    pub database: DatabaseSpec,
    pub redis: RedisConfig,
    #[serde(default)]
    pub main: MainConfig,
    pub workers: WorkerConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhooks: Option<WebhookConfig>,
    /// Sets `N8N_SECURE_COOKIE` on every role. Omit for the n8n default (true).
    /// An `extraEnv` entry of the same name overrides this.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "secureCookie")]
    pub secure_cookie: Option<bool>,
    /// Extra env applied to every role. A role's own `extraEnv` overrides an
    /// entry here with the same name.
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "extraEnv")]
    pub extra_env: Vec<EnvVar>,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct ClusterStatus {
    pub ready: bool,
    #[serde(rename = "mainReplicas")]
    pub main_replicas: i32,
    #[serde(rename = "workerReplicas")]
    pub worker_replicas: i32,
    #[serde(rename = "webhookReplicas")]
    pub webhook_replicas: i32,
    #[serde(skip_serializing_if = "Option::is_none", rename = "encryptionKeySecret")]
    pub encryption_key_secret: Option<String>,
}
