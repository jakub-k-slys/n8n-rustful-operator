use crate::{
    Error, Result,
    builders::{cluster_deployment::build_cluster_deployment, service::build_cluster_service},
    reconciler::{cluster_main_volumes::main_volumes, networking::reconcile_role_networking},
    spec::Cluster,
};
use k8s_openapi::{
    api::{apps::v1::Deployment, core::v1::Service},
    apimachinery::pkg::apis::meta::v1::OwnerReference,
};
use kube::{
    Client,
    api::{Api, Patch, PatchParams},
};
use serde_json::Value;

#[allow(clippy::too_many_arguments)]
pub async fn reconcile_main(
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
    let main_name = format!("{cluster_name}-main");
    let image = c
        .spec
        .main
        .image
        .clone()
        .unwrap_or_else(|| c.spec.image.clone());
    let (vols, mounts) = main_volumes(
        c, client, ns, &main_name, &image, owner, ps, common_vols, common_mounts,
    )
    .await?;
    let dep = build_cluster_deployment(
        &main_name,
        &image,
        "main",
        Some(c.spec.main.replicas),
        common_env,
        &vols,
        &mounts,
        None,
        owner,
    );
    Api::<Deployment>::namespaced(client.clone(), ns)
        .patch(&main_name, ps, &Patch::Apply(&dep))
        .await
        .map_err(Error::KubeError)?;
    Api::<Service>::namespaced(client.clone(), ns)
        .patch(
            &main_name,
            ps,
            &Patch::Apply(&build_cluster_service(
                &main_name,
                &image,
                "main",
                c.spec.main.service.as_ref(),
                owner,
            )),
        )
        .await
        .map_err(Error::KubeError)?;
    reconcile_role_networking(
        client,
        ns,
        &main_name,
        &image,
        "main",
        c.spec.main.host.as_deref(),
        c.spec.main.networking.as_ref(),
        owner,
        ps,
    )
    .await
}
