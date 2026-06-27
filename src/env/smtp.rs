use crate::spec::{EnvVar, EnvVarSource, SecretKeyRef, SmtpConfig};

/// Map `SmtpConfig` to n8n's `N8N_EMAIL_MODE` / `N8N_SMTP_*` env. Returned as
/// `EnvVar`s so it can ride the low-priority `build_user_env` layer (overridable
/// by `extraEnv`); the credentials render as `valueFrom` secret refs.
pub fn build_smtp_env(s: &SmtpConfig) -> Vec<EnvVar> {
    let lit = |name: &str, value: String| EnvVar {
        name: name.to_string(),
        value: Some(value),
        value_from: None,
    };
    let secret = |name: &str, sec: &SecretKeyRef| EnvVar {
        name: name.to_string(),
        value: None,
        value_from: Some(EnvVarSource {
            secret_ref: sec.clone(),
        }),
    };
    let mut out = vec![
        lit("N8N_EMAIL_MODE", "smtp".to_string()),
        lit("N8N_SMTP_HOST", s.host.clone()),
        lit("N8N_SMTP_PORT", s.port.to_string()),
        lit("N8N_SMTP_SENDER", s.sender.clone()),
    ];
    if let Some(ssl) = s.ssl {
        out.push(lit("N8N_SMTP_SSL", ssl.to_string()));
    }
    if let Some(starttls) = s.start_tls {
        out.push(lit("N8N_SMTP_STARTTLS", starttls.to_string()));
    }
    if let Some(auth) = &s.auth {
        out.push(secret("N8N_SMTP_USER", &auth.user_secret));
        out.push(secret("N8N_SMTP_PASS", &auth.password_secret));
    }
    out
}
