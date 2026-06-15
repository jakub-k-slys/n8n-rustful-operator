use crate::{builders::pvc::secret_volume, spec::DatabaseSpec};
use serde_json::{Value, json};

/// Returns (volumes, volume_mounts) for database SSL certificate secrets.
pub fn build_db_volumes(_instance: &str, db: &DatabaseSpec) -> (Vec<Value>, Vec<Value>) {
    let mut vols = vec![];
    let mut mounts = vec![];
    let ssl_ref = match db.type_.as_str() {
        "postgresdb" => db.postgres.as_ref().and_then(|p| p.ssl.as_ref()),
        "mysqldb" | "mariadb" => db.mysql.as_ref().and_then(|m| m.ssl.as_ref()),
        _ => None,
    };
    if let Some(ssl) = ssl_ref {
        if let Some(sec) = &ssl.ca_secret {
            vols.push(secret_volume("n8n-db-ssl-ca", &sec.name, &sec.key, "ca.crt"));
            mounts.push(json!({ "name": "n8n-db-ssl-ca", "mountPath": "/etc/n8n/ssl/ca", "readOnly": true }));
        }
        if let Some(sec) = &ssl.cert_secret {
            vols.push(secret_volume("n8n-db-ssl-cert", &sec.name, &sec.key, "cert.crt"));
            mounts.push(
                json!({ "name": "n8n-db-ssl-cert", "mountPath": "/etc/n8n/ssl/cert", "readOnly": true }),
            );
        }
        if let Some(sec) = &ssl.key_secret {
            vols.push(secret_volume("n8n-db-ssl-key", &sec.name, &sec.key, "key.pem"));
            mounts
                .push(json!({ "name": "n8n-db-ssl-key", "mountPath": "/etc/n8n/ssl/key", "readOnly": true }));
        }
    }
    (vols, mounts)
}
