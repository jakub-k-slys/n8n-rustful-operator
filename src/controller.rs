use crate::{Error, Metrics, Result, telemetry};
use futures::StreamExt;
use jiff::Timestamp;
use k8s_openapi::{
    api::{
        apps::v1::Deployment,
        autoscaling::v2::HorizontalPodAutoscaler,
        core::v1::{PersistentVolumeClaim, Secret, Service, ServicePort},
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

pub static N8N_FINALIZER: &str = "singles.n8n.slys.dev";

/// `Single` is a Kubernetes-native description of an n8n deployment.
///
/// The reconciler creates a Deployment and Service for each instance and reports
/// readiness back through the resource status.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(
    kind = "Single",
    group = "n8n.slys.dev",
    version = "v1",
    namespaced,
    shortname = "n8n",
    plural = "singles",
    status = "SingleStatus"
)]
pub struct SingleSpec {
    /// Container image to deploy (e.g. `n8nio/n8n:1.70.0`).
    #[serde(default = "default_image")]
    pub image: String,
    /// Number of replicas of the n8n pod.
    #[serde(default = "default_replicas")]
    pub replicas: i32,
    /// Externally-facing hostname. Required when `networking` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Service shape. Defaults to ClusterIP (sensible when networking handles ingress).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceConfig>,
    /// Expose n8n via an Ingress OR an HTTPRoute. The two are mutually exclusive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub networking: Option<NetworkingSpec>,
    /// N8N_ENCRYPTION_KEY source. If omitted, the operator generates a Secret
    /// `<instance>-encryption-key` and owns it via ownerReference.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "encryptionKey")]
    pub encryption_key: Option<EncryptionKeySpec>,
    /// Database backend configuration. Omit for n8n's sqlite default with no extra env vars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<DatabaseSpec>,
    /// Mount a PVC at `/home/node/.n8n` so the sqlite file, binary data and runtime
    /// nodes survive pod restarts. Useful regardless of DB type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistence: Option<PersistenceConfig>,
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

#[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
pub struct DatabaseSpec {
    /// `sqlite` (default), `postgresdb`, `mysqldb` or `mariadb` — maps directly to `DB_TYPE`.
    #[serde(default = "default_db_type", rename = "type")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sqlite: Option<SqliteConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postgres: Option<PostgresConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mysql: Option<MysqlConfig>,
}

fn default_db_type() -> String {
    "sqlite".to_string()
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct PostgresConfig {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    pub database: String,
    pub user: String,
    #[serde(rename = "passwordSecret")]
    pub password_secret: SecretKeyRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl: Option<DatabaseSsl>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "poolSize")]
    pub pool_size: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "connectionTimeoutMs"
    )]
    pub connection_timeout_ms: Option<u32>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct MysqlConfig {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    pub database: String,
    pub user: String,
    #[serde(rename = "passwordSecret")]
    pub password_secret: SecretKeyRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl: Option<DatabaseSsl>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "connectionTimeoutMs"
    )]
    pub connection_timeout_ms: Option<u32>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SqliteConfig {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "poolSize")]
    pub pool_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "vacuumOnStartup")]
    pub vacuum_on_startup: Option<bool>,
    /// Path inside the pod, mapped to `DB_SQLITE_DATABASE`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct DatabaseSsl {
    #[serde(default)]
    pub enabled: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "rejectUnauthorized"
    )]
    pub reject_unauthorized: Option<bool>,
    /// Mount as `/etc/n8n/ssl/ca/ca.crt` and pass via `DB_*_SSL_CA`.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "caSecret")]
    pub ca_secret: Option<SecretKeyRef>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "certSecret")]
    pub cert_secret: Option<SecretKeyRef>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "keySecret")]
    pub key_secret: Option<SecretKeyRef>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct PersistenceConfig {
    /// Storage request, e.g. `1Gi`.
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "storageClassName")]
    pub storage_class_name: Option<String>,
    #[serde(default = "default_access_mode", rename = "accessMode")]
    pub access_mode: String,
}

fn default_access_mode() -> String {
    "ReadWriteOnce".to_string()
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct ServiceConfig {
    /// `ClusterIP` (default), `NodePort`, or `LoadBalancer`.
    #[serde(default = "default_service_type", rename = "type")]
    pub type_: String,
}

fn default_service_type() -> String {
    "ClusterIP".to_string()
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct SingleStatus {
    pub ready: bool,
    pub replicas: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Name of the Secret used as N8N_ENCRYPTION_KEY (managed or referenced).
    #[serde(skip_serializing_if = "Option::is_none", rename = "encryptionKeySecret")]
    pub encryption_key_secret: Option<String>,
}

// =====================================================================
// Cluster CRD — queue-mode n8n: main + workers + (optional) webhooks.
// All roles share Redis, Postgres (sqlite rejected) and the encryption key.
// =====================================================================

pub static CLUSTER_FINALIZER: &str = "clusters.n8n.slys.dev";

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(
    kind = "Cluster",
    group = "n8n.slys.dev",
    version = "v1",
    namespaced,
    shortname = "n8nc",
    plural = "clusters",
    status = "ClusterStatus"
)]
pub struct ClusterSpec {
    /// Cascading default image for every role. Each role can override.
    #[serde(default = "default_image")]
    pub image: String,
    /// Shared `N8N_ENCRYPTION_KEY`. Omitted → auto-generated `<cluster>-encryption-key`.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "encryptionKey")]
    pub encryption_key: Option<EncryptionKeySpec>,
    /// Database backend. Queue mode requires a shared DB — sqlite is rejected.
    pub database: DatabaseSpec,
    /// Redis-backed Bull/BullMQ queue.
    pub redis: RedisConfig,
    #[serde(default)]
    pub main: MainConfig,
    pub workers: WorkerConfig,
    /// Optional dedicated webhook pool (sets `N8N_DISABLE_PRODUCTION_MAIN_PROCESS=true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhooks: Option<WebhookConfig>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
pub struct MainConfig {
    #[serde(default = "default_main_replicas")]
    pub replicas: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub networking: Option<NetworkingSpec>,
    /// Mount a PVC at `/home/node/.n8n` on the main pod only. Workers and webhooks
    /// stay stateless (use DB and S3 for binary data instead).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistence: Option<PersistenceConfig>,
}
fn default_main_replicas() -> i32 {
    1
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
pub struct WorkerConfig {
    /// Static replica count. Ignored when `autoscaling` is set (HPA owns the field).
    pub replicas: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Maps to `N8N_CONCURRENCY_PRODUCTION_LIMIT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u32>,
    /// Provision a HorizontalPodAutoscaler for the worker Deployment. When set,
    /// the operator stops managing `spec.replicas` so HPA can drive it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autoscaling: Option<Autoscaling>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct Autoscaling {
    #[serde(rename = "minReplicas")]
    pub min_replicas: i32,
    #[serde(rename = "maxReplicas")]
    pub max_replicas: i32,
    /// Average CPU utilisation target (percent). Defaults to 70.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "targetCPUUtilizationPercentage"
    )]
    pub target_cpu_utilization_percentage: Option<i32>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct WebhookConfig {
    pub replicas: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub networking: Option<NetworkingSpec>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
pub struct RedisConfig {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "passwordSecret")]
    pub password_secret: Option<SecretKeyRef>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "usernameSecret")]
    pub username_secret: Option<SecretKeyRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<bool>,
    /// `QUEUE_BULL_PREFIX` for namespacing within a shared Redis instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct ClusterStatus {
    pub ready: bool,
    #[serde(rename = "mainReplicas")]
    pub main_replicas: i32,
    #[serde(rename = "workerReplicas")]
    pub worker_replicas: i32,
    #[serde(rename = "webhookReplicas")]
    pub webhook_replicas: i32,
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
async fn reconcile(inst: Arc<Single>, ctx: Arc<Context>) -> Result<Action> {
    let trace_id = telemetry::get_trace_id();
    if trace_id != opentelemetry::trace::TraceId::INVALID {
        Span::current().record("trace_id", field::display(&trace_id));
    }
    let _timer = ctx.metrics.reconcile.count_and_measure(&trace_id);
    ctx.diagnostics.write().await.last_event = Timestamp::now();
    let ns = inst.namespace().unwrap();
    let api: Api<Single> = Api::namespaced(ctx.client.clone(), &ns);

    info!("Reconciling Single \"{}\" in {}", inst.name_any(), ns);
    finalizer(&api, N8N_FINALIZER, inst, |event| async {
        match event {
            Finalizer::Apply(i) => i.reconcile(ctx.clone()).await,
            Finalizer::Cleanup(i) => i.cleanup(ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::FinalizerError(Box::new(e)))
}

fn error_policy(inst: Arc<Single>, error: &Error, ctx: Arc<Context>) -> Action {
    warn!("reconcile failed: {:?}", error);
    ctx.metrics.reconcile.set_failure(&inst, error);
    Action::requeue(Duration::from_secs(5 * 60))
}

impl Single {
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action> {
        let client = ctx.client.clone();
        let oref = self.object_ref(&());
        let ns = self.namespace().unwrap();
        let name = self.name_any();

        if name == "illegal" {
            return Err(Error::IllegalSingle);
        }

        if let Some(net) = &self.spec.networking
            && net.ingress.is_some()
            && net.http_route.is_some()
        {
            return Err(Error::ConflictingNetworking);
        }

        if let Some(db) = &self.spec.database {
            validate_database(db)?;
        }

        let owner = self.owner_reference();
        let ps = PatchParams::apply("n8n-rustful-operator").force();

        let key_secret = self.resolve_encryption_secret(&ctx, &ns, &owner).await?;

        let deployments: Api<Deployment> = Api::namespaced(client.clone(), &ns);
        let services: Api<Service> = Api::namespaced(client.clone(), &ns);
        let instances: Api<Single> = Api::namespaced(client.clone(), &ns);
        let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(client.clone(), &ns);

        let pvc_name = format!("{name}-data");
        if let Some(pvc) = build_data_pvc(
            &pvc_name,
            &name,
            &self.spec.image,
            self.spec.persistence.as_ref(),
            &owner,
        ) {
            pvcs.patch(&pvc_name, &ps, &Patch::Apply(&pvc))
                .await
                .map_err(Error::KubeError)?;
        }

        let dep = build_deployment(&name, &self.spec, &key_secret, &owner);
        deployments
            .patch(&name, &ps, &Patch::Apply(&dep))
            .await
            .map_err(Error::KubeError)?;

        let svc = build_service(&name, &self.spec, &owner);
        services
            .patch(&name, &ps, &Patch::Apply(&svc))
            .await
            .map_err(Error::KubeError)?;

        let want_ingress = self.spec.networking.as_ref().and_then(|n| n.ingress.as_ref());
        let want_route = self.spec.networking.as_ref().and_then(|n| n.http_route.as_ref());

        let ingress_api: Api<Ingress> = Api::namespaced(client.clone(), &ns);
        if let Some(ing_cfg) = want_ingress {
            let host = self.spec.host.as_deref().unwrap_or("");
            let ingress = build_ingress(&name, &self.spec.image, host, ing_cfg, &owner);
            ingress_api
                .patch(&name, &ps, &Patch::Apply(&ingress))
                .await
                .map_err(Error::KubeError)?;
        } else if ingress_api
            .get_opt(&name)
            .await
            .map_err(Error::KubeError)?
            .is_some()
        {
            ingress_api
                .delete(&name, &Default::default())
                .await
                .map_err(Error::KubeError)?;
        }

        if let Some(rt_cfg) = want_route {
            let host = self.spec.host.as_deref().unwrap_or("");
            apply_http_route(&client, &ns, &name, &self.spec.image, host, rt_cfg, &owner, &ps).await?;
        } else {
            // Best-effort delete; ignore errors if the Gateway API CRD isn't installed.
            let _ = delete_http_route(&client, &ns, &name).await;
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

        let status = SingleStatus {
            ready: true,
            replicas: self.spec.replicas,
            url: self.spec.host.as_ref().map(|h| format!("https://{h}")),
            encryption_key_secret: Some(key_secret.name.clone()),
        };
        let patch = Patch::Apply(json!({
            "apiVersion": "n8n.slys.dev/v1",
            "kind": "Single",
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
            kind: "Single".to_string(),
            name: self.name_any(),
            uid: self.uid().expect("Single lacks uid; cannot own children"),
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
        if secrets.get_opt(&name).await.map_err(Error::KubeError)?.is_none() {
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
                    labels: Some(common_labels(
                        &self.name_any(),
                        &self.spec.image,
                        "encryption-key",
                    )),
                    annotations: Some(common_annotations()),
                    ..Default::default()
                },
                string_data: Some(data),
                type_: Some("Opaque".to_string()),
                ..Default::default()
            };
            secrets
                .create(&Default::default(), &secret)
                .await
                .map_err(Error::KubeError)?;
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

// =====================================================================
// Cluster reconciler (queue mode)
// =====================================================================

#[instrument(skip(ctx, c), fields(trace_id))]
async fn reconcile_cluster(c: Arc<Cluster>, ctx: Arc<Context>) -> Result<Action> {
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
            Finalizer::Apply(x) => x.reconcile(ctx.clone()).await,
            Finalizer::Cleanup(x) => x.cleanup(ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::FinalizerError(Box::new(e)))
}

fn cluster_error_policy(_c: Arc<Cluster>, error: &Error, _ctx: Arc<Context>) -> Action {
    warn!("cluster reconcile failed: {error:?}");
    Action::requeue(Duration::from_secs(5 * 60))
}

impl Cluster {
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action> {
        let client = ctx.client.clone();
        let oref = self.object_ref(&());
        let ns = self.namespace().unwrap();
        let name = self.name_any();
        let ps = PatchParams::apply("n8n-rustful-operator").force();

        validate_cluster(self)?;
        let owner = self.owner_reference();

        let key_secret = self.resolve_cluster_encryption_secret(&ctx, &ns, &owner).await?;
        let common_env = build_cluster_common_env(self, &key_secret);
        let (common_vols, common_mounts) = build_db_volumes(&name, &self.spec.database);

        let deployments: Api<Deployment> = Api::namespaced(client.clone(), &ns);
        let services: Api<Service> = Api::namespaced(client.clone(), &ns);
        let ingresses: Api<Ingress> = Api::namespaced(client.clone(), &ns);
        let clusters: Api<Cluster> = Api::namespaced(client.clone(), &ns);
        let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(client.clone(), &ns);

        // ----- Main role -----
        let main_name = format!("{name}-main");
        let main_image = self
            .spec
            .main
            .image
            .clone()
            .unwrap_or_else(|| self.spec.image.clone());
        let main_pvc_name = format!("{main_name}-data");
        if let Some(pvc) = build_data_pvc(
            &main_pvc_name,
            &main_name,
            &main_image,
            self.spec.main.persistence.as_ref(),
            &owner,
        ) {
            pvcs.patch(&main_pvc_name, &ps, &Patch::Apply(&pvc))
                .await
                .map_err(Error::KubeError)?;
        }
        let mut main_vols = common_vols.clone();
        let mut main_mounts = common_mounts.clone();
        if self.spec.main.persistence.is_some() {
            let (v, m) = build_persistence_volume(&main_pvc_name);
            main_vols.push(v);
            main_mounts.push(m);
        }
        let main_dep = build_cluster_deployment(
            &main_name,
            &main_image,
            "main",
            Some(self.spec.main.replicas),
            &common_env,
            &main_vols,
            &main_mounts,
            None,
            &owner,
        );
        deployments
            .patch(&main_name, &ps, &Patch::Apply(&main_dep))
            .await
            .map_err(Error::KubeError)?;
        let main_svc = build_cluster_service(
            &main_name,
            &main_image,
            "main",
            self.spec.main.service.as_ref(),
            &owner,
        );
        services
            .patch(&main_name, &ps, &Patch::Apply(&main_svc))
            .await
            .map_err(Error::KubeError)?;
        reconcile_role_networking(
            &client,
            &ns,
            &main_name,
            &main_image,
            "main",
            self.spec.main.host.as_deref(),
            self.spec.main.networking.as_ref(),
            &owner,
            &ps,
        )
        .await?;

        // ----- Worker role -----
        let worker_name = format!("{name}-worker");
        let worker_image = self
            .spec
            .workers
            .image
            .clone()
            .unwrap_or_else(|| self.spec.image.clone());
        let mut worker_env = common_env.clone();
        if let Some(cc) = self.spec.workers.concurrency {
            worker_env.push(env_str("N8N_CONCURRENCY_PRODUCTION_LIMIT", cc.to_string()));
        }
        worker_env.push(env_str("QUEUE_HEALTH_CHECK_ACTIVE", "true"));
        // When HPA owns spec.replicas, omit the field from our SSA patch.
        let worker_replicas_field = if self.spec.workers.autoscaling.is_some() {
            None
        } else {
            Some(self.spec.workers.replicas)
        };
        let worker_dep = build_cluster_deployment(
            &worker_name,
            &worker_image,
            "worker",
            worker_replicas_field,
            &worker_env,
            &common_vols,
            &common_mounts,
            Some(vec!["n8n".to_string(), "worker".to_string()]),
            &owner,
        );
        deployments
            .patch(&worker_name, &ps, &Patch::Apply(&worker_dep))
            .await
            .map_err(Error::KubeError)?;

        let hpas: Api<HorizontalPodAutoscaler> = Api::namespaced(client.clone(), &ns);
        if let Some(as_cfg) = &self.spec.workers.autoscaling {
            let hpa = build_worker_hpa(&worker_name, &worker_image, as_cfg, &owner);
            hpas.patch(&worker_name, &ps, &Patch::Apply(&hpa))
                .await
                .map_err(Error::KubeError)?;
        } else if hpas
            .get_opt(&worker_name)
            .await
            .map_err(Error::KubeError)?
            .is_some()
        {
            hpas.delete(&worker_name, &Default::default())
                .await
                .map_err(Error::KubeError)?;
        }

        // ----- Webhook role (optional) -----
        let webhook_name = format!("{name}-webhook");
        if let Some(wh) = &self.spec.webhooks {
            let wh_image = wh.image.clone().unwrap_or_else(|| self.spec.image.clone());
            let mut wh_env = common_env.clone();
            wh_env.push(env_str("N8N_DISABLE_PRODUCTION_MAIN_PROCESS", "true"));
            let wh_dep = build_cluster_deployment(
                &webhook_name,
                &wh_image,
                "webhook",
                Some(wh.replicas),
                &wh_env,
                &common_vols,
                &common_mounts,
                Some(vec!["n8n".to_string(), "webhook".to_string()]),
                &owner,
            );
            deployments
                .patch(&webhook_name, &ps, &Patch::Apply(&wh_dep))
                .await
                .map_err(Error::KubeError)?;
            let wh_svc =
                build_cluster_service(&webhook_name, &wh_image, "webhook", wh.service.as_ref(), &owner);
            services
                .patch(&webhook_name, &ps, &Patch::Apply(&wh_svc))
                .await
                .map_err(Error::KubeError)?;
            reconcile_role_networking(
                &client,
                &ns,
                &webhook_name,
                &wh_image,
                "webhook",
                wh.host.as_deref(),
                wh.networking.as_ref(),
                &owner,
                &ps,
            )
            .await?;
        } else {
            // declarative cleanup of webhook children if they once existed
            let _ = deployments.delete(&webhook_name, &Default::default()).await;
            let _ = services.delete(&webhook_name, &Default::default()).await;
            let _ = ingresses.delete(&webhook_name, &Default::default()).await;
            let _ = delete_http_route(&client, &ns, &webhook_name).await;
        }

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

        let status = ClusterStatus {
            ready: true,
            main_replicas: self.spec.main.replicas,
            worker_replicas: self.spec.workers.replicas,
            webhook_replicas: self.spec.webhooks.as_ref().map(|w| w.replicas).unwrap_or(0),
            encryption_key_secret: Some(key_secret.name.clone()),
        };
        let patch = Patch::Apply(json!({
            "apiVersion": "n8n.slys.dev/v1",
            "kind": "Cluster",
            "status": status,
        }));
        clusters
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
                    note: Some(format!("Delete cluster `{}`", self.name_any())),
                    action: "Deleting".into(),
                    secondary: None,
                },
                &oref,
            )
            .await
            .map_err(Error::KubeError)?;
        Ok(Action::await_change())
    }

    fn owner_reference(&self) -> OwnerReference {
        OwnerReference {
            api_version: "n8n.slys.dev/v1".to_string(),
            kind: "Cluster".to_string(),
            name: self.name_any(),
            uid: self.uid().expect("Cluster lacks uid; cannot own children"),
            controller: Some(true),
            block_owner_deletion: Some(true),
        }
    }

    async fn resolve_cluster_encryption_secret(
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
        if secrets.get_opt(&name).await.map_err(Error::KubeError)?.is_none() {
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
                    labels: Some(common_labels(
                        &self.name_any(),
                        &self.spec.image,
                        "encryption-key",
                    )),
                    annotations: Some(common_annotations()),
                    ..Default::default()
                },
                string_data: Some(data),
                type_: Some("Opaque".to_string()),
                ..Default::default()
            };
            secrets
                .create(&Default::default(), &secret)
                .await
                .map_err(Error::KubeError)?;
        }
        Ok(SecretKeyRef { name, key })
    }
}

fn validate_cluster(c: &Cluster) -> Result<()> {
    validate_database(&c.spec.database)?;
    if c.spec.database.type_ == "sqlite" {
        return Err(Error::IllegalCluster(
            "queue mode requires a shared DB; sqlite is not supported".into(),
        ));
    }
    Ok(())
}

fn build_redis_env(redis: &RedisConfig) -> Vec<serde_json::Value> {
    let mut out = vec![json!({ "name": "QUEUE_BULL_REDIS_HOST", "value": redis.host })];
    if let Some(p) = redis.port {
        out.push(env_str("QUEUE_BULL_REDIS_PORT", p.to_string()));
    }
    if let Some(d) = redis.db {
        out.push(env_str("QUEUE_BULL_REDIS_DB", d.to_string()));
    }
    if let Some(s) = &redis.password_secret {
        out.push(env_secret("QUEUE_BULL_REDIS_PASSWORD", s));
    }
    if let Some(s) = &redis.username_secret {
        out.push(env_secret("QUEUE_BULL_REDIS_USERNAME", s));
    }
    if let Some(t) = redis.tls {
        out.push(env_str("QUEUE_BULL_REDIS_TLS", t.to_string()));
    }
    if let Some(p) = &redis.prefix {
        out.push(json!({ "name": "QUEUE_BULL_PREFIX", "value": p }));
    }
    out
}

fn build_cluster_common_env(c: &Cluster, key_secret: &SecretKeyRef) -> Vec<serde_json::Value> {
    let mut env = vec![
        env_str("EXECUTIONS_MODE", "queue"),
        json!({
            "name": "N8N_ENCRYPTION_KEY",
            "valueFrom": { "secretKeyRef": { "name": key_secret.name, "key": key_secret.key } }
        }),
    ];
    env.extend(build_db_env(&c.spec.database));
    env.extend(build_redis_env(&c.spec.redis));
    env
}

#[allow(clippy::too_many_arguments)]
fn build_cluster_deployment(
    name: &str,
    image: &str,
    component: &str,
    replicas: Option<i32>,
    env: &[serde_json::Value],
    volumes: &[serde_json::Value],
    mounts: &[serde_json::Value],
    command: Option<Vec<String>>,
    owner: &OwnerReference,
) -> Deployment {
    let selector = selector_labels(name);
    let labels = common_labels(name, image, component);
    let annotations = common_annotations();
    let mut container = json!({
        "name": "n8n",
        "image": image,
        "ports": [{ "containerPort": 5678, "name": "http" }],
        "env": env,
        "volumeMounts": mounts,
        "readinessProbe": {
            "httpGet": { "path": "/healthz", "port": "http" },
            "initialDelaySeconds": 10,
            "periodSeconds": 10
        }
    });
    if let Some(cmd) = command {
        container["command"] = json!(cmd);
    }
    let mut spec = json!({
        "selector": { "matchLabels": selector },
        "template": {
            "metadata": { "labels": labels, "annotations": annotations },
            "spec": {
                "volumes": volumes,
                "containers": [container],
            }
        }
    });
    if let Some(r) = replicas {
        spec["replicas"] = json!(r);
    }
    let json = json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": name,
            "labels": labels,
            "annotations": annotations,
            "ownerReferences": [owner],
        },
        "spec": spec,
    });
    serde_json::from_value(json).expect("static cluster deployment schema is valid")
}

fn build_worker_hpa(
    name: &str,
    image: &str,
    autoscaling: &Autoscaling,
    owner: &OwnerReference,
) -> HorizontalPodAutoscaler {
    let labels = common_labels(name, image, "worker");
    let cpu = autoscaling.target_cpu_utilization_percentage.unwrap_or(70);
    let json = json!({
        "apiVersion": "autoscaling/v2",
        "kind": "HorizontalPodAutoscaler",
        "metadata": {
            "name": name,
            "labels": labels,
            "annotations": common_annotations(),
            "ownerReferences": [owner],
        },
        "spec": {
            "scaleTargetRef": {
                "apiVersion": "apps/v1",
                "kind": "Deployment",
                "name": name,
            },
            "minReplicas": autoscaling.min_replicas,
            "maxReplicas": autoscaling.max_replicas,
            "metrics": [{
                "type": "Resource",
                "resource": {
                    "name": "cpu",
                    "target": { "type": "Utilization", "averageUtilization": cpu }
                }
            }]
        }
    });
    serde_json::from_value(json).expect("static HPA schema is valid")
}

fn build_cluster_service(
    name: &str,
    image: &str,
    component: &str,
    svc: Option<&ServiceConfig>,
    owner: &OwnerReference,
) -> Service {
    let selector = selector_labels(name);
    let labels = common_labels(name, image, component);
    let svc_type = svc.map(|s| s.type_.clone()).unwrap_or_else(default_service_type);
    Service {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(labels),
            annotations: Some(common_annotations()),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        spec: Some(k8s_openapi::api::core::v1::ServiceSpec {
            selector: Some(selector),
            ports: Some(vec![ServicePort {
                name: Some("http".to_string()),
                port: 5678,
                target_port: Some(IntOrString::String("http".to_string())),
                protocol: Some("TCP".to_string()),
                ..Default::default()
            }]),
            type_: Some(svc_type),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[allow(clippy::too_many_arguments)]
async fn reconcile_role_networking(
    client: &Client,
    ns: &str,
    name: &str,
    image: &str,
    component: &str,
    host: Option<&str>,
    net: Option<&NetworkingSpec>,
    owner: &OwnerReference,
    ps: &PatchParams,
) -> Result<()> {
    if let Some(net) = net
        && net.ingress.is_some()
        && net.http_route.is_some()
    {
        return Err(Error::ConflictingNetworking);
    }
    let want_ingress = net.and_then(|n| n.ingress.as_ref());
    let want_route = net.and_then(|n| n.http_route.as_ref());
    let ingress_api: Api<Ingress> = Api::namespaced(client.clone(), ns);
    if let Some(ing_cfg) = want_ingress {
        let host = host.unwrap_or("");
        let mut ingress = build_ingress(name, image, host, ing_cfg, owner);
        // Override the component label that build_ingress sets to "ingress".
        if let Some(meta_labels) = ingress.metadata.labels.as_mut() {
            meta_labels.insert(
                "app.kubernetes.io/component".to_string(),
                format!("{component}-ingress"),
            );
        }
        ingress_api
            .patch(name, ps, &Patch::Apply(&ingress))
            .await
            .map_err(Error::KubeError)?;
    } else if ingress_api
        .get_opt(name)
        .await
        .map_err(Error::KubeError)?
        .is_some()
    {
        ingress_api
            .delete(name, &Default::default())
            .await
            .map_err(Error::KubeError)?;
    }
    if let Some(rt_cfg) = want_route {
        let host = host.unwrap_or("");
        apply_http_route(client, ns, name, image, host, rt_cfg, owner, ps).await?;
    } else {
        let _ = delete_http_route(client, ns, name).await;
    }
    Ok(())
}

/// Stable subset used as `Deployment.spec.selector` and `Service.spec.selector`.
/// These two labels MUST NOT change — selectors are immutable after creation.
fn selector_labels(name: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("app.kubernetes.io/name".to_string(), "n8n".to_string());
    m.insert("app.kubernetes.io/instance".to_string(), name.to_string());
    m
}

/// Full label set put on `metadata.labels` of every managed object and on the
/// pod template. Superset of `selector_labels` (so selectors still match) plus
/// the four other recommended app.kubernetes.io labels.
fn common_labels(name: &str, image: &str, component: &str) -> BTreeMap<String, String> {
    let mut m = selector_labels(name);
    m.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "n8n-rustful-operator".to_string(),
    );
    m.insert("app.kubernetes.io/part-of".to_string(), "n8n".to_string());
    m.insert("app.kubernetes.io/component".to_string(), component.to_string());
    m.insert("app.kubernetes.io/version".to_string(), image_version(image));
    m
}

fn image_version(image: &str) -> String {
    // Strip registry/host parts then take the tag after the last ':'.
    let last = image.rsplit('/').next().unwrap_or(image);
    last.rsplit_once(':')
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| "latest".to_string())
}

fn common_annotations() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert(
        "n8n.slys.dev/operator-version".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    m
}

fn build_deployment(
    name: &str,
    spec: &SingleSpec,
    key_secret: &SecretKeyRef,
    owner: &OwnerReference,
) -> Deployment {
    let selector = selector_labels(name);
    let labels = common_labels(name, &spec.image, "workflow-engine");
    let annotations = common_annotations();
    let mut env = vec![json!({
        "name": "N8N_ENCRYPTION_KEY",
        "valueFrom": { "secretKeyRef": { "name": key_secret.name, "key": key_secret.key } }
    })];
    let mut volumes: Vec<serde_json::Value> = Vec::new();
    let mut mounts: Vec<serde_json::Value> = Vec::new();

    if let Some(db) = &spec.database {
        env.extend(build_db_env(db));
        let (vols, vm) = build_db_volumes(name, db);
        volumes.extend(vols);
        mounts.extend(vm);
    }

    if spec.persistence.is_some() {
        let (v, m) = build_persistence_volume(&format!("{name}-data"));
        volumes.push(v);
        mounts.push(m);
    }

    let dep_json = json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": name,
            "labels": labels,
            "annotations": annotations,
            "ownerReferences": [owner],
        },
        "spec": {
            "replicas": spec.replicas,
            "selector": { "matchLabels": selector },
            "template": {
                "metadata": { "labels": labels, "annotations": annotations },
                "spec": {
                    "volumes": volumes,
                    "containers": [{
                        "name": "n8n",
                        "image": spec.image,
                        "ports": [{ "containerPort": 5678, "name": "http" }],
                        "env": env,
                        "volumeMounts": mounts,
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

fn validate_database(db: &DatabaseSpec) -> Result<()> {
    let illegal = |msg: &str| -> Result<()> { Err(Error::IllegalDatabase(msg.to_string())) };
    let extras_for_type = |ty: &str| -> Vec<&'static str> {
        let mut v = vec![];
        if ty != "sqlite" && db.sqlite.is_some() {
            v.push(".sqlite");
        }
        if ty != "postgresdb" && db.postgres.is_some() {
            v.push(".postgres");
        }
        if !matches!(ty, "mysqldb" | "mariadb") && db.mysql.is_some() {
            v.push(".mysql");
        }
        v
    };
    match db.type_.as_str() {
        "sqlite" => {
            let extras = extras_for_type("sqlite");
            if !extras.is_empty() {
                return illegal(&format!("type=sqlite but {} also set", extras.join(", ")));
            }
        }
        "postgresdb" => {
            if db.postgres.is_none() {
                return illegal("type=postgresdb requires .database.postgres");
            }
            let extras = extras_for_type("postgresdb");
            if !extras.is_empty() {
                return illegal(&format!("type=postgresdb but {} also set", extras.join(", ")));
            }
        }
        "mysqldb" | "mariadb" => {
            if db.mysql.is_none() {
                return illegal(&format!("type={} requires .database.mysql", db.type_));
            }
            let extras = extras_for_type(&db.type_);
            if !extras.is_empty() {
                return illegal(&format!("type={} but {} also set", db.type_, extras.join(", ")));
            }
        }
        other => return illegal(&format!("unknown type {other:?}")),
    }
    Ok(())
}

fn env_str(name: &str, value: impl Into<serde_json::Value>) -> serde_json::Value {
    json!({ "name": name, "value": value.into().to_string().trim_matches('"').to_string() })
}

fn env_secret(name: &str, sec: &SecretKeyRef) -> serde_json::Value {
    json!({
        "name": name,
        "valueFrom": { "secretKeyRef": { "name": sec.name, "key": sec.key } }
    })
}

fn build_db_env(db: &DatabaseSpec) -> Vec<serde_json::Value> {
    let mut out = vec![json!({ "name": "DB_TYPE", "value": db.type_ })];
    match db.type_.as_str() {
        "postgresdb" => {
            if let Some(pg) = &db.postgres {
                out.push(json!({ "name": "DB_POSTGRESDB_HOST", "value": pg.host }));
                if let Some(p) = pg.port {
                    out.push(env_str("DB_POSTGRESDB_PORT", p.to_string()));
                }
                out.push(json!({ "name": "DB_POSTGRESDB_DATABASE", "value": pg.database }));
                out.push(json!({ "name": "DB_POSTGRESDB_USER", "value": pg.user }));
                out.push(env_secret("DB_POSTGRESDB_PASSWORD", &pg.password_secret));
                if let Some(s) = &pg.schema {
                    out.push(json!({ "name": "DB_POSTGRESDB_SCHEMA", "value": s }));
                }
                if let Some(sz) = pg.pool_size {
                    out.push(env_str("DB_POSTGRESDB_POOL_SIZE", sz.to_string()));
                }
                if let Some(t) = pg.connection_timeout_ms {
                    out.push(env_str("DB_POSTGRESDB_CONNECTION_TIMEOUT", t.to_string()));
                }
                if let Some(ssl) = &pg.ssl {
                    push_ssl_env(&mut out, "DB_POSTGRESDB", ssl);
                }
            }
        }
        "mysqldb" | "mariadb" => {
            if let Some(my) = &db.mysql {
                out.push(json!({ "name": "DB_MYSQLDB_HOST", "value": my.host }));
                if let Some(p) = my.port {
                    out.push(env_str("DB_MYSQLDB_PORT", p.to_string()));
                }
                out.push(json!({ "name": "DB_MYSQLDB_DATABASE", "value": my.database }));
                out.push(json!({ "name": "DB_MYSQLDB_USER", "value": my.user }));
                out.push(env_secret("DB_MYSQLDB_PASSWORD", &my.password_secret));
                if let Some(t) = my.connection_timeout_ms {
                    out.push(env_str("DB_MYSQLDB_CONNECTION_TIMEOUT", t.to_string()));
                }
                if let Some(ssl) = &my.ssl {
                    push_ssl_env(&mut out, "DB_MYSQLDB", ssl);
                }
            }
        }
        "sqlite" => {
            if let Some(sq) = &db.sqlite {
                if let Some(sz) = sq.pool_size {
                    out.push(env_str("DB_SQLITE_POOL_SIZE", sz.to_string()));
                }
                if let Some(v) = sq.vacuum_on_startup {
                    out.push(env_str("DB_SQLITE_VACUUM_ON_STARTUP", v.to_string()));
                }
                if let Some(d) = &sq.database {
                    out.push(json!({ "name": "DB_SQLITE_DATABASE", "value": d }));
                }
            }
        }
        _ => {}
    }
    out
}

fn push_ssl_env(out: &mut Vec<serde_json::Value>, prefix: &str, ssl: &DatabaseSsl) {
    out.push(env_str(&format!("{prefix}_SSL_ENABLED"), ssl.enabled.to_string()));
    if let Some(r) = ssl.reject_unauthorized {
        out.push(env_str(
            &format!("{prefix}_SSL_REJECT_UNAUTHORIZED"),
            r.to_string(),
        ));
    }
    if ssl.ca_secret.is_some() {
        out.push(env_str(&format!("{prefix}_SSL_CA"), "/etc/n8n/ssl/ca/ca.crt"));
    }
    if ssl.cert_secret.is_some() {
        out.push(env_str(
            &format!("{prefix}_SSL_CERT"),
            "/etc/n8n/ssl/cert/cert.crt",
        ));
    }
    if ssl.key_secret.is_some() {
        out.push(env_str(&format!("{prefix}_SSL_KEY"), "/etc/n8n/ssl/key/key.pem"));
    }
}

fn build_db_volumes(instance: &str, db: &DatabaseSpec) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let mut vols = vec![];
    let mut mounts = vec![];
    let ssl_ref = match db.type_.as_str() {
        "postgresdb" => db.postgres.as_ref().and_then(|p| p.ssl.as_ref()),
        "mysqldb" | "mariadb" => db.mysql.as_ref().and_then(|m| m.ssl.as_ref()),
        _ => None,
    };
    if let Some(ssl) = ssl_ref {
        if let Some(sec) = &ssl.ca_secret {
            vols.push(secret_volume("n8n-db-ssl-ca", &sec.name, &sec.key, "ca.crt"));
            mounts.push(json!({ "name": "n8n-db-ssl-ca", "mountPath": "/etc/n8n/ssl/ca", "readOnly": true }));
        }
        if let Some(sec) = &ssl.cert_secret {
            vols.push(secret_volume("n8n-db-ssl-cert", &sec.name, &sec.key, "cert.crt"));
            mounts.push(
                json!({ "name": "n8n-db-ssl-cert", "mountPath": "/etc/n8n/ssl/cert", "readOnly": true }),
            );
        }
        if let Some(sec) = &ssl.key_secret {
            vols.push(secret_volume("n8n-db-ssl-key", &sec.name, &sec.key, "key.pem"));
            mounts
                .push(json!({ "name": "n8n-db-ssl-key", "mountPath": "/etc/n8n/ssl/key", "readOnly": true }));
        }
    }
    let _ = instance; // PVC mount handled separately; keeping parameter for symmetry.
    (vols, mounts)
}

/// Build the PVC volume + mount entry attached to a pod that needs persistence.
fn build_persistence_volume(pvc_name: &str) -> (serde_json::Value, serde_json::Value) {
    (
        json!({ "name": "n8n-data", "persistentVolumeClaim": { "claimName": pvc_name } }),
        json!({ "name": "n8n-data", "mountPath": "/home/node/.n8n" }),
    )
}

fn secret_volume(name: &str, secret_name: &str, secret_key: &str, file: &str) -> serde_json::Value {
    json!({
        "name": name,
        "secret": {
            "secretName": secret_name,
            "items": [{ "key": secret_key, "path": file }],
        }
    })
}

fn build_data_pvc(
    pvc_name: &str,
    instance: &str,
    image: &str,
    persistence: Option<&PersistenceConfig>,
    owner: &OwnerReference,
) -> Option<PersistentVolumeClaim> {
    let p = persistence?;
    let labels = common_labels(instance, image, "data");
    let annotations = common_annotations();
    let json = json!({
        "apiVersion": "v1",
        "kind": "PersistentVolumeClaim",
        "metadata": {
            "name": pvc_name,
            "labels": labels,
            "annotations": annotations,
            "ownerReferences": [owner],
        },
        "spec": {
            "accessModes": [p.access_mode],
            "resources": { "requests": { "storage": p.size } },
            "storageClassName": p.storage_class_name,
        }
    });
    Some(serde_json::from_value(json).expect("static pvc schema is valid"))
}

fn build_service(name: &str, spec: &SingleSpec, owner: &OwnerReference) -> Service {
    let selector = selector_labels(name);
    let labels = common_labels(name, &spec.image, "workflow-engine");
    let svc_type = spec
        .service
        .as_ref()
        .map(|s| s.type_.clone())
        .unwrap_or_else(default_service_type);
    Service {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(labels),
            annotations: Some(common_annotations()),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        spec: Some(k8s_openapi::api::core::v1::ServiceSpec {
            selector: Some(selector),
            ports: Some(vec![ServicePort {
                name: Some("http".to_string()),
                port: 5678,
                target_port: Some(IntOrString::String("http".to_string())),
                protocol: Some("TCP".to_string()),
                ..Default::default()
            }]),
            type_: Some(svc_type),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn build_ingress(
    name: &str,
    image: &str,
    host: &str,
    cfg: &IngressConfig,
    owner: &OwnerReference,
) -> Ingress {
    let labels = common_labels(name, image, "ingress");
    let annotations = common_annotations();
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
            "annotations": annotations,
            "ownerReferences": [owner],
        },
        "spec": spec,
    });
    serde_json::from_value(json).expect("static ingress schema is valid")
}

async fn delete_http_route(client: &Client, ns: &str, name: &str) -> Result<()> {
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "HTTPRoute");
    let ar = ApiResource::from_gvk(&gvk);
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), ns, &ar);
    if let Ok(Some(_)) = api.get_opt(name).await {
        api.delete(name, &Default::default())
            .await
            .map_err(Error::KubeError)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn apply_http_route(
    client: &Client,
    ns: &str,
    name: &str,
    image: &str,
    host: &str,
    cfg: &HttpRouteConfig,
    owner: &OwnerReference,
    ps: &PatchParams,
) -> Result<()> {
    let labels = common_labels(name, image, "http-route");
    let annotations = common_annotations();
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
            "annotations": annotations,
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
    let singles = Api::<Single>::all(client.clone());
    if let Err(e) = singles.list(&ListParams::default().limit(1)).await {
        error!("Single CRD is not queryable; {e:?}. Is it installed?");
        info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
        std::process::exit(1);
    }
    let clusters = Api::<Cluster>::all(client.clone());
    if let Err(e) = clusters.list(&ListParams::default().limit(1)).await {
        error!("Cluster CRD is not queryable; {e:?}. Is it installed?");
        info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
        std::process::exit(1);
    }
    let ctx = state.to_context(client).await;
    let single_ctrl = Controller::new(singles, Config::default().any_semantic())
        .shutdown_on_signal()
        .run(reconcile, error_policy, ctx.clone())
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()));
    let cluster_ctrl = Controller::new(clusters, Config::default().any_semantic())
        .shutdown_on_signal()
        .run(reconcile_cluster, cluster_error_policy, ctx)
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()));
    futures::future::join(single_ctrl, cluster_ctrl).await;
}
