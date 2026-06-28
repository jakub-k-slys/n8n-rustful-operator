use crate::spec::{
    common::{DeploymentStrategy, EnvVar, PersistenceConfig, ResourceRequirements, ServiceConfig},
    networking::NetworkingSpec,
    pod::PodConfig,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
pub struct MainConfig {
    /// Number of main pods. More than one auto-enables n8n's multi-main HA setup
    /// (`N8N_MULTI_MAIN_SETUP_ENABLED` + `ClientIP` session affinity on the main
    /// Service) so the at-most-once tasks (timers/pollers/pruning) aren't
    /// duplicated. Multi-main is an n8n Enterprise feature and needs a license.
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
    /// PVC at `/home/node/.n8n` for the main pod only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistence: Option<PersistenceConfig>,
    /// Extra env for the main role; overrides `spec.extraEnv` by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "extraEnv")]
    pub extra_env: Vec<EnvVar>,
    /// Container CPU/memory requests and limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,
    /// Pod-level scheduling and metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod: Option<PodConfig>,
    /// Deployment update strategy (e.g. `Recreate` for an RWO PVC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<DeploymentStrategy>,
    /// `N8N_MULTI_MAIN_SETUP_KEY_TTL` — leader key TTL in seconds. Only takes
    /// effect when multi-main is active (i.e. `replicas` > 1).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "multiMainKeyTtl")]
    pub multi_main_key_ttl: Option<u32>,
    /// `N8N_MULTI_MAIN_SETUP_CHECK_INTERVAL` — leader check interval in seconds.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "multiMainCheckInterval"
    )]
    pub multi_main_check_interval: Option<u32>,
}

fn default_main_replicas() -> i32 {
    1
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
pub struct WorkerConfig {
    /// Static replica count. Ignored when `autoscaling` is set.
    pub replicas: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Maps to `N8N_CONCURRENCY_PRODUCTION_LIMIT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u32>,
    /// HPA opts the worker Deployment into horizontal autoscaling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autoscaling: Option<Autoscaling>,
    /// Extra env for the worker role; overrides `spec.extraEnv` by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "extraEnv")]
    pub extra_env: Vec<EnvVar>,
    /// Container CPU/memory requests and limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,
    /// Pod-level scheduling and metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod: Option<PodConfig>,
    /// Deployment update strategy (e.g. `Recreate` for an RWO PVC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<DeploymentStrategy>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct Autoscaling {
    #[serde(rename = "minReplicas")]
    pub min_replicas: i32,
    #[serde(rename = "maxReplicas")]
    pub max_replicas: i32,
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
    /// Extra env for the webhook role; overrides `spec.extraEnv` by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "extraEnv")]
    pub extra_env: Vec<EnvVar>,
    /// Container CPU/memory requests and limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,
    /// Pod-level scheduling and metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod: Option<PodConfig>,
    /// Deployment update strategy (e.g. `Recreate` for an RWO PVC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<DeploymentStrategy>,
}
