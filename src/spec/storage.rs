use crate::spec::{common::SecretKeyRef, common::SharedStorage};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where n8n stores binary data (workflow attachments). Maps to
/// `N8N_DEFAULT_BINARY_DATA_MODE`. In queue mode only `database`, `s3`, or
/// `filesystem` backed by a shared `ReadWriteMany` volume are shared across
/// roles — `default` (in-memory) and an unshared `filesystem` are per-pod.
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct BinaryDataSpec {
    /// `default` (in-memory), `database` (in the DB), `filesystem`, or `s3`.
    pub mode: String,
    /// External S3-compatible storage; required when `mode` is `s3`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s3: Option<S3Config>,
    /// Shared `ReadWriteMany` volume for `mode: filesystem`, mounted on every
    /// role at the binary-data path (`N8N_BINARY_DATA_STORAGE_PATH`) so files are
    /// shared across roles in queue mode. Only valid with `mode: filesystem`.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "sharedStorage")]
    pub shared_storage: Option<SharedStorage>,
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
