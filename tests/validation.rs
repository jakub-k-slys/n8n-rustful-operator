//! Coverage for the `reconciler::validate` rejection rules. Pure functions over
//! the spec types, so no cluster is needed (the wired-up behaviour is exercised
//! by the BDD suite in `features/`).

use n8n_rustful_operator::reconciler::validate::{
    validate_binary_data, validate_community, validate_database, validate_extra_env, validate_smtp,
};
use n8n_rustful_operator::{
    BinaryDataSpec, CommunityNodesConfig, CommunityPackage, DatabaseSpec, EnvVar, EnvVarSource, MysqlConfig,
    PostgresConfig, S3Config, SecretKeyRef, SharedStorage, SmtpAuth, SmtpConfig,
};

fn skref(name: &str, key: &str) -> SecretKeyRef {
    SecretKeyRef {
        name: name.into(),
        key: key.into(),
    }
}
fn pg() -> PostgresConfig {
    PostgresConfig {
        host: "h".into(),
        port: None,
        database: "d".into(),
        user_secret: skref("s", "user"),
        password_secret: skref("s", "pw"),
        schema: None,
        ssl: None,
        pool_size: None,
        connection_timeout_ms: None,
    }
}
fn mysql() -> MysqlConfig {
    MysqlConfig {
        host: "h".into(),
        port: None,
        database: "d".into(),
        user: "u".into(),
        password_secret: skref("s", "pw"),
        ssl: None,
        connection_timeout_ms: None,
    }
}
fn db(type_: &str) -> DatabaseSpec {
    DatabaseSpec {
        type_: type_.into(),
        sqlite: None,
        postgres: None,
        mysql: None,
    }
}

#[test]
fn database_postgres_requires_block_then_ok() {
    let mut d = db("postgresdb");
    assert!(validate_database(&d).is_err());
    d.postgres = Some(pg());
    assert!(validate_database(&d).is_ok());
}

#[test]
fn database_rejects_cross_type_extras() {
    let d = DatabaseSpec {
        type_: "postgresdb".into(),
        postgres: Some(pg()),
        mysql: Some(mysql()),
        sqlite: None,
    };
    assert!(validate_database(&d).is_err());
}

#[test]
fn database_mysql_sqlite_unknown() {
    let mut my = db("mysqldb");
    assert!(validate_database(&my).is_err());
    my.mysql = Some(mysql());
    assert!(validate_database(&my).is_ok());

    let mut sq = db("sqlite");
    assert!(validate_database(&sq).is_ok());
    sq.postgres = Some(pg());
    assert!(validate_database(&sq).is_err());

    assert!(validate_database(&db("nope")).is_err());
}

fn ev_val(name: &str) -> EnvVar {
    EnvVar {
        name: name.into(),
        value: Some("v".into()),
        value_from: None,
    }
}

#[test]
fn extra_env_rules() {
    assert!(validate_extra_env(&[ev_val("OK")]).is_ok());
    assert!(validate_extra_env(&[ev_val("DB_FOO")]).is_err());
    assert!(validate_extra_env(&[ev_val("N8N_ENCRYPTION_KEY")]).is_err());
    assert!(
        validate_extra_env(&[EnvVar {
            name: "X".into(),
            value: None,
            value_from: None,
        }])
        .is_err()
    );
    assert!(
        validate_extra_env(&[EnvVar {
            name: "X".into(),
            value: Some("v".into()),
            value_from: Some(EnvVarSource {
                secret_ref: skref("s", "k"),
            }),
        }])
        .is_err()
    );
}

#[test]
fn smtp_rules() {
    assert!(validate_smtp(None).is_ok());
    let mut s = SmtpConfig {
        host: "h".into(),
        port: 587,
        sender: "a@b".into(),
        ssl: None,
        start_tls: None,
        auth: Some(SmtpAuth {
            user_secret: skref("s", "u"),
            password_secret: skref("s", "p"),
        }),
    };
    assert!(validate_smtp(Some(&s)).is_ok());
    s.port = 0;
    assert!(validate_smtp(Some(&s)).is_err());
}

fn cn() -> CommunityNodesConfig {
    CommunityNodesConfig {
        enabled: Some(true),
        packages: vec![],
        shared_storage: None,
        reinstall_missing: None,
    }
}
fn rwx() -> SharedStorage {
    SharedStorage {
        size: "1Gi".into(),
        storage_class_name: None,
        access_mode: "ReadWriteMany".into(),
    }
}
fn pkg() -> CommunityPackage {
    CommunityPackage {
        name: "n8n-nodes-x".into(),
        version: None,
        checksum: None,
    }
}

#[test]
fn community_single_strategy_ok() {
    assert!(validate_community(None).is_ok());
    assert!(validate_community(Some(&cn())).is_ok());
    let mut p = cn();
    p.packages = vec![pkg()];
    assert!(validate_community(Some(&p)).is_ok());
    let mut s = cn();
    s.shared_storage = Some(rwx());
    assert!(validate_community(Some(&s)).is_ok());
    let mut r = cn();
    r.reinstall_missing = Some(true);
    assert!(validate_community(Some(&r)).is_ok());
}

#[test]
fn community_multiple_strategies_rejected() {
    let mut both = cn();
    both.packages = vec![pkg()];
    both.shared_storage = Some(rwx());
    assert!(validate_community(Some(&both)).is_err());

    let mut pr = cn();
    pr.packages = vec![pkg()];
    pr.reinstall_missing = Some(true);
    assert!(validate_community(Some(&pr)).is_err());
}

fn s3() -> S3Config {
    S3Config {
        host: "h".into(),
        bucket: "b".into(),
        region: "r".into(),
        access_key_secret: skref("s", "ak"),
        access_secret_secret: skref("s", "as"),
    }
}
fn bd(mode: &str, s3: Option<S3Config>, shared: Option<SharedStorage>) -> BinaryDataSpec {
    BinaryDataSpec {
        mode: mode.into(),
        s3,
        shared_storage: shared,
    }
}

#[test]
fn binary_data_modes() {
    assert!(validate_binary_data(None).is_ok());
    for m in ["default", "database", "filesystem"] {
        assert!(validate_binary_data(Some(&bd(m, None, None))).is_ok());
    }
    assert!(validate_binary_data(Some(&bd("memory", None, None))).is_err());
}

#[test]
fn binary_data_s3_and_shared_storage_rules() {
    assert!(validate_binary_data(Some(&bd("s3", None, None))).is_err());
    assert!(validate_binary_data(Some(&bd("s3", Some(s3()), None))).is_ok());
    assert!(validate_binary_data(Some(&bd("filesystem", None, Some(rwx())))).is_ok());
    assert!(validate_binary_data(Some(&bd("database", None, Some(rwx())))).is_err());
}
