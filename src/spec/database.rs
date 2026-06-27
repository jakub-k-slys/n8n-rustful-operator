use crate::spec::common::SecretKeyRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
pub struct DatabaseSpec {
    /// `sqlite` (default), `postgresdb`, `mysqldb` or `mariadb` — maps to `DB_TYPE`.
    #[serde(default = "default_db_type", rename = "type")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sqlite: Option<SqliteConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postgres: Option<PostgresConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mysql: Option<MysqlConfig>,
}

fn default_db_type() -> String {
    "sqlite".to_string()
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct PostgresConfig {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    pub database: String,
    /// DB user, sourced from a Secret (plaintext is not supported).
    #[serde(rename = "userSecret")]
    pub user_secret: SecretKeyRef,
    #[serde(rename = "passwordSecret")]
    pub password_secret: SecretKeyRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl: Option<DatabaseSsl>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "poolSize")]
    pub pool_size: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "connectionTimeoutMs"
    )]
    pub connection_timeout_ms: Option<u32>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct MysqlConfig {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    pub database: String,
    pub user: String,
    #[serde(rename = "passwordSecret")]
    pub password_secret: SecretKeyRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl: Option<DatabaseSsl>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "connectionTimeoutMs"
    )]
    pub connection_timeout_ms: Option<u32>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SqliteConfig {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "poolSize")]
    pub pool_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "vacuumOnStartup")]
    pub vacuum_on_startup: Option<bool>,
    /// Path inside the pod, mapped to `DB_SQLITE_DATABASE`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct DatabaseSsl {
    #[serde(default)]
    pub enabled: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "rejectUnauthorized"
    )]
    pub reject_unauthorized: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "caSecret")]
    pub ca_secret: Option<SecretKeyRef>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "certSecret")]
    pub cert_secret: Option<SecretKeyRef>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "keySecret")]
    pub key_secret: Option<SecretKeyRef>,
}
