pub mod cluster_deployment;
pub mod deployment;
pub mod hpa;
pub mod http_route;
pub mod ingress;
pub mod pvc;
pub mod service;
pub mod volumes;

use crate::spec::{ResourceList, ResourceRequirements};
use serde_json::{Map, Value, json};

/// Render pod `imagePullSecrets` (a list of Secret names) as the
/// `[]LocalObjectReference` JSON Kubernetes expects.
pub fn image_pull_secrets(names: &[String]) -> Vec<Value> {
    names.iter().map(|n| json!({ "name": n })).collect()
}

/// Render container `resources` JSON from the CPU/memory subset, omitting any
/// quantity that wasn't set.
pub fn resources(r: &ResourceRequirements) -> Value {
    let list = |l: &ResourceList| {
        let mut m = Map::new();
        if let Some(c) = &l.cpu {
            m.insert("cpu".into(), json!(c));
        }
        if let Some(mem) = &l.memory {
            m.insert("memory".into(), json!(mem));
        }
        Value::Object(m)
    };
    let mut out = Map::new();
    if let Some(l) = &r.limits {
        out.insert("limits".into(), list(l));
    }
    if let Some(req) = &r.requests {
        out.insert("requests".into(), list(req));
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resources_omits_unset_quantities() {
        let r = ResourceRequirements {
            limits: Some(ResourceList {
                cpu: Some("1".into()),
                memory: Some("1Gi".into()),
            }),
            requests: Some(ResourceList {
                cpu: Some("200m".into()),
                memory: None,
            }),
        };
        assert_eq!(
            resources(&r),
            json!({
                "limits": { "cpu": "1", "memory": "1Gi" },
                "requests": { "cpu": "200m" }
            })
        );
    }
}
