use crate::builders::{
    INSTANCE_AI_REVISION, apply_pod_config, deployment_strategy, image_pull_secrets, instance_ai_env_from,
    resources,
};
use crate::labels::{common_annotations, common_labels, selector_labels};
use crate::spec::{DeploymentStrategy, PodConfig, ResourceRequirements};
use k8s_openapi::{api::apps::v1::Deployment, apimachinery::pkg::apis::meta::v1::OwnerReference};
use serde_json::{Value, json};

/// The instance-ai Secret's owning CR and its current `resourceVersion`.
/// `cr_name` is the CR's own name — e.g. the `Cluster`'s name, NOT a
/// role-suffixed Deployment name like `<cluster>-main` — because the
/// Assistant controller creates the Secret as `<cr_name>-instance-ai`.
/// Bundled together so the correct name can't be reconstructed (or
/// mis-reconstructed) from `DeploymentInputs::name` by mistake.
pub struct InstanceAi<'a> {
    pub cr_name: &'a str,
    pub revision: &'a str,
}

/// Everything `build_cluster_deployment` needs about a single Deployment.
pub struct DeploymentInputs<'a> {
    pub name: &'a str,
    pub image: &'a str,
    pub component: &'a str,
    /// `None` means "don't manage spec.replicas" (e.g. HPA owns the field).
    pub replicas: Option<i32>,
    pub env: &'a [Value],
    pub volumes: &'a [Value],
    pub mounts: &'a [Value],
    pub command: Option<Vec<String>>,
    /// Secret names for pulling the image from a private registry.
    pub image_pull_secrets: &'a [String],
    /// Container CPU/memory requests and limits, if set for this role.
    pub resources: Option<&'a ResourceRequirements>,
    /// Pod-level scheduling and metadata, if set for this role.
    pub pod: Option<&'a PodConfig>,
    /// Deployment update strategy, if set for this role.
    pub strategy: Option<&'a DeploymentStrategy>,
    /// `Some` pulls the `<cr_name>-instance-ai` Secret in via `envFrom` and
    /// stamps a revision annotation so a Secret change rolls the pods.
    /// `None` means this role gets neither — the Assistant is editor-side
    /// and its module belongs on the main role only.
    pub instance_ai: Option<InstanceAi<'a>>,
}

pub fn build_cluster_deployment(input: &DeploymentInputs<'_>, owner: &OwnerReference) -> Deployment {
    let labels = common_labels(input.name, input.image, input.component);
    let annotations = common_annotations();
    let mut container = json!({
        "name": "n8n",
        "image": input.image,
        "ports": [{ "containerPort": 5678, "name": "http" }],
        "env": input.env,
        "volumeMounts": input.mounts,
        "readinessProbe": {
            "httpGet": { "path": "/healthz", "port": "http" },
            "initialDelaySeconds": 10,
            "periodSeconds": 10
        }
    });
    if let Some(cmd) = &input.command {
        container["command"] = json!(cmd);
    }
    if let Some(r) = input.resources {
        container["resources"] = resources(r);
    }
    let pod_annotations = match &input.instance_ai {
        Some(ai) => {
            container["envFrom"] = instance_ai_env_from(ai.cr_name);
            let mut m = annotations.clone();
            m.insert(INSTANCE_AI_REVISION.to_string(), ai.revision.to_string());
            m
        }
        None => annotations.clone(),
    };
    let mut pod_spec = json!({ "volumes": input.volumes, "containers": [container] });
    if !input.image_pull_secrets.is_empty() {
        pod_spec["imagePullSecrets"] = json!(image_pull_secrets(input.image_pull_secrets));
    }
    let mut spec = json!({
        "selector": { "matchLabels": selector_labels(input.name) },
        "template": {
            "metadata": { "labels": labels, "annotations": pod_annotations },
            "spec": pod_spec,
        }
    });
    if let Some(r) = input.replicas {
        spec["replicas"] = json!(r);
    }
    if let Some(pc) = input.pod {
        apply_pod_config(&mut spec["template"], pc);
    }
    if let Some(st) = input.strategy {
        spec["strategy"] = deployment_strategy(st);
    }
    serde_json::from_value(json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": input.name,
            "labels": labels,
            "annotations": annotations,
            "ownerReferences": [owner],
        },
        "spec": spec,
    }))
    .expect("static cluster deployment schema is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> OwnerReference {
        OwnerReference {
            api_version: "n8n.slys.dev/v1".into(),
            kind: "Cluster".into(),
            name: "demo".into(),
            uid: "u".into(),
            controller: Some(true),
            block_owner_deletion: Some(true),
        }
    }

    fn base_input<'a>(name: &'a str) -> DeploymentInputs<'a> {
        DeploymentInputs {
            name,
            image: "n8nio/n8n:latest",
            component: "main",
            replicas: Some(1),
            env: &[],
            volumes: &[],
            mounts: &[],
            command: None,
            image_pull_secrets: &[],
            resources: None,
            pod: None,
            strategy: None,
            instance_ai: None,
        }
    }

    #[test]
    fn main_role_pulls_the_instance_ai_secret_named_after_the_cluster_cr_not_the_deployment() {
        let mut input = base_input("demo-main");
        input.instance_ai = Some(InstanceAi {
            cr_name: "demo",
            revision: "1234",
        });
        let d = build_cluster_deployment(&input, &owner());
        let v = serde_json::to_value(&d).unwrap();
        let ef = &v["spec"]["template"]["spec"]["containers"][0]["envFrom"][0];
        assert_eq!(ef["secretRef"]["name"], "demo-instance-ai");
        assert_eq!(ef["secretRef"]["optional"], true);
        assert_eq!(
            v["spec"]["template"]["metadata"]["annotations"]["n8n.slys.dev/instance-ai-revision"],
            "1234"
        );
    }

    #[test]
    fn worker_and_webhook_roles_get_no_env_from_and_no_revision_annotation() {
        for name in ["demo-worker", "demo-webhook"] {
            let input = base_input(name);
            let d = build_cluster_deployment(&input, &owner());
            let v = serde_json::to_value(&d).unwrap();
            assert!(
                v["spec"]["template"]["spec"]["containers"][0]
                    .get("envFrom")
                    .is_none(),
                "role {name} must not get envFrom"
            );
            assert!(
                v["spec"]["template"]["metadata"]["annotations"]
                    .get("n8n.slys.dev/instance-ai-revision")
                    .is_none(),
                "role {name} must not get the revision annotation"
            );
        }
    }
}
