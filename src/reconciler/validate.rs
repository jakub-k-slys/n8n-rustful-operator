use crate::{
    Error, Result,
    spec::{Cluster, DatabaseSpec, EnvVar, SmtpConfig},
};

/// Env names (and prefixes) the operator wires itself; users may not shadow
/// them via `extraEnv`.
const RESERVED_ENV: &[&str] = &[
    "N8N_ENCRYPTION_KEY",
    "EXECUTIONS_MODE",
    "N8N_CONCURRENCY_PRODUCTION_LIMIT",
    "QUEUE_HEALTH_CHECK_ACTIVE",
    "N8N_DISABLE_PRODUCTION_MAIN_PROCESS",
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
    if let Some(bd) = &c.spec.binary_data {
        if !matches!(bd.mode.as_str(), "filesystem" | "s3") {
            return Err(Error::IllegalCluster(format!(
                "unknown binaryData.mode {:?} (want filesystem or s3)",
                bd.mode
            )));
        }
        if bd.mode == "s3" && bd.s3.is_none() {
            return Err(Error::IllegalCluster(
                "binaryData.mode=s3 requires .binaryData.s3".into(),
            ));
        }
    }
    Ok(())
}
