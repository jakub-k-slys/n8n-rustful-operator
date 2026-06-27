use crate::{
    Error, Result,
    builders::{
        cluster_deployment::{DeploymentInputs, build_cluster_deployment},
        hpa::build_worker_hpa,
    },
    env::{build_user_env, cluster_role_defaults, env_str},
    reconciler::ctx::{ApplyCtx, Bundle},
    spec::Cluster,
};
use k8s_openapi::api::{apps::v1::Deployment, autoscaling::v2::HorizontalPodAutoscaler};
use kube::api::Patch;

pub async fn reconcile_workers(
    c: &Cluster,
    cluster_name: &str,
    ctx: &ApplyCtx<'_>,
    bundle: &Bundle,
) -> Result<()> {
    let name = format!("{cluster_name}-worker");
    let image = c
        .spec
        .workers
        .image
        .clone()
        .unwrap_or_else(|| c.spec.image.clone());
    let defaults = cluster_role_defaults(c, None, None);
    let env = [
        bundle.env.clone(),
        c.spec
            .workers
            .concurrency
            .map(|cc| env_str("N8N_CONCURRENCY_PRODUCTION_LIMIT", cc.to_string()))
            .into_iter()
            .collect(),
        vec![env_str("QUEUE_HEALTH_CHECK_ACTIVE", "true")],
        build_user_env(
            &defaults,
            c.spec.secure_cookie,
            &c.spec.extra_env,
            &c.spec.workers.extra_env,
        ),
    ]
    .concat();
    let replicas = if c.spec.workers.autoscaling.is_some() {
        None
    } else {
        Some(c.spec.workers.replicas)
    };
    let dep = build_cluster_deployment(
        &DeploymentInputs {
            name: &name,
            image: &image,
            component: "worker",
            replicas,
            env: &env,
            volumes: &bundle.volumes,
            mounts: &bundle.mounts,
            command: Some(vec!["n8n".to_string(), "worker".to_string()]),
            image_pull_secrets: &c.spec.image_pull_secrets,
            resources: c.spec.workers.resources.as_ref(),
            pod: c.spec.workers.pod.as_ref(),
        },
        ctx.owner,
    );
    ctx.api::<Deployment>()
        .patch(&name, ctx.patch, &Patch::Apply(&dep))
        .await
        .map_err(Error::KubeError)?;
    sync_hpa(c, &name, &image, ctx).await
}

async fn sync_hpa(c: &Cluster, name: &str, image: &str, ctx: &ApplyCtx<'_>) -> Result<()> {
    let hpas = ctx.api::<HorizontalPodAutoscaler>();
    if let Some(as_cfg) = &c.spec.workers.autoscaling {
        let hpa = build_worker_hpa(name, image, as_cfg, ctx.owner);
        hpas.patch(name, ctx.patch, &Patch::Apply(&hpa))
            .await
            .map_err(Error::KubeError)?;
    } else if hpas.get_opt(name).await.map_err(Error::KubeError)?.is_some() {
        hpas.delete(name, &Default::default())
            .await
            .map_err(Error::KubeError)?;
    }
    Ok(())
}
