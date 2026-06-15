use crate::{
    Error, Result,
    builders::{
        cluster_deployment::build_cluster_deployment, http_route::delete_http_route,
        service::build_cluster_service,
    },
    env::env_str,
    reconciler::networking::reconcile_role_networking,
    spec::Cluster,
};
use k8s_openapi::{
    api::{apps::v1::Deployment, core::v1::Service, networking::v1::Ingress},
    apimachinery::pkg::apis::meta::v1::OwnerReference,
};
use kube::{
    Client,
    api::{Api, Patch, PatchParams},
};
use serde_json::Value;

#[allow(clippy::too_many_arguments)]
pub async fn reconcile_webhooks(
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
    let name = format!("{cluster_name}-webhook");
    let deps: Api<Deployment> = Api::namespaced(client.clone(), ns);
    let services: Api<Service> = Api::namespaced(client.clone(), ns);
    let ingresses: Api<Ingress> = Api::namespaced(client.clone(), ns);

    let Some(wh) = &c.spec.webhooks else {
        let _ = deps.delete(&name, &Default::default()).await;
        let _ = services.delete(&name, &Default::default()).await;
        let _ = ingresses.delete(&name, &Default::default()).await;
        let _ = delete_http_route(client, ns, &name).await;
        return Ok(());
    };

    let image = wh.image.clone().unwrap_or_else(|| c.spec.image.clone());
    let mut env = common_env.to_vec();
    env.push(env_str("N8N_DISABLE_PRODUCTION_MAIN_PROCESS", "true"));
    let dep = build_cluster_deployment(
        &name,
        &image,
        "webhook",
        Some(wh.replicas),
        &env,
        common_vols,
        common_mounts,
        Some(vec!["n8n".to_string(), "webhook".to_string()]),
        owner,
    );
    deps.patch(&name, ps, &Patch::Apply(&dep))
        .await
        .map_err(Error::KubeError)?;
    services
        .patch(
            &name,
            ps,
            &Patch::Apply(&build_cluster_service(
                &name,
                &image,
                "webhook",
                wh.service.as_ref(),
                owner,
            )),
        )
        .await
        .map_err(Error::KubeError)?;
    reconcile_role_networking(
        client,
        ns,
        &name,
        &image,
        "webhook",
        wh.host.as_deref(),
        wh.networking.as_ref(),
        owner,
        ps,
    )
    .await
}
