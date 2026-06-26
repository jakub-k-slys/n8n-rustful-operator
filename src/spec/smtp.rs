use crate::spec::common::SecretKeyRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// SMTP notification settings. Its presence sets `N8N_EMAIL_MODE=smtp`; the
/// rest map to the `N8N_SMTP_*` env. Wired as a low-priority env layer, so an
/// `extraEnv` entry of the same name still wins.
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    /// `N8N_SMTP_SENDER`, e.g. `n8n <no-reply@example.com>`.
    pub sender: String,
    /// `N8N_SMTP_SSL` — implicit TLS (usually port 465).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl: Option<bool>,
    /// `N8N_SMTP_STARTTLS` — STARTTLS upgrade (usually port 587).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "startTls")]
    pub start_tls: Option<bool>,
    /// Credentials; omit for relays that accept unauthenticated mail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<SmtpAuth>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SmtpAuth {
    /// `N8N_SMTP_USER`, sourced from a Secret.
    #[serde(rename = "userSecret")]
    pub user_secret: SecretKeyRef,
    /// `N8N_SMTP_PASS`, sourced from a Secret.
    #[serde(rename = "passwordSecret")]
    pub password_secret: SecretKeyRef,
}
