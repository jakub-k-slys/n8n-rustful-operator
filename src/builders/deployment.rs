use crate::{
    builders::{pvc::build_persistence_volume, volumes::build_db_volumes},
    env::{build_user_env, database::build_db_env},
    labels::{common_annotations, common_labels, selector_labels},
    spec::{SecretKeyRef, SingleSpec},
};
use k8s_openapi::{api::apps::v1::Deployment, apimachinery::pkg::apis::meta::v1::OwnerReference};
use serde_json::{Value, json};

pub fn build_deployment(
    name: &str,
    spec: &SingleSpec,
    key_secret: &SecretKeyRef,
    owner: &OwnerReference,
) -> Deployment {
    let selector = selector_labels(name);
    let labels = common_labels(name, &spec.image, "workflow-engine");
    let annotations = common_annotations();
    let mut env = vec![json!({
        "name": "N8N_ENCRYPTION_KEY",
        "valueFrom": { "secretKeyRef": { "name": key_secret.name, "key": key_secret.key } }
    })];
    let mut volumes: Vec<Value> = Vec::new();
    let mut mounts: Vec<Value> = Vec::new();

    if let Some(db) = &spec.database {
        env.extend(build_db_env(db));
        let (vols, vm) = build_db_volumes(name, db);
        volumes.extend(vols);
        mounts.extend(vm);
    }
    if spec.persistence.is_some() {
        let (v, m) = build_persistence_volume(&format!("{name}-data"));
        volumes.push(v);
        mounts.push(m);
    }
    env.extend(build_user_env(spec.secure_cookie, &spec.extra_env, &[]));

    let dep_json = json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": name,
            "labels": labels,
            "annotations": annotations,
            "ownerReferences": [owner],
        },
        "spec": {
            "replicas": spec.replicas,
            "selector": { "matchLabels": selector },
            "template": {
                "metadata": { "labels": labels, "annotations": annotations },
                "spec": {
                    "volumes": volumes,
                    "containers": [{
                        "name": "n8n",
                        "image": spec.image,
                        "ports": [{ "containerPort": 5678, "name": "http" }],
                        "env": env,
                        "volumeMounts": mounts,
                        "readinessProbe": {
                            "httpGet": { "path": "/healthz", "port": "http" },
                            "initialDelaySeconds": 10,
                            "periodSeconds": 10
                        }
                    }]
                }
            }
        }
    });
    serde_json::from_value(dep_json).expect("static deployment schema is valid")
}
