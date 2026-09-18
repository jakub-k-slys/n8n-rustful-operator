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
    api::{Api, DeleteParams, ObjectMeta, Patch, PatchParams, PostParams},
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
    let runner_tls = secrets
        .get_opt(&names.tls_runner)
        .await
        .map_err(Error::KubeError)?;
    if let (Some(a), Some(r)) = (&api_tls, &runner_tls) {
        return Ok(Some(TlsRevisions {
            api: a.metadata.resource_version.clone().unwrap_or_default(),
            runner: r.metadata.resource_version.clone().unwrap_or_default(),
        }));
    }
    // Missing TLS material: (re)run the bootstrap Job.
    //
    // The ServiceAccount/Role/RoleBinding are written with server-side apply
    // rather than create-if-absent: this ServiceAccount lives in the same
    // namespace as a privileged Docker-in-Docker pod, so if its Role's verbs
    // are ever widened out-of-band, SSA restores the least-privilege grant on
    // the next reconcile — create-if-absent could never repair that drift.
    // It's also the project-wide convention for every other k8s write here.
    let (sa, role, rb) = build_certs_rbac(names, cr_ns, cr_name, sbx_ns);
    apply(
        &Api::<ServiceAccount>::namespaced(client.clone(), sbx_ns),
        &names.certs,
        &sa,
    )
    .await?;
    apply(
        &Api::<Role>::namespaced(client.clone(), sbx_ns),
        &names.certs,
        &role,
    )
    .await?;
    apply(
        &Api::<RoleBinding>::namespaced(client.clone(), sbx_ns),
        &names.certs,
        &rb,
    )
    .await?;

    // The Job is create-if-absent instead: `spec.template` is immutable, so
    // an SSA carrying a (potentially) changed template would fail outright,
    // and `ttlSecondsAfterFinished` is designed around the Job disappearing
    // and being recreated rather than patched in place.
    //
    // But create-if-absent alone would let a finished Job block its own
    // retry for up to `ttlSecondsAfterFinished` (an hour): the documented
    // rotation procedure deletes both TLS Secrets and expects the Job to be
    // recreated on the next reconcile, and the same is true for a Job that
    // failed out its `backoffLimit`. So a Job that has already finished
    // (succeeded or failed) is deleted here so the create below rebuilds it
    // immediately instead of waiting on the TTL. A still-running Job is left
    // alone — deleting it would abort an in-flight bootstrap for no reason.
    let jobs: Api<Job> = Api::namespaced(client.clone(), sbx_ns);
    if let Some(existing) = jobs.get_opt(&names.certs).await.map_err(Error::KubeError)?
        && job_finished(&existing)
    {
        jobs.delete(&names.certs, &DeleteParams::background())
            .await
            .map_err(Error::KubeError)?;
    }
    let job = build_certs_job(names, spec, cr_ns, cr_name, sbx_ns, operator_image);
    create_if_absent(&jobs, &names.certs, job).await?;
    Ok(None)
}

/// `true` once the Job has reached a terminal state (its `Complete` or
/// `Failed` condition is `True`) — i.e. it is no longer going to make
/// progress on its own and is only sitting around for its
/// `ttlSecondsAfterFinished` window.
fn job_finished(j: &Job) -> bool {
    j.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .is_some_and(|conds| {
            conds
                .iter()
                .any(|c| matches!(c.type_.as_str(), "Complete" | "Failed") && c.status == "True")
        })
}

async fn apply<K>(api: &Api<K>, name: &str, obj: &K) -> Result<()>
where
    K: kube::Resource + Clone + serde::de::DeserializeOwned + serde::Serialize + std::fmt::Debug,
    K::DynamicType: Default,
{
    let pp = PatchParams::apply("n8n-rustful-operator").force();
    api.patch(name, &pp, &Patch::Apply(obj))
        .await
        .map_err(Error::KubeError)?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_three_distinct_sixty_four_char_secrets() {
        let d = new_sandbox_secret_data();
        assert_eq!(d["SANDBOX_API_KEYS"].len(), 64);
        assert_ne!(d["SANDBOX_API_KEYS"], d["SANDBOX_API_RUNNER_API_KEY"]);
        assert_ne!(d["SANDBOX_API_KEYS"], d["SANDBOX_API_RUNNER_REGISTRATION_TOKEN"]);
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

    fn job_with_condition(cond: Option<(&str, &str)>) -> Job {
        Job {
            status: Some(k8s_openapi::api::batch::v1::JobStatus {
                conditions: cond.map(|(t, s)| {
                    vec![k8s_openapi::api::batch::v1::JobCondition {
                        type_: t.to_string(),
                        status: s.to_string(),
                        ..Default::default()
                    }]
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_job_with_no_status_is_not_finished() {
        assert!(!job_finished(&Job::default()));
    }

    #[test]
    fn a_running_job_is_not_finished() {
        assert!(!job_finished(&job_with_condition(None)));
    }

    #[test]
    fn a_completed_job_is_finished() {
        assert!(job_finished(&job_with_condition(Some(("Complete", "True")))));
    }

    #[test]
    fn a_failed_job_is_finished() {
        assert!(job_finished(&job_with_condition(Some(("Failed", "True")))));
    }

    #[test]
    fn a_condition_that_is_not_true_yet_is_not_finished() {
        assert!(!job_finished(&job_with_condition(Some(("Failed", "False")))));
    }
}
