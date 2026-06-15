use crate::{
    Error, Result,
    builders::pvc::{build_data_pvc, build_persistence_volume},
    spec::Cluster,
};
use k8s_openapi::{
    api::core::v1::PersistentVolumeClaim, apimachinery::pkg::apis::meta::v1::OwnerReference,
};
use kube::{
    Client,
    api::{Api, Patch, PatchParams},
};
use serde_json::Value;

#[allow(clippy::too_many_arguments)]
pub async fn main_volumes(
    c: &Cluster,
    client: &Client,
    ns: &str,
    main_name: &str,
    image: &str,
    owner: &OwnerReference,
    ps: &PatchParams,
    common_vols: &[Value],
    common_mounts: &[Value],
) -> Result<(Vec<Value>, Vec<Value>)> {
    let pvc_name = format!("{main_name}-data");
    let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(client.clone(), ns);
    if let Some(pvc) = build_data_pvc(
        &pvc_name,
        main_name,
        image,
        c.spec.main.persistence.as_ref(),
        owner,
    ) {
        pvcs.patch(&pvc_name, ps, &Patch::Apply(&pvc))
            .await
            .map_err(Error::KubeError)?;
    }
    let mut vols = common_vols.to_vec();
    let mut mounts = common_mounts.to_vec();
    if c.spec.main.persistence.is_some() {
        let (v, m) = build_persistence_volume(&pvc_name);
        vols.push(v);
        mounts.push(m);
    }
    Ok((vols, mounts))
}
