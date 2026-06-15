use crate::labels::{common_annotations, common_labels, selector_labels};
use k8s_openapi::{api::apps::v1::Deployment, apimachinery::pkg::apis::meta::v1::OwnerReference};
use serde_json::{Value, json};

#[allow(clippy::too_many_arguments)]
pub fn build_cluster_deployment(
    name: &str,
    image: &str,
    component: &str,
    replicas: Option<i32>,
    env: &[Value],
    volumes: &[Value],
    mounts: &[Value],
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
