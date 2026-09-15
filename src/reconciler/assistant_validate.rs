use crate::{Error, Result, spec::Assistant};
use kube::ResourceExt;

const PROVIDERS: &[&str] = &["anthropic", "openai", "openrouter"];
/// Longest generated object name is the runner's Docker-storage PVC (created
/// only when `sandbox.runner.dockerStorage.persistence` is set) — longer than
/// the runner Service/Deployment name it's derived from. Names are capped at
/// 63 characters by the apiserver.
const NAME_SUFFIX: &str = "-sandbox-runner-1-docker";
const MAX_NAME: usize = 63;

/// `SecretKeyRef::key` defaults to `encryption_key` because the struct was
/// written for the encryption-key use case. `ModelConfig`/`BraveConfig` reuse
/// it for arbitrary API keys, so a caller that omits `key` would silently get
/// a lookup for a key literally named `encryption_key` — require it explicit.
const INHERITED_KEY_DEFAULT: &str = "encryption_key";

pub fn validate_assistant(a: &Assistant, cr_ns: &str) -> Result<()> {
    let spec = &a.spec;
    if !matches!(spec.target_ref.kind.as_str(), "Cluster" | "Single") {
        return Err(Error::IllegalAssistant(format!(
            "targetRef.kind {:?} must be Cluster or Single",
            spec.target_ref.kind
        )));
    }
    if spec.target_ref.name.is_empty() {
        return Err(Error::IllegalAssistant("targetRef.name must not be empty".into()));
    }
    if spec.model.url.is_none() {
        let (provider, model) = spec.model.name.split_once('/').ok_or_else(|| {
            Error::IllegalAssistant(format!(
                "model.name {:?} must be in provider/model form",
                spec.model.name
            ))
        })?;
        if !PROVIDERS.contains(&provider) {
            return Err(Error::IllegalAssistant(format!(
                "unknown model provider {provider:?} (want one of {PROVIDERS:?})"
            )));
        }
        if model.is_empty() {
            return Err(Error::IllegalAssistant(
                "model.name has an empty model part".into(),
            ));
        }
        if spec.model.api_key_secret.is_none() {
            return Err(Error::IllegalAssistant(
                "model.apiKeySecret is required unless model.url is set".into(),
            ));
        }
    } else if spec.model.name.is_empty() {
        return Err(Error::IllegalAssistant("model.name must not be empty".into()));
    }
    if let Some(secret) = &spec.model.api_key_secret {
        validate_api_key_secret("model.apiKeySecret", secret)?;
    }
    if let Some(search) = &spec.search
        && let Some(brave) = &search.brave
    {
        validate_api_key_secret("search.brave.apiKeySecret", &brave.api_key_secret)?;
    }
    let sbx_ns = spec.sandbox_namespace(cr_ns);
    if !is_dns1123_label(&sbx_ns) {
        return Err(Error::IllegalAssistant(format!(
            "sandbox.namespace {sbx_ns:?} is not a valid DNS-1123 label"
        )));
    }
    if spec.sandbox.version.is_empty() || spec.sandbox.sandbox_image.is_empty() {
        return Err(Error::IllegalAssistant(
            "sandbox.version and sandbox.sandboxImage must not be empty".into(),
        ));
    }
    let longest = format!("{cr_ns}-{}{NAME_SUFFIX}", a.name_any()).len();
    if longest > MAX_NAME {
        return Err(Error::IllegalAssistant(format!(
            "the longest generated object name would be {longest} characters, over the {MAX_NAME} limit; \
             shorten the Assistant name or its namespace"
        )));
    }
    Ok(())
}

fn validate_api_key_secret(field: &str, secret: &crate::spec::SecretKeyRef) -> Result<()> {
    if secret.name.is_empty() {
        return Err(Error::IllegalAssistant(format!("{field}.name must not be empty")));
    }
    if secret.key.is_empty() || secret.key == INHERITED_KEY_DEFAULT {
        return Err(Error::IllegalAssistant(format!(
            "{field}.key must be set explicitly (it defaults to {INHERITED_KEY_DEFAULT:?}, which is almost \
             certainly not the intended key)"
        )));
    }
    Ok(())
}

fn is_dns1123_label(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{AssistantSpec, ModelConfig, SandboxConfig, SecretKeyRef, TargetRef};
    use kube::api::ObjectMeta;

    fn assistant(name: &str, spec: AssistantSpec) -> Assistant {
        Assistant {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some("n8n-cluster".to_string()),
                ..Default::default()
            },
            spec,
            status: None,
        }
    }

    fn base_spec() -> AssistantSpec {
        AssistantSpec {
            target_ref: TargetRef {
                kind: "Cluster".into(),
                name: "n8n".into(),
            },
            model: ModelConfig {
                name: "anthropic/claude-opus-4-8".into(),
                api_key_secret: Some(SecretKeyRef {
                    name: "n8n-ai".into(),
                    key: "ANTHROPIC_API_KEY".into(),
                }),
                url: None,
            },
            search: None,
            sandbox: SandboxConfig::default(),
        }
    }

    #[test]
    fn accepts_a_valid_spec() {
        assert!(validate_assistant(&assistant("n8n", base_spec()), "n8n-cluster").is_ok());
    }

    #[test]
    fn rejects_an_unknown_target_kind() {
        let mut s = base_spec();
        s.target_ref.kind = "Deployment".into();
        let err = validate_assistant(&assistant("n8n", s), "n8n-cluster").unwrap_err();
        assert!(format!("{err}").contains("targetRef.kind"));
    }

    #[test]
    fn rejects_a_model_without_a_provider_prefix() {
        let mut s = base_spec();
        s.model.name = "claude-opus-4-8".into();
        assert!(validate_assistant(&assistant("n8n", s), "n8n-cluster").is_err());
    }

    #[test]
    fn rejects_an_unknown_provider() {
        let mut s = base_spec();
        s.model.name = "bedrock/claude".into();
        assert!(validate_assistant(&assistant("n8n", s), "n8n-cluster").is_err());
    }

    #[test]
    fn allows_any_model_name_when_a_custom_url_is_set() {
        let mut s = base_spec();
        s.model.name = "local/whatever".into();
        s.model.url = Some("http://lmstudio:1234/v1".into());
        s.model.api_key_secret = None;
        assert!(validate_assistant(&assistant("n8n", s), "n8n-cluster").is_ok());
    }

    #[test]
    fn requires_an_api_key_without_a_custom_url() {
        let mut s = base_spec();
        s.model.api_key_secret = None;
        assert!(validate_assistant(&assistant("n8n", s), "n8n-cluster").is_err());
    }

    #[test]
    fn rejects_names_that_would_overflow_the_docker_pvc_name_limit() {
        // "<ns>-<name>-sandbox-runner-1-docker" must fit in 63 characters.
        let long = "a".repeat(28);
        let err = validate_assistant(&assistant(&long, base_spec()), "n8n-cluster").unwrap_err();
        assert!(format!("{err}").contains("63"));
    }

    #[test]
    fn accepts_a_name_that_exactly_fits() {
        // 63 - len("n8n-cluster-") - len("-sandbox-runner-1-docker") = 27
        let name = "a".repeat(27);
        assert!(validate_assistant(&assistant(&name, base_spec()), "n8n-cluster").is_ok());
    }

    #[test]
    fn rejects_an_invalid_sandbox_namespace() {
        let mut s = base_spec();
        s.sandbox.namespace = Some("Not_A_Namespace".into());
        assert!(validate_assistant(&assistant("n8n", s), "n8n-cluster").is_err());
    }

    #[test]
    fn rejects_a_model_api_key_secret_without_an_explicit_key() {
        let mut s = base_spec();
        s.model.api_key_secret = Some(SecretKeyRef {
            name: "n8n-ai".into(),
            key: "encryption_key".into(),
        });
        let err = validate_assistant(&assistant("n8n", s), "n8n-cluster").unwrap_err();
        assert!(format!("{err}").contains("model.apiKeySecret"));
    }

    #[test]
    fn rejects_a_brave_api_key_secret_without_an_explicit_key() {
        let mut s = base_spec();
        s.search = Some(crate::spec::SearchConfig {
            brave: Some(crate::spec::BraveConfig {
                api_key_secret: SecretKeyRef {
                    name: "n8n-ai".into(),
                    key: "encryption_key".into(),
                },
            }),
            searxng: None,
        });
        let err = validate_assistant(&assistant("n8n", s), "n8n-cluster").unwrap_err();
        assert!(format!("{err}").contains("search.brave.apiKeySecret"));
    }
}
