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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{SmtpAuth, SmtpConfig};

    #[test]
    fn smtp_env_maps_fields_and_credentials() {
        let s = SmtpConfig {
            host: "smtp.example.com".into(),
            port: 587,
            sender: "n8n <no-reply@example.com>".into(),
            ssl: Some(false),
            start_tls: Some(true),
            auth: Some(SmtpAuth {
                user_secret: SecretKeyRef {
                    name: "smtp".into(),
                    key: "user".into(),
                },
                password_secret: SecretKeyRef {
                    name: "smtp".into(),
                    key: "password".into(),
                },
            }),
        };
        let env = build_smtp_env(&s);
        let by_name = |n: &str| env.iter().find(|e| e.name == n);
        assert_eq!(by_name("N8N_EMAIL_MODE").unwrap().value.as_deref(), Some("smtp"));
        assert_eq!(by_name("N8N_SMTP_PORT").unwrap().value.as_deref(), Some("587"));
        assert_eq!(
            by_name("N8N_SMTP_STARTTLS").unwrap().value.as_deref(),
            Some("true")
        );
        let user = by_name("N8N_SMTP_USER").unwrap();
        assert!(user.value.is_none());
        assert_eq!(user.value_from.as_ref().unwrap().secret_ref.key, "user");
    }

    #[test]
    fn smtp_without_auth_omits_credentials() {
        let s = SmtpConfig {
            host: "relay.internal".into(),
            port: 25,
            sender: "n8n@internal".into(),
            ssl: None,
            start_tls: None,
            auth: None,
        };
        let names: Vec<_> = build_smtp_env(&s).into_iter().map(|e| e.name).collect();
        assert_eq!(
            names,
            [
                "N8N_EMAIL_MODE",
                "N8N_SMTP_HOST",
                "N8N_SMTP_PORT",
                "N8N_SMTP_SENDER"
            ]
        );
    }
}
