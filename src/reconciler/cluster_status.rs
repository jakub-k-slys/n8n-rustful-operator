use crate::{
    Error, Result,
    spec::{Cluster, ClusterStatus},
};
use kube::{
    Client,
    api::{Api, Patch, PatchParams},
};
use serde_json::json;

pub async fn patch_status(
    c: &Cluster,
    client: &Client,
    ns: &str,
    name: &str,
    key_secret: &str,
    ps: &PatchParams,
) -> Result<()> {
    let status = ClusterStatus {
        ready: true,
        main_replicas: c.spec.main.replicas,
        worker_replicas: c.spec.workers.replicas,
        webhook_replicas: c.spec.webhooks.as_ref().map(|w| w.replicas).unwrap_or(0),
        encryption_key_secret: Some(key_secret.to_string()),
    };
    Api::<Cluster>::namespaced(client.clone(), ns)
        .patch_status(
            name,
            ps,
            &Patch::Apply(json!({
                "apiVersion": "n8n.slys.dev/v1",
                "kind": "Cluster",
                "status": status,
            })),
        )
        .await
        .map_err(Error::KubeError)?;
    Ok(())
}
