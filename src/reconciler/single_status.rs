use crate::{
    Error, Result,
    spec::{Single, SingleStatus},
};
use kube::{
    Client,
    api::{Api, Patch, PatchParams},
};
use serde_json::json;

pub async fn patch_status(
    s: &Single,
    client: &Client,
    ns: &str,
    name: &str,
    key_secret: &str,
    ps: &PatchParams,
) -> Result<()> {
    let status = SingleStatus {
        ready: true,
        replicas: s.spec.replicas,
        url: s.spec.host.as_ref().map(|h| format!("https://{h}")),
        encryption_key_secret: Some(key_secret.to_string()),
    };
    Api::<Single>::namespaced(client.clone(), ns)
        .patch_status(
            name,
            ps,
            &Patch::Apply(json!({
                "apiVersion": "n8n.slys.dev/v1",
                "kind": "Single",
                "status": status,
            })),
        )
        .await
        .map_err(Error::KubeError)?;
    Ok(())
}
