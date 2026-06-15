use crate::{
    Error, Result,
    reconciler::{
        ctx::ApplyCtx, encryption::resolve_encryption_secret, owner::single_owner,
        single_children::apply_children, single_status::patch_status, single_validate::validate_single,
    },
    spec::Single,
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

pub async fn apply(s: &Single, ctx: Arc<Context>) -> Result<Action> {
    let client = ctx.client.clone();
    let oref = s.object_ref(&());
    let ns = s.namespace().unwrap();
    let name = s.name_any();

    validate_single(s)?;
    let owner = single_owner(s);
    let patch = PatchParams::apply("n8n-rustful-operator").force();
    let actx = ApplyCtx {
        client: &client,
        ns: &ns,
        owner: &owner,
        patch: &patch,
    };
    let key_secret = resolve_encryption_secret(
        s,
        &s.spec.image,
        s.spec.encryption_key.as_ref(),
        &ctx,
        &ns,
        &owner,
    )
    .await?;
    apply_children(s, &key_secret, &actx).await?;
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
    patch_status(s, &client, &ns, &name, &key_secret.name, &patch).await?;
    Ok(Action::requeue(Duration::from_secs(5 * 60)))
}
