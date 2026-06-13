use cucumber::{World, given, then, when};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::Service};
use kube::{
    Client,
    api::{Api, DeleteParams, Patch, PatchParams, ResourceExt},
};
use n8n_rustful_operator::{Instance, InstanceSpec};
use std::time::Duration;
use tokio::time::{Instant, sleep};

const NS: &str = "default";

#[derive(Default, World)]
pub struct E2eWorld {
    client: Option<Client>,
}

impl std::fmt::Debug for E2eWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("E2eWorld")
            .field("client", &self.client.as_ref().map(|_| "<Client>"))
            .finish()
    }
}

impl E2eWorld {
    fn client(&self) -> &Client {
        self.client.as_ref().expect("kube client not initialised")
    }
}

async fn apply_instance(w: &mut E2eWorld, name: &str, image: &str) {
    let api: Api<Instance> = Api::namespaced(w.client().clone(), NS);
    let inst = Instance::new(
        name,
        InstanceSpec {
            image: image.into(),
            replicas: 1,
            host: Some("e2e.example.com".into()),
            service: None,
            networking: None,
            encryption_key: None,
        },
    );
    let ssa = PatchParams::apply("cucumber").force();
    api.patch(name, &ssa, &Patch::Apply(&inst))
        .await
        .expect("apply Instance");
}

async fn wait_until<F, Fut>(timeout_secs: u64, label: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if check().await {
            return;
        }
        if Instant::now() >= deadline {
            panic!("timeout waiting for: {label}");
        }
        sleep(Duration::from_millis(500)).await;
    }
}

#[given("a kind cluster with the operator installed")]
async fn cluster_ready(w: &mut E2eWorld) {
    let client = Client::try_default().await.expect("kubeconfig");
    let api: Api<Deployment> = Api::namespaced(client.clone(), "n8n-operator");
    let dep = api
        .get("n8n-rustful-operator")
        .await
        .expect("operator deployment not found — is the operator installed?");
    let ready = dep
        .status
        .as_ref()
        .and_then(|s| s.ready_replicas)
        .unwrap_or(0);
    assert!(ready >= 1, "operator deployment has no ready replicas");
    w.client = Some(client);
}

#[given(regex = r#"^an Instance "([^"]+)" exists$"#)]
async fn instance_exists(w: &mut E2eWorld, name: String) {
    apply_instance(w, &name, "nginx:alpine").await;
    let client = w.client().clone();
    let n = name.clone();
    wait_until(60, &format!("Deployment/{name} to appear"), move || {
        let api: Api<Deployment> = Api::namespaced(client.clone(), NS);
        let n = n.clone();
        async move { api.get_opt(&n).await.unwrap().is_some() }
    })
    .await;
}

#[when(regex = r#"^I apply an Instance "([^"]+)" with image "([^"]+)"$"#)]
async fn when_apply_instance(w: &mut E2eWorld, name: String, image: String) {
    apply_instance(w, &name, &image).await;
}

#[when(regex = r#"^I delete the Instance "([^"]+)"$"#)]
async fn when_delete_instance(w: &mut E2eWorld, name: String) {
    let api: Api<Instance> = Api::namespaced(w.client().clone(), NS);
    api.delete(&name, &DeleteParams::default())
        .await
        .expect("delete Instance");
}

#[then(regex = r#"^a Deployment named "([^"]+)" exists in namespace "([^"]+)" within (\d+) seconds$"#)]
async fn deployment_exists(w: &mut E2eWorld, name: String, ns: String, secs: u64) {
    let client = w.client().clone();
    let n = name.clone();
    let ns2 = ns.clone();
    wait_until(secs, &format!("Deployment/{name} in {ns}"), move || {
        let client = client.clone();
        let n = n.clone();
        let ns = ns2.clone();
        async move {
            let api: Api<Deployment> = Api::namespaced(client, &ns);
            api.get_opt(&n).await.unwrap().is_some()
        }
    })
    .await;
}

#[then(regex = r#"^a Service named "([^"]+)" exposes port (\d+)$"#)]
async fn service_exposes_port(w: &mut E2eWorld, name: String, port: i32) {
    let api: Api<Service> = Api::namespaced(w.client().clone(), NS);
    let svc = api.get(&name).await.expect("Service not found");
    let p = svc
        .spec
        .as_ref()
        .and_then(|s| s.ports.as_ref())
        .and_then(|ports| ports.first())
        .map(|port| port.port)
        .expect("no port on Service");
    assert_eq!(p, port);
}

#[then(regex = r#"^the Instance "([^"]+)" has status.ready set to true within (\d+) seconds$"#)]
async fn status_ready(w: &mut E2eWorld, name: String, secs: u64) {
    let client = w.client().clone();
    let n = name.clone();
    wait_until(secs, &format!("Instance/{name} status.ready"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api: Api<Instance> = Api::namespaced(client, NS);
            api.get(&n)
                .await
                .ok()
                .and_then(|i| i.status.map(|s| s.ready))
                .unwrap_or(false)
        }
    })
    .await;
}

#[then(regex = r#"^the Instance "([^"]+)" has the finalizer "([^"]+)"$"#)]
async fn has_finalizer(w: &mut E2eWorld, name: String, finalizer: String) {
    let api: Api<Instance> = Api::namespaced(w.client().clone(), NS);
    let inst = api.get(&name).await.expect("Instance");
    let finalizers = inst.finalizers();
    assert!(
        finalizers.iter().any(|f| f == &finalizer),
        "finalizer {finalizer:?} not in {finalizers:?}"
    );
}

#[then(regex = r#"^the Instance "([^"]+)" is gone within (\d+) seconds$"#)]
async fn instance_gone(w: &mut E2eWorld, name: String, secs: u64) {
    let client = w.client().clone();
    let n = name.clone();
    wait_until(secs, &format!("Instance/{name} gone"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api: Api<Instance> = Api::namespaced(client, NS);
            api.get_opt(&n).await.unwrap().is_none()
        }
    })
    .await;
}

#[tokio::main]
async fn main() {
    E2eWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("features")
        .await;
}
