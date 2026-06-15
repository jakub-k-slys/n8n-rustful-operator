use crate::{
    Error, Result,
    reconciler::single_apply::apply,
    spec::{SINGLE_FINALIZER, Single},
    state::Context,
    telemetry,
};
use jiff::Timestamp;
use kube::{
    Resource, ResourceExt,
    api::Api,
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

#[instrument(skip(ctx, s), fields(trace_id))]
pub async fn reconcile(s: Arc<Single>, ctx: Arc<Context>) -> Result<Action> {
    let trace_id = telemetry::get_trace_id();
    if trace_id != opentelemetry::trace::TraceId::INVALID {
        Span::current().record("trace_id", field::display(&trace_id));
    }
    let _timer = ctx.metrics.reconcile.count_and_measure(&trace_id);
    ctx.diagnostics.write().await.last_event = Timestamp::now();
    let ns = s.namespace().unwrap();
    let api: Api<Single> = Api::namespaced(ctx.client.clone(), &ns);
    info!("Reconciling Single \"{}\" in {}", s.name_any(), ns);
    finalizer(&api, SINGLE_FINALIZER, s, |event| async {
        match event {
            Finalizer::Apply(x) => apply(&x, ctx.clone()).await,
            Finalizer::Cleanup(x) => cleanup(&x, ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::FinalizerError(Box::new(e)))
}

pub fn error_policy(inst: Arc<Single>, error: &Error, ctx: Arc<Context>) -> Action {
    warn!("reconcile failed: {error:?}");
    ctx.metrics.reconcile.set_failure(&*inst, error);
    Action::requeue(Duration::from_secs(5 * 60))
}

async fn cleanup(s: &Single, ctx: Arc<Context>) -> Result<Action> {
    let oref = s.object_ref(&());
    ctx.recorder
        .publish(
            &Event {
                type_: EventType::Normal,
                reason: "DeleteRequested".into(),
                note: Some(format!("Delete `{}`", s.name_any())),
                action: "Deleting".into(),
                secondary: None,
            },
            &oref,
        )
        .await
        .map_err(Error::KubeError)?;
    Ok(Action::await_change())
}
