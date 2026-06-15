use crate::{
    Error, Result,
    builders::{
        http_route::{apply_http_route, delete_http_route},
        ingress::build_ingress,
    },
    spec::NetworkingSpec,
};
use k8s_openapi::{api::networking::v1::Ingress, apimachinery::pkg::apis::meta::v1::OwnerReference};
use kube::{
    Client,
    api::{Api, Patch, PatchParams},
};

#[allow(clippy::too_many_arguments)]
pub async fn reconcile_role_networking(
    client: &Client,
    ns: &str,
    name: &str,
    image: &str,
    component: &str,
    host: Option<&str>,
    net: Option<&NetworkingSpec>,
    owner: &OwnerReference,
    ps: &PatchParams,
) -> Result<()> {
    if let Some(net) = net
        && net.ingress.is_some()
        && net.http_route.is_some()
    {
        return Err(Error::ConflictingNetworking);
    }
    let want_ingress = net.and_then(|n| n.ingress.as_ref());
    let want_route = net.and_then(|n| n.http_route.as_ref());
    let ingress_api: Api<Ingress> = Api::namespaced(client.clone(), ns);
    if let Some(ing_cfg) = want_ingress {
        let host = host.unwrap_or("");
        let mut ingress = build_ingress(name, image, host, ing_cfg, owner);
        if let Some(meta_labels) = ingress.metadata.labels.as_mut() {
            meta_labels.insert(
                "app.kubernetes.io/component".to_string(),
                format!("{component}-ingress"),
            );
        }
        ingress_api
            .patch(name, ps, &Patch::Apply(&ingress))
            .await
            .map_err(Error::KubeError)?;
    } else if ingress_api
        .get_opt(name)
        .await
        .map_err(Error::KubeError)?
        .is_some()
    {
        ingress_api
            .delete(name, &Default::default())
            .await
            .map_err(Error::KubeError)?;
    }
    if let Some(rt_cfg) = want_route {
        apply_http_route(client, ns, name, image, host.unwrap_or(""), rt_cfg, owner, ps).await?;
    } else {
        let _ = delete_http_route(client, ns, name).await;
    }
    Ok(())
}
