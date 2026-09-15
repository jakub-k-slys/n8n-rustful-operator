use crate::builders::{
    INSTANCE_AI_REVISION, apply_pod_config, deployment_strategy, image_pull_secrets, instance_ai_env_from,
    resources,
};
use crate::labels::{common_annotations, common_labels, selector_labels};
use crate::spec::{DeploymentStrategy, PodConfig, ResourceRequirements};
use k8s_openapi::{api::apps::v1::Deployment, apimachinery::pkg::apis::meta::v1::OwnerReference};
use serde_json::{Value, json};

/// Everything `build_cluster_deployment` needs about a single Deployment.
pub struct DeploymentInputs<'a> {
    pub name: &'a str,
    pub image: &'a str,
    pub component: &'a str,
    /// `None` means "don't manage spec.replicas" (e.g. HPA owns the field).
    pub replicas: Option<i32>,
    pub env: &'a [Value],
    pub volumes: &'a [Value],
    pub mounts: &'a [Value],
    pub command: Option<Vec<String>>,
    /// Secret names for pulling the image from a private registry.
    pub image_pull_secrets: &'a [String],
    /// Container CPU/memory requests and limits, if set for this role.
    pub resources: Option<&'a ResourceRequirements>,
    /// Pod-level scheduling and metadata, if set for this role.
    pub pod: Option<&'a PodConfig>,
    /// Deployment update strategy, if set for this role.
    pub strategy: Option<&'a DeploymentStrategy>,
    /// `Some(resource_version)` of the `<name>-instance-ai` Secret pulls it
    /// in via `envFrom` and stamps a revision annotation so a Secret change
    /// rolls the pods. `None` means this role gets neither — the Assistant
    /// is editor-side and its module belongs on the main role only.
    pub instance_ai_revision: Option<&'a str>,
}

pub fn build_cluster_deployment(input: &DeploymentInputs<'_>, owner: &OwnerReference) -> Deployment {
    let labels = common_labels(input.name, input.image, input.component);
    let annotations = common_annotations();
    let mut container = json!({
        "name": "n8n",
        "image": input.image,
        "ports": [{ "containerPort": 5678, "name": "http" }],
        "env": input.env,
        "volumeMounts": input.mounts,
        "readinessProbe": {
            "httpGet": { "path": "/healthz", "port": "http" },
            "initialDelaySeconds": 10,
            "periodSeconds": 10
        }
    });
    if let Some(cmd) = &input.command {
        container["command"] = json!(cmd);
    }
    if let Some(r) = input.resources {
        container["resources"] = resources(r);
    }
    let mut pod_annotations = annotations.clone();
    if let Some(rev) = input.instance_ai_revision {
        container["envFrom"] = instance_ai_env_from(input.name);
        pod_annotations.insert(INSTANCE_AI_REVISION.to_string(), rev.to_string());
    }
    let mut pod_spec = json!({ "volumes": input.volumes, "containers": [container] });
    if !input.image_pull_secrets.is_empty() {
        pod_spec["imagePullSecrets"] = json!(image_pull_secrets(input.image_pull_secrets));
    }
    let mut spec = json!({
        "selector": { "matchLabels": selector_labels(input.name) },
        "template": {
            "metadata": { "labels": labels, "annotations": pod_annotations },
            "spec": pod_spec,
        }
    });
    if let Some(r) = input.replicas {
        spec["replicas"] = json!(r);
    }
    if let Some(pc) = input.pod {
        apply_pod_config(&mut spec["template"], pc);
    }
    if let Some(st) = input.strategy {
        spec["strategy"] = deployment_strategy(st);
    }
    serde_json::from_value(json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": input.name,
            "labels": labels,
            "annotations": annotations,
            "ownerReferences": [owner],
        },
        "spec": spec,
    }))
    .expect("static cluster deployment schema is valid")
}
