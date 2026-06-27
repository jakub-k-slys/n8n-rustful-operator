use crate::{
    Error, Result,
    spec::{BinaryDataSpec, Cluster, CommunityNodesConfig, DatabaseSpec, EnvVar, SmtpConfig},
};

/// Env names (and prefixes) the operator wires itself; users may not shadow
/// them via `extraEnv`.
const RESERVED_ENV: &[&str] = &[
    "N8N_ENCRYPTION_KEY",
    "EXECUTIONS_MODE",
    "N8N_CONCURRENCY_PRODUCTION_LIMIT",
    "QUEUE_HEALTH_CHECK_ACTIVE",
    "N8N_DISABLE_PRODUCTION_MAIN_PROCESS",
    "N8N_MULTI_MAIN_SETUP_ENABLED",
];
const RESERVED_PREFIXES: &[&str] = &["DB_", "QUEUE_BULL_"];

pub fn validate_extra_env(env: &[EnvVar]) -> Result<()> {
    for e in env {
        let reserved = RESERVED_ENV.contains(&e.name.as_str())
            || RESERVED_PREFIXES.iter().any(|p| e.name.starts_with(p));
        if reserved {
            return Err(Error::IllegalEnv(format!(
                "extraEnv may not set operator-managed variable {:?}",
                e.name
            )));
        }
        if e.value.is_some() == e.value_from.is_some() {
            return Err(Error::IllegalEnv(format!(
                "extraEnv {:?} must set exactly one of value or valueFrom",
                e.name
            )));
        }
    }
    Ok(())
}

pub fn validate_smtp(smtp: Option<&SmtpConfig>) -> Result<()> {
    if let Some(s) = smtp
        && s.port == 0
    {
        return Err(Error::IllegalSmtp("smtp.port must be 1-65535".into()));
    }
    Ok(())
}

pub fn validate_community(cn: Option<&CommunityNodesConfig>) -> Result<()> {
    if let Some(cn) = cn {
        let active = [
            !cn.packages.is_empty(),
            cn.shared_storage.is_some(),
            cn.reinstall_missing == Some(true),
        ];
        if active.iter().filter(|x| **x).count() > 1 {
            return Err(Error::IllegalCluster(
                "communityNodes: set at most one of packages, sharedStorage, reinstallMissing=true".into(),
            ));
        }
    }
    Ok(())
}

pub fn validate_database(db: &DatabaseSpec) -> Result<()> {
    let illegal = |msg: &str| -> Result<()> { Err(Error::IllegalDatabase(msg.to_string())) };
    let extras_for_type = |ty: &str| -> Vec<&'static str> {
        let mut v = vec![];
        if ty != "sqlite" && db.sqlite.is_some() {
            v.push(".sqlite");
        }
        if ty != "postgresdb" && db.postgres.is_some() {
            v.push(".postgres");
        }
        if !matches!(ty, "mysqldb" | "mariadb") && db.mysql.is_some() {
            v.push(".mysql");
        }
        v
    };
    match db.type_.as_str() {
        "sqlite" => {
            let extras = extras_for_type("sqlite");
            if !extras.is_empty() {
                return illegal(&format!("type=sqlite but {} also set", extras.join(", ")));
            }
        }
        "postgresdb" => {
            if db.postgres.is_none() {
                return illegal("type=postgresdb requires .database.postgres");
            }
            let extras = extras_for_type("postgresdb");
            if !extras.is_empty() {
                return illegal(&format!("type=postgresdb but {} also set", extras.join(", ")));
            }
        }
        "mysqldb" | "mariadb" => {
            if db.mysql.is_none() {
                return illegal(&format!("type={} requires .database.mysql", db.type_));
            }
            let extras = extras_for_type(&db.type_);
            if !extras.is_empty() {
                return illegal(&format!("type={} but {} also set", db.type_, extras.join(", ")));
            }
        }
        other => return illegal(&format!("unknown type {other:?}")),
    }
    Ok(())
}

pub fn validate_cluster(c: &Cluster) -> Result<()> {
    validate_database(&c.spec.database)?;
    if c.spec.database.type_ == "sqlite" {
        return Err(Error::IllegalCluster(
            "queue mode requires a shared DB; sqlite is not supported".into(),
        ));
    }
    validate_extra_env(&c.spec.extra_env)?;
    validate_extra_env(&c.spec.main.extra_env)?;
    validate_extra_env(&c.spec.workers.extra_env)?;
    if let Some(wh) = &c.spec.webhooks {
        validate_extra_env(&wh.extra_env)?;
    }
    validate_smtp(c.spec.smtp.as_ref())?;
    validate_community(c.spec.community_nodes.as_ref())?;
    validate_binary_data(c.spec.binary_data.as_ref())?;
    Ok(())
}

pub fn validate_binary_data(bd: Option<&BinaryDataSpec>) -> Result<()> {
    if let Some(bd) = bd {
        if !matches!(bd.mode.as_str(), "default" | "database" | "filesystem" | "s3") {
            return Err(Error::IllegalCluster(format!(
                "unknown binaryData.mode {:?} (want default, database, filesystem or s3)",
                bd.mode
            )));
        }
        if bd.mode == "s3" && bd.s3.is_none() {
            return Err(Error::IllegalCluster(
                "binaryData.mode=s3 requires .binaryData.s3".into(),
            ));
        }
        if bd.shared_storage.is_some() && bd.mode != "filesystem" {
            return Err(Error::IllegalCluster(
                "binaryData.sharedStorage is only valid with mode=filesystem".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{
        BinaryDataSpec, CommunityNodesConfig, CommunityPackage, DatabaseSpec, EnvVar, EnvVarSource,
        MysqlConfig, PostgresConfig, S3Config, SecretKeyRef, SharedStorage, SmtpAuth, SmtpConfig,
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
}
