use crate::{
    Error, Result,
    builders::{
        cluster_deployment::{DeploymentInputs, build_cluster_deployment},
        service::build_cluster_service,
    },
    env::build_user_env,
    reconciler::{
        cluster_main_volumes::main_volumes,
        ctx::{ApplyCtx, Bundle},
        networking::{RoleNetworking, reconcile_role_networking},
    },
    spec::Cluster,
};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::Service};
use kube::api::Patch;

pub async fn reconcile_main(
    c: &Cluster,
    cluster_name: &str,
    ctx: &ApplyCtx<'_>,
    bundle: &Bundle,
) -> Result<()> {
    let name = format!("{cluster_name}-main");
    let image = c.spec.main.image.clone().unwrap_or_else(|| c.spec.image.clone());
    let (vols, mounts) = main_volumes(c, &name, &image, ctx, bundle).await?;
    let mut env = bundle.env.clone();
    env.extend(build_user_env(
        c.spec.secure_cookie,
        &c.spec.extra_env,
        &c.spec.main.extra_env,
    ));
    let dep = build_cluster_deployment(
        &DeploymentInputs {
            name: &name,
            image: &image,
            component: "main",
            replicas: Some(c.spec.main.replicas),
            env: &env,
            volumes: &vols,
            mounts: &mounts,
            command: None,
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
                "main",
                c.spec.main.service.as_ref(),
                ctx.owner,
            )),
        )
        .await
        .map_err(Error::KubeError)?;
    reconcile_role_networking(
        &RoleNetworking {
            name: &name,
            image: &image,
            component: "main",
            host: c.spec.main.host.as_deref(),
            net: c.spec.main.networking.as_ref(),
        },
        ctx,
    )
    .await
}
