use crate::{
    Error, Result,
    builders::{
        pvc::{build_nodes_volume, build_shared_pvc},
        volumes::build_db_volumes,
    },
    env::redis::build_cluster_common_env,
    reconciler::{
        cluster_main::reconcile_main,
        cluster_status::patch_status,
        cluster_webhook::reconcile_webhooks,
        cluster_worker::reconcile_workers,
        ctx::{ApplyCtx, Bundle},
        encryption::resolve_encryption_secret,
        owner::cluster_owner,
        validate::validate_cluster,
    },
    spec::Cluster,
    state::Context,
};
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use kube::{
    Resource, ResourceExt,
    api::{Patch, PatchParams},
    runtime::{
        controller::Action,
        events::{Event, EventType},
    },
};
use std::sync::Arc;
use tokio::time::Duration;

pub async fn apply(c: &Cluster, ctx: Arc<Context>) -> Result<Action> {
    let client = ctx.client.clone();
    let oref = c.object_ref(&());
    let ns = c.namespace().unwrap();
    let name = c.name_any();
    let patch = PatchParams::apply("n8n-rustful-operator").force();

    validate_cluster(c)?;
    let owner = cluster_owner(c);
    let actx = ApplyCtx {
        client: &client,
        ns: &ns,
        owner: &owner,
        patch: &patch,
    };
    let key_secret = resolve_encryption_secret(
        c,
        &c.spec.image,
        c.spec.encryption_key.as_ref(),
        &ctx,
        &ns,
        &owner,
    )
    .await?;
    let (mut volumes, mut mounts) = build_db_volumes(&name, &c.spec.database);
    // Shared RWX community-nodes volume mounted on every role (when configured).
    if let Some(storage) = c
        .spec
        .community_nodes
        .as_ref()
        .and_then(|cn| cn.shared_storage.as_ref())
    {
        let pvc_name = format!("{name}-nodes");
        let pvc = build_shared_pvc(&pvc_name, &name, &c.spec.image, storage, &owner);
        actx.api::<PersistentVolumeClaim>()
            .patch(&pvc_name, &patch, &Patch::Apply(&pvc))
            .await
            .map_err(Error::KubeError)?;
        let (v, m) = build_nodes_volume(&pvc_name);
        volumes.push(v);
        mounts.push(m);
    }
    let bundle = Bundle {
        env: build_cluster_common_env(c, &key_secret),
        volumes,
        mounts,
    };

    reconcile_main(c, &name, &actx, &bundle).await?;
    reconcile_workers(c, &name, &actx, &bundle).await?;
    reconcile_webhooks(c, &name, &actx, &bundle).await?;

    ctx.recorder
        .publish(
            &Event {
                type_: EventType::Normal,
                reason: "Applied".into(),
                note: Some(format!("Applied cluster children for `{name}`")),
                action: "Reconciling".into(),
                secondary: None,
            },
            &oref,
        )
        .await
        .map_err(Error::KubeError)?;
    patch_status(c, &client, &ns, &name, &key_secret.name, &patch).await?;
    Ok(Action::requeue(Duration::from_secs(5 * 60)))
}
