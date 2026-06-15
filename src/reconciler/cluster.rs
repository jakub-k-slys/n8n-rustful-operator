use crate::{
    Error, Result,
    reconciler::cluster_apply::apply,
    spec::{CLUSTER_FINALIZER, Cluster},
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

#[instrument(skip(ctx, c), fields(trace_id))]
pub async fn reconcile(c: Arc<Cluster>, ctx: Arc<Context>) -> Result<Action> {
    let trace_id = telemetry::get_trace_id();
    if trace_id != opentelemetry::trace::TraceId::INVALID {
        Span::current().record("trace_id", field::display(&trace_id));
    }
    let _timer = ctx.metrics.reconcile.count_and_measure(&trace_id);
    ctx.diagnostics.write().await.last_event = Timestamp::now();
    let ns = c.namespace().unwrap();
    let api: Api<Cluster> = Api::namespaced(ctx.client.clone(), &ns);
    info!("Reconciling Cluster \"{}\" in {}", c.name_any(), ns);
    finalizer(&api, CLUSTER_FINALIZER, c, |event| async {
        match event {
            Finalizer::Apply(x) => apply(&x, ctx.clone()).await,
            Finalizer::Cleanup(x) => cleanup(&x, ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::FinalizerError(Box::new(e)))
}

pub fn error_policy(_c: Arc<Cluster>, error: &Error, _ctx: Arc<Context>) -> Action {
    warn!("cluster reconcile failed: {error:?}");
    Action::requeue(Duration::from_secs(5 * 60))
}

async fn cleanup(c: &Cluster, ctx: Arc<Context>) -> Result<Action> {
    let oref = c.object_ref(&());
    ctx.recorder
        .publish(
            &Event {
                type_: EventType::Normal,
                reason: "DeleteRequested".into(),
                note: Some(format!("Delete cluster `{}`", c.name_any())),
                action: "Deleting".into(),
                secondary: None,
            },
            &oref,
        )
        .await
        .map_err(Error::KubeError)?;
    Ok(Action::await_change())
}
