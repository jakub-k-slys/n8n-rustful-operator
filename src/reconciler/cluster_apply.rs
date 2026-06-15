use crate::{
    Error, Result,
    builders::volumes::build_db_volumes,
    env::redis::build_cluster_common_env,
    reconciler::{
        cluster_main::reconcile_main, cluster_status::patch_status,
        cluster_webhook::reconcile_webhooks, cluster_worker::reconcile_workers,
        encryption::resolve_encryption_secret, owner::cluster_owner, validate::validate_cluster,
    },
    spec::Cluster,
    state::Context,
};
use kube::{
    Resource, ResourceExt,
    api::PatchParams,
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
    let ps = PatchParams::apply("n8n-rustful-operator").force();

    validate_cluster(c)?;
    let owner = cluster_owner(c);
    let key_secret = resolve_encryption_secret(
        c,
        &c.spec.image,
        c.spec.encryption_key.as_ref(),
        &ctx,
        &ns,
        &owner,
    )
    .await?;
    let common_env = build_cluster_common_env(c, &key_secret);
    let (common_vols, common_mounts) = build_db_volumes(&name, &c.spec.database);

    reconcile_main(c, &client, &ns, &name, &common_env, &common_vols, &common_mounts, &owner, &ps).await?;
    reconcile_workers(c, &client, &ns, &name, &common_env, &common_vols, &common_mounts, &owner, &ps).await?;
    reconcile_webhooks(c, &client, &ns, &name, &common_env, &common_vols, &common_mounts, &owner, &ps).await?;

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

    patch_status(c, &client, &ns, &name, &key_secret.name, &ps).await?;
    Ok(Action::requeue(Duration::from_secs(5 * 60)))
}
