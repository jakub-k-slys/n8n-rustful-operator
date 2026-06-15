use cucumber::{World, given, then, when};
use k8s_openapi::api::{
    apps::v1::Deployment,
    autoscaling::v2::HorizontalPodAutoscaler,
    core::v1::{PersistentVolumeClaim, Secret, Service},
    networking::v1::Ingress,
};
use kube::{
    Client,
    api::{Api, DeleteParams, DynamicObject, GroupVersionKind, ObjectMeta, Patch, PatchParams, ResourceExt},
    discovery::ApiResource,
};
use n8n_rustful_operator::{
    Autoscaling, Cluster, ClusterSpec, DatabaseSpec, DatabaseSsl, EncryptionKeySpec, GatewayRef,
    HttpRouteConfig, IngressConfig, MainConfig, MysqlConfig, NetworkingSpec, PersistenceConfig,
    PostgresConfig, RedisConfig, SecretKeyRef, ServiceConfig, Single, SingleSpec, SqliteConfig,
    WebhookConfig, WorkerConfig,
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

fn base_spec(image: &str) -> SingleSpec {
    SingleSpec {
        image: image.into(),
        replicas: 1,
        host: Some("e2e.example.com".into()),
        service: None,
        networking: None,
        encryption_key: None,
        database: None,
        persistence: None,
    }
}

async fn apply_with_spec(w: &mut E2eWorld, name: &str, spec: SingleSpec) {
    let api: Api<Single> = Api::namespaced(w.client().clone(), NS);
    let inst = Single::new(name, spec);
    let ssa = PatchParams::apply("cucumber").force();
    api.patch(name, &ssa, &Patch::Apply(&inst))
        .await
        .expect("apply Single");
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
    let ready = dep.status.as_ref().and_then(|s| s.ready_replicas).unwrap_or(0);
    assert!(ready >= 1, "operator deployment has no ready replicas");
    w.client = Some(client);
}

#[given(regex = r#"^a Single "([^"]+)" exists$"#)]
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

#[given(regex = r#"^a Single "([^"]+)" exists with ingress class "([^"]+)" and host "([^"]+)"$"#)]
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
    let ssa = PatchParams::apply("cucumber").force();
    api.patch(&name, &ssa, &Patch::Apply(&secret))
        .await
        .expect("upsert Secret");
}

// ----- When -----

#[when(regex = r#"^I apply a Single "([^"]+)" with image "([^"]+)"$"#)]
async fn when_apply_basic(w: &mut E2eWorld, name: String, image: String) {
    apply_with_spec(w, &name, base_spec(&image)).await;
}

#[when(regex = r#"^I apply a Single "([^"]+)" with image "([^"]+)" and service type "([^"]+)"$"#)]
async fn when_apply_svc_type(w: &mut E2eWorld, name: String, image: String, svc_type: String) {
    let mut spec = base_spec(&image);
    spec.service = Some(ServiceConfig { type_: svc_type });
    apply_with_spec(w, &name, spec).await;
}

#[when(regex = r#"^I apply a Single "([^"]+)" with ingress class "([^"]+)" and host "([^"]+)"$"#)]
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

#[when(
    regex = r#"^I apply a Single "([^"]+)" with image "([^"]+)" and encryption key from secret "([^"]+)" key "([^"]+)"$"#
)]
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

#[when(regex = r#"^I apply a Single "([^"]+)" with both ingress and httpRoute$"#)]
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

#[when(regex = r#"^I update the Single "([^"]+)" to have no networking$"#)]
async fn when_drop_networking(w: &mut E2eWorld, name: String) {
    let api: Api<Single> = Api::namespaced(w.client().clone(), NS);
    let current = api.get(&name).await.expect("Single");
    let mut spec = current.spec.clone();
    spec.networking = None;
    let new = Single::new(&name, spec);
    let ssa = PatchParams::apply("cucumber").force();
    api.patch(&name, &ssa, &Patch::Apply(&new))
        .await
        .expect("update Single");
}

#[when(regex = r#"^I delete the Single "([^"]+)"$"#)]
async fn when_delete_instance(w: &mut E2eWorld, name: String) {
    let api: Api<Single> = Api::namespaced(w.client().clone(), NS);
    api.delete(&name, &DeleteParams::default())
        .await
        .expect("delete Single");
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
    let client = w.client().clone();
    let n = name.clone();
    wait_until(60, &format!("Service/{name} port={port}"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api: Api<Service> = Api::namespaced(client, NS);
            match api.get_opt(&n).await.unwrap() {
                Some(svc) => {
                    svc.spec
                        .and_then(|s| s.ports)
                        .and_then(|ports| ports.first().map(|p| p.port))
                        == Some(port)
                }
                None => false,
            }
        }
    })
    .await;
}

#[then(regex = r#"^the Service "([^"]+)" has type "([^"]+)"$"#)]
async fn service_has_type(w: &mut E2eWorld, name: String, expected: String) {
    let client = w.client().clone();
    let n = name.clone();
    let exp = expected.clone();
    wait_until(60, &format!("Service/{name} type={expected}"), move || {
        let client = client.clone();
        let n = n.clone();
        let exp = exp.clone();
        async move {
            let api: Api<Service> = Api::namespaced(client, NS);
            match api.get_opt(&n).await.unwrap() {
                Some(svc) => svc.spec.and_then(|s| s.type_) == Some(exp),
                None => false,
            }
        }
    })
    .await;
}

#[then(regex = r#"^the Single "([^"]+)" has status.ready set to true within (\d+) seconds$"#)]
async fn status_ready(w: &mut E2eWorld, name: String, secs: u64) {
    let client = w.client().clone();
    let n = name.clone();
    wait_until(secs, &format!("Single/{name} status.ready"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api: Api<Single> = Api::namespaced(client, NS);
            api.get(&n)
                .await
                .ok()
                .and_then(|i| i.status.map(|s| s.ready))
                .unwrap_or(false)
        }
    })
    .await;
}

#[then(regex = r#"^the Single "([^"]+)" has the finalizer "([^"]+)"$"#)]
async fn has_finalizer(w: &mut E2eWorld, name: String, finalizer: String) {
    let api: Api<Single> = Api::namespaced(w.client().clone(), NS);
    let inst = api.get(&name).await.expect("Single");
    let finalizers = inst.finalizers();
    assert!(
        finalizers.iter().any(|f| f == &finalizer),
        "finalizer {finalizer:?} not in {finalizers:?}"
    );
}

#[then(regex = r#"^the Single "([^"]+)" is gone within (\d+) seconds$"#)]
async fn instance_gone(w: &mut E2eWorld, name: String, secs: u64) {
    let client = w.client().clone();
    let n = name.clone();
    wait_until(secs, &format!("Single/{name} gone"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api: Api<Single> = Api::namespaced(client, NS);
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

#[then(regex = r#"^the Secret "([^"]+)" is owned by the Single "([^"]+)"$"#)]
async fn secret_owned(w: &mut E2eWorld, secret: String, owner: String) {
    let api: Api<Secret> = Api::namespaced(w.client().clone(), NS);
    let s = api.get(&secret).await.expect("Secret");
    let owners = s.owner_references();
    assert!(
        owners.iter().any(|o| o.kind == "Single" && o.name == owner),
        "Secret/{secret} has no Single/{owner} owner, got {:?}",
        owners
    );
}

#[then(regex = r#"^the Deployment "([^"]+)" sources env var "([^"]+)" from secret "([^"]+)" key "([^"]+)"$"#)]
async fn deployment_env(w: &mut E2eWorld, deployment: String, var: String, secret: String, key: String) {
    let client = w.client().clone();
    let d = deployment.clone();
    let v = var.clone();
    let s = secret.clone();
    let k = key.clone();
    wait_until(
        60,
        &format!("Deployment/{deployment} env {var} ← secret {secret}/{key}"),
        move || {
            let client = client.clone();
            let d = d.clone();
            let v = v.clone();
            let s = s.clone();
            let k = k.clone();
            async move {
                let api: Api<Deployment> = Api::namespaced(client, NS);
                let Some(dep) = api.get_opt(&d).await.unwrap() else {
                    return false;
                };
                let env = deployment_env_var(&dep, &v);
                env.and_then(|e| e.value_from)
                    .and_then(|vf| vf.secret_key_ref)
                    .map(|r| r.name == s && r.key == k)
                    .unwrap_or(false)
            }
        },
    )
    .await;
}

#[then(regex = r#"^no Secret named "([^"]+)" exists$"#)]
async fn no_secret(w: &mut E2eWorld, name: String) {
    let api: Api<Secret> = Api::namespaced(w.client().clone(), NS);
    // small grace so the operator has a chance to (not) create it
    sleep(Duration::from_secs(3)).await;
    assert!(
        api.get_opt(&name).await.unwrap().is_none(),
        "Secret/{name} unexpectedly exists"
    );
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

#[then(regex = r#"^the Single "([^"]+)" never reaches status.ready=true within (\d+) seconds$"#)]
async fn never_ready(w: &mut E2eWorld, name: String, secs: u64) {
    let api: Api<Single> = Api::namespaced(w.client().clone(), NS);
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        let inst = api.get(&name).await.expect("Single");
        if inst.status.as_ref().map(|s| s.ready).unwrap_or(false) {
            panic!("Single/{name} became ready, but spec is invalid (mutex)");
        }
        sleep(Duration::from_millis(500)).await;
    }
}

#[then(regex = r#"^no Ingress named "([^"]+)" exists$"#)]
async fn no_ingress(w: &mut E2eWorld, name: String) {
    let api: Api<Ingress> = Api::namespaced(w.client().clone(), NS);
    assert!(
        api.get_opt(&name).await.unwrap().is_none(),
        "Ingress/{name} unexpectedly exists"
    );
}

// ----- database -----

#[allow(clippy::too_many_arguments)]
#[when(
    regex = r#"^I apply a Single "([^"]+)" with Postgres host "([^"]+)" port (\d+) database "([^"]+)" user "([^"]+)" password from secret "([^"]+)" key "([^"]+)" schema "([^"]+)" pool size (\d+)$"#
)]
async fn apply_postgres_full(
    w: &mut E2eWorld,
    name: String,
    host: String,
    port: i32,
    database: String,
    user: String,
    secret: String,
    key: String,
    schema: String,
    pool_size: u32,
) {
    let mut spec = base_spec("nginx:alpine");
    spec.database = Some(DatabaseSpec {
        type_: "postgresdb".into(),
        sqlite: None,
        postgres: Some(PostgresConfig {
            host,
            port: Some(port),
            database,
            user,
            password_secret: SecretKeyRef { name: secret, key },
            schema: Some(schema),
            ssl: None,
            pool_size: Some(pool_size),
            connection_timeout_ms: None,
        }),
        mysql: None,
    });
    apply_with_spec(w, &name, spec).await;
}

#[allow(clippy::too_many_arguments)]
#[when(
    regex = r#"^I apply a Single "([^"]+)" with Postgres host "([^"]+)" database "([^"]+)" user "([^"]+)" password from secret "([^"]+)" key "([^"]+)" and SSL CA from secret "([^"]+)" key "([^"]+)"$"#
)]
async fn apply_postgres_ssl(
    w: &mut E2eWorld,
    name: String,
    host: String,
    database: String,
    user: String,
    pw_secret: String,
    pw_key: String,
    ca_secret: String,
    ca_key: String,
) {
    let mut spec = base_spec("nginx:alpine");
    spec.database = Some(DatabaseSpec {
        type_: "postgresdb".into(),
        sqlite: None,
        postgres: Some(PostgresConfig {
            host,
            port: None,
            database,
            user,
            password_secret: SecretKeyRef {
                name: pw_secret,
                key: pw_key,
            },
            schema: None,
            ssl: Some(DatabaseSsl {
                enabled: true,
                reject_unauthorized: None,
                ca_secret: Some(SecretKeyRef {
                    name: ca_secret,
                    key: ca_key,
                }),
                cert_secret: None,
                key_secret: None,
            }),
            pool_size: None,
            connection_timeout_ms: None,
        }),
        mysql: None,
    });
    apply_with_spec(w, &name, spec).await;
}

#[allow(clippy::too_many_arguments)]
#[when(
    regex = r#"^I apply a Single "([^"]+)" with MySQL host "([^"]+)" port (\d+) database "([^"]+)" user "([^"]+)" password from secret "([^"]+)" key "([^"]+)"$"#
)]
async fn apply_mysql(
    w: &mut E2eWorld,
    name: String,
    host: String,
    port: i32,
    database: String,
    user: String,
    secret: String,
    key: String,
) {
    let mut spec = base_spec("nginx:alpine");
    spec.database = Some(DatabaseSpec {
        type_: "mysqldb".into(),
        sqlite: None,
        postgres: None,
        mysql: Some(MysqlConfig {
            host,
            port: Some(port),
            database,
            user,
            password_secret: SecretKeyRef { name: secret, key },
            ssl: None,
            connection_timeout_ms: None,
        }),
    });
    apply_with_spec(w, &name, spec).await;
}

#[when(regex = r#"^I apply a Single "([^"]+)" with persistence size "([^"]+)"$"#)]
async fn apply_with_persistence(w: &mut E2eWorld, name: String, size: String) {
    let mut spec = base_spec("nginx:alpine");
    spec.persistence = Some(PersistenceConfig {
        size,
        storage_class_name: None,
        access_mode: "ReadWriteOnce".into(),
    });
    apply_with_spec(w, &name, spec).await;
}

#[when(regex = r#"^I apply a Single "([^"]+)" with database type "([^"]+)" and only a MySQL config$"#)]
async fn apply_db_type_mismatch(w: &mut E2eWorld, name: String, type_: String) {
    let mut spec = base_spec("nginx:alpine");
    spec.database = Some(DatabaseSpec {
        type_,
        sqlite: None,
        postgres: None,
        mysql: Some(MysqlConfig {
            host: "wrong.example.com".into(),
            port: None,
            database: "n8n".into(),
            user: "n8n".into(),
            password_secret: SecretKeyRef {
                name: "pg-creds".into(),
                key: "password".into(),
            },
            ssl: None,
            connection_timeout_ms: None,
        }),
    });
    apply_with_spec(w, &name, spec).await;
}

fn deployment_env_var(dep: &Deployment, var: &str) -> Option<k8s_openapi::api::core::v1::EnvVar> {
    let containers = dep.spec.as_ref()?.template.spec.as_ref()?.containers.clone();
    let env = containers.first().and_then(|c| c.env.clone()).unwrap_or_default();
    env.into_iter().find(|e| e.name == var)
}

#[then(regex = r#"^the Deployment "([^"]+)" has env var "([^"]+)" set to "([^"]+)"$"#)]
async fn deployment_env_value(w: &mut E2eWorld, name: String, var: String, expected: String) {
    let client = w.client().clone();
    let n = name.clone();
    let v = var.clone();
    let exp = expected.clone();
    wait_until(
        60,
        &format!("Deployment/{name} env {var}={expected}"),
        move || {
            let client = client.clone();
            let n = n.clone();
            let v = v.clone();
            let exp = exp.clone();
            async move {
                let api: Api<Deployment> = Api::namespaced(client, NS);
                match api.get_opt(&n).await.unwrap() {
                    Some(d) => deployment_env_var(&d, &v).and_then(|e| e.value) == Some(exp),
                    None => false,
                }
            }
        },
    )
    .await;
}

#[then(regex = r#"^the Deployment "([^"]+)" has no env var "([^"]+)"$"#)]
async fn deployment_env_absent(w: &mut E2eWorld, name: String, var: String) {
    let api: Api<Deployment> = Api::namespaced(w.client().clone(), NS);
    // Wait a short period so reconciler had a chance to render the Deployment.
    sleep(Duration::from_secs(2)).await;
    let dep = api.get(&name).await.expect("Deployment");
    assert!(
        deployment_env_var(&dep, &var).is_none(),
        "env var {var} unexpectedly present"
    );
}

#[then(regex = r#"^a PersistentVolumeClaim named "([^"]+)" exists with size "([^"]+)"$"#)]
async fn pvc_exists(w: &mut E2eWorld, name: String, size: String) {
    let client = w.client().clone();
    let n = name.clone();
    let s = size.clone();
    wait_until(60, &format!("PVC/{name} size={size}"), move || {
        let client = client.clone();
        let n = n.clone();
        let s = s.clone();
        async move {
            let api: Api<PersistentVolumeClaim> = Api::namespaced(client, NS);
            match api.get_opt(&n).await.unwrap() {
                Some(p) => {
                    p.spec
                        .and_then(|sp| sp.resources)
                        .and_then(|r| r.requests)
                        .and_then(|r| r.get("storage").map(|q| q.0.clone()))
                        == Some(s.clone())
                }
                None => false,
            }
        }
    })
    .await;
}

#[then(regex = r#"^the Deployment "([^"]+)" mounts pvc "([^"]+)" at "([^"]+)"$"#)]
async fn deployment_mounts_pvc(w: &mut E2eWorld, name: String, pvc: String, path: String) {
    let api: Api<Deployment> = Api::namespaced(w.client().clone(), NS);
    let dep = api.get(&name).await.expect("Deployment");
    let pod_spec = dep.spec.and_then(|s| s.template.spec).expect("pod spec");
    let vol = pod_spec
        .volumes
        .unwrap_or_default()
        .into_iter()
        .find(|v| {
            v.persistent_volume_claim
                .as_ref()
                .map(|p| p.claim_name == pvc)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("no PVC volume claiming {pvc}"));
    let containers = pod_spec.containers;
    let mounts = containers
        .first()
        .and_then(|c| c.volume_mounts.clone())
        .unwrap_or_default();
    let mount = mounts
        .into_iter()
        .find(|m| m.name == vol.name)
        .unwrap_or_else(|| panic!("no mount for volume {}", vol.name));
    assert_eq!(mount.mount_path, path);
}

#[then(regex = r#"^the Deployment "([^"]+)" mounts secret "([^"]+)" at "([^"]+)"$"#)]
async fn deployment_mounts_secret(w: &mut E2eWorld, name: String, secret: String, path: String) {
    let api: Api<Deployment> = Api::namespaced(w.client().clone(), NS);
    let dep = api.get(&name).await.expect("Deployment");
    let pod_spec = dep.spec.and_then(|s| s.template.spec).expect("pod spec");
    let vol = pod_spec
        .volumes
        .unwrap_or_default()
        .into_iter()
        .find(|v| {
            v.secret
                .as_ref()
                .and_then(|s| s.secret_name.as_deref())
                .map(|n| n == secret)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("no secret volume referencing {secret}"));
    let containers = pod_spec.containers;
    let mount = containers
        .first()
        .and_then(|c| c.volume_mounts.clone())
        .unwrap_or_default()
        .into_iter()
        .find(|m| m.name == vol.name)
        .unwrap_or_else(|| panic!("no mount for volume {}", vol.name));
    // The mount itself is a directory; the file lives under it.
    assert!(
        path.starts_with(&mount.mount_path),
        "mount path {} doesn't contain {path}",
        mount.mount_path
    );
}

#[then(regex = r#"^the Deployment "([^"]+)" has label "([^=]+)=([^"]+)"$"#)]
async fn deployment_has_label(w: &mut E2eWorld, name: String, key: String, value: String) {
    let api: Api<Deployment> = Api::namespaced(w.client().clone(), NS);
    let dep = api.get(&name).await.expect("Deployment");
    let labels = dep.metadata.labels.unwrap_or_default();
    let got = labels
        .get(&key)
        .unwrap_or_else(|| panic!("no label {key}, got {labels:?}"));
    assert_eq!(got, &value);
}

#[then(regex = r#"^the Deployment "([^"]+)" has annotation "([^"]+)"$"#)]
async fn deployment_has_annotation(w: &mut E2eWorld, name: String, key: String) {
    let api: Api<Deployment> = Api::namespaced(w.client().clone(), NS);
    let dep = api.get(&name).await.expect("Deployment");
    let ann = dep.metadata.annotations.unwrap_or_default();
    assert!(ann.contains_key(&key), "no annotation {key}, got {ann:?}");
}

#[then(regex = r#"^the Deployment "([^"]+)" pods select on label "([^=]+)=([^"]+)"$"#)]
async fn deployment_selects_label(w: &mut E2eWorld, name: String, key: String, value: String) {
    let api: Api<Deployment> = Api::namespaced(w.client().clone(), NS);
    let dep = api.get(&name).await.expect("Deployment");
    let selector = dep
        .spec
        .and_then(|s| s.selector.match_labels)
        .expect("Deployment.spec.selector.matchLabels missing");
    let got = selector
        .get(&key)
        .unwrap_or_else(|| panic!("selector has no label {key}"));
    assert_eq!(got, &value, "selector {key} mismatch");
}

#[then(regex = r#"^a Secret named "([^"]+)" exists$"#)]
async fn secret_exists(w: &mut E2eWorld, name: String) {
    let client = w.client().clone();
    let n = name.clone();
    wait_until(60, &format!("Secret/{name} to appear"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api: Api<Secret> = Api::namespaced(client, NS);
            api.get_opt(&n).await.unwrap().is_some()
        }
    })
    .await;
}

// ----- Cluster -----

fn pg_postgres_config() -> PostgresConfig {
    PostgresConfig {
        host: "pg.example.com".into(),
        port: Some(5432),
        database: "n8n".into(),
        user: "n8n".into(),
        password_secret: SecretKeyRef {
            name: "pg-creds".into(),
            key: "password".into(),
        },
        schema: None,
        ssl: None,
        pool_size: None,
        connection_timeout_ms: None,
    }
}

async fn apply_cluster(w: &mut E2eWorld, name: &str, spec: ClusterSpec) {
    let api: Api<Cluster> = Api::namespaced(w.client().clone(), NS);
    let c = Cluster::new(name, spec);
    let ssa = PatchParams::apply("cucumber").force();
    api.patch(name, &ssa, &Patch::Apply(&c))
        .await
        .expect("apply Cluster");
}

#[when(
    regex = r#"^I apply a Cluster "([^"]+)" backed by Postgres "([^"]+)" and Redis "([^"]+)" with (\d+) workers and webhooks$"#
)]
async fn apply_cluster_full(
    w: &mut E2eWorld,
    name: String,
    pg_host: String,
    redis_host: String,
    workers: i32,
) {
    let mut pg = pg_postgres_config();
    pg.host = pg_host;
    let spec = ClusterSpec {
        image: "nginx:alpine".into(),
        encryption_key: None,
        database: DatabaseSpec {
            type_: "postgresdb".into(),
            sqlite: None,
            postgres: Some(pg),
            mysql: None,
        },
        redis: RedisConfig {
            host: redis_host,
            port: Some(6379),
            db: Some(0),
            password_secret: Some(SecretKeyRef {
                name: "redis-creds".into(),
                key: "password".into(),
            }),
            username_secret: None,
            tls: None,
            prefix: None,
        },
        main: MainConfig {
            replicas: 1,
            ..Default::default()
        },
        workers: WorkerConfig {
            replicas: workers,
            image: None,
            concurrency: Some(5),
            autoscaling: None,
        },
        webhooks: Some(WebhookConfig {
            replicas: 1,
            image: None,
            host: None,
            service: None,
            networking: None,
        }),
    };
    apply_cluster(w, &name, spec).await;
}

#[when(regex = r#"^I apply a Cluster "([^"]+)" with main persistence size "([^"]+)"$"#)]
async fn apply_cluster_with_main_pv(w: &mut E2eWorld, name: String, size: String) {
    let mut pg = pg_postgres_config();
    pg.host = "pg.example.com".into();
    let spec = ClusterSpec {
        image: "nginx:alpine".into(),
        encryption_key: None,
        database: DatabaseSpec {
            type_: "postgresdb".into(),
            sqlite: None,
            postgres: Some(pg),
            mysql: None,
        },
        redis: RedisConfig {
            host: "redis.example.com".into(),
            port: Some(6379),
            db: None,
            password_secret: Some(SecretKeyRef {
                name: "redis-creds".into(),
                key: "password".into(),
            }),
            username_secret: None,
            tls: None,
            prefix: None,
        },
        main: MainConfig {
            replicas: 1,
            persistence: Some(PersistenceConfig {
                size,
                storage_class_name: None,
                access_mode: "ReadWriteOnce".into(),
            }),
            ..Default::default()
        },
        workers: WorkerConfig {
            replicas: 1,
            image: None,
            concurrency: None,
            autoscaling: None,
        },
        webhooks: None,
    };
    apply_cluster(w, &name, spec).await;
}

#[when(regex = r#"^I apply a Cluster "([^"]+)" with sqlite database$"#)]
async fn apply_cluster_sqlite(w: &mut E2eWorld, name: String) {
    let spec = ClusterSpec {
        image: "nginx:alpine".into(),
        encryption_key: None,
        database: DatabaseSpec {
            type_: "sqlite".into(),
            sqlite: Some(SqliteConfig {
                pool_size: None,
                vacuum_on_startup: None,
                database: None,
            }),
            postgres: None,
            mysql: None,
        },
        redis: RedisConfig {
            host: "redis.example.com".into(),
            ..Default::default()
        },
        main: MainConfig::default(),
        workers: WorkerConfig {
            replicas: 1,
            image: None,
            concurrency: None,
            autoscaling: None,
        },
        webhooks: None,
    };
    apply_cluster(w, &name, spec).await;
}

#[given(regex = r#"^a Cluster "([^"]+)" exists with webhooks$"#)]
async fn cluster_with_webhooks(w: &mut E2eWorld, name: String) {
    apply_cluster_full(
        w,
        name.clone(),
        "pg.example.com".into(),
        "redis.example.com".into(),
        1,
    )
    .await;
    let client = w.client().clone();
    let wh = format!("{name}-webhook");
    let wh2 = wh.clone();
    wait_until(60, &format!("Deployment/{wh2} to appear"), move || {
        let client = client.clone();
        let n = wh.clone();
        async move {
            let api: Api<Deployment> = Api::namespaced(client, NS);
            api.get_opt(&n).await.unwrap().is_some()
        }
    })
    .await;
}

#[when(regex = r#"^I update the Cluster "([^"]+)" to have no webhooks$"#)]
async fn drop_cluster_webhooks(w: &mut E2eWorld, name: String) {
    let api: Api<Cluster> = Api::namespaced(w.client().clone(), NS);
    let current = api.get(&name).await.expect("Cluster");
    let mut spec = current.spec.clone();
    spec.webhooks = None;
    let new = Cluster::new(&name, spec);
    let ssa = PatchParams::apply("cucumber").force();
    api.patch(&name, &ssa, &Patch::Apply(&new))
        .await
        .expect("update Cluster");
}

#[then(regex = r#"^the Cluster "([^"]+)" never reaches status.ready=true within (\d+) seconds$"#)]
async fn cluster_never_ready(w: &mut E2eWorld, name: String, secs: u64) {
    let api: Api<Cluster> = Api::namespaced(w.client().clone(), NS);
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        let c = api.get(&name).await.expect("Cluster");
        if c.status.as_ref().map(|s| s.ready).unwrap_or(false) {
            panic!("Cluster/{name} became ready, but spec is invalid");
        }
        sleep(Duration::from_millis(500)).await;
    }
}

#[then(regex = r#"^the Deployment "([^"]+)" is gone within (\d+) seconds$"#)]
async fn deployment_gone(w: &mut E2eWorld, name: String, secs: u64) {
    let client = w.client().clone();
    let n = name.clone();
    wait_until(secs, &format!("Deployment/{name} gone"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api: Api<Deployment> = Api::namespaced(client, NS);
            api.get_opt(&n).await.unwrap().is_none()
        }
    })
    .await;
}

// ----- extra Single steps -----

#[when(regex = r#"^I apply a Single "([^"]+)" with image "([^"]+)" and replicas (\d+)$"#)]
async fn apply_single_replicas(w: &mut E2eWorld, name: String, image: String, replicas: i32) {
    let mut spec = base_spec(&image);
    spec.replicas = replicas;
    apply_with_spec(w, &name, spec).await;
}

#[when(
    regex = r#"^I apply a Single "([^"]+)" with ingress class "([^"]+)" host "([^"]+)" and TLS secret "([^"]+)"$"#
)]
async fn apply_single_ingress_tls(w: &mut E2eWorld, name: String, class: String, host: String, tls: String) {
    let mut spec = base_spec("nginx:alpine");
    spec.host = Some(host);
    spec.networking = Some(NetworkingSpec {
        ingress: Some(IngressConfig {
            class_name: Some(class),
            tls_secret_name: Some(tls),
        }),
        http_route: None,
    });
    apply_with_spec(w, &name, spec).await;
}

#[then(regex = r#"^the Deployment "([^"]+)" has (\d+) replicas$"#)]
async fn deployment_has_replicas(w: &mut E2eWorld, name: String, replicas: i32) {
    let client = w.client().clone();
    let n = name.clone();
    wait_until(60, &format!("Deployment/{name} replicas={replicas}"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api: Api<Deployment> = Api::namespaced(client, NS);
            match api.get_opt(&n).await.unwrap() {
                Some(d) => d.spec.and_then(|s| s.replicas) == Some(replicas),
                None => false,
            }
        }
    })
    .await;
}

#[then(regex = r#"^the Ingress "([^"]+)" terminates TLS with secret "([^"]+)"$"#)]
async fn ingress_tls(w: &mut E2eWorld, name: String, secret: String) {
    let api: Api<Ingress> = Api::namespaced(w.client().clone(), NS);
    let ing = api.get(&name).await.expect("Ingress");
    let tls = ing
        .spec
        .and_then(|s| s.tls)
        .and_then(|v| v.into_iter().next())
        .expect("no TLS block");
    assert_eq!(tls.secret_name.as_deref(), Some(secret.as_str()));
}

// ----- extra Cluster steps -----

#[when(regex = r#"^I apply a Cluster "([^"]+)" with encryption key from secret "([^"]+)" key "([^"]+)"$"#)]
async fn apply_cluster_byo_key(w: &mut E2eWorld, name: String, secret: String, key: String) {
    let spec = ClusterSpec {
        image: "nginx:alpine".into(),
        encryption_key: Some(EncryptionKeySpec {
            secret_ref: Some(SecretKeyRef { name: secret, key }),
        }),
        database: DatabaseSpec {
            type_: "postgresdb".into(),
            sqlite: None,
            postgres: Some(pg_postgres_config()),
            mysql: None,
        },
        redis: RedisConfig {
            host: "redis.example.com".into(),
            port: Some(6379),
            password_secret: Some(SecretKeyRef {
                name: "redis-creds".into(),
                key: "password".into(),
            }),
            ..Default::default()
        },
        main: MainConfig {
            replicas: 1,
            ..Default::default()
        },
        workers: WorkerConfig {
            replicas: 1,
            image: None,
            concurrency: None,
            autoscaling: None,
        },
        webhooks: None,
    };
    apply_cluster(w, &name, spec).await;
}

#[when(regex = r#"^I apply a Cluster "([^"]+)" with main ingress class "([^"]+)" and host "([^"]+)"$"#)]
async fn apply_cluster_main_ingress(w: &mut E2eWorld, name: String, class: String, host: String) {
    let spec = ClusterSpec {
        image: "nginx:alpine".into(),
        encryption_key: None,
        database: DatabaseSpec {
            type_: "postgresdb".into(),
            sqlite: None,
            postgres: Some(pg_postgres_config()),
            mysql: None,
        },
        redis: RedisConfig {
            host: "redis.example.com".into(),
            port: Some(6379),
            password_secret: Some(SecretKeyRef {
                name: "redis-creds".into(),
                key: "password".into(),
            }),
            ..Default::default()
        },
        main: MainConfig {
            replicas: 1,
            host: Some(host),
            networking: Some(NetworkingSpec {
                ingress: Some(IngressConfig {
                    class_name: Some(class),
                    tls_secret_name: None,
                }),
                http_route: None,
            }),
            ..Default::default()
        },
        workers: WorkerConfig {
            replicas: 1,
            image: None,
            concurrency: None,
            autoscaling: None,
        },
        webhooks: None,
    };
    apply_cluster(w, &name, spec).await;
}

#[when(regex = r#"^I apply a Cluster "([^"]+)" with main image "([^"]+)" and worker image "([^"]+)"$"#)]
async fn apply_cluster_image_overrides(
    w: &mut E2eWorld,
    name: String,
    main_image: String,
    worker_image: String,
) {
    let spec = ClusterSpec {
        image: "nginx:alpine".into(),
        encryption_key: None,
        database: DatabaseSpec {
            type_: "postgresdb".into(),
            sqlite: None,
            postgres: Some(pg_postgres_config()),
            mysql: None,
        },
        redis: RedisConfig {
            host: "redis.example.com".into(),
            port: Some(6379),
            password_secret: Some(SecretKeyRef {
                name: "redis-creds".into(),
                key: "password".into(),
            }),
            ..Default::default()
        },
        main: MainConfig {
            replicas: 1,
            image: Some(main_image),
            ..Default::default()
        },
        workers: WorkerConfig {
            replicas: 1,
            image: Some(worker_image),
            concurrency: None,
            autoscaling: None,
        },
        webhooks: None,
    };
    apply_cluster(w, &name, spec).await;
}

#[when(regex = r#"^I apply a Cluster "([^"]+)" with Redis prefix "([^"]+)"$"#)]
async fn apply_cluster_redis_prefix(w: &mut E2eWorld, name: String, prefix: String) {
    let spec = ClusterSpec {
        image: "nginx:alpine".into(),
        encryption_key: None,
        database: DatabaseSpec {
            type_: "postgresdb".into(),
            sqlite: None,
            postgres: Some(pg_postgres_config()),
            mysql: None,
        },
        redis: RedisConfig {
            host: "redis.example.com".into(),
            port: Some(6379),
            password_secret: Some(SecretKeyRef {
                name: "redis-creds".into(),
                key: "password".into(),
            }),
            prefix: Some(prefix),
            ..Default::default()
        },
        main: MainConfig {
            replicas: 1,
            ..Default::default()
        },
        workers: WorkerConfig {
            replicas: 1,
            image: None,
            concurrency: None,
            autoscaling: None,
        },
        webhooks: None,
    };
    apply_cluster(w, &name, spec).await;
}

#[when(regex = r#"^I delete the Cluster "([^"]+)"$"#)]
async fn when_delete_cluster(w: &mut E2eWorld, name: String) {
    let api: Api<Cluster> = Api::namespaced(w.client().clone(), NS);
    api.delete(&name, &DeleteParams::default())
        .await
        .expect("delete Cluster");
}

#[then(regex = r#"^no Service named "([^"]+)" exists$"#)]
async fn no_service(w: &mut E2eWorld, name: String) {
    let api: Api<Service> = Api::namespaced(w.client().clone(), NS);
    // grace so the reconciler could mistakenly create one
    sleep(Duration::from_secs(3)).await;
    assert!(
        api.get_opt(&name).await.unwrap().is_none(),
        "Service/{name} unexpectedly exists"
    );
}

#[then(regex = r#"^the Deployment "([^"]+)" runs command "([^"]+)"$"#)]
async fn deployment_runs_command(w: &mut E2eWorld, name: String, command: String) {
    let expected: Vec<String> = command.split_whitespace().map(|s| s.to_string()).collect();
    let client = w.client().clone();
    let n = name.clone();
    let exp = expected.clone();
    wait_until(60, &format!("Deployment/{name} command={command:?}"), move || {
        let client = client.clone();
        let n = n.clone();
        let exp = exp.clone();
        async move {
            let api: Api<Deployment> = Api::namespaced(client, NS);
            match api.get_opt(&n).await.unwrap() {
                Some(d) => {
                    d.spec
                        .and_then(|s| s.template.spec)
                        .and_then(|s| s.containers.into_iter().next())
                        .and_then(|c| c.command)
                        == Some(exp.clone())
                }
                None => false,
            }
        }
    })
    .await;
}

#[then(regex = r#"^the Deployment "([^"]+)" runs image "([^"]+)"$"#)]
async fn deployment_runs_image(w: &mut E2eWorld, name: String, image: String) {
    let client = w.client().clone();
    let n = name.clone();
    let img = image.clone();
    wait_until(60, &format!("Deployment/{name} image={image}"), move || {
        let client = client.clone();
        let n = n.clone();
        let img = img.clone();
        async move {
            let api: Api<Deployment> = Api::namespaced(client, NS);
            match api.get_opt(&n).await.unwrap() {
                Some(d) => {
                    d.spec
                        .and_then(|s| s.template.spec)
                        .and_then(|s| s.containers.into_iter().next())
                        .and_then(|c| c.image)
                        == Some(img.clone())
                }
                None => false,
            }
        }
    })
    .await;
}

#[then(
    regex = r#"^the Cluster "([^"]+)" has status mainReplicas (\d+) workerReplicas (\d+) webhookReplicas (\d+)$"#
)]
async fn cluster_status_replicas(w: &mut E2eWorld, name: String, main: i32, worker: i32, webhook: i32) {
    let client = w.client().clone();
    let n = name.clone();
    wait_until(60, &format!("Cluster/{name} status replicas"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api: Api<Cluster> = Api::namespaced(client, NS);
            match api.get_opt(&n).await.unwrap() {
                Some(c) => match c.status {
                    Some(s) => {
                        s.main_replicas == main
                            && s.worker_replicas == worker
                            && s.webhook_replicas == webhook
                    }
                    None => false,
                },
                None => false,
            }
        }
    })
    .await;
}

#[then(regex = r#"^the Cluster "([^"]+)" is gone within (\d+) seconds$"#)]
async fn cluster_gone(w: &mut E2eWorld, name: String, secs: u64) {
    let client = w.client().clone();
    let n = name.clone();
    wait_until(secs, &format!("Cluster/{name} gone"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api: Api<Cluster> = Api::namespaced(client, NS);
            api.get_opt(&n).await.unwrap().is_none()
        }
    })
    .await;
}

// ----- HTTPRoute (Gateway API) -----

fn http_route_api(client: Client) -> Api<DynamicObject> {
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "HTTPRoute");
    let ar = ApiResource::from_gvk(&gvk);
    Api::namespaced_with(client, NS, &ar)
}

#[when(
    regex = r#"^I apply a Single "([^"]+)" with httpRoute gateway "([^"]+)" namespace "([^"]+)" and host "([^"]+)"$"#
)]
async fn apply_single_route(
    w: &mut E2eWorld,
    name: String,
    gateway: String,
    gateway_ns: String,
    host: String,
) {
    let mut spec = base_spec("nginx:alpine");
    spec.host = Some(host);
    spec.networking = Some(NetworkingSpec {
        ingress: None,
        http_route: Some(HttpRouteConfig {
            gateway: GatewayRef {
                name: gateway,
                namespace: Some(gateway_ns),
            },
        }),
    });
    apply_with_spec(w, &name, spec).await;
}

#[given(regex = r#"^a Single "([^"]+)" exists with httpRoute gateway "([^"]+)" and host "([^"]+)"$"#)]
async fn single_with_route_exists(w: &mut E2eWorld, name: String, gateway: String, host: String) {
    let mut spec = base_spec("nginx:alpine");
    spec.host = Some(host);
    spec.networking = Some(NetworkingSpec {
        ingress: None,
        http_route: Some(HttpRouteConfig {
            gateway: GatewayRef {
                name: gateway,
                namespace: Some("default".into()),
            },
        }),
    });
    apply_with_spec(w, &name, spec).await;
    let api = http_route_api(w.client().clone());
    let n = name.clone();
    wait_until(60, &format!("HTTPRoute/{name} to appear"), move || {
        let api = api.clone();
        let n = n.clone();
        async move { api.get_opt(&n).await.unwrap().is_some() }
    })
    .await;
}

#[when(
    regex = r#"^I apply a Cluster "([^"]+)" with main httpRoute gateway "([^"]+)" namespace "([^"]+)" and host "([^"]+)"$"#
)]
async fn apply_cluster_main_route(
    w: &mut E2eWorld,
    name: String,
    gateway: String,
    gateway_ns: String,
    host: String,
) {
    let spec = ClusterSpec {
        image: "nginx:alpine".into(),
        encryption_key: None,
        database: DatabaseSpec {
            type_: "postgresdb".into(),
            sqlite: None,
            postgres: Some(pg_postgres_config()),
            mysql: None,
        },
        redis: RedisConfig {
            host: "redis.example.com".into(),
            port: Some(6379),
            password_secret: Some(SecretKeyRef {
                name: "redis-creds".into(),
                key: "password".into(),
            }),
            ..Default::default()
        },
        main: MainConfig {
            replicas: 1,
            host: Some(host),
            networking: Some(NetworkingSpec {
                ingress: None,
                http_route: Some(HttpRouteConfig {
                    gateway: GatewayRef {
                        name: gateway,
                        namespace: Some(gateway_ns),
                    },
                }),
            }),
            ..Default::default()
        },
        workers: WorkerConfig {
            replicas: 1,
            image: None,
            concurrency: None,
            autoscaling: None,
        },
        webhooks: None,
    };
    apply_cluster(w, &name, spec).await;
}

#[then(regex = r#"^an HTTPRoute named "([^"]+)" exists with host "([^"]+)" within (\d+) seconds$"#)]
async fn httproute_exists(w: &mut E2eWorld, name: String, host: String, secs: u64) {
    let client = w.client().clone();
    let n = name.clone();
    let h = host.clone();
    wait_until(secs, &format!("HTTPRoute/{name} host={host}"), move || {
        let client = client.clone();
        let n = n.clone();
        let h = h.clone();
        async move {
            let api = http_route_api(client);
            match api.get_opt(&n).await.unwrap() {
                Some(rt) => rt
                    .data
                    .get("spec")
                    .and_then(|s| s.get("hostnames"))
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().any(|v| v.as_str() == Some(&h)))
                    .unwrap_or(false),
                None => false,
            }
        }
    })
    .await;
}

#[then(regex = r#"^the HTTPRoute "([^"]+)" has parent gateway "([^"]+)" namespace "([^"]+)"$"#)]
async fn httproute_parent(w: &mut E2eWorld, name: String, gateway: String, gw_ns: String) {
    let api = http_route_api(w.client().clone());
    let rt = api.get(&name).await.expect("HTTPRoute");
    let parent = rt
        .data
        .get("spec")
        .and_then(|s| s.get("parentRefs"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .expect("no parentRefs");
    assert_eq!(
        parent.get("name").and_then(|v| v.as_str()),
        Some(gateway.as_str())
    );
    assert_eq!(
        parent.get("namespace").and_then(|v| v.as_str()),
        Some(gw_ns.as_str())
    );
}

#[then(regex = r#"^the HTTPRoute "([^"]+)" is gone within (\d+) seconds$"#)]
async fn httproute_gone(w: &mut E2eWorld, name: String, secs: u64) {
    let client = w.client().clone();
    let n = name.clone();
    wait_until(secs, &format!("HTTPRoute/{name} gone"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api = http_route_api(client);
            api.get_opt(&n).await.unwrap().is_none()
        }
    })
    .await;
}

// ----- HPA -----

#[when(regex = r#"^I apply a Cluster "([^"]+)" with worker autoscaling min (\d+) max (\d+)$"#)]
async fn apply_cluster_hpa(w: &mut E2eWorld, name: String, min: i32, max: i32) {
    let spec = ClusterSpec {
        image: "nginx:alpine".into(),
        encryption_key: None,
        database: DatabaseSpec {
            type_: "postgresdb".into(),
            sqlite: None,
            postgres: Some(pg_postgres_config()),
            mysql: None,
        },
        redis: RedisConfig {
            host: "redis.example.com".into(),
            port: Some(6379),
            password_secret: Some(SecretKeyRef {
                name: "redis-creds".into(),
                key: "password".into(),
            }),
            ..Default::default()
        },
        main: MainConfig {
            replicas: 1,
            ..Default::default()
        },
        workers: WorkerConfig {
            replicas: 1,
            image: None,
            concurrency: None,
            autoscaling: Some(Autoscaling {
                min_replicas: min,
                max_replicas: max,
                target_cpu_utilization_percentage: None,
            }),
        },
        webhooks: None,
    };
    apply_cluster(w, &name, spec).await;
}

#[given(regex = r#"^a Cluster "([^"]+)" exists with worker autoscaling min (\d+) max (\d+)$"#)]
async fn cluster_with_hpa_exists(w: &mut E2eWorld, name: String, min: i32, max: i32) {
    apply_cluster_hpa(w, name.clone(), min, max).await;
    let client = w.client().clone();
    let hpa_name = format!("{name}-worker");
    let hpa_name2 = hpa_name.clone();
    wait_until(60, &format!("HPA/{hpa_name2} to appear"), move || {
        let client = client.clone();
        let n = hpa_name.clone();
        async move {
            let api: Api<HorizontalPodAutoscaler> = Api::namespaced(client, NS);
            api.get_opt(&n).await.unwrap().is_some()
        }
    })
    .await;
}

#[when(regex = r#"^I update the Cluster "([^"]+)" to have no worker autoscaling$"#)]
async fn drop_cluster_hpa(w: &mut E2eWorld, name: String) {
    let api: Api<Cluster> = Api::namespaced(w.client().clone(), NS);
    let current = api.get(&name).await.expect("Cluster");
    let mut spec = current.spec.clone();
    spec.workers.autoscaling = None;
    let new = Cluster::new(&name, spec);
    let ssa = PatchParams::apply("cucumber").force();
    api.patch(&name, &ssa, &Patch::Apply(&new))
        .await
        .expect("update Cluster");
}

#[then(
    regex = r#"^a HorizontalPodAutoscaler named "([^"]+)" exists with min (\d+) max (\d+) within (\d+) seconds$"#
)]
async fn hpa_min_max(w: &mut E2eWorld, name: String, min: i32, max: i32, secs: u64) {
    let client = w.client().clone();
    let n = name.clone();
    wait_until(secs, &format!("HPA/{name} min={min} max={max}"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api: Api<HorizontalPodAutoscaler> = Api::namespaced(client, NS);
            match api.get_opt(&n).await.unwrap() {
                Some(h) => match h.spec {
                    Some(s) => s.min_replicas == Some(min) && s.max_replicas == max,
                    None => false,
                },
                None => false,
            }
        }
    })
    .await;
}

#[then(regex = r#"^the HorizontalPodAutoscaler "([^"]+)" targets Deployment "([^"]+)"$"#)]
async fn hpa_target(w: &mut E2eWorld, name: String, target: String) {
    let api: Api<HorizontalPodAutoscaler> = Api::namespaced(w.client().clone(), NS);
    let hpa = api.get(&name).await.expect("HPA");
    let r = hpa.spec.expect("hpa spec").scale_target_ref;
    assert_eq!(r.kind, "Deployment");
    assert_eq!(r.name, target);
}

#[then(regex = r#"^the HorizontalPodAutoscaler "([^"]+)" is gone within (\d+) seconds$"#)]
async fn hpa_gone(w: &mut E2eWorld, name: String, secs: u64) {
    let client = w.client().clone();
    let n = name.clone();
    wait_until(secs, &format!("HPA/{name} gone"), move || {
        let client = client.clone();
        let n = n.clone();
        async move {
            let api: Api<HorizontalPodAutoscaler> = Api::namespaced(client, NS);
            api.get_opt(&n).await.unwrap().is_none()
        }
    })
    .await;
}

// ----- main -----

#[tokio::main]
async fn main() {
    E2eWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("features")
        .await;
}
