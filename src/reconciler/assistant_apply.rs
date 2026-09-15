use crate::{
    Error, Result,
    builders::{
        sandbox_names::{SandboxNames, sandbox_labels},
        sandbox_workloads::{
            build_docker_pvc, build_sandbox_api, build_sandbox_runner, build_sandbox_service, docker_pvc_name,
        },
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
    api::{Api, ObjectMeta, Patch, PatchParams},
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
    check_namespace_pinned(
        a.status.as_ref().and_then(|s| s.sandbox_namespace.as_deref()),
        &sbx_ns,
    )?;
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

    let Some(tls) = ensure_certs(&client, &sbx_ns, &names, &a.spec, &ns, &name, &operator_image()).await?
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
            .patch(&docker_pvc_name(&names), &patch, &Patch::Apply(&pvc))
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
            &names,
            &a.spec,
            &ns,
            &name,
            &sbx_ns,
            &tls.runner,
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
    let ready = api_ready && runner_ready;
    let message = failure_message(api_dep.as_ref()).or_else(|| failure_message(runner_dep.as_ref()));
    patch_status(
        &client,
        &ns,
        &name,
        AssistantStatus {
            ready,
            certs_ready: true,
            api_ready,
            runner_ready,
            target_secret: Some(names.target_secret.clone()),
            sandbox_namespace: Some(sbx_ns.clone()),
            message,
        },
        &patch,
    )
    .await?;
    // No watch is possible on the sandbox Deployments — they carry no
    // ownerReference and live in another namespace — so this requeue is the
    // only way a Pod-Security rejection (surfaced via `failure_message`
    // above) gets picked up. Requeue soon while not ready so the operator
    // notices quickly instead of leaving the failure invisible for 5 minutes.
    let delay = if ready { 5 * 60 } else { 15 };
    Ok(Action::requeue(Duration::from_secs(delay)))
}

/// Refuse a `spec.sandbox.namespace` edit once the stack has been built
/// somewhere: moving it would mean tearing down mTLS and both workloads, and
/// the old, unlabelled-by-the-new-namespace stack has no ownerReference and
/// no reaper once `cleanup` starts looking in the new namespace instead.
/// Deleting and recreating the `Assistant` is the safe path — the finalizer
/// cleans up the old namespace before the CR is gone.
fn check_namespace_pinned(recorded: Option<&str>, computed: &str) -> Result<()> {
    match recorded {
        Some(r) if r != computed => Err(Error::IllegalAssistant(format!(
            "sandbox.namespace changed from {r:?} to {computed:?}; the sandbox stack already \
             exists in {r:?} and moving it would orphan a privileged pod. Delete and recreate \
             this Assistant instead — deleting runs the finalizer, which cleans up {r:?} properly."
        ))),
        _ => Ok(()),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_recorded_namespace_passes() {
        assert!(check_namespace_pinned(None, "n8n-sandbox").is_ok());
    }

    #[test]
    fn a_matching_recorded_namespace_passes() {
        assert!(check_namespace_pinned(Some("n8n-sandbox"), "n8n-sandbox").is_ok());
    }

    #[test]
    fn a_changed_namespace_is_rejected_naming_both() {
        let err = check_namespace_pinned(Some("n8n-sandbox"), "n8n-sandbox-2").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("n8n-sandbox"));
        assert!(msg.contains("n8n-sandbox-2"));
    }
}
