use crate::spec::{
    common::{EncryptionKeySpec, PersistenceConfig, ServiceConfig},
    database::DatabaseSpec,
    networking::NetworkingSpec,
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
