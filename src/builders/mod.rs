pub mod cluster_deployment;
pub mod deployment;
pub mod hpa;
pub mod http_route;
pub mod ingress;
pub mod pvc;
pub mod service;
pub mod volumes;

use crate::spec::{PodConfig, ResourceList, ResourceRequirements};
use serde_json::{Map, Value, json};

/// Apply a `PodConfig` to a Deployment's pod `template` (the object with
/// `metadata` and `spec`): scheduling onto `template.spec`, extra labels and
/// annotations merged into `template.metadata`. A no-op for unset fields.
pub fn apply_pod_config(template: &mut Value, pc: &PodConfig) {
    let spec = &mut template["spec"];
    if let Some(sa) = &pc.service_account_name {
        spec["serviceAccountName"] = json!(sa);
    }
    if let Some(ns) = &pc.node_selector {
        spec["nodeSelector"] = json!(ns);
    }
    if let Some(t) = &pc.tolerations {
        spec["tolerations"] = t.clone();
    }
    if let Some(a) = &pc.affinity {
        spec["affinity"] = a.clone();
    }
    if let Some(labels) = &pc.pod_labels {
        merge_string_map(&mut template["metadata"]["labels"], labels);
    }
    if let Some(ann) = &pc.pod_annotations {
        merge_string_map(&mut template["metadata"]["annotations"], ann);
    }
}

fn merge_string_map(target: &mut Value, extra: &std::collections::BTreeMap<String, String>) {
    if !target.is_object() {
        *target = json!({});
    }
    let obj = target.as_object_mut().expect("metadata map is an object");
    for (k, v) in extra {
        obj.insert(k.clone(), json!(v));
    }
}

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
    fn pod_config_applies_scheduling_and_merges_metadata() {
        let pc = PodConfig {
            service_account_name: Some("n8n-sa".into()),
            node_selector: Some([("disktype".to_string(), "ssd".to_string())].into()),
            tolerations: Some(json!([{ "key": "dedicated", "operator": "Exists" }])),
            affinity: None,
            pod_labels: Some([("team".to_string(), "ops".to_string())].into()),
            pod_annotations: None,
        };
        let mut template = json!({
            "metadata": { "labels": { "app.kubernetes.io/name": "x" } },
            "spec": { "containers": [] }
        });
        apply_pod_config(&mut template, &pc);
        assert_eq!(template["spec"]["serviceAccountName"], json!("n8n-sa"));
        assert_eq!(template["spec"]["nodeSelector"], json!({ "disktype": "ssd" }));
        assert_eq!(template["spec"]["tolerations"][0]["key"], json!("dedicated"));
        // existing labels are preserved, new ones merged in
        assert_eq!(
            template["metadata"]["labels"]["app.kubernetes.io/name"],
            json!("x")
        );
        assert_eq!(template["metadata"]["labels"]["team"], json!("ops"));
    }

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
