# Assistant CRD (self-hosted n8n Assistant sandbox) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dodać CRD `Assistant`, który uruchamia self-hosted stack sandboxa n8n Assistant (Job z certami mTLS, `sandbox-api`, uprzywilejowany DinD runner) w osobnym namespace i wstrzykuje konfigurację `instance-ai` do `Cluster`/`Single` przez Secret + `envFrom`.

**Architecture:** Trzeci kontroler obok `Single` i `Cluster`. `Assistant` żyje w namespace targetu i posiada tam wyłącznie `Secret <target>-instance-ai` (ownerRef → GC). Stack sandboxa trafia do osobnego namespace z PSA `privileged`; tamtejsze obiekty nie mają ownerRef (cross-namespace ownerRef = kasowanie jako sieroty), więc identyfikuje je etykieta, a sprząta finalizer. Certy mTLS generuje jednorazowy Job, którego drugi kontener (`tlspub`, trzeci bin w obrazie operatora) publikuje wynik jako dwa Secrety po whitelist ie nazw plików — klucz root CA nie jest nigdzie utrwalany.

**Tech Stack:** Rust 2024, `kube` v4 (`runtime`, `client`, `derive`), `k8s-openapi` 0.28 (`latest`), `schemars` 1, `serde_json`, `rand`, `hex`, `thiserror`. Testy: wbudowany harness `cargo test` + `cucumber` 0.23 dla e2e.

**Spec:** `docs/superpowers/specs/2026-09-15-n8n-assistant-sandbox-design.md`

## Global Constraints

- Wszystkie zapisy do k8s przez server-side apply z field managerem `n8n-rustful-operator`. Nie zmieniać managera.
- `selector_labels` są niemutowalne — nigdy ich nie zmieniać dla istniejących obiektów.
- Błędy reconcilera muszą przechodzić przez `Error::FinalizerError(Box<...>)`. Nie spłaszczać boxa.
- `#[kube(...)]` musi jawnie ustawiać `plural` — kube-derive źle odmienia nazwy.
- Każde pole `status` musi występować w bloku `Patch::Apply` — SSA usuwa pola pominięte w patchu.
- Po zmianie dowolnego typu spec/status: `just generate` (odświeża `yaml/crd.yaml`), a na klastrze `just install-crd`.
- Formatowanie: `cargo fmt` na stabilnym toolchainie (`rustfmt.toml` ma tylko stabilne opcje). Maksymalna szerokość linii wynika z `rustfmt.toml` — nie łamać ręcznie.
- Commity w konwencji Conventional Commits (`feat:`, `fix:`, `test:`, `docs:`, `build:`, `refactor:`).
- Obrazy stacka, wersja przypięta w `spec.sandbox.version`, domyślnie `1.2.0`:
  - `ghcr.io/n8n-io/n8n-sandbox-service-api:<version>` (certy **i** API)
  - `ghcr.io/n8n-io/n8n-sandbox-service-runner-dind:<version>`
  - domyślny obraz sandboxa: `ghcr.io/n8n-io/n8n-sandbox-service-sandbox:latest`
- `INSTANCE_AI_BRAVE_SEARCH_API_KEY` celowo **bez** prefiksu `N8N_`. Nie „poprawiać".
- Prefiks nazw obiektów sandboxa: `<namespace-CR-a>-<nazwa-CR-a>`, dalej `<p>`.
- **W repo nie ma dziś żadnych testów jednostkowych.** Task 1 ustanawia konwencję: `#[cfg(test)] mod tests` na końcu pliku z testowanym kodem.

---

### Task 1: Typy CRD `Assistant`

**Files:**
- Create: `src/spec/assistant.rs`
- Modify: `src/spec/mod.rs`, `src/lib.rs`, `src/crdgen.rs`

**Interfaces:**
- Produces: `Assistant`, `AssistantSpec`, `AssistantStatus`, `TargetRef`, `ModelConfig`, `SearchConfig`, `BraveConfig`, `SearxngConfig`, `SandboxConfig`, `SandboxRoleConfig`, `DockerStorage`, `ASSISTANT_FINALIZER: &str`, oraz metody `AssistantSpec::sandbox_namespace(&self, cr_ns: &str) -> String`.
- Consumes: `SecretKeyRef`, `ResourceRequirements`, `PodConfig`, `PersistenceConfig` z `crate::spec`.

- [ ] **Step 1: Write the failing test**

Utwórz `src/spec/assistant.rs` wyłącznie z blokiem testów (kod produkcyjny dopisze Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn spec_json() -> serde_json::Value {
        serde_json::json!({
            "targetRef": { "kind": "Cluster", "name": "n8n" },
            "model": {
                "name": "anthropic/claude-opus-4-8",
                "apiKeySecret": { "name": "n8n-ai", "key": "ANTHROPIC_API_KEY" }
            }
        })
    }

    #[test]
    fn minimal_spec_applies_documented_defaults() {
        let s: AssistantSpec = serde_json::from_value(spec_json()).unwrap();
        assert_eq!(s.target_ref.kind, "Cluster");
        assert_eq!(s.sandbox.version, "1.2.0");
        assert_eq!(
            s.sandbox.sandbox_image,
            "ghcr.io/n8n-io/n8n-sandbox-service-sandbox:latest"
        );
        assert_eq!(s.sandbox.runner.docker_storage.size, "20Gi");
        assert!(s.sandbox.namespace.is_none());
        assert!(s.search.is_none());
    }

    #[test]
    fn sandbox_namespace_defaults_to_the_cr_namespace() {
        let s: AssistantSpec = serde_json::from_value(spec_json()).unwrap();
        assert_eq!(s.sandbox_namespace("n8n-cluster"), "n8n-cluster");
    }

    #[test]
    fn sandbox_namespace_honours_the_override() {
        let mut v = spec_json();
        v["sandbox"] = serde_json::json!({ "namespace": "n8n-sandbox" });
        let s: AssistantSpec = serde_json::from_value(v).unwrap();
        assert_eq!(s.sandbox_namespace("n8n-cluster"), "n8n-sandbox");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib spec::assistant`
Expected: FAIL — `cannot find type AssistantSpec in this scope` (modułu nie ma jeszcze w `spec/mod.rs`, więc najpierw pojawi się błąd kompilacji całego targetu).

- [ ] **Step 3: Write minimal implementation**

Wstaw na początek `src/spec/assistant.rs` (przed blokiem `mod tests`):

```rust
use crate::spec::common::{PersistenceConfig, ResourceRequirements, SecretKeyRef};
use crate::spec::pod::PodConfig;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub static ASSISTANT_FINALIZER: &str = "assistants.n8n.slys.dev";

/// `Assistant` configures the n8n Assistant (`instance-ai`) module for a
/// `Cluster` or `Single` in the same namespace, and owns the self-hosted
/// sandbox stack the module requires.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(
    kind = "Assistant",
    group = "n8n.slys.dev",
    version = "v1",
    namespaced,
    shortname = "n8na",
    plural = "assistants",
    status = "AssistantStatus"
)]
pub struct AssistantSpec {
    /// The n8n instance this assistant configures. Must be in the same
    /// namespace — the generated Secret is owned by this CR, and owner
    /// references cannot cross namespaces.
    #[serde(rename = "targetRef")]
    pub target_ref: TargetRef,
    pub model: ModelConfig,
    /// Optional web search. Brave takes priority over SearXNG when both are set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchConfig>,
    #[serde(default)]
    pub sandbox: SandboxConfig,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
pub struct TargetRef {
    /// `Cluster` or `Single`.
    pub kind: String,
    pub name: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
pub struct ModelConfig {
    /// `provider/model`, e.g. `anthropic/claude-opus-4-8`. Providers:
    /// `anthropic`, `openai`, `openrouter`.
    pub name: String,
    /// Provider API key. Required unless `url` points at an endpoint that
    /// needs none.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "apiKeySecret")]
    pub api_key_secret: Option<SecretKeyRef>,
    /// Custom OpenAI-compatible endpoint (`N8N_INSTANCE_AI_MODEL_URL`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SearchConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brave: Option<BraveConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub searxng: Option<SearxngConfig>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct BraveConfig {
    #[serde(rename = "apiKeySecret")]
    pub api_key_secret: SecretKeyRef,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SearxngConfig {
    /// URL of an externally-hosted SearXNG with the JSON API enabled.
    pub url: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SandboxConfig {
    /// Namespace for the sandbox stack. Defaults to the CR's namespace. Must
    /// carry `pod-security.kubernetes.io/enforce: privileged`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Tag for both `n8n-sandbox-service-*` images.
    #[serde(default = "default_sandbox_version")]
    pub version: String,
    /// Image the runner starts per execution.
    #[serde(default = "default_sandbox_image", rename = "sandboxImage")]
    pub sandbox_image: String,
    #[serde(default)]
    pub api: SandboxRoleConfig,
    #[serde(default)]
    pub runner: SandboxRunnerConfig,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            namespace: None,
            version: default_sandbox_version(),
            sandbox_image: default_sandbox_image(),
            api: SandboxRoleConfig::default(),
            runner: SandboxRunnerConfig::default(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
pub struct SandboxRoleConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod: Option<PodConfig>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SandboxRunnerConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod: Option<PodConfig>,
    /// Backing store for the runner's inner Docker (`/var/lib/docker`).
    #[serde(default, rename = "dockerStorage")]
    pub docker_storage: DockerStorage,
}

impl Default for SandboxRunnerConfig {
    fn default() -> Self {
        Self {
            resources: None,
            pod: None,
            docker_storage: DockerStorage::default(),
        }
    }
}

/// `/var/lib/docker` for the runner. An `emptyDir` with a size limit by
/// default; set `persistence` to keep the pulled sandbox image across restarts.
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct DockerStorage {
    /// `emptyDir.sizeLimit`. Ignored when `persistence` is set.
    #[serde(default = "default_docker_size")]
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistence: Option<PersistenceConfig>,
}

impl Default for DockerStorage {
    fn default() -> Self {
        Self {
            size: default_docker_size(),
            persistence: None,
        }
    }
}

fn default_sandbox_version() -> String {
    "1.2.0".to_string()
}
fn default_sandbox_image() -> String {
    "ghcr.io/n8n-io/n8n-sandbox-service-sandbox:latest".to_string()
}
fn default_docker_size() -> String {
    "20Gi".to_string()
}

impl AssistantSpec {
    /// Namespace the sandbox stack lives in: the override, or the CR's own.
    pub fn sandbox_namespace(&self, cr_ns: &str) -> String {
        self.sandbox
            .namespace
            .clone()
            .unwrap_or_else(|| cr_ns.to_string())
    }
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct AssistantStatus {
    pub ready: bool,
    #[serde(rename = "certsReady")]
    pub certs_ready: bool,
    #[serde(rename = "apiReady")]
    pub api_ready: bool,
    #[serde(rename = "runnerReady")]
    pub runner_ready: bool,
    #[serde(skip_serializing_if = "Option::is_none", rename = "targetSecret")]
    pub target_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
```

W `src/spec/mod.rs` dopisz `pub mod assistant;` (alfabetycznie, przed `cluster`) i re-eksport:

```rust
pub use assistant::{
    ASSISTANT_FINALIZER, Assistant, AssistantSpec, AssistantStatus, BraveConfig, DockerStorage, ModelConfig,
    SandboxConfig, SandboxRoleConfig, SandboxRunnerConfig, SearchConfig, SearxngConfig, TargetRef,
};
```

W `src/lib.rs` dopisz te same nazwy do listy `pub use spec::{...}` (zachowaj porządek alfabetyczny).

W `src/crdgen.rs` dołóż trzeci blok:

```rust
    println!("---");
    print!(
        "{}",
        serde_yaml::to_string(&n8n_rustful_operator::Assistant::crd()).unwrap()
    );
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib spec::assistant`
Expected: PASS, 3 testy.

- [ ] **Step 5: Regenerate the CRD manifest**

Run: `just generate && git diff --stat yaml/crd.yaml`
Expected: `yaml/crd.yaml` rośnie o definicję `assistants.n8n.slys.dev`.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add src/spec/assistant.rs src/spec/mod.rs src/lib.rs src/crdgen.rs yaml/crd.yaml
git commit -m "feat: add Assistant CRD types"
```

---

### Task 2: Walidacja `Assistant`

**Files:**
- Create: `src/reconciler/assistant_validate.rs`
- Modify: `src/error.rs`, `src/reconciler/mod.rs`

**Interfaces:**
- Consumes: `Assistant` (Task 1).
- Produces: `validate_assistant(a: &Assistant, cr_ns: &str) -> Result<()>`, `Error::IllegalAssistant(String)`.

- [ ] **Step 1: Write the failing test**

Utwórz `src/reconciler/assistant_validate.rs` z samym blokiem testów:

```rust
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
    fn rejects_names_that_would_overflow_the_service_name_limit() {
        // "<ns>-<name>-sandbox-runner-1" must fit in 63 characters.
        let long = "a".repeat(45);
        let err = validate_assistant(&assistant(&long, base_spec()), "n8n-cluster").unwrap_err();
        assert!(format!("{err}").contains("63"));
    }

    #[test]
    fn accepts_a_name_that_exactly_fits() {
        // 63 - len("n8n-cluster-") - len("-sandbox-runner-1") = 34
        let name = "a".repeat(34);
        assert!(validate_assistant(&assistant(&name, base_spec()), "n8n-cluster").is_ok());
    }

    #[test]
    fn rejects_an_invalid_sandbox_namespace() {
        let mut s = base_spec();
        s.sandbox.namespace = Some("Not_A_Namespace".into());
        assert!(validate_assistant(&assistant("n8n", s), "n8n-cluster").is_err());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib assistant_validate`
Expected: FAIL — `cannot find function validate_assistant`.

- [ ] **Step 3: Write minimal implementation**

W `src/error.rs` dopisz wariant przed `pub type Result`:

```rust
    #[error("IllegalAssistant: {0}")]
    IllegalAssistant(String),
```

Na początek `src/reconciler/assistant_validate.rs`:

```rust
use crate::{Error, Result, spec::Assistant};
use kube::ResourceExt;

const PROVIDERS: &[&str] = &["anthropic", "openai", "openrouter"];
/// Longest generated object name is the runner Service; Service names are
/// capped at 63 characters by the apiserver.
const NAME_SUFFIX: &str = "-sandbox-runner-1";
const MAX_NAME: usize = 63;

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
            return Err(Error::IllegalAssistant("model.name has an empty model part".into()));
        }
        if spec.model.api_key_secret.is_none() {
            return Err(Error::IllegalAssistant(
                "model.apiKeySecret is required unless model.url is set".into(),
            ));
        }
    } else if spec.model.name.is_empty() {
        return Err(Error::IllegalAssistant("model.name must not be empty".into()));
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
            "generated Service name would be {longest} characters, over the {MAX_NAME} limit; \
             shorten the Assistant name or its namespace"
        )));
    }
    Ok(())
}

fn is_dns1123_label(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}
```

W `src/reconciler/mod.rs` dopisz `pub mod assistant_validate;` (alfabetycznie).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib assistant_validate`
Expected: PASS, 9 testów.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/error.rs src/reconciler/assistant_validate.rs src/reconciler/mod.rs
git commit -m "feat: validate Assistant spec"
```

---

### Task 3: Binarka `tlspub` — publikacja certów jako Secretów

**Files:**
- Create: `src/bin/tlspub.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Produces: bin `tlspub`, CLI `tlspub --dir <DIR> --namespace <NS> --api-secret <NAME> --runner-secret <NAME> [--label k=v]...`; funkcja `collect(dir: &Path, side: Side) -> Result<BTreeMap<String, String>, String>` i `enum Side { Api, Runner }`.

**Uwaga bezpieczeństwa:** `collect` kopiuje **wyłącznie** pliki z whitelisty. Klucz root CA (dowolna nazwa, jaką nada mu `bootstrap-mtls.sh`) nie może trafić do żadnego Secreta — to jest testowane.

- [ ] **Step 1: Write the failing test**

Utwórz `src/bin/tlspub.rs` z samym blokiem testów:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("tlspub-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("api")).unwrap();
        fs::create_dir_all(d.join("runner")).unwrap();
        d
    }

    fn write(dir: &std::path::Path, rel: &str, body: &str) {
        fs::write(dir.join(rel), body).unwrap();
    }

    #[test]
    fn collects_exactly_the_five_api_files() {
        let d = tmpdir("api");
        for f in API_FILES {
            write(&d, &format!("api/{f}"), f);
        }
        let out = collect(&d, Side::Api).unwrap();
        let mut keys: Vec<&String> = out.keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "ca.crt",
                "control-grpc-api-client.crt",
                "control-grpc-api-client.key",
                "grpc-server.crt",
                "grpc-server.key"
            ]
        );
        assert_eq!(out["ca.crt"], "ca.crt");
    }

    #[test]
    fn never_publishes_the_ca_private_key() {
        let d = tmpdir("cakey");
        for f in API_FILES {
            write(&d, &format!("api/{f}"), f);
        }
        write(&d, "api/ca.key", "SUPER SECRET CA KEY");
        write(&d, "api/root-ca.key", "SUPER SECRET CA KEY");
        let out = collect(&d, Side::Api).unwrap();
        assert!(!out.contains_key("ca.key"));
        assert!(!out.contains_key("root-ca.key"));
        assert!(!out.values().any(|v| v.contains("SUPER SECRET")));
    }

    #[test]
    fn collects_exactly_the_five_runner_files() {
        let d = tmpdir("runner");
        for f in RUNNER_FILES {
            write(&d, &format!("runner/{f}"), f);
        }
        let out = collect(&d, Side::Runner).unwrap();
        assert_eq!(out.len(), 5);
        assert!(out.contains_key("control-grpc-server.key"));
    }

    #[test]
    fn fails_loudly_when_a_whitelisted_file_is_missing() {
        let d = tmpdir("missing");
        write(&d, "api/ca.crt", "x");
        let err = collect(&d, Side::Api).unwrap_err();
        assert!(err.contains("grpc-server.crt"));
    }

    #[test]
    fn parses_repeated_label_flags() {
        let labels = parse_labels(&["a=1".to_string(), "b=2".to_string()]).unwrap();
        assert_eq!(labels["a"], "1");
        assert_eq!(labels["b"], "2");
    }

    #[test]
    fn rejects_a_malformed_label() {
        assert!(parse_labels(&["nope".to_string()]).is_err());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bin tlspub`
Expected: FAIL — bin nie jest zadeklarowany w `Cargo.toml`, więc target nie istnieje.

- [ ] **Step 3: Write minimal implementation**

W `Cargo.toml`, po bloku `[[bin]] crdgen`:

```toml
[[bin]]
doc = false
name = "tlspub"
path = "src/bin/tlspub.rs"
```

Na początek `src/bin/tlspub.rs`:

```rust
//! Publishes the mTLS material produced by `bootstrap-mtls.sh` as two Secrets.
//!
//! Runs as the second container of the `<p>-sandbox-certs` Job. Only the files
//! on the whitelists below are copied — the root CA private key stays in the
//! Job's emptyDir and dies with it.

use k8s_openapi::api::core::v1::Secret;
use kube::{
    Client,
    api::{Api, ObjectMeta, PostParams},
};
use std::{collections::BTreeMap, path::Path};

pub const API_FILES: &[&str] = &[
    "ca.crt",
    "grpc-server.crt",
    "grpc-server.key",
    "control-grpc-api-client.crt",
    "control-grpc-api-client.key",
];

pub const RUNNER_FILES: &[&str] = &[
    "ca.crt",
    "grpc-client.crt",
    "grpc-client.key",
    "control-grpc-server.crt",
    "control-grpc-server.key",
];

#[derive(Clone, Copy)]
pub enum Side {
    Api,
    Runner,
}

impl Side {
    fn dir(self) -> &'static str {
        match self {
            Side::Api => "api",
            Side::Runner => "runner",
        }
    }
    fn files(self) -> &'static [&'static str] {
        match self {
            Side::Api => API_FILES,
            Side::Runner => RUNNER_FILES,
        }
    }
}

/// Read the whitelisted files for one side into Secret `stringData`.
/// Everything not on the whitelist is ignored, by design.
pub fn collect(dir: &Path, side: Side) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for f in side.files() {
        let path = dir.join(side.dir()).join(f);
        let body = std::fs::read_to_string(&path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        out.insert((*f).to_string(), body);
    }
    Ok(out)
}

pub fn parse_labels(pairs: &[String]) -> Result<BTreeMap<String, String>, String> {
    pairs
        .iter()
        .map(|p| {
            p.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| format!("label {p:?} is not k=v"))
        })
        .collect()
}

fn flag(args: &[String], name: &str) -> Result<String, String> {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].clone())
        .ok_or_else(|| format!("missing required flag {name}"))
}

fn repeated(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|w| w[0] == name)
        .map(|w| w[1].clone())
        .collect()
}

async fn publish(
    api: &Api<Secret>,
    name: &str,
    ns: &str,
    labels: &BTreeMap<String, String>,
    data: BTreeMap<String, String>,
) -> Result<(), String> {
    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        string_data: Some(data),
        type_: Some("Opaque".to_string()),
        ..Default::default()
    };
    match api.create(&PostParams::default(), &secret).await {
        Ok(_) => Ok(()),
        // Another run got there first: the desired state already holds.
        Err(kube::Error::Api(ae)) if ae.code == 409 => Ok(()),
        Err(e) => Err(format!("creating Secret {name}: {e}")),
    }
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("tlspub: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let dir = flag(&args, "--dir")?;
    let ns = flag(&args, "--namespace")?;
    let api_secret = flag(&args, "--api-secret")?;
    let runner_secret = flag(&args, "--runner-secret")?;
    let labels = parse_labels(&repeated(&args, "--label"))?;

    let dir = Path::new(&dir);
    let api_data = collect(dir, Side::Api)?;
    let runner_data = collect(dir, Side::Runner)?;

    let client = Client::try_default()
        .await
        .map_err(|e| format!("kube client: {e}"))?;
    let secrets: Api<Secret> = Api::namespaced(client, &ns);
    publish(&secrets, &api_secret, &ns, &labels, api_data).await?;
    publish(&secrets, &runner_secret, &ns, &labels, runner_data).await?;
    println!("tlspub: published {api_secret} and {runner_secret} in {ns}");
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin tlspub`
Expected: PASS, 6 testów.

- [ ] **Step 5: Verify the Dockerfile ships the new binary**

Run: `grep -n "crdgen\|COPY --from\|n8n-rustful-operator" Dockerfile`
Jeśli `Dockerfile` kopiuje konkretne binarki po nazwie, dopisz `tlspub` obok pozostałych. Jeśli kopiuje cały katalog wyjściowy, nie zmieniaj nic.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add Cargo.toml src/bin/tlspub.rs Dockerfile
git commit -m "feat: add tlspub binary publishing sandbox mTLS material as Secrets"
```

---

### Task 4: Zawartość Secreta `instance-ai`

**Files:**
- Create: `src/env/instance_ai.rs`
- Modify: `src/env/mod.rs`

**Interfaces:**
- Consumes: `AssistantSpec` (Task 1).
- Produces: `build_instance_ai_data(spec: &AssistantSpec, sandbox_url: &str, sandbox_api_key: &str, model_api_key: Option<&str>, brave_api_key: Option<&str>) -> BTreeMap<String, String>`.

Klucze modelu i Brave są **kopiowane** — funkcja dostaje już rozwiązane wartości, odczyt Secretów źródłowych należy do reconcilera (Task 8).

- [ ] **Step 1: Write the failing test**

Utwórz `src/env/instance_ai.rs` z samym blokiem testów:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{
        AssistantSpec, BraveConfig, ModelConfig, SandboxConfig, SearchConfig, SearxngConfig, SecretKeyRef,
        TargetRef,
    };

    fn spec() -> AssistantSpec {
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

    const URL: &str = "http://n8n-cluster-n8n-sandbox-api.n8n-sandbox.svc:8080";

    #[test]
    fn wires_the_module_sandbox_and_model() {
        let d = build_instance_ai_data(&spec(), URL, "k3y", Some("sk-ant-x"), None);
        assert_eq!(d["N8N_ENABLED_MODULES"], "instance-ai");
        assert_eq!(d["N8N_INSTANCE_AI_MODEL"], "anthropic/claude-opus-4-8");
        assert_eq!(d["N8N_INSTANCE_AI_MODEL_API_KEY"], "sk-ant-x");
        assert_eq!(d["N8N_INSTANCE_AI_SANDBOX_ENABLED"], "true");
        assert_eq!(d["N8N_INSTANCE_AI_SANDBOX_PROVIDER"], "n8n-sandbox");
        assert_eq!(
            d["N8N_INSTANCE_AI_SANDBOX_IMAGE"],
            "ghcr.io/n8n-io/n8n-sandbox-service-sandbox:latest"
        );
        assert_eq!(d["N8N_INSTANCE_AI_SANDBOX_API_URL"], URL);
        assert_eq!(d["N8N_SANDBOX_SERVICE_URL"], URL);
        assert_eq!(d["N8N_SANDBOX_SERVICE_API_KEY"], "k3y");
    }

    #[test]
    fn omits_the_model_key_entry_when_there_is_none() {
        let mut s = spec();
        s.model.api_key_secret = None;
        s.model.url = Some("http://lmstudio:1234/v1".into());
        let d = build_instance_ai_data(&s, URL, "k3y", None, None);
        assert!(!d.contains_key("N8N_INSTANCE_AI_MODEL_API_KEY"));
        assert_eq!(d["N8N_INSTANCE_AI_MODEL_URL"], "http://lmstudio:1234/v1");
    }

    #[test]
    fn brave_key_keeps_its_unprefixed_name() {
        let mut s = spec();
        s.search = Some(SearchConfig {
            brave: Some(BraveConfig {
                api_key_secret: SecretKeyRef {
                    name: "n8n-ai".into(),
                    key: "BRAVE_API_KEY".into(),
                },
            }),
            searxng: None,
        });
        let d = build_instance_ai_data(&s, URL, "k3y", Some("sk"), Some("BSA-x"));
        assert_eq!(d["INSTANCE_AI_BRAVE_SEARCH_API_KEY"], "BSA-x");
        assert!(!d.contains_key("N8N_INSTANCE_AI_BRAVE_SEARCH_API_KEY"));
    }

    #[test]
    fn searxng_url_is_passed_through() {
        let mut s = spec();
        s.search = Some(SearchConfig {
            brave: None,
            searxng: Some(SearxngConfig {
                url: "http://searxng.search.svc:8080".into(),
            }),
        });
        let d = build_instance_ai_data(&s, URL, "k3y", Some("sk"), None);
        assert_eq!(d["N8N_INSTANCE_AI_SEARXNG_URL"], "http://searxng.search.svc:8080");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib env::instance_ai`
Expected: FAIL — `cannot find function build_instance_ai_data`.

- [ ] **Step 3: Write minimal implementation**

Na początek `src/env/instance_ai.rs`:

```rust
use crate::spec::AssistantSpec;
use std::collections::BTreeMap;

/// Contents of `Secret <target>-instance-ai`, pulled into the n8n container
/// with `envFrom`. `model_api_key` and `brave_api_key` are already-resolved
/// values copied from the user's Secrets.
pub fn build_instance_ai_data(
    spec: &AssistantSpec,
    sandbox_url: &str,
    sandbox_api_key: &str,
    model_api_key: Option<&str>,
    brave_api_key: Option<&str>,
) -> BTreeMap<String, String> {
    let mut d = BTreeMap::new();
    let mut set = |k: &str, v: &str| {
        d.insert(k.to_string(), v.to_string());
    };
    set("N8N_ENABLED_MODULES", "instance-ai");
    set("N8N_INSTANCE_AI_MODEL", &spec.model.name);
    set("N8N_INSTANCE_AI_SANDBOX_ENABLED", "true");
    set("N8N_INSTANCE_AI_SANDBOX_PROVIDER", "n8n-sandbox");
    set("N8N_INSTANCE_AI_SANDBOX_IMAGE", &spec.sandbox.sandbox_image);
    set("N8N_INSTANCE_AI_SANDBOX_API_URL", sandbox_url);
    set("N8N_SANDBOX_SERVICE_URL", sandbox_url);
    set("N8N_SANDBOX_SERVICE_API_KEY", sandbox_api_key);
    if let Some(u) = &spec.model.url {
        set("N8N_INSTANCE_AI_MODEL_URL", u);
    }
    if let Some(k) = model_api_key {
        set("N8N_INSTANCE_AI_MODEL_API_KEY", k);
    }
    // Deliberately unprefixed — the n8n docs require this exact name.
    if let Some(k) = brave_api_key {
        set("INSTANCE_AI_BRAVE_SEARCH_API_KEY", k);
    }
    if let Some(s) = &spec.search
        && let Some(sx) = &s.searxng
    {
        set("N8N_INSTANCE_AI_SEARXNG_URL", &sx.url);
    }
    d
}
```

W `src/env/mod.rs` dopisz `pub mod instance_ai;` (alfabetycznie, po `database`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib env::instance_ai`
Expected: PASS, 4 testy.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/env/instance_ai.rs src/env/mod.rs
git commit -m "feat: build instance-ai Secret contents"
```

---

### Task 5: Nazwy obiektów sandboxa i etykiety

**Files:**
- Create: `src/builders/sandbox_names.rs`
- Modify: `src/builders/mod.rs`

**Interfaces:**
- Produces: `struct SandboxNames` z polami `prefix, secrets, certs, tls_api, tls_runner, api, runner, target_secret` (wszystkie `String`), `SandboxNames::new(cr_ns: &str, cr_name: &str, target_name: &str) -> SandboxNames`, `SandboxNames::api_url(&self, sbx_ns: &str) -> String`, `sandbox_labels(cr_ns: &str, cr_name: &str, component: &str) -> BTreeMap<String, String>`, `sandbox_selector(cr_ns: &str, cr_name: &str) -> String`.

- [ ] **Step 1: Write the failing test**

Utwórz `src/builders/sandbox_names.rs` z samym blokiem testów:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_every_object_name_from_the_prefix() {
        let n = SandboxNames::new("n8n-cluster", "n8n", "n8n");
        assert_eq!(n.prefix, "n8n-cluster-n8n");
        assert_eq!(n.secrets, "n8n-cluster-n8n-sandbox-secrets");
        assert_eq!(n.certs, "n8n-cluster-n8n-sandbox-certs");
        assert_eq!(n.tls_api, "n8n-cluster-n8n-sandbox-tls-api");
        assert_eq!(n.tls_runner, "n8n-cluster-n8n-sandbox-tls-runner");
        assert_eq!(n.api, "n8n-cluster-n8n-sandbox-api");
        assert_eq!(n.runner, "n8n-cluster-n8n-sandbox-runner-1");
        assert_eq!(n.target_secret, "n8n-instance-ai");
    }

    #[test]
    fn api_url_is_the_cross_namespace_service_dns() {
        let n = SandboxNames::new("n8n-cluster", "n8n", "n8n");
        assert_eq!(
            n.api_url("n8n-sandbox"),
            "http://n8n-cluster-n8n-sandbox-api.n8n-sandbox.svc:8080"
        );
    }

    #[test]
    fn labels_identify_the_owning_assistant() {
        let l = sandbox_labels("n8n-cluster", "n8n", "sandbox-api");
        assert_eq!(l["n8n.slys.dev/assistant"], "n8n");
        assert_eq!(l["n8n.slys.dev/assistant-namespace"], "n8n-cluster");
        assert_eq!(l["app.kubernetes.io/component"], "sandbox-api");
        assert_eq!(l["app.kubernetes.io/managed-by"], "n8n-rustful-operator");
    }

    #[test]
    fn selector_matches_only_this_assistant() {
        assert_eq!(
            sandbox_selector("n8n-cluster", "n8n"),
            "n8n.slys.dev/assistant=n8n,n8n.slys.dev/assistant-namespace=n8n-cluster"
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib sandbox_names`
Expected: FAIL — `cannot find struct SandboxNames`.

- [ ] **Step 3: Write minimal implementation**

Na początek `src/builders/sandbox_names.rs`:

```rust
use std::collections::BTreeMap;

pub const ASSISTANT_LABEL: &str = "n8n.slys.dev/assistant";
pub const ASSISTANT_NS_LABEL: &str = "n8n.slys.dev/assistant-namespace";

/// Every object name the Assistant controller generates. The prefix carries
/// the CR's namespace so several Assistants can share one sandbox namespace.
pub struct SandboxNames {
    pub prefix: String,
    pub secrets: String,
    pub certs: String,
    pub tls_api: String,
    pub tls_runner: String,
    pub api: String,
    pub runner: String,
    pub target_secret: String,
}

impl SandboxNames {
    pub fn new(cr_ns: &str, cr_name: &str, target_name: &str) -> Self {
        let prefix = format!("{cr_ns}-{cr_name}");
        Self {
            secrets: format!("{prefix}-sandbox-secrets"),
            certs: format!("{prefix}-sandbox-certs"),
            tls_api: format!("{prefix}-sandbox-tls-api"),
            tls_runner: format!("{prefix}-sandbox-tls-runner"),
            api: format!("{prefix}-sandbox-api"),
            runner: format!("{prefix}-sandbox-runner-1"),
            target_secret: format!("{target_name}-instance-ai"),
            prefix,
        }
    }

    /// URL n8n uses to reach the sandbox API across namespaces.
    pub fn api_url(&self, sbx_ns: &str) -> String {
        format!("http://{}.{sbx_ns}.svc:8080", self.api)
    }
}

/// Labels on every object in the sandbox namespace. These objects carry no
/// ownerReference — a cross-namespace owner would make the GC treat them as
/// orphans and delete them — so the finalizer finds them by these labels.
pub fn sandbox_labels(cr_ns: &str, cr_name: &str, component: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("app.kubernetes.io/name".to_string(), "n8n-sandbox".to_string());
    m.insert("app.kubernetes.io/instance".to_string(), cr_name.to_string());
    m.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "n8n-rustful-operator".to_string(),
    );
    m.insert("app.kubernetes.io/part-of".to_string(), "n8n".to_string());
    m.insert("app.kubernetes.io/component".to_string(), component.to_string());
    m.insert(ASSISTANT_LABEL.to_string(), cr_name.to_string());
    m.insert(ASSISTANT_NS_LABEL.to_string(), cr_ns.to_string());
    m
}

/// Label selector for `delete_collection` during cleanup.
pub fn sandbox_selector(cr_ns: &str, cr_name: &str) -> String {
    format!("{ASSISTANT_LABEL}={cr_name},{ASSISTANT_NS_LABEL}={cr_ns}")
}
```

W `src/builders/mod.rs` dopisz `pub mod sandbox_names;` (alfabetycznie, po `pvc`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib sandbox_names`
Expected: PASS, 4 testy.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/builders/sandbox_names.rs src/builders/mod.rs
git commit -m "feat: derive sandbox object names and labels"
```

---

### Task 6: Ustalenie UID/GID obrazu i builder Joba z certami

**Files:**
- Create: `src/builders/sandbox_certs.rs`
- Modify: `src/builders/mod.rs`, `docs/superpowers/specs/2026-09-15-n8n-assistant-sandbox-design.md`

**Interfaces:**
- Consumes: `SandboxNames`, `sandbox_labels` (Task 5), `AssistantSpec` (Task 1).
- Produces: `build_certs_rbac(names: &SandboxNames, cr_ns: &str, cr_name: &str, sbx_ns: &str) -> (ServiceAccount, Role, RoleBinding)`, `build_certs_job(names: &SandboxNames, spec: &AssistantSpec, cr_ns: &str, cr_name: &str, sbx_ns: &str, operator_image: &str) -> Job`, stała `TLS_FS_GROUP: Option<i64>`.

- [ ] **Step 1: Determine the image's user (open item from the spec)**

Run:

```bash
docker run --rm --entrypoint id ghcr.io/n8n-io/n8n-sandbox-service-api:1.2.0
```

Zapisz wynik. Reguła decyzyjna:
- wynik `uid=0(root)` → `TLS_FS_GROUP: Option<i64> = None`, montaż certów z `defaultMode: 0440` bez `fsGroup`
- wynik `uid=N(sandbox-api) gid=M(...)` → `TLS_FS_GROUP: Option<i64> = Some(M)`

Zaktualizuj sekcję „Uprawnienia do plików certów" w specu, zamieniając akapit „Otwarta pozycja do domknięcia w implementacji" na ustaloną wartość i datę sprawdzenia. To jedyny krok w planie, który zmienia spec.

- [ ] **Step 2: Write the failing test**

Utwórz `src/builders/sandbox_certs.rs` z blokiem testów:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{AssistantSpec, ModelConfig, SandboxConfig, TargetRef};

    fn spec() -> AssistantSpec {
        AssistantSpec {
            target_ref: TargetRef {
                kind: "Cluster".into(),
                name: "n8n".into(),
            },
            model: ModelConfig {
                name: "anthropic/claude-opus-4-8".into(),
                api_key_secret: None,
                url: Some("http://x/v1".into()),
            },
            search: None,
            sandbox: SandboxConfig::default(),
        }
    }

    fn names() -> SandboxNames {
        SandboxNames::new("n8n-cluster", "n8n", "n8n")
    }

    #[test]
    fn job_runs_bootstrap_then_tlspub() {
        let j = build_certs_job(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "op:1");
        let v = serde_json::to_value(&j).unwrap();
        let init = &v["spec"]["template"]["spec"]["initContainers"][0];
        assert_eq!(init["image"], "ghcr.io/n8n-io/n8n-sandbox-service-api:1.2.0");
        assert_eq!(init["securityContext"]["runAsUser"], 0);
        assert_eq!(init["env"][0]["name"], "NUM_RUNNERS");
        assert_eq!(init["env"][0]["value"], "1");
        let cmd = init["args"][0].as_str().unwrap();
        assert!(cmd.contains("--api-san n8n-cluster-n8n-sandbox-api"));
        assert!(cmd.contains("--control-san-prefix n8n-cluster-n8n-sandbox-runner"));
        let main = &v["spec"]["template"]["spec"]["containers"][0];
        assert_eq!(main["image"], "op:1");
        let args: Vec<&str> = main["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
        assert!(args.contains(&"--api-secret"));
        assert!(args.contains(&"n8n-cluster-n8n-sandbox-tls-api"));
        assert!(args.contains(&"n8n-cluster-n8n-sandbox-tls-runner"));
    }

    #[test]
    fn job_cleans_itself_up_and_does_not_retry_forever() {
        let j = build_certs_job(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "op:1");
        let v = serde_json::to_value(&j).unwrap();
        assert_eq!(v["spec"]["ttlSecondsAfterFinished"], 3600);
        assert_eq!(v["spec"]["backoffLimit"], 4);
        assert_eq!(v["spec"]["template"]["spec"]["restartPolicy"], "Never");
    }

    #[test]
    fn job_carries_no_owner_reference() {
        let j = build_certs_job(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "op:1");
        assert!(j.metadata.owner_references.is_none());
        let l = j.metadata.labels.unwrap();
        assert_eq!(l["n8n.slys.dev/assistant"], "n8n");
    }

    #[test]
    fn rbac_grants_only_secret_get_and_create() {
        let (sa, role, rb) = build_certs_rbac(&names(), "n8n-cluster", "n8n", "n8n-sandbox");
        assert_eq!(sa.metadata.name.as_deref(), Some("n8n-cluster-n8n-sandbox-certs"));
        let rules = role.rules.unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].resources.as_ref().unwrap(), &vec!["secrets".to_string()]);
        let mut verbs = rules[0].verbs.clone();
        verbs.sort();
        assert_eq!(verbs, vec!["create".to_string(), "get".to_string()]);
        assert_eq!(rb.subjects.unwrap()[0].name, "n8n-cluster-n8n-sandbox-certs");
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib sandbox_certs`
Expected: FAIL — `cannot find function build_certs_job`.

- [ ] **Step 4: Write minimal implementation**

Na początek `src/builders/sandbox_certs.rs`:

```rust
use crate::{
    builders::sandbox_names::{SandboxNames, sandbox_labels},
    labels::common_annotations,
    spec::AssistantSpec,
};
use k8s_openapi::api::{
    batch::v1::Job,
    core::v1::ServiceAccount,
    rbac::v1::{Role, RoleBinding},
};
use serde_json::json;

/// GID the sandbox-api image runs as, used as the pod `fsGroup` so the mounted
/// key files are readable. `None` when the image runs as root.
/// Determined by `docker run --rm --entrypoint id <api image>`.
pub const TLS_FS_GROUP: Option<i64> = None;

/// Secret volume mode `0440` — private keys must not be world-readable.
pub const TLS_MODE: i32 = 0o440;

pub fn certs_image(spec: &AssistantSpec) -> String {
    format!("ghcr.io/n8n-io/n8n-sandbox-service-api:{}", spec.sandbox.version)
}

pub fn build_certs_rbac(
    names: &SandboxNames,
    cr_ns: &str,
    cr_name: &str,
    sbx_ns: &str,
) -> (ServiceAccount, Role, RoleBinding) {
    let labels = sandbox_labels(cr_ns, cr_name, "sandbox-certs");
    let meta = json!({
        "name": names.certs,
        "namespace": sbx_ns,
        "labels": labels,
        "annotations": common_annotations(),
    });
    let sa = json!({
        "apiVersion": "v1",
        "kind": "ServiceAccount",
        "metadata": meta,
    });
    let role = json!({
        "apiVersion": "rbac.authorization.k8s.io/v1",
        "kind": "Role",
        "metadata": meta,
        "rules": [{
            "apiGroups": [""],
            "resources": ["secrets"],
            "verbs": ["get", "create"],
        }],
    });
    let rb = json!({
        "apiVersion": "rbac.authorization.k8s.io/v1",
        "kind": "RoleBinding",
        "metadata": meta,
        "roleRef": {
            "apiGroup": "rbac.authorization.k8s.io",
            "kind": "Role",
            "name": names.certs,
        },
        "subjects": [{
            "kind": "ServiceAccount",
            "name": names.certs,
            "namespace": sbx_ns,
        }],
    });
    (
        serde_json::from_value(sa).expect("static serviceaccount schema is valid"),
        serde_json::from_value(role).expect("static role schema is valid"),
        serde_json::from_value(rb).expect("static rolebinding schema is valid"),
    )
}

/// One-shot Job: `bootstrap-mtls.sh` into an emptyDir, then `tlspub` publishes
/// the whitelisted files as two Secrets. Created only when those Secrets are
/// missing — rerunning it would mint a new CA under the running pods.
pub fn build_certs_job(
    names: &SandboxNames,
    spec: &AssistantSpec,
    cr_ns: &str,
    cr_name: &str,
    sbx_ns: &str,
    operator_image: &str,
) -> Job {
    let labels = sandbox_labels(cr_ns, cr_name, "sandbox-certs");
    let bootstrap = format!(
        "bootstrap-mtls.sh --out-dir /tls --api-san {} --control-san-prefix {}-sandbox-runner --world-readable",
        names.api, names.prefix
    );
    let job = json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {
            "name": names.certs,
            "namespace": sbx_ns,
            "labels": labels,
            "annotations": common_annotations(),
        },
        "spec": {
            "ttlSecondsAfterFinished": 3600,
            "backoffLimit": 4,
            "template": {
                "metadata": { "labels": labels },
                "spec": {
                    "restartPolicy": "Never",
                    "serviceAccountName": names.certs,
                    "volumes": [{ "name": "tls", "emptyDir": {} }],
                    "initContainers": [{
                        "name": "bootstrap",
                        "image": certs_image(spec),
                        "securityContext": { "runAsUser": 0 },
                        "command": ["sh", "-c"],
                        "args": [bootstrap],
                        "env": [{ "name": "NUM_RUNNERS", "value": "1" }],
                        "volumeMounts": [{ "name": "tls", "mountPath": "/tls" }],
                    }],
                    "containers": [{
                        "name": "tlspub",
                        "image": operator_image,
                        "command": ["tlspub"],
                        "args": [
                            "--dir", "/tls",
                            "--namespace", sbx_ns,
                            "--api-secret", names.tls_api,
                            "--runner-secret", names.tls_runner,
                            "--label", format!("n8n.slys.dev/assistant={cr_name}"),
                            "--label", format!("n8n.slys.dev/assistant-namespace={cr_ns}"),
                            "--label", "app.kubernetes.io/managed-by=n8n-rustful-operator",
                        ],
                        "volumeMounts": [{ "name": "tls", "mountPath": "/tls", "readOnly": true }],
                    }],
                }
            }
        }
    });
    serde_json::from_value(job).expect("static job schema is valid")
}
```

W `src/builders/mod.rs` dopisz `pub mod sandbox_certs;`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib sandbox_certs`
Expected: PASS, 4 testy.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add src/builders/sandbox_certs.rs src/builders/mod.rs docs/superpowers/specs/2026-09-15-n8n-assistant-sandbox-design.md
git commit -m "feat: build the sandbox mTLS bootstrap Job and its RBAC"
```

---

### Task 7: Buildery `sandbox-api` i `sandbox-runner-1`

**Files:**
- Create: `src/builders/sandbox_workloads.rs`
- Modify: `src/builders/mod.rs`

**Interfaces:**
- Consumes: `SandboxNames`, `sandbox_labels` (Task 5), `TLS_MODE`, `TLS_FS_GROUP` (Task 6), `apply_pod_config`, `resources` (istniejące w `builders/mod.rs`).
- Produces: `build_sandbox_api(names, spec, cr_ns, cr_name, sbx_ns, tls_revision: &str) -> Deployment`, `build_sandbox_runner(names, spec, cr_ns, cr_name, sbx_ns, tls_revision: &str) -> Deployment`, `build_sandbox_service(name: &str, cr_ns, cr_name, sbx_ns, component: &str, ports: &[(&str, i32)]) -> Service`, `build_docker_pvc(names, spec, cr_ns, cr_name, sbx_ns) -> Option<PersistentVolumeClaim>`.

- [ ] **Step 1: Write the failing test**

Utwórz `src/builders/sandbox_workloads.rs` z blokiem testów:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{AssistantSpec, ModelConfig, PersistenceConfig, SandboxConfig, TargetRef};

    fn spec() -> AssistantSpec {
        AssistantSpec {
            target_ref: TargetRef { kind: "Cluster".into(), name: "n8n".into() },
            model: ModelConfig {
                name: "anthropic/claude-opus-4-8".into(),
                api_key_secret: None,
                url: Some("http://x/v1".into()),
            },
            search: None,
            sandbox: SandboxConfig::default(),
        }
    }

    fn names() -> SandboxNames {
        SandboxNames::new("n8n-cluster", "n8n", "n8n")
    }

    fn env_of(v: &serde_json::Value, name: &str) -> String {
        v["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == name)
            .unwrap_or_else(|| panic!("env {name} not found"))["value"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn api_points_tls_paths_at_the_flat_secret_mount() {
        let d = build_sandbox_api(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "42");
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(env_of(&v, "SANDBOX_API_GRPC_TLS_CERT_FILE"), "/tls/grpc-server.crt");
        assert_eq!(env_of(&v, "SANDBOX_API_GRPC_TLS_CLIENT_CA_FILE"), "/tls/ca.crt");
        assert_eq!(
            env_of(&v, "SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_CERT_FILE"),
            "/tls/control-grpc-api-client.crt"
        );
    }

    #[test]
    fn api_verifies_the_runner_by_its_short_san() {
        let d = build_sandbox_api(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "42");
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(
            env_of(&v, "SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_SERVER_NAME"),
            "n8n-cluster-n8n-sandbox-runner-1"
        );
    }

    #[test]
    fn api_pulls_shared_secrets_and_probes_healthz() {
        let d = build_sandbox_api(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "42");
        let v = serde_json::to_value(&d).unwrap();
        let c = &v["spec"]["template"]["spec"]["containers"][0];
        assert_eq!(
            c["envFrom"][0]["secretRef"]["name"],
            "n8n-cluster-n8n-sandbox-secrets"
        );
        assert_eq!(c["readinessProbe"]["httpGet"]["path"], "/healthz");
        assert_eq!(v["spec"]["strategy"]["type"], "Recreate");
        assert_eq!(v["spec"]["replicas"], 1);
    }

    #[test]
    fn tls_revision_annotation_forces_a_rollout() {
        let d = build_sandbox_api(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "42");
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(
            v["spec"]["template"]["metadata"]["annotations"]["n8n.slys.dev/tls-revision"],
            "42"
        );
    }

    #[test]
    fn tls_secret_is_mounted_read_only_with_restricted_mode() {
        let d = build_sandbox_api(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "42");
        let v = serde_json::to_value(&d).unwrap();
        let vol = &v["spec"]["template"]["spec"]["volumes"][0];
        assert_eq!(vol["secret"]["secretName"], "n8n-cluster-n8n-sandbox-tls-api");
        assert_eq!(vol["secret"]["defaultMode"], 0o440);
        assert_eq!(
            v["spec"]["template"]["spec"]["containers"][0]["volumeMounts"][0]["readOnly"],
            true
        );
    }

    #[test]
    fn runner_is_privileged_and_advertises_its_own_service() {
        let d = build_sandbox_runner(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "7");
        let v = serde_json::to_value(&d).unwrap();
        let c = &v["spec"]["template"]["spec"]["containers"][0];
        assert_eq!(c["securityContext"]["privileged"], true);
        assert_eq!(
            env_of(&v, "SANDBOX_RUNNER_CONTROL_GRPC_ADVERTISE_ADDR"),
            "n8n-cluster-n8n-sandbox-runner-1.n8n-sandbox.svc:9091"
        );
        assert_eq!(
            env_of(&v, "SANDBOX_RUNNER_API_GRPC_ADDR"),
            "n8n-cluster-n8n-sandbox-api.n8n-sandbox.svc:9090"
        );
        assert_eq!(
            env_of(&v, "SANDBOX_RUNNER_REGISTRATION_GRPC_SERVER_NAME"),
            "n8n-cluster-n8n-sandbox-api"
        );
        assert_eq!(env_of(&v, "SANDBOX_RUNNER_ID"), "runner-1");
    }

    #[test]
    fn runner_defaults_docker_storage_to_a_capped_emptydir() {
        let d = build_sandbox_runner(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "7");
        let v = serde_json::to_value(&d).unwrap();
        let vols = v["spec"]["template"]["spec"]["volumes"].as_array().unwrap();
        let docker = vols.iter().find(|x| x["name"] == "docker").unwrap();
        assert_eq!(docker["emptyDir"]["sizeLimit"], "20Gi");
        assert!(build_docker_pvc(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox").is_none());
    }

    #[test]
    fn runner_uses_a_pvc_when_persistence_is_set() {
        let mut s = spec();
        s.sandbox.runner.docker_storage.persistence = Some(PersistenceConfig {
            size: "40Gi".into(),
            storage_class_name: Some("longhorn".into()),
            access_mode: "ReadWriteOnce".into(),
        });
        let d = build_sandbox_runner(&names(), &s, "n8n-cluster", "n8n", "n8n-sandbox", "7");
        let v = serde_json::to_value(&d).unwrap();
        let vols = v["spec"]["template"]["spec"]["volumes"].as_array().unwrap();
        let docker = vols.iter().find(|x| x["name"] == "docker").unwrap();
        assert_eq!(
            docker["persistentVolumeClaim"]["claimName"],
            "n8n-cluster-n8n-sandbox-runner-1-docker"
        );
        let pvc = build_docker_pvc(&names(), &s, "n8n-cluster", "n8n", "n8n-sandbox").unwrap();
        assert_eq!(
            pvc.metadata.name.as_deref(),
            Some("n8n-cluster-n8n-sandbox-runner-1-docker")
        );
    }

    #[test]
    fn service_exposes_the_named_ports_and_selects_the_workload() {
        let s = build_sandbox_service(
            "n8n-cluster-n8n-sandbox-api",
            "n8n-cluster",
            "n8n",
            "n8n-sandbox",
            "sandbox-api",
            &[("http", 8080), ("grpc", 9090)],
        );
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["spec"]["ports"][0]["name"], "http");
        assert_eq!(v["spec"]["ports"][1]["port"], 9090);
        assert_eq!(v["spec"]["type"], "ClusterIP");
        assert_eq!(
            v["spec"]["selector"]["app.kubernetes.io/component"],
            "sandbox-api"
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib sandbox_workloads`
Expected: FAIL — `cannot find function build_sandbox_api`.

- [ ] **Step 3: Write minimal implementation**

Na początek `src/builders/sandbox_workloads.rs`:

```rust
use crate::{
    builders::{
        apply_pod_config, resources,
        sandbox_certs::{TLS_FS_GROUP, TLS_MODE},
        sandbox_names::{SandboxNames, sandbox_labels},
    },
    labels::common_annotations,
    spec::AssistantSpec,
};
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{PersistentVolumeClaim, Service},
};
use serde_json::{Value, json};

fn api_image(spec: &AssistantSpec) -> String {
    format!("ghcr.io/n8n-io/n8n-sandbox-service-api:{}", spec.sandbox.version)
}

fn runner_image(spec: &AssistantSpec) -> String {
    format!(
        "ghcr.io/n8n-io/n8n-sandbox-service-runner-dind:{}",
        spec.sandbox.version
    )
}

fn env(name: &str, value: impl Into<String>) -> Value {
    json!({ "name": name, "value": value.into() })
}

/// Selector labels for a sandbox workload. Distinct per component so the API
/// and runner Services don't select each other's pods.
fn selector(cr_ns: &str, cr_name: &str, component: &str) -> Value {
    json!({
        "app.kubernetes.io/name": "n8n-sandbox",
        "app.kubernetes.io/instance": cr_name,
        "app.kubernetes.io/component": component,
        "n8n.slys.dev/assistant-namespace": cr_ns,
    })
}

fn deployment(
    name: &str,
    sbx_ns: &str,
    cr_ns: &str,
    cr_name: &str,
    component: &str,
    tls_revision: &str,
    container: Value,
    volumes: Vec<Value>,
    pod_cfg: Option<&crate::spec::PodConfig>,
) -> Deployment {
    let labels = sandbox_labels(cr_ns, cr_name, component);
    let mut annotations = common_annotations();
    annotations.insert("n8n.slys.dev/tls-revision".to_string(), tls_revision.to_string());
    let mut pod_spec = json!({ "volumes": volumes, "containers": [container] });
    if let Some(fsg) = TLS_FS_GROUP {
        pod_spec["securityContext"] = json!({ "fsGroup": fsg });
    }
    let mut dep = json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": name,
            "namespace": sbx_ns,
            "labels": labels,
            "annotations": common_annotations(),
        },
        "spec": {
            "replicas": 1,
            // A second pod would register a duplicate runner identity, so never
            // surge: replace in place.
            "strategy": { "type": "Recreate" },
            "selector": { "matchLabels": selector(cr_ns, cr_name, component) },
            "template": {
                "metadata": { "labels": labels, "annotations": annotations },
                "spec": pod_spec,
            }
        }
    });
    if let Some(pc) = pod_cfg {
        apply_pod_config(&mut dep["spec"]["template"], pc);
    }
    serde_json::from_value(dep).expect("static sandbox deployment schema is valid")
}

pub fn build_sandbox_api(
    names: &SandboxNames,
    spec: &AssistantSpec,
    cr_ns: &str,
    cr_name: &str,
    sbx_ns: &str,
    tls_revision: &str,
) -> Deployment {
    let mut container = json!({
        "name": "sandbox-api",
        "image": api_image(spec),
        "ports": [
            { "containerPort": 8080, "name": "http" },
            { "containerPort": 9090, "name": "grpc" }
        ],
        "envFrom": [{ "secretRef": { "name": names.secrets } }],
        "env": [
            env("SANDBOX_API_GRPC_TLS_CERT_FILE", "/tls/grpc-server.crt"),
            env("SANDBOX_API_GRPC_TLS_KEY_FILE", "/tls/grpc-server.key"),
            env("SANDBOX_API_GRPC_TLS_CLIENT_CA_FILE", "/tls/ca.crt"),
            env("SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_CA_FILE", "/tls/ca.crt"),
            env(
                "SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_CERT_FILE",
                "/tls/control-grpc-api-client.crt"
            ),
            env(
                "SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_KEY_FILE",
                "/tls/control-grpc-api-client.key"
            ),
            // The cert is issued for the short name; the address is an FQDN.
            env("SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_SERVER_NAME", names.runner.clone()),
        ],
        "volumeMounts": [{ "name": "tls", "mountPath": "/tls", "readOnly": true }],
        "readinessProbe": {
            "httpGet": { "path": "/healthz", "port": "http" },
            "initialDelaySeconds": 10,
            "periodSeconds": 5
        },
        "livenessProbe": {
            "httpGet": { "path": "/healthz", "port": "http" },
            "initialDelaySeconds": 30,
            "periodSeconds": 10
        }
    });
    if let Some(r) = &spec.sandbox.api.resources {
        container["resources"] = resources(r);
    }
    let volumes = vec![json!({
        "name": "tls",
        "secret": { "secretName": names.tls_api, "defaultMode": TLS_MODE }
    })];
    deployment(
        &names.api,
        sbx_ns,
        cr_ns,
        cr_name,
        "sandbox-api",
        tls_revision,
        container,
        volumes,
        spec.sandbox.api.pod.as_ref(),
    )
}

fn docker_pvc_name(names: &SandboxNames) -> String {
    format!("{}-docker", names.runner)
}

pub fn build_sandbox_runner(
    names: &SandboxNames,
    spec: &AssistantSpec,
    cr_ns: &str,
    cr_name: &str,
    sbx_ns: &str,
    tls_revision: &str,
) -> Deployment {
    let store = &spec.sandbox.runner.docker_storage;
    let docker_volume = match &store.persistence {
        Some(_) => json!({
            "name": "docker",
            "persistentVolumeClaim": { "claimName": docker_pvc_name(names) }
        }),
        None => json!({ "name": "docker", "emptyDir": { "sizeLimit": store.size } }),
    };
    let mut container = json!({
        "name": "sandbox-runner",
        "image": runner_image(spec),
        // Docker-in-Docker: equivalent to root on the node. Never expose.
        "securityContext": { "privileged": true },
        "ports": [
            { "containerPort": 9091, "name": "control" },
            { "containerPort": 8080, "name": "http" }
        ],
        "envFrom": [{ "secretRef": { "name": names.secrets } }],
        "env": [
            env("SANDBOX_RUNNER_API_GRPC_ADDR", format!("{}.{sbx_ns}.svc:9090", names.api)),
            env("SANDBOX_RUNNER_REGISTRATION_GRPC_SERVER_NAME", names.api.clone()),
            env("SANDBOX_RUNNER_CONTROL_GRPC_LISTEN_ADDR", ":9091"),
            env(
                "SANDBOX_RUNNER_CONTROL_GRPC_ADVERTISE_ADDR",
                format!("{}.{sbx_ns}.svc:9091", names.runner)
            ),
            env(
                "SANDBOX_RUNNER_HTTP_BASE_URL",
                format!("http://{}.{sbx_ns}.svc:8080", names.runner)
            ),
            env("SANDBOX_RUNNER_ID", "runner-1"),
            env("SANDBOX_RUNNER_DOCKER_SANDBOX_IMAGE", spec.sandbox.sandbox_image.clone()),
            env("SANDBOX_RUNNER_REGISTRATION_GRPC_CA_FILE", "/tls/ca.crt"),
            env("SANDBOX_RUNNER_REGISTRATION_GRPC_CERT_FILE", "/tls/grpc-client.crt"),
            env("SANDBOX_RUNNER_REGISTRATION_GRPC_KEY_FILE", "/tls/grpc-client.key"),
            env("SANDBOX_RUNNER_CONTROL_GRPC_TLS_CERT_FILE", "/tls/control-grpc-server.crt"),
            env("SANDBOX_RUNNER_CONTROL_GRPC_TLS_KEY_FILE", "/tls/control-grpc-server.key"),
            env("SANDBOX_RUNNER_CONTROL_GRPC_TLS_CLIENT_CA_FILE", "/tls/ca.crt"),
        ],
        "volumeMounts": [
            { "name": "tls", "mountPath": "/tls", "readOnly": true },
            { "name": "docker", "mountPath": "/var/lib/docker" }
        ]
    });
    if let Some(r) = &spec.sandbox.runner.resources {
        container["resources"] = resources(r);
    }
    let volumes = vec![
        json!({
            "name": "tls",
            "secret": { "secretName": names.tls_runner, "defaultMode": TLS_MODE }
        }),
        docker_volume,
    ];
    deployment(
        &names.runner,
        sbx_ns,
        cr_ns,
        cr_name,
        "sandbox-runner",
        tls_revision,
        container,
        volumes,
        spec.sandbox.runner.pod.as_ref(),
    )
}

pub fn build_docker_pvc(
    names: &SandboxNames,
    spec: &AssistantSpec,
    cr_ns: &str,
    cr_name: &str,
    sbx_ns: &str,
) -> Option<PersistentVolumeClaim> {
    let p = spec.sandbox.runner.docker_storage.persistence.as_ref()?;
    let json = json!({
        "apiVersion": "v1",
        "kind": "PersistentVolumeClaim",
        "metadata": {
            "name": docker_pvc_name(names),
            "namespace": sbx_ns,
            "labels": sandbox_labels(cr_ns, cr_name, "sandbox-runner"),
            "annotations": common_annotations(),
        },
        "spec": {
            "accessModes": [p.access_mode],
            "resources": { "requests": { "storage": p.size } },
            "storageClassName": p.storage_class_name,
        }
    });
    Some(serde_json::from_value(json).expect("static docker pvc schema is valid"))
}

pub fn build_sandbox_service(
    name: &str,
    cr_ns: &str,
    cr_name: &str,
    sbx_ns: &str,
    component: &str,
    ports: &[(&str, i32)],
) -> Service {
    let ports: Vec<Value> = ports
        .iter()
        .map(|(n, p)| json!({ "name": n, "port": p, "targetPort": n }))
        .collect();
    let json = json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {
            "name": name,
            "namespace": sbx_ns,
            "labels": sandbox_labels(cr_ns, cr_name, component),
            "annotations": common_annotations(),
        },
        "spec": {
            "type": "ClusterIP",
            "selector": selector(cr_ns, cr_name, component),
            "ports": ports,
        }
    });
    serde_json::from_value(json).expect("static sandbox service schema is valid")
}
```

W `src/builders/mod.rs` dopisz `pub mod sandbox_workloads;`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib sandbox_workloads`
Expected: PASS, 9 testów.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/builders/sandbox_workloads.rs src/builders/mod.rs
git commit -m "feat: build sandbox-api and privileged DinD runner workloads"
```

---

### Task 8: Sekrety stacka i wykrywanie gotowości certów

**Files:**
- Create: `src/reconciler/assistant_certs.rs`
- Modify: `src/reconciler/mod.rs`

**Interfaces:**
- Consumes: `SandboxNames` (Task 5), buildery z Tasku 6.
- Produces: `ensure_sandbox_secrets(client: &Client, sbx_ns, names, cr_ns, cr_name) -> Result<SandboxKeys>`, `struct SandboxKeys { api_key: String }`, `ensure_certs(client, sbx_ns, names, spec, cr_ns, cr_name, operator_image) -> Result<Option<TlsRevisions>>`, `struct TlsRevisions { api: String, runner: String }`.

`ensure_certs` zwraca `Ok(None)`, gdy certy jeszcze nie istnieją (Job dopiero poszedł) — reconciler ma wtedy wrócić za 15 s.

- [ ] **Step 1: Write the failing test**

Ta logika jest w całości I/O — testowalna jest funkcja czysta, którą wydzielamy. Utwórz `src/reconciler/assistant_certs.rs` z blokiem testów:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_three_distinct_sixty_four_char_secrets() {
        let d = new_sandbox_secret_data();
        assert_eq!(d["SANDBOX_API_KEYS"].len(), 64);
        assert_ne!(d["SANDBOX_API_KEYS"], d["SANDBOX_API_RUNNER_API_KEY"]);
        assert_ne!(
            d["SANDBOX_API_KEYS"],
            d["SANDBOX_API_RUNNER_REGISTRATION_TOKEN"]
        );
    }

    #[test]
    fn mirrors_every_secret_under_both_api_and_runner_names() {
        let d = new_sandbox_secret_data();
        assert_eq!(d["SANDBOX_API_KEYS"], d["N8N_SANDBOX_SERVICE_API_KEY"]);
        assert_eq!(
            d["SANDBOX_API_RUNNER_REGISTRATION_TOKEN"],
            d["SANDBOX_RUNNER_REGISTRATION_TOKEN"]
        );
        assert_eq!(d["SANDBOX_API_RUNNER_API_KEY"], d["SANDBOX_RUNNER_API_KEYS"]);
        assert_eq!(d.len(), 6);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib assistant_certs`
Expected: FAIL — `cannot find function new_sandbox_secret_data`.

- [ ] **Step 3: Write minimal implementation**

Na początek `src/reconciler/assistant_certs.rs`:

```rust
use crate::{
    Error, Result,
    builders::{
        sandbox_certs::{build_certs_job, build_certs_rbac},
        sandbox_names::SandboxNames,
    },
    labels::common_annotations,
    spec::AssistantSpec,
};
use k8s_openapi::api::{
    batch::v1::Job,
    core::v1::{Secret, ServiceAccount},
    rbac::v1::{Role, RoleBinding},
};
use kube::{
    Client,
    api::{Api, ObjectMeta, PostParams},
};
use rand::RngCore;
use std::collections::BTreeMap;

pub struct SandboxKeys {
    pub api_key: String,
}

pub struct TlsRevisions {
    pub api: String,
    pub runner: String,
}

fn random_hex() -> String {
    let mut buf = [0u8; 32];
    rand::rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// The three shared secrets, each under both the API-side and runner-side
/// variable name — the two sides of the stack spell them differently.
pub fn new_sandbox_secret_data() -> BTreeMap<String, String> {
    let api_key = random_hex();
    let registration = random_hex();
    let runner_key = random_hex();
    let mut d = BTreeMap::new();
    d.insert("SANDBOX_API_KEYS".to_string(), api_key.clone());
    d.insert("N8N_SANDBOX_SERVICE_API_KEY".to_string(), api_key);
    d.insert(
        "SANDBOX_API_RUNNER_REGISTRATION_TOKEN".to_string(),
        registration.clone(),
    );
    d.insert("SANDBOX_RUNNER_REGISTRATION_TOKEN".to_string(), registration);
    d.insert("SANDBOX_API_RUNNER_API_KEY".to_string(), runner_key.clone());
    d.insert("SANDBOX_RUNNER_API_KEYS".to_string(), runner_key);
    d
}

fn decode(secret: &Secret, key: &str) -> Result<String> {
    let raw = secret
        .data
        .as_ref()
        .and_then(|d| d.get(key))
        .ok_or_else(|| Error::IllegalAssistant(format!("Secret is missing key {key}")))?;
    String::from_utf8(raw.0.clone())
        .map_err(|e| Error::IllegalAssistant(format!("key {key} is not valid UTF-8: {e}")))
}

/// Create the shared-secret Secret once and return the API key from whatever
/// version is live — never regenerate over an existing one.
pub async fn ensure_sandbox_secrets(
    client: &Client,
    sbx_ns: &str,
    names: &SandboxNames,
    labels: &BTreeMap<String, String>,
) -> Result<SandboxKeys> {
    let api: Api<Secret> = Api::namespaced(client.clone(), sbx_ns);
    if let Some(existing) = api.get_opt(&names.secrets).await.map_err(Error::KubeError)? {
        return Ok(SandboxKeys {
            api_key: decode(&existing, "SANDBOX_API_KEYS")?,
        });
    }
    let data = new_sandbox_secret_data();
    let api_key = data["SANDBOX_API_KEYS"].clone();
    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(names.secrets.clone()),
            namespace: Some(sbx_ns.to_string()),
            labels: Some(labels.clone()),
            annotations: Some(common_annotations()),
            ..Default::default()
        },
        string_data: Some(data),
        type_: Some("Opaque".to_string()),
        ..Default::default()
    };
    match api.create(&PostParams::default(), &secret).await {
        Ok(_) => Ok(SandboxKeys { api_key }),
        // Lost a race: re-read rather than fail the reconcile.
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            let existing = api.get(&names.secrets).await.map_err(Error::KubeError)?;
            Ok(SandboxKeys {
                api_key: decode(&existing, "SANDBOX_API_KEYS")?,
            })
        }
        Err(e) => Err(Error::KubeError(e)),
    }
}

/// `Ok(Some(revisions))` once both TLS Secrets exist. `Ok(None)` means the
/// bootstrap Job has been dispatched and the caller should requeue shortly.
pub async fn ensure_certs(
    client: &Client,
    sbx_ns: &str,
    names: &SandboxNames,
    spec: &AssistantSpec,
    cr_ns: &str,
    cr_name: &str,
    operator_image: &str,
) -> Result<Option<TlsRevisions>> {
    let secrets: Api<Secret> = Api::namespaced(client.clone(), sbx_ns);
    let api_tls = secrets.get_opt(&names.tls_api).await.map_err(Error::KubeError)?;
    let runner_tls = secrets.get_opt(&names.tls_runner).await.map_err(Error::KubeError)?;
    if let (Some(a), Some(r)) = (&api_tls, &runner_tls) {
        return Ok(Some(TlsRevisions {
            api: a.metadata.resource_version.clone().unwrap_or_default(),
            runner: r.metadata.resource_version.clone().unwrap_or_default(),
        }));
    }
    // Missing TLS material: (re)run the bootstrap Job. Creating it is
    // idempotent — an existing Job means one is already in flight.
    let (sa, role, rb) = build_certs_rbac(names, cr_ns, cr_name, sbx_ns);
    create_if_absent(&Api::<ServiceAccount>::namespaced(client.clone(), sbx_ns), &names.certs, sa).await?;
    create_if_absent(&Api::<Role>::namespaced(client.clone(), sbx_ns), &names.certs, role).await?;
    create_if_absent(&Api::<RoleBinding>::namespaced(client.clone(), sbx_ns), &names.certs, rb).await?;
    let job = build_certs_job(names, spec, cr_ns, cr_name, sbx_ns, operator_image);
    create_if_absent(&Api::<Job>::namespaced(client.clone(), sbx_ns), &names.certs, job).await?;
    Ok(None)
}

async fn create_if_absent<K>(api: &Api<K>, name: &str, obj: K) -> Result<()>
where
    K: kube::Resource + Clone + serde::de::DeserializeOwned + serde::Serialize + std::fmt::Debug,
    K::DynamicType: Default,
{
    if api.get_opt(name).await.map_err(Error::KubeError)?.is_some() {
        return Ok(());
    }
    match api.create(&PostParams::default(), &obj).await {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(ae)) if ae.code == 409 => Ok(()),
        Err(e) => Err(Error::KubeError(e)),
    }
}
```

W `src/reconciler/mod.rs` dopisz `pub mod assistant_certs;`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib assistant_certs`
Expected: PASS, 2 testy.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/reconciler/assistant_certs.rs src/reconciler/mod.rs
git commit -m "feat: ensure sandbox shared secrets and bootstrap TLS material"
```

---

### Task 9: Reconciler `Assistant`

**Files:**
- Create: `src/reconciler/assistant.rs`, `src/reconciler/assistant_apply.rs`, `src/reconciler/assistant_status.rs`
- Modify: `src/reconciler/mod.rs`, `src/reconciler/owner.rs`, `src/reconciler/run.rs`

**Interfaces:**
- Consumes: wszystko z Tasków 1–8.
- Produces: `assistant::watcher_config()`, `assistant::reconcile()`, `assistant::error_policy()`, `assistant_apply::apply()`, `assistant_status::patch_status()`, `owner::assistant_owner(a: &Assistant) -> OwnerReference`.

- [ ] **Step 1: Write the failing test**

Utwórz `src/reconciler/assistant_status.rs` z blokiem testów (reszta Tasku to I/O, weryfikowane w Tasku 12):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::{Deployment, DeploymentCondition, DeploymentStatus};

    fn dep(available: i32, cond: Option<(&str, &str, &str)>) -> Deployment {
        Deployment {
            status: Some(DeploymentStatus {
                available_replicas: Some(available),
                conditions: cond.map(|(t, s, m)| {
                    vec![DeploymentCondition {
                        type_: t.to_string(),
                        status: s.to_string(),
                        message: Some(m.to_string()),
                        ..Default::default()
                    }]
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_deployment_with_an_available_replica_is_ready() {
        assert!(is_ready(Some(&dep(1, None))));
    }

    #[test]
    fn a_missing_deployment_is_not_ready() {
        assert!(!is_ready(None));
        assert!(!is_ready(Some(&dep(0, None))));
    }

    #[test]
    fn replica_failure_surfaces_as_the_status_message() {
        let d = dep(
            0,
            Some((
                "ReplicaFailure",
                "True",
                "pods \"x\" is forbidden: violates PodSecurity \"restricted:latest\": privileged",
            )),
        );
        let msg = failure_message(Some(&d)).unwrap();
        assert!(msg.contains("PodSecurity"));
    }

    #[test]
    fn a_healthy_deployment_has_no_failure_message() {
        assert!(failure_message(Some(&dep(1, None))).is_none());
        assert!(failure_message(Some(&dep(0, Some(("Available", "False", "scaling"))))).is_none());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib assistant_status`
Expected: FAIL — `cannot find function is_ready`.

- [ ] **Step 3: Write minimal implementation**

Na początek `src/reconciler/assistant_status.rs`:

```rust
use crate::{
    Error, Result,
    spec::{Assistant, AssistantStatus},
};
use k8s_openapi::api::apps::v1::Deployment;
use kube::{
    Client,
    api::{Api, Patch, PatchParams},
};
use serde_json::json;

pub fn is_ready(d: Option<&Deployment>) -> bool {
    d.and_then(|d| d.status.as_ref())
        .and_then(|s| s.available_replicas)
        .unwrap_or(0)
        > 0
}

/// `ReplicaFailure` is where the apiserver reports a pod it refused to admit —
/// most often Pod Security rejecting the privileged runner. Surfacing it beats
/// leaving an empty Deployment with no explanation.
pub fn failure_message(d: Option<&Deployment>) -> Option<String> {
    d?.status
        .as_ref()?
        .conditions
        .as_ref()?
        .iter()
        .find(|c| c.type_ == "ReplicaFailure" && c.status == "True")
        .and_then(|c| c.message.clone())
}

pub async fn patch_status(
    client: &Client,
    ns: &str,
    name: &str,
    status: AssistantStatus,
    ps: &PatchParams,
) -> Result<()> {
    Api::<Assistant>::namespaced(client.clone(), ns)
        .patch_status(
            name,
            ps,
            &Patch::Apply(json!({
                "apiVersion": "n8n.slys.dev/v1",
                "kind": "Assistant",
                "status": status,
            })),
        )
        .await
        .map_err(Error::KubeError)?;
    Ok(())
}
```

Utwórz `src/reconciler/assistant_apply.rs`:

```rust
use crate::{
    Error, Result,
    builders::{
        sandbox_names::{SandboxNames, sandbox_labels},
        sandbox_workloads::{build_docker_pvc, build_sandbox_api, build_sandbox_runner, build_sandbox_service},
    },
    env::instance_ai::build_instance_ai_data,
    labels::common_annotations,
    reconciler::{
        assistant_certs::{ensure_certs, ensure_sandbox_secrets},
        assistant_status::{failure_message, is_ready, patch_status},
        assistant_validate::validate_assistant,
        owner::assistant_owner,
    },
    spec::{Assistant, AssistantStatus, SecretKeyRef},
    state::Context,
};
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{Namespace, PersistentVolumeClaim, Secret, Service},
};
use kube::{
    Resource, ResourceExt,
    api::{Api, ObjectMeta, Patch, PatchParams, PostParams},
    runtime::{
        controller::Action,
        events::{Event, EventType},
    },
};
use std::sync::Arc;
use tokio::time::Duration;

/// Image the certs Job uses for its `tlspub` container: the operator's own.
/// Injected through the Deployment so the Job always matches the running
/// operator version.
fn operator_image() -> String {
    std::env::var("OPERATOR_IMAGE")
        .unwrap_or_else(|_| "ghcr.io/jakub-k-slys/n8n-rustful-operator:latest".to_string())
}

pub async fn apply(a: &Assistant, ctx: Arc<Context>) -> Result<Action> {
    let client = ctx.client.clone();
    let oref = a.object_ref(&());
    let ns = a.namespace().unwrap();
    let name = a.name_any();
    let patch = PatchParams::apply("n8n-rustful-operator").force();

    validate_assistant(a, &ns)?;
    let sbx_ns = a.spec.sandbox_namespace(&ns);
    if Api::<Namespace>::all(client.clone())
        .get_opt(&sbx_ns)
        .await
        .map_err(Error::KubeError)?
        .is_none()
    {
        return Err(Error::IllegalAssistant(format!(
            "sandbox namespace {sbx_ns:?} does not exist; create it with \
             pod-security.kubernetes.io/enforce=privileged"
        )));
    }

    let names = SandboxNames::new(&ns, &name, &a.spec.target_ref.name);
    let labels = sandbox_labels(&ns, &name, "sandbox");
    let keys = ensure_sandbox_secrets(&client, &sbx_ns, &names, &labels).await?;

    let Some(tls) = ensure_certs(
        &client,
        &sbx_ns,
        &names,
        &a.spec,
        &ns,
        &name,
        &operator_image(),
    )
    .await?
    else {
        patch_status(
            &client,
            &ns,
            &name,
            AssistantStatus {
                message: Some("waiting for the mTLS bootstrap Job".into()),
                ..Default::default()
            },
            &patch,
        )
        .await?;
        return Ok(Action::requeue(Duration::from_secs(15)));
    };

    // Workloads.
    let deps: Api<Deployment> = Api::namespaced(client.clone(), &sbx_ns);
    let svcs: Api<Service> = Api::namespaced(client.clone(), &sbx_ns);
    if let Some(pvc) = build_docker_pvc(&names, &a.spec, &ns, &name, &sbx_ns) {
        Api::<PersistentVolumeClaim>::namespaced(client.clone(), &sbx_ns)
            .patch(&format!("{}-docker", names.runner), &patch, &Patch::Apply(&pvc))
            .await
            .map_err(Error::KubeError)?;
    }
    deps.patch(
        &names.api,
        &patch,
        &Patch::Apply(&build_sandbox_api(&names, &a.spec, &ns, &name, &sbx_ns, &tls.api)),
    )
    .await
    .map_err(Error::KubeError)?;
    svcs.patch(
        &names.api,
        &patch,
        &Patch::Apply(&build_sandbox_service(
            &names.api,
            &ns,
            &name,
            &sbx_ns,
            "sandbox-api",
            &[("http", 8080), ("grpc", 9090)],
        )),
    )
    .await
    .map_err(Error::KubeError)?;
    deps.patch(
        &names.runner,
        &patch,
        &Patch::Apply(&build_sandbox_runner(
            &names, &a.spec, &ns, &name, &sbx_ns, &tls.runner,
        )),
    )
    .await
    .map_err(Error::KubeError)?;
    svcs.patch(
        &names.runner,
        &patch,
        &Patch::Apply(&build_sandbox_service(
            &names.runner,
            &ns,
            &name,
            &sbx_ns,
            "sandbox-runner",
            &[("control", 9091), ("http", 8080)],
        )),
    )
    .await
    .map_err(Error::KubeError)?;

    // Target Secret, in the target's namespace, owned by this CR.
    let model_key = read_key(&client, &ns, a.spec.model.api_key_secret.as_ref()).await?;
    let brave_key = read_key(
        &client,
        &ns,
        a.spec
            .search
            .as_ref()
            .and_then(|s| s.brave.as_ref())
            .map(|b| &b.api_key_secret),
    )
    .await?;
    let data = build_instance_ai_data(
        &a.spec,
        &names.api_url(&sbx_ns),
        &keys.api_key,
        model_key.as_deref(),
        brave_key.as_deref(),
    );
    let target = Secret {
        metadata: ObjectMeta {
            name: Some(names.target_secret.clone()),
            namespace: Some(ns.clone()),
            owner_references: Some(vec![assistant_owner(a)]),
            labels: Some(sandbox_labels(&ns, &name, "instance-ai")),
            annotations: Some(common_annotations()),
            ..Default::default()
        },
        string_data: Some(data),
        type_: Some("Opaque".to_string()),
        ..Default::default()
    };
    Api::<Secret>::namespaced(client.clone(), &ns)
        .patch(&names.target_secret, &patch, &Patch::Apply(&target))
        .await
        .map_err(Error::KubeError)?;

    ctx.recorder
        .publish(
            &Event {
                type_: EventType::Normal,
                reason: "Applied".into(),
                note: Some(format!("Applied sandbox stack for `{name}`")),
                action: "Reconciling".into(),
                secondary: None,
            },
            &oref,
        )
        .await
        .map_err(Error::KubeError)?;

    let api_dep = deps.get_opt(&names.api).await.map_err(Error::KubeError)?;
    let runner_dep = deps.get_opt(&names.runner).await.map_err(Error::KubeError)?;
    let api_ready = is_ready(api_dep.as_ref());
    let runner_ready = is_ready(runner_dep.as_ref());
    let message = failure_message(api_dep.as_ref()).or_else(|| failure_message(runner_dep.as_ref()));
    patch_status(
        &client,
        &ns,
        &name,
        AssistantStatus {
            ready: api_ready && runner_ready,
            certs_ready: true,
            api_ready,
            runner_ready,
            target_secret: Some(names.target_secret.clone()),
            message,
        },
        &patch,
    )
    .await?;
    Ok(Action::requeue(Duration::from_secs(5 * 60)))
}

async fn read_key(client: &kube::Client, ns: &str, r: Option<&SecretKeyRef>) -> Result<Option<String>> {
    let Some(r) = r else { return Ok(None) };
    let s = Api::<Secret>::namespaced(client.clone(), ns)
        .get(&r.name)
        .await
        .map_err(Error::KubeError)?;
    let raw = s
        .data
        .as_ref()
        .and_then(|d| d.get(&r.key))
        .ok_or_else(|| Error::IllegalAssistant(format!("Secret {:?} has no key {:?}", r.name, r.key)))?;
    Ok(Some(String::from_utf8(raw.0.clone()).map_err(|e| {
        Error::IllegalAssistant(format!("Secret {:?} key {:?} is not UTF-8: {e}", r.name, r.key))
    })?))
}

/// Silence the unused-import warning for `PostParams` when this module grows.
#[allow(unused_imports)]
use kube::api::PostParams as _PostParams;
```

Uwaga: jeśli `PostParams` nie jest używany, usuń oba importy zamiast dodawać `#[allow]` — to był tylko zapis intencji, nie wymóg.

Utwórz `src/reconciler/assistant.rs` (kopiuj strukturę z `single.rs`):

```rust
use crate::{
    Error, Result,
    builders::sandbox_names::sandbox_selector,
    reconciler::assistant_apply::apply,
    spec::{ASSISTANT_FINALIZER, Assistant},
    state::Context,
    telemetry,
};
use jiff::Timestamp;
use k8s_openapi::api::{
    apps::v1::Deployment,
    batch::v1::Job,
    core::v1::{PersistentVolumeClaim, Secret, Service, ServiceAccount},
    rbac::v1::{Role, RoleBinding},
};
use kube::{
    Resource, ResourceExt,
    api::{Api, DeleteParams, ListParams},
    runtime::{
        controller::Action,
        events::{Event, EventType},
        finalizer::{Event as Finalizer, finalizer},
        watcher::Config,
    },
};
use std::sync::Arc;
use tokio::time::Duration;
use tracing::*;

pub fn watcher_config() -> Config {
    Config::default().any_semantic()
}

#[instrument(skip(ctx, a), fields(trace_id))]
pub async fn reconcile(a: Arc<Assistant>, ctx: Arc<Context>) -> Result<Action> {
    let trace_id = telemetry::get_trace_id();
    if trace_id != opentelemetry::trace::TraceId::INVALID {
        Span::current().record("trace_id", field::display(&trace_id));
    }
    let _timer = ctx.metrics.reconcile.count_and_measure(&trace_id);
    ctx.diagnostics.write().await.last_event = Timestamp::now();
    let ns = a.namespace().unwrap();
    let api: Api<Assistant> = Api::namespaced(ctx.client.clone(), &ns);
    info!("Reconciling Assistant \"{}\" in {}", a.name_any(), ns);
    finalizer(&api, ASSISTANT_FINALIZER, a, |event| async {
        match event {
            Finalizer::Apply(x) => apply(&x, ctx.clone()).await,
            Finalizer::Cleanup(x) => cleanup(&x, ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::FinalizerError(Box::new(e)))
}

pub fn error_policy(a: Arc<Assistant>, error: &Error, ctx: Arc<Context>) -> Action {
    warn!("reconcile failed: {error:?}");
    ctx.metrics.reconcile.set_failure(&*a, error);
    Action::requeue(Duration::from_secs(5 * 60))
}

/// Objects in the sandbox namespace carry no ownerReference — a cross-namespace
/// owner would have the GC delete them as orphans — so the finalizer removes
/// them by label before it lets the CR go.
async fn cleanup(a: &Assistant, ctx: Arc<Context>) -> Result<Action> {
    let ns = a.namespace().unwrap();
    let name = a.name_any();
    let sbx_ns = a.spec.sandbox_namespace(&ns);
    let selector = sandbox_selector(&ns, &name);
    let lp = ListParams::default().labels(&selector);
    let dp = DeleteParams::background();
    let client = ctx.client.clone();

    macro_rules! purge {
        ($t:ty) => {
            Api::<$t>::namespaced(client.clone(), &sbx_ns)
                .delete_collection(&dp, &lp)
                .await
                .map_err(Error::KubeError)?;
        };
    }
    purge!(Deployment);
    purge!(Service);
    purge!(Job);
    purge!(Secret);
    purge!(PersistentVolumeClaim);
    purge!(RoleBinding);
    purge!(Role);
    purge!(ServiceAccount);

    ctx.recorder
        .publish(
            &Event {
                type_: EventType::Normal,
                reason: "DeleteRequested".into(),
                note: Some(format!("Delete `{name}` and its sandbox stack in {sbx_ns}")),
                action: "Deleting".into(),
                secondary: None,
            },
            &a.object_ref(&()),
        )
        .await
        .map_err(Error::KubeError)?;
    Ok(Action::await_change())
}
```

W `src/reconciler/owner.rs` dopisz:

```rust
pub fn assistant_owner(a: &Assistant) -> OwnerReference {
    OwnerReference {
        api_version: "n8n.slys.dev/v1".to_string(),
        kind: "Assistant".to_string(),
        name: a.name_any(),
        uid: a.uid().expect("Assistant lacks uid"),
        controller: Some(true),
        block_owner_deletion: Some(true),
    }
}
```

(zaktualizuj też import na `use crate::spec::{Assistant, Cluster, Single};`)

W `src/reconciler/mod.rs` dopisz `pub mod assistant;`, `pub mod assistant_apply;`, `pub mod assistant_status;`.

W `src/reconciler/run.rs`: dodaj `assistant` do importu reconcilerów i `Assistant` do importu specu, sprawdzenie CRD i trzeci kontroler:

```rust
    let assistants = Api::<Assistant>::all(client.clone());
    if let Err(e) = assistants.list(&ListParams::default().limit(1)).await {
        error!("Assistant CRD is not queryable; {e:?}. Is it installed?");
        std::process::exit(1);
    }
```

oraz, po `cluster_ctrl`:

```rust
    let assistant_ctrl = Controller::new(assistants, assistant::watcher_config())
        .shutdown_on_signal()
        .run(assistant::reconcile, assistant::error_policy, ctx2)
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()));
    futures::future::join3(single_ctrl, cluster_ctrl, assistant_ctrl).await;
```

Zmień klonowanie kontekstu tak, by starczyło dla trzech kontrolerów: `let ctx2 = ctx.clone();` przed `cluster_ctrl`, a do `cluster_ctrl` przekaż `ctx`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib assistant_status && cargo build`
Expected: PASS (4 testy) i czysta kompilacja.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/reconciler/ src/lib.rs
git commit -m "feat: reconcile Assistant into a self-hosted sandbox stack"
```

---

### Task 10: `envFrom` i annotacja rewizji w `Single`/`Cluster`

**Files:**
- Modify: `src/builders/deployment.rs`, `src/builders/cluster_deployment.rs`, `src/reconciler/single_children.rs`, `src/reconciler/cluster_main.rs`

**Interfaces:**
- Produces: `instance_ai_env_from(cr_name: &str) -> Value` (w `src/builders/mod.rs`), parametr `instance_ai_revision: &str` dodany do `build_deployment` i `build_cluster_deployment` (dla roli `main`).

- [ ] **Step 1: Write the failing test**

Dopisz blok testów na końcu `src/builders/deployment.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::SingleSpec;

    fn key() -> SecretKeyRef {
        SecretKeyRef {
            name: "k".into(),
            key: "encryption_key".into(),
        }
    }

    fn owner() -> OwnerReference {
        OwnerReference {
            api_version: "n8n.slys.dev/v1".into(),
            kind: "Single".into(),
            name: "demo".into(),
            uid: "u".into(),
            controller: Some(true),
            block_owner_deletion: Some(true),
        }
    }

    #[test]
    fn pulls_the_optional_instance_ai_secret() {
        let d = build_deployment("demo", &SingleSpec::default(), &key(), &owner(), "none");
        let v = serde_json::to_value(&d).unwrap();
        let ef = &v["spec"]["template"]["spec"]["containers"][0]["envFrom"][0];
        assert_eq!(ef["secretRef"]["name"], "demo-instance-ai");
        assert_eq!(ef["secretRef"]["optional"], true);
    }

    #[test]
    fn stamps_the_instance_ai_revision_so_a_secret_change_rolls_pods() {
        let d = build_deployment("demo", &SingleSpec::default(), &key(), &owner(), "1234");
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(
            v["spec"]["template"]["metadata"]["annotations"]["n8n.slys.dev/instance-ai-revision"],
            "1234"
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib builders::deployment`
Expected: FAIL — `build_deployment` przyjmuje 4 argumenty, nie 5.

- [ ] **Step 3: Write minimal implementation**

W `src/builders/mod.rs` dopisz:

```rust
/// Secret the Assistant controller writes for this instance. Declared
/// `optional` so the Deployment is valid whether or not an Assistant exists,
/// and so the two controllers never contend for the same field — `env` is an
/// atomic list in server-side apply and cannot be co-owned.
pub fn instance_ai_env_from(cr_name: &str) -> Value {
    json!([{ "secretRef": { "name": format!("{cr_name}-instance-ai"), "optional": true } }])
}

pub const INSTANCE_AI_REVISION: &str = "n8n.slys.dev/instance-ai-revision";
```

W `src/builders/deployment.rs`: dodaj parametr `instance_ai_revision: &str` na końcu sygnatury, a w budowie kontenera i szablonu:

```rust
    container["envFrom"] = instance_ai_env_from(name);
```

oraz przed zbudowaniem `dep_json`:

```rust
    let mut pod_annotations = annotations.clone();
    pod_annotations.insert(INSTANCE_AI_REVISION.to_string(), instance_ai_revision.to_string());
```

i w `template.metadata` użyj `pod_annotations` zamiast `annotations`. Dodaj `instance_ai_env_from` oraz `INSTANCE_AI_REVISION` do importu z `crate::builders`.

Powtórz to samo w `src/builders/cluster_deployment.rs` **wyłącznie dla roli `main`** — sprawdź, jak ten builder rozróżnia role (parametr `component` lub podobny) i wstaw `envFrom` tylko wtedy, gdy rola to `main`. Workery i webhooki nie dostają nic.

W `src/reconciler/single_children.rs` przed wywołaniem `build_deployment`:

```rust
    let instance_ai_revision = ctx
        .api::<k8s_openapi::api::core::v1::Secret>()
        .get_opt(&format!("{name}-instance-ai"))
        .await
        .map_err(Error::KubeError)?
        .and_then(|s| s.metadata.resource_version)
        .unwrap_or_else(|| "none".to_string());
```

i przekaż `&instance_ai_revision` jako ostatni argument. Analogicznie w `src/reconciler/cluster_main.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib && cargo build`
Expected: PASS wszystkie testy, czysta kompilacja.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/builders/ src/reconciler/single_children.rs src/reconciler/cluster_main.rs
git commit -m "feat: pull the optional instance-ai Secret into n8n deployments"
```

---

### Task 11: RBAC, manifesty i dokumentacja

**Files:**
- Modify: `yaml/install.yaml`, `yaml/crd.yaml`, `CLAUDE.md`
- Create: `docs/assistant.md`

- [ ] **Step 1: Extend the ClusterRole**

W `yaml/install.yaml`, w regule dla `n8n.slys.dev`, dodaj zasoby asystenta:

```yaml
  - apiGroups: ["n8n.slys.dev"]
    resources: ["singles", "singles/status", "singles/finalizers", "clusters", "clusters/status", "clusters/finalizers", "assistants", "assistants/status", "assistants/finalizers"]
    verbs: ["get", "list", "watch", "patch", "update"]
```

i dopisz nowe reguły przed regułą `events`:

```yaml
  - apiGroups: ["batch"]
    resources: ["jobs"]
    verbs: ["get", "list", "watch", "create", "patch", "update", "delete", "deletecollection"]
  - apiGroups: [""]
    resources: ["serviceaccounts"]
    verbs: ["get", "list", "watch", "create", "patch", "update", "delete", "deletecollection"]
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources: ["roles", "rolebindings"]
    verbs: ["get", "list", "watch", "create", "patch", "update", "delete", "deletecollection"]
  # Read-only: the Assistant controller refuses to proceed when the sandbox
  # namespace is absent, rather than creating a stack nobody can admit.
  - apiGroups: [""]
    resources: ["namespaces"]
    verbs: ["get", "list", "watch"]
```

Rozszerz też istniejące reguły dla `deployments`, `services`, `secrets` i `persistentvolumeclaims` o `deletecollection` — finalizer używa `delete_collection`.

- [ ] **Step 2: Inject the operator image into its own Deployment**

W `yaml/install.yaml`, w kontenerze operatora, dodaj zmienną, z której `assistant_apply::operator_image()` czyta obraz dla Joba:

```yaml
            - name: OPERATOR_IMAGE
              value: ghcr.io/jakub-k-slys/n8n-rustful-operator:__IMAGE_TAG__
```

- [ ] **Step 3: Verify the RBAC covers every verb the controller uses**

Run:

```bash
grep -n "delete_collection\|\.create(\|\.patch(\|get_opt\|\.get(" src/reconciler/assistant*.rs | sed 's/:.*//' | sort -u
```

Przejdź listę wywołań i potwierdź, że każdy typ obiektu ma odpowiedni czasownik w `ClusterRole`. Brakujące dopisz.

- [ ] **Step 4: Regenerate the CRD and write the user documentation**

Run: `just generate`

Utwórz `docs/assistant.md` z: przykładowym CR-em (jak w specu), listą wymagań klastra (namespace z `pod-security.kubernetes.io/enforce=privileged`, ruch cross-namespace przy NetworkPolicy/Istio, 4 GB RAM i 2 vCPU, żaden port sandboxa nie wystawiony publicznie), procedurą rotacji certów (`kubectl delete secret -n <sbx-ns> <p>-sandbox-tls-api <p>-sandbox-tls-runner`) i weryfikacją (`curl <api>:8080/healthz` → `{"status":"ok"}`, `kubectl logs <api> | grep -i runner`).

W `CLAUDE.md` dopisz `Assistant` do sekcji „What this is", `src/spec/assistant.rs` i moduły reconcilera do „Architecture", oraz `tlspub` do listy binarek.

- [ ] **Step 5: Commit**

```bash
git add yaml/install.yaml yaml/crd.yaml docs/assistant.md CLAUDE.md
git commit -m "build: grant the operator RBAC for the sandbox stack and document Assistant"
```

---

### Task 12: Scenariusz e2e

**Files:**
- Create: `features/assistant.feature`
- Modify: `features/cucumber.rs`, `.github/workflows/e2e.yml`

- [ ] **Step 1: Write the feature file**

Utwórz `features/assistant.feature`:

```gherkin
Feature: Assistant provisions a self-hosted sandbox stack

  Scenario: the sandbox stack comes up and the target Secret is written
    Given a clean cluster
    And a privileged namespace "n8n-sandbox"
    When I apply an Assistant named "e2e" targeting Cluster "e2e-cluster"
    Then the Job "default-e2e-sandbox-certs" completes
    And the Secret "default-e2e-sandbox-tls-api" exists in "n8n-sandbox"
    And the Secret "default-e2e-sandbox-tls-runner" exists in "n8n-sandbox"
    And the Secret "default-e2e-sandbox-tls-api" has no key "ca.key"
    And the Deployment "default-e2e-sandbox-api" becomes available
    And the Deployment "default-e2e-sandbox-runner-1" becomes available
    And the Secret "e2e-cluster-instance-ai" contains key "N8N_SANDBOX_SERVICE_API_KEY"
    And the Secret "e2e-cluster-instance-ai" contains key "N8N_ENABLED_MODULES"

  Scenario: deleting the Assistant clears the sandbox namespace
    When I delete the Assistant named "e2e"
    Then no Deployments remain in "n8n-sandbox" for assistant "e2e"
    And no Secrets remain in "n8n-sandbox" for assistant "e2e"
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --test cucumber -- features/assistant.feature`
Expected: FAIL — brak implementacji kroków.

- [ ] **Step 3: Implement the steps**

W `features/cucumber.rs` dopisz kroki wzorowane na istniejących (`given a clean cluster`, oczekiwanie na obiekt z timeoutem). Nowe kroki:

- `a privileged namespace {string}` — tworzy Namespace z etykietą `pod-security.kubernetes.io/enforce: privileged` (idempotentnie, tolerując 409)
- `I apply an Assistant named {string} targeting Cluster {string}` — buduje `Assistant` przez typy z `n8n_rustful_operator` i aplikuje przez SSA
- `the Job {string} completes` — poll `Job.status.succeeded > 0` z limitem 180 s (pierwsze uruchomienie ściąga obraz)
- `the Secret {string} has no key {string}` — asercja negatywna na `data`
- `the Deployment {string} becomes available` — poll `available_replicas > 0` z limitem 300 s (runner ściąga obraz sandboxa do wewnętrznego Dockera)
- `no Deployments remain in {string} for assistant {string}` — list z label selectorem `n8n.slys.dev/assistant=<name>`, oczekiwanie na pustą listę

Dopisz `Assistant`, `AssistantSpec`, `ModelConfig`, `SandboxConfig`, `TargetRef` do importu z `n8n_rustful_operator`.

- [ ] **Step 4: Run the suite against kind**

Run: `cargo test --test cucumber`
Expected: PASS oba scenariusze. Jeśli runner nie wstaje — sprawdź `kubectl -n n8n-sandbox describe deploy` pod kątem `ReplicaFailure` (PSA) i `kubectl logs` Joba certów.

- [ ] **Step 5: Wire it into CI**

W `.github/workflows/e2e.yml` dodaj krok tworzący namespace `n8n-sandbox` z etykietą PSA przed uruchomieniem suite'y (jeśli workflow tworzy namespace'y jawnie) oraz upewnij się, że obraz operatora ładowany do `kind` zawiera binarkę `tlspub`.

- [ ] **Step 6: Commit**

```bash
git add features/assistant.feature features/cucumber.rs .github/workflows/e2e.yml
git commit -m "test: cover the Assistant sandbox stack end to end"
```

---

## Self-Review

**Pokrycie specu:**

| sekcja specu | zadanie |
| --- | --- |
| API CRD `Assistant` | Task 1 |
| Walidacja (w tym limit 63 znaków) | Task 2 |
| Publisher certów, whitelista, brak klucza CA | Task 3 |
| Zawartość Secreta `instance-ai` | Task 4 |
| Nazwy, prefiks, etykiety, brak ownerRef | Task 5 |
| Uprawnienia do plików certów (otwarta pozycja) | Task 6, Step 1 |
| Job certów + RBAC Joba | Task 6 |
| `sandbox-api`, runner, SAN vs adres, `/var/lib/docker` | Task 7 |
| Sekrety współdzielone, detekcja gotowości certów, rotacja | Task 8 |
| Reconciler, finalizer, status, `ReplicaFailure`/PSA | Task 9 |
| Wstrzykiwanie przez `envFrom` + annotacja rewizji | Task 10 |
| RBAC, manifesty, wymagania klastra | Task 11 |
| Testy e2e | Task 12 |

**Spójność typów:** `SandboxNames` (Task 5) jest konsumowany w Taskach 6–9 z tymi samymi nazwami pól. `build_instance_ai_data` (Task 4) ma tę samą sygnaturę w Tasku 9. `build_certs_job` przyjmuje `operator_image` w Tasku 6 i dostaje go z `operator_image()` w Tasku 9. `TLS_MODE`/`TLS_FS_GROUP` definiowane w Tasku 6, używane w Tasku 7.

**Znane odstępstwo od specu:** spec wymieniał jeden moduł `src/builders/sandbox.rs`; plan rozbija go na `sandbox_names.rs`, `sandbox_certs.rs` i `sandbox_workloads.rs`, zgodnie z konwencją „moduł per concern" z `CLAUDE.md`. Zbiór funkcji jest ten sam.
