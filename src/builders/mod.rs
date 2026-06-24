pub mod cluster_deployment;
pub mod deployment;
pub mod hpa;
pub mod http_route;
pub mod ingress;
pub mod pvc;
pub mod service;
pub mod volumes;

use serde_json::{Value, json};

/// Render pod `imagePullSecrets` (a list of Secret names) as the
/// `[]LocalObjectReference` JSON Kubernetes expects.
pub fn image_pull_secrets(names: &[String]) -> Vec<Value> {
    names.iter().map(|n| json!({ "name": n })).collect()
}
