//! Coverage for the per-concern env builders (smtp / logging / community /
//! binary-data). Pure functions over the spec types; no cluster needed.

use n8n_rustful_operator::env::{
    community::build_community_env, logging::build_logging_env, smtp::build_smtp_env,
    storage::build_binary_data_env,
};
use n8n_rustful_operator::{
    BinaryDataSpec, CommunityNodesConfig, CommunityPackage, LoggingConfig, S3Config, SecretKeyRef,
    SharedStorage, SmtpAuth, SmtpConfig,
};
use serde_json::json;

fn skref(name: &str, key: &str) -> SecretKeyRef {
    SecretKeyRef {
        name: name.into(),
        key: key.into(),
    }
}

// ----- smtp -----

#[test]
fn smtp_env_maps_fields_and_credentials() {
    let s = SmtpConfig {
        host: "smtp.example.com".into(),
        port: 587,
        sender: "n8n <no-reply@example.com>".into(),
        ssl: Some(false),
        start_tls: Some(true),
        auth: Some(SmtpAuth {
            user_secret: skref("smtp", "user"),
            password_secret: skref("smtp", "password"),
        }),
    };
    let env = build_smtp_env(&s);
    let by_name = |n: &str| env.iter().find(|e| e.name == n);
    assert_eq!(by_name("N8N_EMAIL_MODE").unwrap().value.as_deref(), Some("smtp"));
    assert_eq!(by_name("N8N_SMTP_PORT").unwrap().value.as_deref(), Some("587"));
    assert_eq!(
        by_name("N8N_SMTP_STARTTLS").unwrap().value.as_deref(),
        Some("true")
    );
    let user = by_name("N8N_SMTP_USER").unwrap();
    assert!(user.value.is_none());
    assert_eq!(user.value_from.as_ref().unwrap().secret_ref.key, "user");
}

#[test]
fn smtp_without_auth_omits_credentials() {
    let s = SmtpConfig {
        host: "relay.internal".into(),
        port: 25,
        sender: "n8n@internal".into(),
        ssl: None,
        start_tls: None,
        auth: None,
    };
    let names: Vec<_> = build_smtp_env(&s).into_iter().map(|e| e.name).collect();
    assert_eq!(
        names,
        [
            "N8N_EMAIL_MODE",
            "N8N_SMTP_HOST",
            "N8N_SMTP_PORT",
            "N8N_SMTP_SENDER"
        ]
    );
}

// ----- logging -----

#[test]
fn logging_env_maps_set_fields() {
    let l = LoggingConfig {
        level: Some("debug".into()),
        output: Some("console".into()),
        diagnostics: Some(true),
        version_notifications: Some(false),
        metrics: Some(true),
    };
    let env = build_logging_env(&l);
    let by_name = |n: &str| env.iter().find(|e| e.name == n).and_then(|e| e.value.as_deref());
    assert_eq!(by_name("N8N_LOG_LEVEL"), Some("debug"));
    assert_eq!(by_name("N8N_LOG_OUTPUT"), Some("console"));
    assert_eq!(by_name("N8N_DIAGNOSTICS_ENABLED"), Some("true"));
    assert_eq!(by_name("N8N_VERSION_NOTIFICATIONS_ENABLED"), Some("false"));
    assert_eq!(by_name("N8N_METRICS"), Some("true"));
}

#[test]
fn logging_env_omits_unset_fields() {
    let l = LoggingConfig {
        level: Some("info".into()),
        ..Default::default()
    };
    let names: Vec<_> = build_logging_env(&l).into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["N8N_LOG_LEVEL"]);
}

// ----- community nodes -----

fn cn() -> CommunityNodesConfig {
    CommunityNodesConfig {
        enabled: None,
        packages: vec![],
        shared_storage: None,
        reinstall_missing: None,
    }
}

#[test]
fn packages_enable_declarative_management() {
    let mut c = cn();
    c.enabled = Some(true);
    c.packages = vec![
        CommunityPackage {
            name: "n8n-nodes-foo".into(),
            version: Some("1.2.3".into()),
            checksum: None,
        },
        CommunityPackage {
            name: "n8n-nodes-bar".into(),
            version: None,
            checksum: None,
        },
    ];
    let env = build_community_env(&c);
    let by_name = |n: &str| env.iter().find(|e| e.name == n).and_then(|e| e.value.as_deref());
    assert_eq!(by_name("N8N_COMMUNITY_PACKAGES_ENABLED"), Some("true"));
    assert_eq!(by_name("N8N_COMMUNITY_PACKAGES_MANAGED_BY_ENV"), Some("true"));
    assert_eq!(
        by_name("N8N_COMMUNITY_PACKAGES"),
        Some(r#"[{"name":"n8n-nodes-foo","version":"1.2.3"},{"name":"n8n-nodes-bar"}]"#)
    );
    assert!(by_name("N8N_REINSTALL_MISSING_PACKAGES").is_none());
}

#[test]
fn shared_storage_emits_no_env() {
    let mut c = cn();
    c.shared_storage = Some(SharedStorage {
        size: "5Gi".into(),
        storage_class_name: None,
        access_mode: "ReadWriteMany".into(),
    });
    assert!(build_community_env(&c).is_empty());
}

#[test]
fn reinstall_missing_sets_env() {
    let mut c = cn();
    c.reinstall_missing = Some(true);
    let env = build_community_env(&c);
    assert!(
        env.iter()
            .any(|e| e.name == "N8N_REINSTALL_MISSING_PACKAGES" && e.value.as_deref() == Some("true"))
    );
    // not the declarative path
    assert!(
        !env.iter()
            .any(|e| e.name == "N8N_COMMUNITY_PACKAGES_MANAGED_BY_ENV")
    );
}

// ----- binary data -----

fn bd(mode: &str, s3: Option<S3Config>, shared: Option<SharedStorage>) -> BinaryDataSpec {
    BinaryDataSpec {
        mode: mode.into(),
        s3,
        shared_storage: shared,
    }
}

#[test]
fn binary_data_s3_wires_bucket_and_credentials() {
    let env = build_binary_data_env(&bd(
        "s3",
        Some(S3Config {
            host: "minio.local".into(),
            bucket: "n8n".into(),
            region: "us-east-1".into(),
            access_key_secret: skref("s3", "key"),
            access_secret_secret: skref("s3", "secret"),
        }),
        None,
    ));
    assert!(env.contains(&json!({ "name": "N8N_DEFAULT_BINARY_DATA_MODE", "value": "s3" })));
    assert!(env.contains(&json!({ "name": "N8N_EXTERNAL_STORAGE_S3_BUCKET_NAME", "value": "n8n" })));
    assert!(env.contains(&json!({
        "name": "N8N_EXTERNAL_STORAGE_S3_ACCESS_SECRET",
        "valueFrom": { "secretKeyRef": { "name": "s3", "key": "secret" } }
    })));
}

#[test]
fn binary_data_database_and_default() {
    assert_eq!(
        build_binary_data_env(&bd("database", None, None)),
        vec![
            json!({ "name": "N8N_AVAILABLE_BINARY_DATA_MODES", "value": "filesystem,database" }),
            json!({ "name": "N8N_DEFAULT_BINARY_DATA_MODE", "value": "database" }),
        ]
    );
    assert_eq!(
        build_binary_data_env(&bd("default", None, None)),
        vec![json!({ "name": "N8N_DEFAULT_BINARY_DATA_MODE", "value": "default" })]
    );
}

#[test]
fn binary_data_filesystem_path_only_with_shared_storage() {
    assert_eq!(
        build_binary_data_env(&bd("filesystem", None, None)),
        vec![json!({ "name": "N8N_DEFAULT_BINARY_DATA_MODE", "value": "filesystem" })]
    );
    let env = build_binary_data_env(&bd(
        "filesystem",
        None,
        Some(SharedStorage {
            size: "20Gi".into(),
            storage_class_name: None,
            access_mode: "ReadWriteMany".into(),
        }),
    ));
    assert!(env.contains(&json!({
        "name": "N8N_BINARY_DATA_STORAGE_PATH",
        "value": "/home/node/binary-data"
    })));
}
