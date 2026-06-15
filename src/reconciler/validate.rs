use crate::{
    Error, Result,
    spec::{Cluster, DatabaseSpec},
};

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
    Ok(())
}
