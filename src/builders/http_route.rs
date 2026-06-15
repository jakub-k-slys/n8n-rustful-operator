use crate::{
    Error, Result,
    labels::{common_annotations, common_labels},
    spec::HttpRouteConfig,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::{
    Client,
    api::{Api, DynamicObject, GroupVersionKind, Patch, PatchParams},
    discovery::ApiResource,
};
use serde_json::json;

fn http_route_api(client: Client, ns: &str) -> Api<DynamicObject> {
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "HTTPRoute");
    let ar = ApiResource::from_gvk(&gvk);
    Api::namespaced_with(client, ns, &ar)
}

pub async fn delete_http_route(client: &Client, ns: &str, name: &str) -> Result<()> {
    let api = http_route_api(client.clone(), ns);
    if let Ok(Some(_)) = api.get_opt(name).await {
        api.delete(name, &Default::default())
            .await
            .map_err(Error::KubeError)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn apply_http_route(
    client: &Client,
    ns: &str,
    name: &str,
    image: &str,
    host: &str,
    cfg: &HttpRouteConfig,
    owner: &OwnerReference,
    ps: &PatchParams,
) -> Result<()> {
    let mut parent = json!({
        "name": cfg.gateway.name,
        "kind": "Gateway",
        "group": "gateway.networking.k8s.io",
    });
    if let Some(gw_ns) = &cfg.gateway.namespace {
        parent["namespace"] = json!(gw_ns);
    }
    let body = json!({
        "apiVersion": "gateway.networking.k8s.io/v1",
        "kind": "HTTPRoute",
        "metadata": {
            "name": name,
            "labels": common_labels(name, image, "http-route"),
            "annotations": common_annotations(),
            "ownerReferences": [owner],
        },
        "spec": {
            "parentRefs": [parent],
            "hostnames": [host],
            "rules": [{
                "backendRefs": [{ "name": name, "port": 5678 }]
            }]
        }
    });
    let api = http_route_api(client.clone(), ns);
    let route: DynamicObject = serde_json::from_value(body).expect("static httproute schema is valid");
    api.patch(name, ps, &Patch::Apply(&route))
        .await
        .map_err(Error::KubeError)?;
    Ok(())
}
