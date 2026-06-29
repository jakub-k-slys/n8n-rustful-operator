use crate::{
    Error, Result,
    builders::{
        cluster_deployment::{DeploymentInputs, build_cluster_deployment},
        destination_rule::{apply_destination_rule, delete_destination_rule},
        service::build_cluster_service,
    },
    env::{build_user_env, cluster_role_defaults, env_str},
    reconciler::{
        cluster_main_volumes::main_volumes,
        ctx::{ApplyCtx, Bundle},
        networking::{RoleNetworking, reconcile_role_networking},
    },
    spec::Cluster,
};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::Service};
use kube::api::Patch;
use serde_json::Value;

/// Multi-main HA env for the main role; empty unless multi-main is active.
fn multi_main_env(c: &Cluster, enabled: bool) -> Vec<Value> {
    if !enabled {
        return Vec::new();
    }
    [
        Some(env_str("N8N_MULTI_MAIN_SETUP_ENABLED", "true")),
        c.spec
            .main
            .multi_main_key_ttl
            .map(|ttl| env_str("N8N_MULTI_MAIN_SETUP_KEY_TTL", ttl.to_string())),
        c.spec
            .main
            .multi_main_check_interval
            .map(|iv| env_str("N8N_MULTI_MAIN_SETUP_CHECK_INTERVAL", iv.to_string())),
    ]
    .into_iter()
    .flatten()
    .collect()
}

pub async fn reconcile_main(
    c: &Cluster,
    cluster_name: &str,
    ctx: &ApplyCtx<'_>,
    bundle: &Bundle,
) -> Result<()> {
    let name = format!("{cluster_name}-main");
    let image = c.spec.main.image.clone().unwrap_or_else(|| c.spec.image.clone());
    // More than one main requires n8n's multi-main HA setup, otherwise each
    // main duplicates the at-most-once tasks. Auto-enable it.
    let multi_main = c.spec.main.replicas > 1;
    let (vols, mounts) = main_volumes(c, &name, &image, ctx, bundle).await?;
    let defaults = cluster_role_defaults(c, c.spec.main.host.as_deref(), c.spec.main.networking.as_ref());
    // With dedicated webhook processors, the main process must stop serving
    // production webhooks so they are handled solely by the webhook role.
    let webhook_offload = if c.spec.webhooks.is_some() {
        vec![env_str("N8N_DISABLE_PRODUCTION_MAIN_PROCESS", "true")]
    } else {
        Vec::new()
    };
    let env = [
        bundle.env.clone(),
        multi_main_env(c, multi_main),
        webhook_offload,
        build_user_env(
            &defaults,
            c.spec.secure_cookie,
            &c.spec.extra_env,
            &c.spec.main.extra_env,
        ),
    ]
    .concat();
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
            image_pull_secrets: &c.spec.image_pull_secrets,
            resources: c.spec.main.resources.as_ref(),
            pod: c.spec.main.pod.as_ref(),
            strategy: c.spec.main.strategy.as_ref(),
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
                multi_main,
                ctx.owner,
            )),
        )
        .await
        .map_err(Error::KubeError)?;
    // Sticky sessions for multi-main behind Istio (Service sessionAffinity is
    // bypassed by Envoy). Both calls are no-ops when the Istio CRDs aren't served.
    if multi_main {
        apply_destination_rule(&name, &image, ctx).await?;
    } else {
        delete_destination_rule(ctx.client, ctx.ns, &name).await?;
    }
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
