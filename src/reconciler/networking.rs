use crate::{
    Error, Result,
    builders::{
        http_route::{RouteTarget, apply_http_route, delete_http_route},
        ingress::build_ingress,
    },
    reconciler::ctx::ApplyCtx,
    spec::NetworkingSpec,
};
use k8s_openapi::api::networking::v1::Ingress;
use kube::api::Patch;

/// Identity of a single role's networking surface (main, webhook, single's workflow-engine).
pub struct RoleNetworking<'a> {
    pub name: &'a str,
    pub image: &'a str,
    pub component: &'a str,
    pub host: Option<&'a str>,
    pub net: Option<&'a NetworkingSpec>,
}

pub async fn reconcile_role_networking(role: &RoleNetworking<'_>, ctx: &ApplyCtx<'_>) -> Result<()> {
    if let Some(net) = role.net
        && net.ingress.is_some()
        && net.http_route.is_some()
    {
        return Err(Error::ConflictingNetworking);
    }
    apply_or_delete_ingress(role, ctx).await?;
    apply_or_delete_route(role, ctx).await
}

async fn apply_or_delete_ingress(role: &RoleNetworking<'_>, ctx: &ApplyCtx<'_>) -> Result<()> {
    let api: kube::Api<Ingress> = ctx.api();
    let want = role.net.and_then(|n| n.ingress.as_ref());
    if let Some(cfg) = want {
        let mut ing = build_ingress(role.name, role.image, role.host.unwrap_or(""), cfg, ctx.owner);
        if let Some(labels) = ing.metadata.labels.as_mut() {
            labels.insert(
                "app.kubernetes.io/component".to_string(),
                format!("{}-ingress", role.component),
            );
        }
        api.patch(role.name, ctx.patch, &Patch::Apply(&ing))
            .await
            .map_err(Error::KubeError)?;
    } else if api.get_opt(role.name).await.map_err(Error::KubeError)?.is_some() {
        api.delete(role.name, &Default::default())
            .await
            .map_err(Error::KubeError)?;
    }
    Ok(())
}

async fn apply_or_delete_route(role: &RoleNetworking<'_>, ctx: &ApplyCtx<'_>) -> Result<()> {
    if let Some(cfg) = role.net.and_then(|n| n.http_route.as_ref()) {
        let target = RouteTarget {
            name: role.name,
            image: role.image,
            host: role.host.unwrap_or(""),
            cfg,
        };
        apply_http_route(&target, ctx).await
    } else {
        let _ = delete_http_route(ctx.client, ctx.ns, role.name).await;
        Ok(())
    }
}
