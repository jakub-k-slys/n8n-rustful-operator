use crate::builders::image_pull_secrets;
use crate::labels::{common_annotations, common_labels, selector_labels};
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
    let mut pod_spec = json!({ "volumes": input.volumes, "containers": [container] });
    if !input.image_pull_secrets.is_empty() {
        pod_spec["imagePullSecrets"] = json!(image_pull_secrets(input.image_pull_secrets));
    }
    let mut spec = json!({
        "selector": { "matchLabels": selector_labels(input.name) },
        "template": {
            "metadata": { "labels": labels, "annotations": annotations },
            "spec": pod_spec,
        }
    });
    if let Some(r) = input.replicas {
        spec["replicas"] = json!(r);
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
