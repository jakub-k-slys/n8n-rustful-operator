use crate::spec::common::SecretKeyRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where n8n stores binary data (workflow attachments). In queue mode the
/// default `filesystem` mode is per-pod, so binary produced on a worker is not
/// visible to main; `s3` shares it across all roles.
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct BinaryDataSpec {
    /// `filesystem` (default) or `s3`. Maps to `N8N_DEFAULT_BINARY_DATA_MODE`.
    pub mode: String,
    /// External S3-compatible storage; required when `mode` is `s3`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s3: Option<S3Config>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct S3Config {
    /// Endpoint host, e.g. `s3.eu-central-1.amazonaws.com` or a MinIO address.
    pub host: String,
    pub bucket: String,
    #[serde(rename = "bucketRegion")]
    pub region: String,
    #[serde(rename = "accessKeySecret")]
    pub access_key_secret: SecretKeyRef,
    #[serde(rename = "accessSecretSecret")]
    pub access_secret_secret: SecretKeyRef,
}
