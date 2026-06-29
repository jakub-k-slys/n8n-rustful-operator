pub mod cluster_deployment;
pub mod deployment;
pub mod destination_rule;
pub mod hpa;
pub mod http_route;
pub mod ingress;
pub mod pvc;
pub mod service;
pub mod volumes;

use crate::spec::{DeploymentStrategy, PodConfig, ResourceList, ResourceRequirements};
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
    if let Some(sc) = &pc.security_context {
        spec["securityContext"] = sc.clone();
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

/// Render a k8s `IntOrString` from the CRD's string field: a purely-numeric
/// value (e.g. `"1"`) becomes a JSON number (an absolute count), anything else
/// (e.g. `"25%"`) stays a string. The apiserver rejects a bare-integer string
/// like `"1"` for `maxSurge`/`maxUnavailable` — a string intstr must be a
/// percent — so the numeric form must be emitted as a number.
fn int_or_string(v: &str) -> Value {
    match v.parse::<i64>() {
        Ok(n) => json!(n),
        Err(_) => json!(v),
    }
}

/// Render a Deployment `spec.strategy`. `Recreate` is emitted bare; for
/// `RollingUpdate` the optional `maxSurge`/`maxUnavailable` go under
/// `rollingUpdate`.
pub fn deployment_strategy(s: &DeploymentStrategy) -> Value {
    let mut out = Map::new();
    out.insert("type".into(), json!(s.type_));
    if s.type_ == "RollingUpdate" {
        let mut ru = Map::new();
        if let Some(ms) = &s.max_surge {
            ru.insert("maxSurge".into(), int_or_string(ms));
        }
        if let Some(mu) = &s.max_unavailable {
            ru.insert("maxUnavailable".into(), int_or_string(mu));
        }
        if !ru.is_empty() {
            out.insert("rollingUpdate".into(), Value::Object(ru));
        }
    }
    Value::Object(out)
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
