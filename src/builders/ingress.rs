use crate::{
    labels::{common_annotations, common_labels},
    spec::IngressConfig,
};
use k8s_openapi::{api::networking::v1::Ingress, apimachinery::pkg::apis::meta::v1::OwnerReference};
use serde_json::json;

pub fn build_ingress(
    name: &str,
    image: &str,
    host: &str,
    cfg: &IngressConfig,
    owner: &OwnerReference,
) -> Ingress {
    let mut spec = json!({
        "ingressClassName": cfg.class_name,
        "rules": [{
            "host": host,
            "http": {
                "paths": [{
                    "path": "/",
                    "pathType": "Prefix",
                    "backend": { "service": { "name": name, "port": { "number": 5678 } } }
                }]
            }
        }]
    });
    if let Some(tls) = &cfg.tls_secret_name {
        spec["tls"] = json!([{ "hosts": [host], "secretName": tls }]);
    }
    let json = json!({
        "apiVersion": "networking.k8s.io/v1",
        "kind": "Ingress",
        "metadata": {
            "name": name,
            "labels": common_labels(name, image, "ingress"),
            "annotations": common_annotations(),
            "ownerReferences": [owner],
        },
        "spec": spec,
    });
    serde_json::from_value(json).expect("static ingress schema is valid")
}
