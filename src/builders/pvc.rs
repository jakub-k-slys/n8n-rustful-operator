use crate::{
    labels::{common_annotations, common_labels},
    spec::{PersistenceConfig, SharedStorage},
};
use k8s_openapi::{api::core::v1::PersistentVolumeClaim, apimachinery::pkg::apis::meta::v1::OwnerReference};
use serde_json::{Value, json};

pub fn build_persistence_volume(pvc_name: &str) -> (Value, Value) {
    (
        json!({ "name": "n8n-data", "persistentVolumeClaim": { "claimName": pvc_name } }),
        json!({ "name": "n8n-data", "mountPath": "/home/node/.n8n" }),
    )
}

/// Volume + mount for the shared community-nodes PVC, mounted at the n8n nodes
/// directory on every role so a UI install propagates.
pub fn build_nodes_volume(pvc_name: &str) -> (Value, Value) {
    (
        json!({ "name": "n8n-nodes", "persistentVolumeClaim": { "claimName": pvc_name } }),
        json!({ "name": "n8n-nodes", "mountPath": "/home/node/.n8n/nodes" }),
    )
}

/// Mount path for the shared binary-data volume; also the value set for
/// `N8N_STORAGE_PATH` so n8n writes binary data there. Kept outside `~/.n8n`
/// to avoid nesting under a role's own persistence mount.
pub const BINARY_DATA_STORAGE_PATH: &str = "/home/node/binary-data";

/// Volume + mount for the shared binary-data PVC (`mode: filesystem`), mounted
/// at [`BINARY_DATA_STORAGE_PATH`] on every role.
pub fn build_binary_data_volume(pvc_name: &str) -> (Value, Value) {
    (
        json!({ "name": "n8n-binary-data", "persistentVolumeClaim": { "claimName": pvc_name } }),
        json!({ "name": "n8n-binary-data", "mountPath": BINARY_DATA_STORAGE_PATH }),
    )
}

/// A shared (typically ReadWriteMany) PVC, e.g. for community nodes across roles.
pub fn build_shared_pvc(
    pvc_name: &str,
    instance: &str,
    image: &str,
    storage: &SharedStorage,
    owner: &OwnerReference,
) -> PersistentVolumeClaim {
    let json = json!({
        "apiVersion": "v1",
        "kind": "PersistentVolumeClaim",
        "metadata": {
            "name": pvc_name,
            "labels": common_labels(instance, image, "nodes"),
            "annotations": common_annotations(),
            "ownerReferences": [owner],
        },
        "spec": {
            "accessModes": [storage.access_mode],
            "resources": { "requests": { "storage": storage.size } },
            "storageClassName": storage.storage_class_name,
        }
    });
    serde_json::from_value(json).expect("static shared pvc schema is valid")
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
