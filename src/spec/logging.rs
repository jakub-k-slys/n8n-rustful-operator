use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Logging, diagnostics and metrics toggles. Every field is optional and maps
/// to one n8n env var. Wired as a low-priority env layer, so an `extraEnv`
/// entry of the same name still wins.
#[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
pub struct LoggingConfig {
    /// `N8N_LOG_LEVEL`, e.g. `error`, `warn`, `info`, `debug`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// `N8N_LOG_OUTPUT`, e.g. `console`, `file`, or `console,file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// `N8N_DIAGNOSTICS_ENABLED` — anonymous telemetry to n8n.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<bool>,
    /// `N8N_VERSION_NOTIFICATIONS_ENABLED` — in-app update notices.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "versionNotifications"
    )]
    pub version_notifications: Option<bool>,
    /// `N8N_METRICS` — expose the Prometheus `/metrics` endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<bool>,
}
