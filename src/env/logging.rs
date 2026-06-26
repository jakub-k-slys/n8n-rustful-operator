use crate::spec::{EnvVar, LoggingConfig};

/// Map `LoggingConfig` to n8n's logging / diagnostics / metrics env. Returned
/// as `EnvVar`s so it can ride the low-priority `build_user_env` layer
/// (overridable by `extraEnv`). Only set fields are emitted.
pub fn build_logging_env(l: &LoggingConfig) -> Vec<EnvVar> {
    let lit = |name: &str, value: String| EnvVar {
        name: name.to_string(),
        value: Some(value),
        value_from: None,
    };
    let mut out = Vec::new();
    if let Some(level) = &l.level {
        out.push(lit("N8N_LOG_LEVEL", level.clone()));
    }
    if let Some(output) = &l.output {
        out.push(lit("N8N_LOG_OUTPUT", output.clone()));
    }
    if let Some(d) = l.diagnostics {
        out.push(lit("N8N_DIAGNOSTICS_ENABLED", d.to_string()));
    }
    if let Some(v) = l.version_notifications {
        out.push(lit("N8N_VERSION_NOTIFICATIONS_ENABLED", v.to_string()));
    }
    if let Some(m) = l.metrics {
        out.push(lit("N8N_METRICS", m.to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logging_env_maps_set_fields() {
        let l = LoggingConfig {
            level: Some("debug".into()),
            output: Some("console".into()),
            diagnostics: Some(true),
            version_notifications: Some(false),
            metrics: Some(true),
        };
        let env = build_logging_env(&l);
        let by_name = |n: &str| env.iter().find(|e| e.name == n).and_then(|e| e.value.as_deref());
        assert_eq!(by_name("N8N_LOG_LEVEL"), Some("debug"));
        assert_eq!(by_name("N8N_LOG_OUTPUT"), Some("console"));
        assert_eq!(by_name("N8N_DIAGNOSTICS_ENABLED"), Some("true"));
        assert_eq!(by_name("N8N_VERSION_NOTIFICATIONS_ENABLED"), Some("false"));
        assert_eq!(by_name("N8N_METRICS"), Some("true"));
    }

    #[test]
    fn logging_env_omits_unset_fields() {
        let l = LoggingConfig {
            level: Some("info".into()),
            ..Default::default()
        };
        let names: Vec<_> = build_logging_env(&l).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["N8N_LOG_LEVEL"]);
    }
}
