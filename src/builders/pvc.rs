use crate::{
    labels::{common_annotations, common_labels},
    spec::PersistenceConfig,
};
use k8s_openapi::{api::core::v1::PersistentVolumeClaim, apimachinery::pkg::apis::meta::v1::OwnerReference};
use serde_json::{Value, json};

pub fn build_persistence_volume(pvc_name: &str) -> (Value, Value) {
    (
        json!({ "name": "n8n-data", "persistentVolumeClaim": { "claimName": pvc_name } }),
        json!({ "name": "n8n-data", "mountPath": "/home/node/.n8n" }),
    )
}

pub fn secret_volume(name: &str, secret_name: &str, secret_key: &str, file: &str) -> Value {
    json!({
        "name": name,
        "secret": {
            "secretName": secret_name,
            "items": [{ "key": secret_key, "path": file }],
        }
    })
}

pub fn build_data_pvc(
    pvc_name: &str,
    instance: &str,
    image: &str,
    persistence: Option<&PersistenceConfig>,
    owner: &OwnerReference,
) -> Option<PersistentVolumeClaim> {
    let p = persistence?;
    let json = json!({
        "apiVersion": "v1",
        "kind": "PersistentVolumeClaim",
        "metadata": {
            "name": pvc_name,
            "labels": common_labels(instance, image, "data"),
            "annotations": common_annotations(),
            "ownerReferences": [owner],
        },
        "spec": {
            "accessModes": [p.access_mode],
            "resources": { "requests": { "storage": p.size } },
            "storageClassName": p.storage_class_name,
        }
    });
    Some(serde_json::from_value(json).expect("static pvc schema is valid"))
}
