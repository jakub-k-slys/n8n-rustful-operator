use crate::{
    Error, Result,
    labels::{common_annotations, common_labels},
    spec::{EncryptionKeySpec, SecretKeyRef},
    state::Context,
};
use k8s_openapi::{api::core::v1::Secret, apimachinery::pkg::apis::meta::v1::OwnerReference};
use kube::{
    ResourceExt,
    api::{Api, ObjectMeta},
};
use rand::RngCore;
use std::collections::BTreeMap;

pub async fn resolve_encryption_secret<R: ResourceExt>(
    obj: &R,
    image: &str,
    spec: Option<&EncryptionKeySpec>,
    ctx: &Context,
    ns: &str,
    owner: &OwnerReference,
) -> Result<SecretKeyRef> {
    if let Some(s) = spec
        && let Some(r) = &s.secret_ref
    {
        return Ok(r.clone());
    }
    let name = format!("{}-encryption-key", obj.name_any());
    let key = "encryption_key".to_string();
    let secrets: Api<Secret> = Api::namespaced(ctx.client.clone(), ns);
    if secrets.get_opt(&name).await.map_err(Error::KubeError)?.is_none() {
        let mut buf = [0u8; 32];
        rand::rng().fill_bytes(&mut buf);
        let mut data = BTreeMap::new();
        data.insert(key.clone(), hex::encode(buf));
        let secret = Secret {
            metadata: ObjectMeta {
                name: Some(name.clone()),
                namespace: Some(ns.to_string()),
                owner_references: Some(vec![owner.clone()]),
                labels: Some(common_labels(&obj.name_any(), image, "encryption-key")),
                annotations: Some(common_annotations()),
                ..Default::default()
            },
            string_data: Some(data),
            type_: Some("Opaque".to_string()),
            ..Default::default()
        };
        secrets
            .create(&Default::default(), &secret)
            .await
            .map_err(Error::KubeError)?;
    }
    Ok(SecretKeyRef { name, key })
}
