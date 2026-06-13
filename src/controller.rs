use crate::{Error, Metrics, Result, telemetry};
use futures::StreamExt;
use jiff::Timestamp;
use k8s_openapi::{
    api::{
        apps::v1::Deployment,
        core::v1::{Secret, Service, ServicePort},
        networking::v1::Ingress,
    },
    apimachinery::pkg::{apis::meta::v1::OwnerReference, util::intstr::IntOrString},
};
use kube::{
    CustomResource, Resource,
    api::{Api, DynamicObject, GroupVersionKind, ListParams, ObjectMeta, Patch, PatchParams, ResourceExt},
    client::Client,
    discovery::ApiResource,
    runtime::{
        controller::{Action, Controller},
        events::{Event, EventType, Recorder, Reporter},
        finalizer::{Event as Finalizer, finalizer},
        watcher::Config,
    },
};
use rand::RngCore;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use tokio::{sync::RwLock, time::Duration};
use tracing::*;

pub static N8N_FINALIZER: &str = "instances.n8n.slys.dev";

/// `Instance` is a Kubernetes-native description of an n8n deployment.
///
/// The reconciler creates a Deployment and Service for each instance and reports
/// readiness back through the resource status.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(
    kind = "Instance",
    group = "n8n.slys.dev",
    version = "v1",
    namespaced,
    shortname = "n8n",
    plural = "instances",
    status = "InstanceStatus"
)]
pub struct InstanceSpec {
    /// Container image to deploy (e.g. `n8nio/n8n:1.70.0`).
    #[serde(default = "default_image")]
    pub image: String,
    /// Number of replicas of the n8n pod.
    #[serde(default = "default_replicas")]
    pub replicas: i32,
    /// Externally-facing hostname. Required when `networking` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Expose n8n via an Ingress OR an HTTPRoute. The two are mutually exclusive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub networking: Option<NetworkingSpec>,
    /// N8N_ENCRYPTION_KEY source. If omitted, the operator generates a Secret
    /// `<instance>-encryption-key` and owns it via ownerReference.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "encryptionKey")]
    pub encryption_key: Option<EncryptionKeySpec>,
}

fn default_image() -> String {
    "n8nio/n8n:latest".to_string()
}
fn default_replicas() -> i32 {
    1
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct NetworkingSpec {
    /// Provision a `networking.k8s.io/v1` Ingress. Mutually exclusive with `httpRoute`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingress: Option<IngressConfig>,
    /// Provision a `gateway.networking.k8s.io/v1` HTTPRoute. Mutually exclusive with `ingress`.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "httpRoute")]
    pub http_route: Option<HttpRouteConfig>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct IngressConfig {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "className")]
    pub class_name: Option<String>,
    /// Name of a TLS Secret in the same namespace. If set, terminates TLS on the Ingress.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "tlsSecretName")]
    pub tls_secret_name: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct HttpRouteConfig {
    /// Gateway to attach this HTTPRoute to (parentRefs[0]).
    pub gateway: GatewayRef,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct GatewayRef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct EncryptionKeySpec {
    /// Reference to an existing Secret. Mutually exclusive with auto-generation
    /// (omit the whole `encryptionKey` block to auto-generate).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "secretRef")]
    pub secret_ref: Option<SecretKeyRef>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SecretKeyRef {
    pub name: String,
    /// Key within the Secret. Defaults to `encryption_key`.
    #[serde(default = "default_secret_key")]
    pub key: String,
}

fn default_secret_key() -> String {
    "encryption_key".to_string()
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct InstanceStatus {
    pub ready: bool,
    pub replicas: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Name of the Secret used as N8N_ENCRYPTION_KEY (managed or referenced).
    #[serde(skip_serializing_if = "Option::is_none", rename = "encryptionKeySecret")]
    pub encryption_key_secret: Option<String>,
}

#[derive(Clone)]
pub struct Context {
    pub client: Client,
    pub recorder: Recorder,
    pub diagnostics: Arc<RwLock<Diagnostics>>,
    pub metrics: Arc<Metrics>,
}

#[instrument(skip(ctx, inst), fields(trace_id))]
async fn reconcile(inst: Arc<Instance>, ctx: Arc<Context>) -> Result<Action> {
    let trace_id = telemetry::get_trace_id();
    if trace_id != opentelemetry::trace::TraceId::INVALID {
        Span::current().record("trace_id", field::display(&trace_id));
    }
    let _timer = ctx.metrics.reconcile.count_and_measure(&trace_id);
    ctx.diagnostics.write().await.last_event = Timestamp::now();
    let ns = inst.namespace().unwrap();
    let api: Api<Instance> = Api::namespaced(ctx.client.clone(), &ns);

    info!("Reconciling Instance \"{}\" in {}", inst.name_any(), ns);
    finalizer(&api, N8N_FINALIZER, inst, |event| async {
        match event {
            Finalizer::Apply(i) => i.reconcile(ctx.clone()).await,
            Finalizer::Cleanup(i) => i.cleanup(ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::FinalizerError(Box::new(e)))
}

fn error_policy(inst: Arc<Instance>, error: &Error, ctx: Arc<Context>) -> Action {
    warn!("reconcile failed: {:?}", error);
    ctx.metrics.reconcile.set_failure(&inst, error);
    Action::requeue(Duration::from_secs(5 * 60))
}

impl Instance {
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action> {
        let client = ctx.client.clone();
        let oref = self.object_ref(&());
        let ns = self.namespace().unwrap();
        let name = self.name_any();

        if name == "illegal" {
            return Err(Error::IllegalInstance);
        }

        if let Some(net) = &self.spec.networking
            && net.ingress.is_some()
            && net.http_route.is_some()
        {
            return Err(Error::ConflictingNetworking);
        }

        let owner = self.owner_reference();
        let ps = PatchParams::apply("n8n-rustful-operator").force();

        let key_secret = self.resolve_encryption_secret(&ctx, &ns, &owner).await?;

        let deployments: Api<Deployment> = Api::namespaced(client.clone(), &ns);
        let services: Api<Service> = Api::namespaced(client.clone(), &ns);
        let instances: Api<Instance> = Api::namespaced(client.clone(), &ns);

        let dep = build_deployment(&name, &self.spec, &key_secret, &owner);
        deployments
            .patch(&name, &ps, &Patch::Apply(&dep))
            .await
            .map_err(Error::KubeError)?;

        let svc = build_service(&name, &owner);
        services
            .patch(&name, &ps, &Patch::Apply(&svc))
            .await
            .map_err(Error::KubeError)?;

        if let Some(net) = &self.spec.networking {
            if let Some(ing_cfg) = &net.ingress {
                let host = self.spec.host.as_deref().unwrap_or("");
                let ingress = build_ingress(&name, host, ing_cfg, &owner);
                let api: Api<Ingress> = Api::namespaced(client.clone(), &ns);
                api.patch(&name, &ps, &Patch::Apply(&ingress))
                    .await
                    .map_err(Error::KubeError)?;
            }
            if let Some(rt_cfg) = &net.http_route {
                let host = self.spec.host.as_deref().unwrap_or("");
                apply_http_route(&client, &ns, &name, host, rt_cfg, &owner, &ps).await?;
            }
        }

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

        let status = InstanceStatus {
            ready: true,
            replicas: self.spec.replicas,
            url: self.spec.host.as_ref().map(|h| format!("https://{h}")),
            encryption_key_secret: Some(key_secret.name.clone()),
        };
        let patch = Patch::Apply(json!({
            "apiVersion": "n8n.slys.dev/v1",
            "kind": "Instance",
            "status": status,
        }));
        instances
            .patch_status(&name, &ps, &patch)
            .await
            .map_err(Error::KubeError)?;

        Ok(Action::requeue(Duration::from_secs(5 * 60)))
    }

    fn owner_reference(&self) -> OwnerReference {
        OwnerReference {
            api_version: "n8n.slys.dev/v1".to_string(),
            kind: "Instance".to_string(),
            name: self.name_any(),
            uid: self.uid().expect("Instance lacks uid; cannot own children"),
            controller: Some(true),
            block_owner_deletion: Some(true),
        }
    }

    /// Returns the Secret/key pair to mount as `N8N_ENCRYPTION_KEY`.
    /// If the user referenced an existing Secret, we trust it as-is. Otherwise
    /// we create `<instance>-encryption-key` (idempotent) and own it.
    async fn resolve_encryption_secret(
        &self,
        ctx: &Context,
        ns: &str,
        owner: &OwnerReference,
    ) -> Result<SecretKeyRef> {
        if let Some(spec) = &self.spec.encryption_key
            && let Some(r) = &spec.secret_ref
        {
            return Ok(r.clone());
        }
        let name = format!("{}-encryption-key", self.name_any());
        let key = "encryption_key".to_string();
        let secrets: Api<Secret> = Api::namespaced(ctx.client.clone(), ns);
        if secrets
            .get_opt(&name)
            .await
            .map_err(Error::KubeError)?
            .is_none()
        {
            let mut buf = [0u8; 32];
            rand::rng().fill_bytes(&mut buf);
            let value = hex::encode(buf);
            let mut data = BTreeMap::new();
            data.insert(key.clone(), value);
            let secret = Secret {
                metadata: ObjectMeta {
                    name: Some(name.clone()),
                    namespace: Some(ns.to_string()),
                    owner_references: Some(vec![owner.clone()]),
                    labels: Some(selector(&self.name_any())),
                    ..Default::default()
                },
                string_data: Some(data),
                type_: Some("Opaque".to_string()),
                ..Default::default()
            };
            secrets.create(&Default::default(), &secret).await.map_err(Error::KubeError)?;
        }
        Ok(SecretKeyRef { name, key })
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

fn build_deployment(
    name: &str,
    spec: &InstanceSpec,
    key_secret: &SecretKeyRef,
    owner: &OwnerReference,
) -> Deployment {
    let labels = selector(name);
    let dep_json = json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": name,
            "labels": labels,
            "ownerReferences": [owner],
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
                        "env": [{
                            "name": "N8N_ENCRYPTION_KEY",
                            "valueFrom": {
                                "secretKeyRef": {
                                    "name": key_secret.name,
                                    "key": key_secret.key,
                                }
                            }
                        }],
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

fn build_service(name: &str, owner: &OwnerReference) -> Service {
    let labels = selector(name);
    Service {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(labels.clone()),
            owner_references: Some(vec![owner.clone()]),
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

fn build_ingress(name: &str, host: &str, cfg: &IngressConfig, owner: &OwnerReference) -> Ingress {
    let labels = selector(name);
    let mut spec = json!({
        "ingressClassName": cfg.class_name,
        "rules": [{
            "host": host,
            "http": {
                "paths": [{
                    "path": "/",
                    "pathType": "Prefix",
                    "backend": {
                        "service": { "name": name, "port": { "number": 5678 } }
                    }
                }]
            }
        }]
    });
    if let Some(tls) = &cfg.tls_secret_name {
        spec["tls"] = json!([{ "hosts": [host], "secretName": tls }]);
    }
    let json = json!({
        "apiVersion": "networking.k8s.io/v1",
        "kind": "Ingress",
        "metadata": {
            "name": name,
            "labels": labels,
            "ownerReferences": [owner],
        },
        "spec": spec,
    });
    serde_json::from_value(json).expect("static ingress schema is valid")
}

async fn apply_http_route(
    client: &Client,
    ns: &str,
    name: &str,
    host: &str,
    cfg: &HttpRouteConfig,
    owner: &OwnerReference,
    ps: &PatchParams,
) -> Result<()> {
    let labels = selector(name);
    let mut parent = json!({
        "name": cfg.gateway.name,
        "kind": "Gateway",
        "group": "gateway.networking.k8s.io",
    });
    if let Some(gw_ns) = &cfg.gateway.namespace {
        parent["namespace"] = json!(gw_ns);
    }
    let body = json!({
        "apiVersion": "gateway.networking.k8s.io/v1",
        "kind": "HTTPRoute",
        "metadata": {
            "name": name,
            "labels": labels,
            "ownerReferences": [owner],
        },
        "spec": {
            "parentRefs": [parent],
            "hostnames": [host],
            "rules": [{
                "backendRefs": [{
                    "name": name,
                    "port": 5678,
                }]
            }]
        }
    });
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "HTTPRoute");
    let ar = ApiResource::from_gvk(&gvk);
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), ns, &ar);
    let route: DynamicObject = serde_json::from_value(body).expect("static httproute schema is valid");
    api.patch(name, ps, &Patch::Apply(&route))
        .await
        .map_err(Error::KubeError)?;
    Ok(())
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
    let api = Api::<Instance>::all(client.clone());
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
