use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SecretKeyRef {
    pub name: String,
    /// Key within the Secret. Defaults to `encryption_key`.
    #[serde(default = "default_secret_key")]
    pub key: String,
}

fn default_secret_key() -> String {
    "encryption_key".to_string()
}

/// A user-supplied environment variable passed straight through to the n8n
/// container. Set exactly one of `value` (inline literal) or `valueFrom`
/// (pulled from a Secret). Operator-managed variables (encryption key, `DB_*`,
/// `QUEUE_BULL_*`, …) are rejected by validation so they can't be shadowed.
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct EnvVar {
    pub name: String,
    /// Inline literal value. Mutually exclusive with `valueFrom`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Pull the value from a Secret. Mutually exclusive with `value`.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "valueFrom")]
    pub value_from: Option<EnvVarSource>,
}

/// Source for an `EnvVar` whose value is resolved at runtime. Mirrors the k8s
/// `valueFrom.secretKeyRef`; only a Secret key is supported.
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct EnvVarSource {
    #[serde(rename = "secretRef")]
    pub secret_ref: SecretKeyRef,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct EncryptionKeySpec {
    /// Reference to an existing Secret. Omit the whole block to auto-generate.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "secretRef")]
    pub secret_ref: Option<SecretKeyRef>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct ServiceConfig {
    /// `ClusterIP` (default), `NodePort`, or `LoadBalancer`.
    #[serde(default = "default_service_type", rename = "type")]
    pub type_: String,
}

pub fn default_service_type() -> String {
    "ClusterIP".to_string()
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct PersistenceConfig {
    /// Storage request, e.g. `1Gi`.
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "storageClassName")]
    pub storage_class_name: Option<String>,
    #[serde(default = "default_access_mode", rename = "accessMode")]
    pub access_mode: String,
}

fn default_access_mode() -> String {
    "ReadWriteOnce".to_string()
}
