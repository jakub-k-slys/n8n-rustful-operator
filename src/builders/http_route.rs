use crate::{
    Error, Result,
    labels::{common_annotations, common_labels},
    reconciler::ctx::ApplyCtx,
    spec::{GatewayRef, HttpRouteConfig},
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::APIGroupList;
use kube::{
    Client,
    api::{Api, DynamicObject, GroupVersionKind, Patch},
    discovery::ApiResource,
};
use serde_json::{Value, json};

/// True if the cluster currently serves `gateway.networking.k8s.io/v1` (the
/// group/version HTTPRoute lives in). Pure so it can be unit-tested without a
/// cluster.
pub fn gateway_v1_served(groups: &APIGroupList) -> bool {
    groups
        .groups
        .iter()
        .any(|g| g.name == "gateway.networking.k8s.io" && g.versions.iter().any(|v| v.version == "v1"))
}

/// Live check (one `/apis` call per invocation) for whether the Gateway API is
/// installed. Re-evaluated on every reconcile, so installing or removing the
/// Gateway API CRDs takes effect without restarting the operator. Treats a
/// failed lookup as "not available".
async fn gateway_api_available(client: &Client) -> bool {
    match client.list_api_groups().await {
        Ok(groups) => gateway_v1_served(&groups),
        Err(_) => false,
    }
}

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
    // Best-effort GC of a role's HTTPRoute (and its companion redirect route).
    // When the Gateway API isn't served, no HTTPRoute can exist, and probing
    // the unregistered group makes the apiserver return a plain-text 404 that
    // kube-rs can't parse as a Status — spamming a WARN every reconcile. Skip
    // the probe in that case.
    if !gateway_api_available(client).await {
        return Ok(());
    }
    let api = http_route_api(client.clone(), ns);
    for n in [name.to_string(), redirect_name(name)] {
        if let Ok(Some(_)) = api.get_opt(&n).await {
            api.delete(&n, &Default::default())
                .await
                .map_err(Error::KubeError)?;
        }
    }
    Ok(())
}

fn redirect_name(name: &str) -> String {
    format!("{name}-redirect")
}

/// Build a `parentRefs[0]` entry for a Gateway, optionally pinned to a listener.
fn gateway_parent(gw: &GatewayRef, section: Option<&str>) -> Value {
    let mut parent = json!({
        "name": gw.name,
        "kind": "Gateway",
        "group": "gateway.networking.k8s.io",
    });
    if let Some(gw_ns) = &gw.namespace {
        parent["namespace"] = json!(gw_ns);
    }
    if let Some(s) = section {
        parent["sectionName"] = json!(s);
    }
    parent
}

async fn apply_route(api: &Api<DynamicObject>, ctx: &ApplyCtx<'_>, name: &str, body: Value) -> Result<()> {
    let route: DynamicObject = serde_json::from_value(body).expect("static httproute schema is valid");
    api.patch(name, ctx.patch, &Patch::Apply(&route))
        .await
        .map_err(Error::KubeError)?;
    Ok(())
}

pub async fn apply_http_route(target: &RouteTarget<'_>, ctx: &ApplyCtx<'_>) -> Result<()> {
    let api = http_route_api(ctx.client.clone(), ctx.ns);
    let gw = &target.cfg.gateway;

    // Primary route: forwards to the n8n Service, pinned to the gateway's
    // listener when `sectionName` is set.
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
            "parentRefs": [gateway_parent(gw, gw.section_name.as_deref())],
            "hostnames": [target.host],
            "rules": [{ "backendRefs": [{ "name": target.name, "port": 5678 }] }]
        }
    });
    apply_route(&api, ctx, target.name, body).await?;

    // Companion redirect route on the HTTP listener (when requested); otherwise
    // GC any stale one left from a previous spec.
    let rname = redirect_name(target.name);
    if let Some(http_section) = &target.cfg.https_redirect_section_name {
        let rbody = json!({
            "apiVersion": "gateway.networking.k8s.io/v1",
            "kind": "HTTPRoute",
            "metadata": {
                "name": rname,
                "labels": common_labels(&rname, target.image, "http-redirect"),
                "annotations": common_annotations(),
                "ownerReferences": [ctx.owner],
            },
            "spec": {
                "parentRefs": [gateway_parent(gw, Some(http_section))],
                "hostnames": [target.host],
                "rules": [{ "filters": [{
                    "type": "RequestRedirect",
                    "requestRedirect": { "scheme": "https", "statusCode": 301 }
                }] }]
            }
        });
        apply_route(&api, ctx, &rname, rbody).await?;
    } else if let Ok(Some(_)) = api.get_opt(&rname).await {
        api.delete(&rname, &Default::default())
            .await
            .map_err(Error::KubeError)?;
    }
    Ok(())
}
