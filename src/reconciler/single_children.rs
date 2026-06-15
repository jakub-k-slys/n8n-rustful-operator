use crate::{
    Error, Result,
    builders::{deployment::build_deployment, pvc::build_data_pvc, service::build_service},
    reconciler::{
        ctx::ApplyCtx,
        networking::{RoleNetworking, reconcile_role_networking},
    },
    spec::{SecretKeyRef, Single},
};
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{PersistentVolumeClaim, Service},
};
use kube::api::Patch;

pub async fn apply_children(s: &Single, key_secret: &SecretKeyRef, ctx: &ApplyCtx<'_>) -> Result<()> {
    let name = kube::ResourceExt::name_any(s);
    let pvc_name = format!("{name}-data");
    if let Some(pvc) = build_data_pvc(
        &pvc_name,
        &name,
        &s.spec.image,
        s.spec.persistence.as_ref(),
        ctx.owner,
    ) {
        ctx.api::<PersistentVolumeClaim>()
            .patch(&pvc_name, ctx.patch, &Patch::Apply(&pvc))
            .await
            .map_err(Error::KubeError)?;
    }
    ctx.api::<Deployment>()
        .patch(
            &name,
            ctx.patch,
            &Patch::Apply(&build_deployment(&name, &s.spec, key_secret, ctx.owner)),
        )
        .await
        .map_err(Error::KubeError)?;
    ctx.api::<Service>()
        .patch(
            &name,
            ctx.patch,
            &Patch::Apply(&build_service(&name, &s.spec, ctx.owner)),
        )
        .await
        .map_err(Error::KubeError)?;
    reconcile_role_networking(
        &RoleNetworking {
            name: &name,
            image: &s.spec.image,
            component: "workflow-engine",
            host: s.spec.host.as_deref(),
            net: s.spec.networking.as_ref(),
        },
        ctx,
    )
    .await
}
