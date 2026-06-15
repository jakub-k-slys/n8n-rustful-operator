use crate::{
    Error, Result,
    builders::pvc::{build_data_pvc, build_persistence_volume},
    reconciler::ctx::{ApplyCtx, Bundle},
    spec::Cluster,
};
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use kube::api::Patch;
use serde_json::Value;

pub async fn main_volumes(
    c: &Cluster,
    main_name: &str,
    image: &str,
    ctx: &ApplyCtx<'_>,
    bundle: &Bundle,
) -> Result<(Vec<Value>, Vec<Value>)> {
    let pvc_name = format!("{main_name}-data");
    if let Some(pvc) = build_data_pvc(
        &pvc_name,
        main_name,
        image,
        c.spec.main.persistence.as_ref(),
        ctx.owner,
    ) {
        ctx.api::<PersistentVolumeClaim>()
            .patch(&pvc_name, ctx.patch, &Patch::Apply(&pvc))
            .await
            .map_err(Error::KubeError)?;
    }
    let mut vols = bundle.volumes.clone();
    let mut mounts = bundle.mounts.clone();
    if c.spec.main.persistence.is_some() {
        let (v, m) = build_persistence_volume(&pvc_name);
        vols.push(v);
        mounts.push(m);
    }
    Ok((vols, mounts))
}
