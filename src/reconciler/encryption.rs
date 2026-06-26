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
        // Tolerate a concurrent creator (or a name reused by another CR): a 409
        // AlreadyExists means the Secret now exists, which is the desired state —
        // don't fail the whole reconcile over it.
        match secrets.create(&Default::default(), &secret).await {
            Ok(_) => {}
            Err(kube::Error::Api(ae)) if ae.code == 409 => {}
            Err(e) => return Err(Error::KubeError(e)),
        }
    }
    Ok(SecretKeyRef { name, key })
}
