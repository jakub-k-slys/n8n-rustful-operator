use crate::{
    Error, Result,
    builders::{
        cluster_deployment::{DeploymentInputs, build_cluster_deployment},
        http_route::delete_http_route,
        service::build_cluster_service,
    },
    env::{build_user_env, env_str},
    reconciler::{
        ctx::{ApplyCtx, Bundle},
        networking::{RoleNetworking, reconcile_role_networking},
    },
    spec::Cluster,
};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::Service, networking::v1::Ingress};
use kube::api::Patch;

pub async fn reconcile_webhooks(
    c: &Cluster,
    cluster_name: &str,
    ctx: &ApplyCtx<'_>,
    bundle: &Bundle,
) -> Result<()> {
    let name = format!("{cluster_name}-webhook");
    let Some(wh) = &c.spec.webhooks else {
        let _ = ctx.api::<Deployment>().delete(&name, &Default::default()).await;
        let _ = ctx.api::<Service>().delete(&name, &Default::default()).await;
        let _ = ctx.api::<Ingress>().delete(&name, &Default::default()).await;
        let _ = delete_http_route(ctx.client, ctx.ns, &name).await;
        return Ok(());
    };
    let image = wh.image.clone().unwrap_or_else(|| c.spec.image.clone());
    let mut env = bundle.env.clone();
    env.push(env_str("N8N_DISABLE_PRODUCTION_MAIN_PROCESS", "true"));
    env.extend(build_user_env(
        c.spec.secure_cookie,
        &c.spec.extra_env,
        &wh.extra_env,
    ));
    let dep = build_cluster_deployment(
        &DeploymentInputs {
            name: &name,
            image: &image,
            component: "webhook",
            replicas: Some(wh.replicas),
            env: &env,
            volumes: &bundle.volumes,
            mounts: &bundle.mounts,
            command: Some(vec!["n8n".to_string(), "webhook".to_string()]),
            image_pull_secrets: &c.spec.image_pull_secrets,
        },
        ctx.owner,
    );
    ctx.api::<Deployment>()
        .patch(&name, ctx.patch, &Patch::Apply(&dep))
        .await
        .map_err(Error::KubeError)?;
    ctx.api::<Service>()
        .patch(
            &name,
            ctx.patch,
            &Patch::Apply(&build_cluster_service(
                &name,
                &image,
                "webhook",
                wh.service.as_ref(),
                ctx.owner,
            )),
        )
        .await
        .map_err(Error::KubeError)?;
    reconcile_role_networking(
        &RoleNetworking {
            name: &name,
            image: &image,
            component: "webhook",
            host: wh.host.as_deref(),
            net: wh.networking.as_ref(),
        },
        ctx,
    )
    .await
}
