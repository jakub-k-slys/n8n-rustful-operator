use crate::{
    Error, Result,
    labels::{common_annotations, common_labels},
    reconciler::ctx::ApplyCtx,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::APIGroupList;
use kube::{
    Client,
    api::{Api, DynamicObject, GroupVersionKind, Patch},
    discovery::ApiResource,
};
use serde_json::json;

/// True if the cluster serves `networking.istio.io/v1` (where `DestinationRule`
/// lives). Pure so it can be unit-tested without a cluster.
pub fn destination_rule_v1_served(groups: &APIGroupList) -> bool {
    groups
        .groups
        .iter()
        .any(|g| g.name == "networking.istio.io" && g.versions.iter().any(|v| v.version == "v1"))
}

/// Live check (one `/apis` call) for whether Istio's networking CRDs are served.
/// Re-evaluated per reconcile; a failed lookup counts as "not available".
async fn istio_available(client: &Client) -> bool {
    match client.list_api_groups().await {
        Ok(groups) => destination_rule_v1_served(&groups),
        Err(_) => false,
    }
}

fn destination_rule_api(client: Client, ns: &str) -> Api<DynamicObject> {
    let gvk = GroupVersionKind::gvk("networking.istio.io", "v1", "DestinationRule");
    let ar = ApiResource::from_gvk(&gvk);
    Api::namespaced_with(client, ns, &ar)
}

/// Apply a cookie-based consistent-hash `DestinationRule` so a multi-main setup
/// gets sticky sessions behind Istio (Service `sessionAffinity` is bypassed by
/// Envoy). No-op when the Istio CRDs aren't served — same guard the operator
/// uses for HTTPRoutes.
pub async fn apply_destination_rule(name: &str, image: &str, ctx: &ApplyCtx<'_>) -> Result<()> {
    if !istio_available(ctx.client).await {
        return Ok(());
    }
    let body = json!({
        "apiVersion": "networking.istio.io/v1",
        "kind": "DestinationRule",
        "metadata": {
            "name": name,
            "labels": common_labels(name, image, "main"),
            "annotations": common_annotations(),
            "ownerReferences": [ctx.owner],
        },
        "spec": {
            "host": format!("{name}.{}.svc.cluster.local", ctx.ns),
            "trafficPolicy": {
                "loadBalancer": {
                    "consistentHash": {
                        "httpCookie": { "name": "n8n-route", "ttl": "3600s" }
                    }
                }
            }
        }
    });
    let api = destination_rule_api(ctx.client.clone(), ctx.ns);
    let dr: DynamicObject = serde_json::from_value(body).expect("static destinationrule schema is valid");
    api.patch(name, ctx.patch, &Patch::Apply(&dr))
        .await
        .map_err(Error::KubeError)?;
    Ok(())
}

/// Best-effort GC of the `DestinationRule` (e.g. when main scales back to one).
/// No-op when the Istio CRDs aren't served.
pub async fn delete_destination_rule(client: &Client, ns: &str, name: &str) -> Result<()> {
    if !istio_available(client).await {
        return Ok(());
    }
    let api = destination_rule_api(client.clone(), ns);
    if let Ok(Some(_)) = api.get_opt(name).await {
        api.delete(name, &Default::default())
            .await
            .map_err(Error::KubeError)?;
    }
    Ok(())
}
