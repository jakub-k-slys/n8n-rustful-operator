use crate::spec::{
    common::{EnvVar, PersistenceConfig, ResourceRequirements, ServiceConfig},
    networking::NetworkingSpec,
    pod::PodConfig,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
}
