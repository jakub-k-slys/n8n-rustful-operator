use crate::spec::common::SecretKeyRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
pub struct RedisConfig {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "passwordSecret")]
    pub password_secret: Option<SecretKeyRef>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "usernameSecret")]
    pub username_secret: Option<SecretKeyRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<bool>,
    /// `QUEUE_BULL_PREFIX` for namespacing within a shared Redis instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}
