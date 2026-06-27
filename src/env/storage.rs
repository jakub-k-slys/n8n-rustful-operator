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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{S3Config, SecretKeyRef, SharedStorage};
    use serde_json::json;

    #[test]
    fn s3_mode_wires_bucket_and_credentials() {
        let bd = BinaryDataSpec {
            mode: "s3".into(),
            shared_storage: None,
            s3: Some(S3Config {
                host: "minio.local".into(),
                bucket: "n8n".into(),
                region: "us-east-1".into(),
                access_key_secret: SecretKeyRef {
                    name: "s3".into(),
                    key: "key".into(),
                },
                access_secret_secret: SecretKeyRef {
                    name: "s3".into(),
                    key: "secret".into(),
                },
            }),
        };
        let env = build_binary_data_env(&bd);
        assert!(env.contains(&json!({ "name": "N8N_DEFAULT_BINARY_DATA_MODE", "value": "s3" })));
        assert!(env.contains(&json!({ "name": "N8N_EXTERNAL_STORAGE_S3_BUCKET_NAME", "value": "n8n" })));
        assert!(env.contains(&json!({
            "name": "N8N_EXTERNAL_STORAGE_S3_ACCESS_SECRET",
            "valueFrom": { "secretKeyRef": { "name": "s3", "key": "secret" } }
        })));
    }

    fn bd(mode: &str, shared: Option<SharedStorage>) -> BinaryDataSpec {
        BinaryDataSpec {
            mode: mode.into(),
            s3: None,
            shared_storage: shared,
        }
    }

    #[test]
    fn database_mode_sets_default_and_available() {
        assert_eq!(
            build_binary_data_env(&bd("database", None)),
            vec![
                json!({ "name": "N8N_AVAILABLE_BINARY_DATA_MODES", "value": "filesystem,database" }),
                json!({ "name": "N8N_DEFAULT_BINARY_DATA_MODE", "value": "database" }),
            ]
        );
    }

    #[test]
    fn default_mode_sets_only_default_mode() {
        assert_eq!(
            build_binary_data_env(&bd("default", None)),
            vec![json!({ "name": "N8N_DEFAULT_BINARY_DATA_MODE", "value": "default" })]
        );
    }

    #[test]
    fn filesystem_without_shared_storage_sets_no_path() {
        assert_eq!(
            build_binary_data_env(&bd("filesystem", None)),
            vec![json!({ "name": "N8N_DEFAULT_BINARY_DATA_MODE", "value": "filesystem" })]
        );
    }

    #[test]
    fn filesystem_with_shared_storage_sets_path() {
        let env = build_binary_data_env(&bd(
            "filesystem",
            Some(SharedStorage {
                size: "20Gi".into(),
                storage_class_name: None,
                access_mode: "ReadWriteMany".into(),
            }),
        ));
        assert!(env.contains(&json!({ "name": "N8N_DEFAULT_BINARY_DATA_MODE", "value": "filesystem" })));
        assert!(env.contains(&json!({
            "name": "N8N_BINARY_DATA_STORAGE_PATH",
            "value": "/home/node/binary-data"
        })));
    }
}
