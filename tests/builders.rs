//! Coverage for the pure object-builder helpers in `builders` (resources +
//! pod scheduling/metadata). No cluster needed.

use n8n_rustful_operator::builders::{apply_pod_config, deployment_strategy, resources};
use n8n_rustful_operator::{DeploymentStrategy, PodConfig, ResourceList, ResourceRequirements};
use serde_json::json;

#[test]
fn strategy_recreate_is_bare() {
    let s = DeploymentStrategy {
        type_: "Recreate".into(),
        max_surge: None,
        max_unavailable: None,
    };
    assert_eq!(deployment_strategy(&s), json!({ "type": "Recreate" }));
}

#[test]
fn strategy_rolling_update_numeric_tuning_is_integer() {
    // Bare numbers must render as JSON integers (absolute counts); the apiserver
    // rejects a string intstr like "1" for maxSurge/maxUnavailable.
    let s = DeploymentStrategy {
        type_: "RollingUpdate".into(),
        max_surge: Some("1".into()),
        max_unavailable: Some("0".into()),
    };
    assert_eq!(
        deployment_strategy(&s),
        json!({ "type": "RollingUpdate", "rollingUpdate": { "maxSurge": 1, "maxUnavailable": 0 } })
    );
}

#[test]
fn strategy_rolling_update_percent_stays_string() {
    let s = DeploymentStrategy {
        type_: "RollingUpdate".into(),
        max_surge: Some("0%".into()),
        max_unavailable: Some("100%".into()),
    };
    assert_eq!(
        deployment_strategy(&s),
        json!({ "type": "RollingUpdate", "rollingUpdate": { "maxSurge": "0%", "maxUnavailable": "100%" } })
    );
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

#[test]
fn pod_config_applies_all_scheduling_and_merges_metadata() {
    let pc = PodConfig {
        service_account_name: Some("n8n-sa".into()),
        node_selector: Some([("disktype".to_string(), "ssd".to_string())].into()),
        tolerations: Some(json!([{ "key": "dedicated", "operator": "Exists" }])),
        affinity: Some(json!({
            "nodeAffinity": { "requiredDuringSchedulingIgnoredDuringExecution": { "nodeSelectorTerms": [] } }
        })),
        security_context: Some(json!({ "fsGroup": 1000, "fsGroupChangePolicy": "OnRootMismatch" })),
        pod_labels: Some([("team".to_string(), "ops".to_string())].into()),
        pod_annotations: Some([("sidecar.istio.io/inject".to_string(), "true".to_string())].into()),
    };
    let mut template = json!({
        "metadata": { "labels": { "app.kubernetes.io/name": "x" } },
        "spec": { "containers": [] }
    });
    apply_pod_config(&mut template, &pc);

    // scheduling onto template.spec
    assert_eq!(template["spec"]["serviceAccountName"], json!("n8n-sa"));
    assert_eq!(template["spec"]["nodeSelector"], json!({ "disktype": "ssd" }));
    assert_eq!(template["spec"]["tolerations"][0]["key"], json!("dedicated"));
    assert!(template["spec"]["affinity"]["nodeAffinity"].is_object());
    assert_eq!(template["spec"]["securityContext"]["fsGroup"], json!(1000));

    // labels: existing preserved, new merged; annotations created and merged
    assert_eq!(
        template["metadata"]["labels"]["app.kubernetes.io/name"],
        json!("x")
    );
    assert_eq!(template["metadata"]["labels"]["team"], json!("ops"));
    assert_eq!(
        template["metadata"]["annotations"]["sidecar.istio.io/inject"],
        json!("true")
    );
}

#[test]
fn pod_config_is_a_no_op_for_unset_fields() {
    let pc = PodConfig::default();
    let mut template = json!({ "metadata": { "labels": {} }, "spec": { "containers": [] } });
    apply_pod_config(&mut template, &pc);
    assert!(template["spec"].get("serviceAccountName").is_none());
    assert!(template["spec"].get("nodeSelector").is_none());
    assert!(template["spec"].get("tolerations").is_none());
    assert!(template["spec"].get("affinity").is_none());
}
