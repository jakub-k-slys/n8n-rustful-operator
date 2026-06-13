use cucumber::{World, given, then, when};
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{Secret, Service},
    networking::v1::Ingress,
};
use kube::{
    Client,
    api::{Api, DeleteParams, ObjectMeta, Patch, PatchParams, PostParams, ResourceExt},
};
use n8n_rustful_operator::{
    EncryptionKeySpec, GatewayRef, HttpRouteConfig, IngressConfig, Instance, InstanceSpec,
    NetworkingSpec, SecretKeyRef, ServiceConfig,
};
use std::{collections::BTreeMap, time::Duration};
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

// ----- builders -----

fn base_spec(image: &str) -> InstanceSpec {
    InstanceSpec {
        image: image.into(),
        replicas: 1,
        host: Some("e2e.example.com".into()),
        service: None,
        networking: None,
        encryption_key: None,
    }
}

async fn apply_with_spec(w: &mut E2eWorld, name: &str, spec: InstanceSpec) {
    let api: Api<Instance> = Api::namespaced(w.client().clone(), NS);
    let inst = Instance::new(name, spec);
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

// ----- Given -----

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
    apply_with_spec(w, &name, base_spec("nginx:alpine")).await;
    let client = w.client().clone();
    let n = name.clone();
    wait_until(60, &format!("Deployment/{name} to appear"), move || {
        let api: Api<Deployment> = Api::namespaced(client.clone(), NS);
        let n = n.clone();
        async move { api.get_opt(&n).await.unwrap().is_some() }
    })
    .await;
}

#[given(regex = r#"^an Instance "([^"]+)" exists with ingress class "([^"]+)" and host "([^"]+)"$"#)]
async fn instance_with_ingress_exists(w: &mut E2eWorld, name: String, class: String, host: String) {
    let mut spec = base_spec("nginx:alpine");
    spec.host = Some(host);
    spec.networking = Some(NetworkingSpec {
        ingress: Some(IngressConfig {
            class_name: Some(class),
            tls_secret_name: None,
        }),
        http_route: None,
    });
    apply_with_spec(w, &name, spec).await;
    let client = w.client().clone();
    let n = name.clone();
    wait_until(60, &format!("Ingress/{name} to appear"), move || {
        let api: Api<Ingress> = Api::namespaced(client.clone(), NS);
        let n = n.clone();
        async move { api.get_opt(&n).await.unwrap().is_some() }
    })
    .await;
}

#[given(regex = r#"^a Secret "([^"]+)" exists with key "([^"]+)" set to "([^"]+)"$"#)]
async fn create_secret(w: &mut E2eWorld, name: String, key: String, value: String) {
    let api: Api<Secret> = Api::namespaced(w.client().clone(), NS);
    let mut data = BTreeMap::new();
    data.insert(key, value);
    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(NS.to_string()),
            ..Default::default()
        },
        string_data: Some(data),
        type_: Some("Opaque".to_string()),
        ..Default::default()
    };
    // upsert: create or replace
    if api.get_opt(&name).await.unwrap().is_some() {
        api.delete(&name, &DeleteParams::default()).await.ok();
        sleep(Duration::from_millis(200)).await;
    }
    api.create(&PostParams::default(), &secret)
        .await
        .expect("create Secret");
}

// ----- When -----

#[when(regex = r#"^I apply an Instance "([^"]+)" with image "([^"]+)"$"#)]
async fn when_apply_basic(w: &mut E2eWorld, name: String, image: String) {
    apply_with_spec(w, &name, base_spec(&image)).await;
}

#[when(regex = r#"^I apply an Instance "([^"]+)" with image "([^"]+)" and service type "([^"]+)"$"#)]
async fn when_apply_svc_type(w: &mut E2eWorld, name: String, image: String, svc_type: String) {
    let mut spec = base_spec(&image);
    spec.service = Some(ServiceConfig { type_: svc_type });
    apply_with_spec(w, &name, spec).await;
}

#[when(regex = r#"^I apply an Instance "([^"]+)" with ingress class "([^"]+)" and host "([^"]+)"$"#)]
async fn when_apply_ingress(w: &mut E2eWorld, name: String, class: String, host: String) {
    let mut spec = base_spec("nginx:alpine");
    spec.host = Some(host);
    spec.networking = Some(NetworkingSpec {
        ingress: Some(IngressConfig {
            class_name: Some(class),
            tls_secret_name: None,
        }),
        http_route: None,
    });
    apply_with_spec(w, &name, spec).await;
}

#[when(regex = r#"^I apply an Instance "([^"]+)" with image "([^"]+)" and encryption key from secret "([^"]+)" key "([^"]+)"$"#)]
async fn when_apply_byo_key(
    w: &mut E2eWorld,
    name: String,
    image: String,
    secret_name: String,
    secret_key: String,
) {
    let mut spec = base_spec(&image);
    spec.encryption_key = Some(EncryptionKeySpec {
        secret_ref: Some(SecretKeyRef {
            name: secret_name,
            key: secret_key,
        }),
    });
    apply_with_spec(w, &name, spec).await;
}

#[when(regex = r#"^I apply an Instance "([^"]+)" with both ingress and httpRoute$"#)]
async fn when_apply_both(w: &mut E2eWorld, name: String) {
    let mut spec = base_spec("nginx:alpine");
    spec.networking = Some(NetworkingSpec {
        ingress: Some(IngressConfig {
            class_name: Some("nginx".into()),
            tls_secret_name: None,
        }),
        http_route: Some(HttpRouteConfig {
            gateway: GatewayRef {
                name: "gw".into(),
                namespace: None,
            },
        }),
    });
    apply_with_spec(w, &name, spec).await;
}

#[when(regex = r#"^I update the Instance "([^"]+)" to have no networking$"#)]
async fn when_drop_networking(w: &mut E2eWorld, name: String) {
    let api: Api<Instance> = Api::namespaced(w.client().clone(), NS);
    let current = api.get(&name).await.expect("Instance");
    let mut spec = current.spec.clone();
    spec.networking = None;
    let new = Instance::new(&name, spec);
    let ssa = PatchParams::apply("cucumber").force();
    api.patch(&name, &ssa, &Patch::Apply(&new))
        .await
        .expect("update Instance");
}

#[when(regex = r#"^I delete the Instance "([^"]+)"$"#)]
async fn when_delete_instance(w: &mut E2eWorld, name: String) {
    let api: Api<Instance> = Api::namespaced(w.client().clone(), NS);
    api.delete(&name, &DeleteParams::default())
        .await
        .expect("delete Instance");
}

// ----- Then -----

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

#[then(regex = r#"^the Service "([^"]+)" has type "([^"]+)"$"#)]
async fn service_has_type(w: &mut E2eWorld, name: String, expected: String) {
    let api: Api<Service> = Api::namespaced(w.client().clone(), NS);
    let svc = api.get(&name).await.expect("Service");
    let ty = svc
        .spec
        .as_ref()
        .and_then(|s| s.type_.clone())
        .expect("no service type");
    assert_eq!(ty, expected);
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

#[then(regex = r#"^a Secret named "([^"]+)" eventually exists with a non-empty key "([^"]+)"$"#)]
async fn secret_with_key(w: &mut E2eWorld, name: String, key: String) {
    let client = w.client().clone();
    let n = name.clone();
    let k = key.clone();
    wait_until(60, &format!("Secret/{name}.{key}"), move || {
        let client = client.clone();
        let n = n.clone();
        let k = k.clone();
        async move {
            let api: Api<Secret> = Api::namespaced(client, NS);
            match api.get_opt(&n).await.unwrap() {
                Some(s) => {
                    let data = s.data.as_ref();
                    data.and_then(|m| m.get(&k))
                        .map(|v| !v.0.is_empty())
                        .unwrap_or(false)
                }
                None => false,
            }
        }
    })
    .await;
}

#[then(regex = r#"^the Secret "([^"]+)" is owned by the Instance "([^"]+)"$"#)]
async fn secret_owned(w: &mut E2eWorld, secret: String, owner: String) {
    let api: Api<Secret> = Api::namespaced(w.client().clone(), NS);
    let s = api.get(&secret).await.expect("Secret");
    let owners = s.owner_references();
    assert!(
        owners.iter().any(|o| o.kind == "Instance" && o.name == owner),
        "Secret/{secret} has no Instance/{owner} owner, got {:?}",
        owners
    );
}

#[then(regex = r#"^the Deployment "([^"]+)" sources env var "([^"]+)" from secret "([^"]+)" key "([^"]+)"$"#)]
async fn deployment_env(
    w: &mut E2eWorld,
    deployment: String,
    var: String,
    secret: String,
    key: String,
) {
    let api: Api<Deployment> = Api::namespaced(w.client().clone(), NS);
    let dep = api.get(&deployment).await.expect("Deployment");
    let containers = dep
        .spec
        .and_then(|s| s.template.spec)
        .map(|s| s.containers)
        .expect("containers");
    let envs = containers
        .first()
        .and_then(|c| c.env.clone())
        .unwrap_or_default();
    let env = envs
        .iter()
        .find(|e| e.name == var)
        .unwrap_or_else(|| panic!("env var {var} not found"));
    let secret_ref = env
        .value_from
        .as_ref()
        .and_then(|v| v.secret_key_ref.as_ref())
        .expect("env var not sourced from a secret");
    assert_eq!(secret_ref.name, secret, "secret name mismatch");
    assert_eq!(secret_ref.key, key, "secret key mismatch");
}

#[then(regex = r#"^no Secret named "([^"]+)" exists$"#)]
async fn no_secret(w: &mut E2eWorld, name: String) {
    let api: Api<Secret> = Api::namespaced(w.client().clone(), NS);
    // small grace so the operator has a chance to (not) create it
    sleep(Duration::from_secs(3)).await;
    assert!(api.get_opt(&name).await.unwrap().is_none(), "Secret/{name} unexpectedly exists");
}

#[then(regex = r#"^an Ingress named "([^"]+)" exists with host "([^"]+)" within (\d+) seconds$"#)]
async fn ingress_exists(w: &mut E2eWorld, name: String, host: String, secs: u64) {
    let client = w.client().clone();
    let n = name.clone();
    let h = host.clone();
    wait_until(secs, &format!("Ingress/{name} host={host}"), move || {
        let client = client.clone();
        let n = n.clone();
        let h = h.clone();
        async move {
            let api: Api<Ingress> = Api::namespaced(client, NS);
            match api.get_opt(&n).await.unwrap() {
                Some(ing) => ing
                    .spec
                    .and_then(|s| s.rules)
                    .map(|rules| rules.iter().any(|r| r.host.as_deref() == Some(&h)))
                    .unwrap_or(false),
                None => false,
            }
        }
    })
    .await;
}

#[then(regex = r#"^the Ingress "([^"]+)" is gone within (\d+) seconds$"#)]
async fn ingress_gone(w: &mut E2eWorld, name: String, secs: u64) {
    let client = w.client().clone();
    let n = name.clone();
    wait_until(secs, &format!("Ingress/{name} gone"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api: Api<Ingress> = Api::namespaced(client, NS);
            api.get_opt(&n).await.unwrap().is_none()
        }
    })
    .await;
}

#[then(regex = r#"^the Instance "([^"]+)" never reaches status.ready=true within (\d+) seconds$"#)]
async fn never_ready(w: &mut E2eWorld, name: String, secs: u64) {
    let api: Api<Instance> = Api::namespaced(w.client().clone(), NS);
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        let inst = api.get(&name).await.expect("Instance");
        if inst.status.as_ref().map(|s| s.ready).unwrap_or(false) {
            panic!("Instance/{name} became ready, but spec is invalid (mutex)");
        }
        sleep(Duration::from_millis(500)).await;
    }
}

#[then(regex = r#"^no Ingress named "([^"]+)" exists$"#)]
async fn no_ingress(w: &mut E2eWorld, name: String) {
    let api: Api<Ingress> = Api::namespaced(w.client().clone(), NS);
    assert!(api.get_opt(&name).await.unwrap().is_none(), "Ingress/{name} unexpectedly exists");
}

// ----- main -----

#[tokio::main]
async fn main() {
    E2eWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("features")
        .await;
}
