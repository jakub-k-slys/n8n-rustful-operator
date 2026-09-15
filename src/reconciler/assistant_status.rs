use crate::{
    Error, Result,
    spec::{Assistant, AssistantStatus},
};
use k8s_openapi::api::apps::v1::Deployment;
use kube::{
    Client,
    api::{Api, Patch, PatchParams},
};
use serde_json::json;

pub fn is_ready(d: Option<&Deployment>) -> bool {
    d.and_then(|d| d.status.as_ref())
        .and_then(|s| s.available_replicas)
        .unwrap_or(0)
        > 0
}

/// `ReplicaFailure` is where the apiserver reports a pod it refused to admit —
/// most often Pod Security rejecting the privileged runner. Surfacing it beats
/// leaving an empty Deployment with no explanation.
pub fn failure_message(d: Option<&Deployment>) -> Option<String> {
    d?.status
        .as_ref()?
        .conditions
        .as_ref()?
        .iter()
        .find(|c| c.type_ == "ReplicaFailure" && c.status == "True")
        .and_then(|c| c.message.clone())
}

pub async fn patch_status(
    client: &Client,
    ns: &str,
    name: &str,
    status: AssistantStatus,
    ps: &PatchParams,
) -> Result<()> {
    Api::<Assistant>::namespaced(client.clone(), ns)
        .patch_status(
            name,
            ps,
            &Patch::Apply(json!({
                "apiVersion": "n8n.slys.dev/v1",
                "kind": "Assistant",
                "status": status,
            })),
        )
        .await
        .map_err(Error::KubeError)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::{Deployment, DeploymentCondition, DeploymentStatus};

    fn dep(available: i32, cond: Option<(&str, &str, &str)>) -> Deployment {
        Deployment {
            status: Some(DeploymentStatus {
                available_replicas: Some(available),
                conditions: cond.map(|(t, s, m)| {
                    vec![DeploymentCondition {
                        type_: t.to_string(),
                        status: s.to_string(),
                        message: Some(m.to_string()),
                        ..Default::default()
                    }]
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_deployment_with_an_available_replica_is_ready() {
        assert!(is_ready(Some(&dep(1, None))));
    }

    #[test]
    fn a_missing_deployment_is_not_ready() {
        assert!(!is_ready(None));
        assert!(!is_ready(Some(&dep(0, None))));
    }

    #[test]
    fn replica_failure_surfaces_as_the_status_message() {
        let d = dep(
            0,
            Some((
                "ReplicaFailure",
                "True",
                "pods \"x\" is forbidden: violates PodSecurity \"restricted:latest\": privileged",
            )),
        );
        let msg = failure_message(Some(&d)).unwrap();
        assert!(msg.contains("PodSecurity"));
    }

    #[test]
    fn a_healthy_deployment_has_no_failure_message() {
        assert!(failure_message(Some(&dep(1, None))).is_none());
        assert!(failure_message(Some(&dep(0, Some(("Available", "False", "scaling"))))).is_none());
    }
}
