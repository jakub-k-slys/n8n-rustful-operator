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
