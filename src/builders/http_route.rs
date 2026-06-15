use crate::{
    Error, Result,
    labels::{common_annotations, common_labels},
    reconciler::ctx::ApplyCtx,
    spec::HttpRouteConfig,
};
use kube::{
    Client,
    api::{Api, DynamicObject, GroupVersionKind, Patch},
    discovery::ApiResource,
};
use serde_json::json;

pub struct RouteTarget<'a> {
    pub name: &'a str,
    pub image: &'a str,
    pub host: &'a str,
    pub cfg: &'a HttpRouteConfig,
}

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

pub async fn apply_http_route(target: &RouteTarget<'_>, ctx: &ApplyCtx<'_>) -> Result<()> {
    let mut parent = json!({
        "name": target.cfg.gateway.name,
        "kind": "Gateway",
        "group": "gateway.networking.k8s.io",
    });
    if let Some(gw_ns) = &target.cfg.gateway.namespace {
        parent["namespace"] = json!(gw_ns);
    }
    let body = json!({
        "apiVersion": "gateway.networking.k8s.io/v1",
        "kind": "HTTPRoute",
        "metadata": {
            "name": target.name,
            "labels": common_labels(target.name, target.image, "http-route"),
            "annotations": common_annotations(),
            "ownerReferences": [ctx.owner],
        },
        "spec": {
            "parentRefs": [parent],
            "hostnames": [target.host],
            "rules": [{ "backendRefs": [{ "name": target.name, "port": 5678 }] }]
        }
    });
    let api = http_route_api(ctx.client.clone(), ctx.ns);
    let route: DynamicObject = serde_json::from_value(body).expect("static httproute schema is valid");
    api.patch(target.name, ctx.patch, &Patch::Apply(&route))
        .await
        .map_err(Error::KubeError)?;
    Ok(())
}
