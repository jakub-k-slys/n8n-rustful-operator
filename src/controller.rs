use crate::{Error, Metrics, Result, telemetry};
use futures::StreamExt;
use jiff::Timestamp;
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{Service, ServicePort},
};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::{
    CustomResource, Resource,
    api::{Api, ListParams, ObjectMeta, Patch, PatchParams, ResourceExt},
    client::Client,
    runtime::{
        controller::{Action, Controller},
        events::{Event, EventType, Recorder, Reporter},
        finalizer::{Event as Finalizer, finalizer},
        watcher::Config,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use tokio::{sync::RwLock, time::Duration};
use tracing::*;

pub static N8N_FINALIZER: &str = "n8ninstances.n8n.slys.dev";

/// `N8nInstance` is a Kubernetes-native description of an n8n deployment.
///
/// The reconciler creates a Deployment and Service for each instance and reports
/// readiness back through the resource status.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(
    kind = "N8nInstance",
    group = "n8n.slys.dev",
    version = "v1",
    namespaced,
    shortname = "n8n",
    status = "N8nInstanceStatus"
)]
pub struct N8nInstanceSpec {
    /// Container image to deploy (e.g. `n8nio/n8n:1.70.0`).
    #[serde(default = "default_image")]
    pub image: String,
    /// Number of replicas of the n8n pod.
    #[serde(default = "default_replicas")]
    pub replicas: i32,
    /// Optional externally-facing host. Surfaced in status for observability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

fn default_image() -> String {
    "n8nio/n8n:latest".to_string()
}
fn default_replicas() -> i32 {
    1
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct N8nInstanceStatus {
    pub ready: bool,
    pub replicas: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Clone)]
pub struct Context {
    pub client: Client,
    pub recorder: Recorder,
    pub diagnostics: Arc<RwLock<Diagnostics>>,
    pub metrics: Arc<Metrics>,
}

#[instrument(skip(ctx, inst), fields(trace_id))]
async fn reconcile(inst: Arc<N8nInstance>, ctx: Arc<Context>) -> Result<Action> {
    let trace_id = telemetry::get_trace_id();
    if trace_id != opentelemetry::trace::TraceId::INVALID {
        Span::current().record("trace_id", field::display(&trace_id));
    }
    let _timer = ctx.metrics.reconcile.count_and_measure(&trace_id);
    ctx.diagnostics.write().await.last_event = Timestamp::now();
    let ns = inst.namespace().unwrap();
    let api: Api<N8nInstance> = Api::namespaced(ctx.client.clone(), &ns);

    info!("Reconciling N8nInstance \"{}\" in {}", inst.name_any(), ns);
    finalizer(&api, N8N_FINALIZER, inst, |event| async {
        match event {
            Finalizer::Apply(i) => i.reconcile(ctx.clone()).await,
            Finalizer::Cleanup(i) => i.cleanup(ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::FinalizerError(Box::new(e)))
}

fn error_policy(inst: Arc<N8nInstance>, error: &Error, ctx: Arc<Context>) -> Action {
    warn!("reconcile failed: {:?}", error);
    ctx.metrics.reconcile.set_failure(&inst, error);
    Action::requeue(Duration::from_secs(5 * 60))
}

impl N8nInstance {
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action> {
        let client = ctx.client.clone();
        let oref = self.object_ref(&());
        let ns = self.namespace().unwrap();
        let name = self.name_any();

        if name == "illegal" {
            return Err(Error::IllegalN8nInstance);
        }

        let deployments: Api<Deployment> = Api::namespaced(client.clone(), &ns);
        let services: Api<Service> = Api::namespaced(client.clone(), &ns);
        let instances: Api<N8nInstance> = Api::namespaced(client.clone(), &ns);

        let ps = PatchParams::apply("n8n-rustful-operator").force();

        let dep = build_deployment(&name, &self.spec);
        deployments
            .patch(&name, &ps, &Patch::Apply(&dep))
            .await
            .map_err(Error::KubeError)?;

        let svc = build_service(&name);
        services
            .patch(&name, &ps, &Patch::Apply(&svc))
            .await
            .map_err(Error::KubeError)?;

        ctx.recorder
            .publish(
                &Event {
                    type_: EventType::Normal,
                    reason: "Applied".into(),
                    note: Some(format!("Applied Deployment/Service for `{name}`")),
                    action: "Reconciling".into(),
                    secondary: None,
                },
                &oref,
            )
            .await
            .map_err(Error::KubeError)?;

        let status = N8nInstanceStatus {
            ready: true,
            replicas: self.spec.replicas,
            url: self.spec.host.as_ref().map(|h| format!("https://{h}")),
        };
        let patch = Patch::Apply(json!({
            "apiVersion": "n8n.slys.dev/v1",
            "kind": "N8nInstance",
            "status": status,
        }));
        instances
            .patch_status(&name, &ps, &patch)
            .await
            .map_err(Error::KubeError)?;

        Ok(Action::requeue(Duration::from_secs(5 * 60)))
    }

    async fn cleanup(&self, ctx: Arc<Context>) -> Result<Action> {
        let oref = self.object_ref(&());
        ctx.recorder
            .publish(
                &Event {
                    type_: EventType::Normal,
                    reason: "DeleteRequested".into(),
                    note: Some(format!("Delete `{}`", self.name_any())),
                    action: "Deleting".into(),
                    secondary: None,
                },
                &oref,
            )
            .await
            .map_err(Error::KubeError)?;
        Ok(Action::await_change())
    }
}

fn selector(name: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("app.kubernetes.io/name".to_string(), "n8n".to_string());
    m.insert("app.kubernetes.io/instance".to_string(), name.to_string());
    m.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "n8n-rustful-operator".to_string(),
    );
    m
}

fn build_deployment(name: &str, spec: &N8nInstanceSpec) -> Deployment {
    let labels = selector(name);
    let dep_json = json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": name,
            "labels": labels,
        },
        "spec": {
            "replicas": spec.replicas,
            "selector": { "matchLabels": labels },
            "template": {
                "metadata": { "labels": labels },
                "spec": {
                    "containers": [{
                        "name": "n8n",
                        "image": spec.image,
                        "ports": [{ "containerPort": 5678, "name": "http" }],
                        "readinessProbe": {
                            "httpGet": { "path": "/healthz", "port": "http" },
                            "initialDelaySeconds": 10,
                            "periodSeconds": 10
                        }
                    }]
                }
            }
        }
    });
    serde_json::from_value(dep_json).expect("static deployment schema is valid")
}

fn build_service(name: &str) -> Service {
    let labels = selector(name);
    Service {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(k8s_openapi::api::core::v1::ServiceSpec {
            selector: Some(labels),
            ports: Some(vec![ServicePort {
                name: Some("http".to_string()),
                port: 5678,
                target_port: Some(IntOrString::String("http".to_string())),
                protocol: Some("TCP".to_string()),
                ..Default::default()
            }]),
            type_: Some("ClusterIP".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[derive(Clone, Serialize)]
pub struct Diagnostics {
    pub last_event: Timestamp,
    #[serde(skip)]
    pub reporter: Reporter,
}
impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            last_event: Timestamp::now(),
            reporter: "n8n-rustful-operator".into(),
        }
    }
}
impl Diagnostics {
    fn recorder(&self, client: Client) -> Recorder {
        Recorder::new(client, self.reporter.clone())
    }
}

#[derive(Clone, Default)]
pub struct State {
    diagnostics: Arc<RwLock<Diagnostics>>,
    metrics: Arc<Metrics>,
}

impl State {
    pub fn metrics(&self) -> String {
        let mut buffer = String::new();
        let registry = &*self.metrics.registry;
        prometheus_client::encoding::text::encode(&mut buffer, registry).unwrap();
        buffer
    }

    pub async fn diagnostics(&self) -> Diagnostics {
        self.diagnostics.read().await.clone()
    }

    pub async fn to_context(&self, client: Client) -> Arc<Context> {
        Arc::new(Context {
            client: client.clone(),
            recorder: self.diagnostics.read().await.recorder(client),
            metrics: self.metrics.clone(),
            diagnostics: self.diagnostics.clone(),
        })
    }
}

pub async fn run(state: State) {
    let client = Client::try_default().await.expect("failed to create kube Client");
    let api = Api::<N8nInstance>::all(client.clone());
    if let Err(e) = api.list(&ListParams::default().limit(1)).await {
        error!("CRD is not queryable; {e:?}. Is the CRD installed?");
        info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
        std::process::exit(1);
    }
    Controller::new(api, Config::default().any_semantic())
        .shutdown_on_signal()
        .run(reconcile, error_policy, state.to_context(client).await)
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()))
        .await;
}
