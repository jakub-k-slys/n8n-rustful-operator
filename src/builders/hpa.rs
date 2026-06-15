use crate::{
    labels::{common_annotations, common_labels},
    spec::Autoscaling,
};
use k8s_openapi::{
    api::autoscaling::v2::HorizontalPodAutoscaler, apimachinery::pkg::apis::meta::v1::OwnerReference,
};
use serde_json::json;

pub fn build_worker_hpa(
    name: &str,
    image: &str,
    autoscaling: &Autoscaling,
    owner: &OwnerReference,
) -> HorizontalPodAutoscaler {
    let cpu = autoscaling.target_cpu_utilization_percentage.unwrap_or(70);
    let json = json!({
        "apiVersion": "autoscaling/v2",
        "kind": "HorizontalPodAutoscaler",
        "metadata": {
            "name": name,
            "labels": common_labels(name, image, "worker"),
            "annotations": common_annotations(),
            "ownerReferences": [owner],
        },
        "spec": {
            "scaleTargetRef": {
                "apiVersion": "apps/v1",
                "kind": "Deployment",
                "name": name,
            },
            "minReplicas": autoscaling.min_replicas,
            "maxReplicas": autoscaling.max_replicas,
            "metrics": [{
                "type": "Resource",
                "resource": {
                    "name": "cpu",
                    "target": { "type": "Utilization", "averageUtilization": cpu }
                }
            }]
        }
    });
    serde_json::from_value(json).expect("static HPA schema is valid")
}
