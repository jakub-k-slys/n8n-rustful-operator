use crate::{
    Error, Result,
    builders::{cluster_deployment::build_cluster_deployment, hpa::build_worker_hpa},
    env::env_str,
    spec::Cluster,
};
use k8s_openapi::{
    api::{apps::v1::Deployment, autoscaling::v2::HorizontalPodAutoscaler},
    apimachinery::pkg::apis::meta::v1::OwnerReference,
};
use kube::{
    Client,
    api::{Api, Patch, PatchParams},
};
use serde_json::Value;

#[allow(clippy::too_many_arguments)]
pub async fn reconcile_workers(
    c: &Cluster,
    client: &Client,
    ns: &str,
    cluster_name: &str,
    common_env: &[Value],
    common_vols: &[Value],
    common_mounts: &[Value],
    owner: &OwnerReference,
    ps: &PatchParams,
) -> Result<()> {
    let name = format!("{cluster_name}-worker");
    let image = c
        .spec
        .workers
        .image
        .clone()
        .unwrap_or_else(|| c.spec.image.clone());

    let mut env = common_env.to_vec();
    if let Some(cc) = c.spec.workers.concurrency {
        env.push(env_str("N8N_CONCURRENCY_PRODUCTION_LIMIT", cc.to_string()));
    }
    env.push(env_str("QUEUE_HEALTH_CHECK_ACTIVE", "true"));

    let replicas = if c.spec.workers.autoscaling.is_some() {
        None
    } else {
        Some(c.spec.workers.replicas)
    };
    let dep = build_cluster_deployment(
        &name,
        &image,
        "worker",
        replicas,
        &env,
        common_vols,
        common_mounts,
        Some(vec!["n8n".to_string(), "worker".to_string()]),
        owner,
    );
    let deps: Api<Deployment> = Api::namespaced(client.clone(), ns);
    deps.patch(&name, ps, &Patch::Apply(&dep))
        .await
        .map_err(Error::KubeError)?;

    let hpas: Api<HorizontalPodAutoscaler> = Api::namespaced(client.clone(), ns);
    if let Some(as_cfg) = &c.spec.workers.autoscaling {
        let hpa = build_worker_hpa(&name, &image, as_cfg, owner);
        hpas.patch(&name, ps, &Patch::Apply(&hpa))
            .await
            .map_err(Error::KubeError)?;
    } else if hpas.get_opt(&name).await.map_err(Error::KubeError)?.is_some() {
        hpas.delete(&name, &Default::default())
            .await
            .map_err(Error::KubeError)?;
    }
    Ok(())
}
