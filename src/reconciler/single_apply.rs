use crate::{
    Error, Result,
    builders::{deployment::build_deployment, pvc::build_data_pvc, service::build_service},
    reconciler::{
        encryption::resolve_encryption_secret, networking::reconcile_role_networking, owner::single_owner,
        single_status::patch_status, single_validate::validate_single,
    },
    spec::Single,
    state::Context,
};
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{PersistentVolumeClaim, Service},
};
use kube::{
    Resource, ResourceExt,
    api::{Api, Patch, PatchParams},
    runtime::{
        controller::Action,
        events::{Event, EventType},
    },
};
use std::sync::Arc;
use tokio::time::Duration;

pub async fn apply(s: &Single, ctx: Arc<Context>) -> Result<Action> {
    let client = ctx.client.clone();
    let oref = s.object_ref(&());
    let ns = s.namespace().unwrap();
    let name = s.name_any();

    validate_single(s)?;
    let owner = single_owner(s);
    let ps = PatchParams::apply("n8n-rustful-operator").force();
    let key_secret = resolve_encryption_secret(
        s,
        &s.spec.image,
        s.spec.encryption_key.as_ref(),
        &ctx,
        &ns,
        &owner,
    )
    .await?;

    let pvc_name = format!("{name}-data");
    let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(client.clone(), &ns);
    if let Some(pvc) = build_data_pvc(
        &pvc_name,
        &name,
        &s.spec.image,
        s.spec.persistence.as_ref(),
        &owner,
    ) {
        pvcs.patch(&pvc_name, &ps, &Patch::Apply(&pvc))
            .await
            .map_err(Error::KubeError)?;
    }

    Api::<Deployment>::namespaced(client.clone(), &ns)
        .patch(
            &name,
            &ps,
            &Patch::Apply(&build_deployment(&name, &s.spec, &key_secret, &owner)),
        )
        .await
        .map_err(Error::KubeError)?;
    Api::<Service>::namespaced(client.clone(), &ns)
        .patch(&name, &ps, &Patch::Apply(&build_service(&name, &s.spec, &owner)))
        .await
        .map_err(Error::KubeError)?;
    reconcile_role_networking(
        &client,
        &ns,
        &name,
        &s.spec.image,
        "workflow-engine",
        s.spec.host.as_deref(),
        s.spec.networking.as_ref(),
        &owner,
        &ps,
    )
    .await?;
    ctx.recorder
        .publish(
            &Event {
                type_: EventType::Normal,
                reason: "Applied".into(),
                note: Some(format!("Applied child resources for `{name}`")),
                action: "Reconciling".into(),
                secondary: None,
            },
            &oref,
        )
        .await
        .map_err(Error::KubeError)?;
    patch_status(s, &client, &ns, &name, &key_secret.name, &ps).await?;
    Ok(Action::requeue(Duration::from_secs(5 * 60)))
}
