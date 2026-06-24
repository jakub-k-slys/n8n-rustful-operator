use crate::{
    env::{env_secret, env_str},
    spec::BinaryDataSpec,
};
use serde_json::Value;

/// Map `BinaryDataSpec` to the n8n binary-data / external-storage env. For `s3`
/// it enables the mode and wires the bucket and credentials (the access key and
/// secret come from Secrets); any other mode just sets the default mode.
pub fn build_binary_data_env(bd: &BinaryDataSpec) -> Vec<Value> {
    if bd.mode != "s3" {
        return vec![env_str("N8N_DEFAULT_BINARY_DATA_MODE", bd.mode.clone())];
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{S3Config, SecretKeyRef};
    use serde_json::json;

    #[test]
    fn s3_mode_wires_bucket_and_credentials() {
        let bd = BinaryDataSpec {
            mode: "s3".into(),
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

    #[test]
    fn filesystem_mode_sets_only_default_mode() {
        let bd = BinaryDataSpec {
            mode: "filesystem".into(),
            s3: None,
        };
        assert_eq!(
            build_binary_data_env(&bd),
            vec![json!({ "name": "N8N_DEFAULT_BINARY_DATA_MODE", "value": "filesystem" })]
        );
    }
}
