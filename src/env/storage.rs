use crate::{
    builders::pvc::BINARY_DATA_STORAGE_PATH,
    env::{env_secret, env_str},
    spec::BinaryDataSpec,
};
use serde_json::Value;

/// Map `BinaryDataSpec` to the n8n binary-data / external-storage env.
/// - `s3` enables the mode and wires the bucket + credentials (key/secret from Secrets).
/// - `database` stores binary data in the DB.
/// - `filesystem` writes to disk; with `sharedStorage` the path is set to the
///   shared mount so files are shared across roles.
/// - `default` keeps binary data in memory.
pub fn build_binary_data_env(bd: &BinaryDataSpec) -> Vec<Value> {
    match bd.mode.as_str() {
        "s3" => {
            let mut out = vec![
                env_str("N8N_AVAILABLE_BINARY_DATA_MODES", "filesystem,s3"),
                env_str("N8N_DEFAULT_BINARY_DATA_MODE", "s3"),
            ];
            if let Some(s3) = &bd.s3 {
                out.push(env_str("N8N_EXTERNAL_STORAGE_S3_HOST", s3.host.clone()));
                out.push(env_str("N8N_EXTERNAL_STORAGE_S3_BUCKET_NAME", s3.bucket.clone()));
                out.push(env_str(
                    "N8N_EXTERNAL_STORAGE_S3_BUCKET_REGION",
                    s3.region.clone(),
                ));
                out.push(env_secret(
                    "N8N_EXTERNAL_STORAGE_S3_ACCESS_KEY",
                    &s3.access_key_secret,
                ));
                out.push(env_secret(
                    "N8N_EXTERNAL_STORAGE_S3_ACCESS_SECRET",
                    &s3.access_secret_secret,
                ));
            }
            out
        }
        "database" => vec![
            env_str("N8N_AVAILABLE_BINARY_DATA_MODES", "filesystem,database"),
            env_str("N8N_DEFAULT_BINARY_DATA_MODE", "database"),
        ],
        "filesystem" => {
            let mut out = vec![env_str("N8N_DEFAULT_BINARY_DATA_MODE", "filesystem")];
            if bd.shared_storage.is_some() {
                out.push(env_str("N8N_BINARY_DATA_STORAGE_PATH", BINARY_DATA_STORAGE_PATH));
            }
            out
        }
        // `default` (in-memory) and anything else validation has accepted.
        other => vec![env_str("N8N_DEFAULT_BINARY_DATA_MODE", other.to_string())],
    }
}
