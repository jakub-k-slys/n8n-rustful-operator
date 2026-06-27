use crate::{
    env::{env_secret, env_str},
    spec::{DatabaseSpec, DatabaseSsl},
};
use serde_json::{Value, json};

pub fn build_db_env(db: &DatabaseSpec) -> Vec<Value> {
    let mut out = vec![json!({ "name": "DB_TYPE", "value": db.type_ })];
    match db.type_.as_str() {
        "postgresdb" => {
            if let Some(pg) = &db.postgres {
                out.push(json!({ "name": "DB_POSTGRESDB_HOST", "value": pg.host }));
                if let Some(p) = pg.port {
                    out.push(env_str("DB_POSTGRESDB_PORT", p.to_string()));
                }
                out.push(json!({ "name": "DB_POSTGRESDB_DATABASE", "value": pg.database }));
                out.push(env_secret("DB_POSTGRESDB_USER", &pg.user_secret));
                out.push(env_secret("DB_POSTGRESDB_PASSWORD", &pg.password_secret));
                if let Some(s) = &pg.schema {
                    out.push(json!({ "name": "DB_POSTGRESDB_SCHEMA", "value": s }));
                }
                if let Some(sz) = pg.pool_size {
                    out.push(env_str("DB_POSTGRESDB_POOL_SIZE", sz.to_string()));
                }
                if let Some(t) = pg.connection_timeout_ms {
                    out.push(env_str("DB_POSTGRESDB_CONNECTION_TIMEOUT", t.to_string()));
                }
                if let Some(ssl) = &pg.ssl {
                    push_ssl_env(&mut out, "DB_POSTGRESDB", ssl);
                }
            }
        }
        "mysqldb" | "mariadb" => {
            if let Some(my) = &db.mysql {
                out.push(json!({ "name": "DB_MYSQLDB_HOST", "value": my.host }));
                if let Some(p) = my.port {
                    out.push(env_str("DB_MYSQLDB_PORT", p.to_string()));
                }
                out.push(json!({ "name": "DB_MYSQLDB_DATABASE", "value": my.database }));
                out.push(json!({ "name": "DB_MYSQLDB_USER", "value": my.user }));
                out.push(env_secret("DB_MYSQLDB_PASSWORD", &my.password_secret));
                if let Some(t) = my.connection_timeout_ms {
                    out.push(env_str("DB_MYSQLDB_CONNECTION_TIMEOUT", t.to_string()));
                }
                if let Some(ssl) = &my.ssl {
                    push_ssl_env(&mut out, "DB_MYSQLDB", ssl);
                }
            }
        }
        "sqlite" => {
            if let Some(sq) = &db.sqlite {
                if let Some(sz) = sq.pool_size {
                    out.push(env_str("DB_SQLITE_POOL_SIZE", sz.to_string()));
                }
                if let Some(v) = sq.vacuum_on_startup {
                    out.push(env_str("DB_SQLITE_VACUUM_ON_STARTUP", v.to_string()));
                }
                if let Some(d) = &sq.database {
                    out.push(json!({ "name": "DB_SQLITE_DATABASE", "value": d }));
                }
            }
        }
        _ => {}
    }
    out
}

fn push_ssl_env(out: &mut Vec<Value>, prefix: &str, ssl: &DatabaseSsl) {
    out.push(env_str(&format!("{prefix}_SSL_ENABLED"), ssl.enabled.to_string()));
    if let Some(r) = ssl.reject_unauthorized {
        out.push(env_str(
            &format!("{prefix}_SSL_REJECT_UNAUTHORIZED"),
            r.to_string(),
        ));
    }
    if ssl.ca_secret.is_some() {
        out.push(env_str(&format!("{prefix}_SSL_CA"), "/etc/n8n/ssl/ca/ca.crt"));
    }
    if ssl.cert_secret.is_some() {
        out.push(env_str(
            &format!("{prefix}_SSL_CERT"),
            "/etc/n8n/ssl/cert/cert.crt",
        ));
    }
    if ssl.key_secret.is_some() {
        out.push(env_str(&format!("{prefix}_SSL_KEY"), "/etc/n8n/ssl/key/key.pem"));
    }
}
