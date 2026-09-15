use crate::{
    builders::{
        apply_pod_config, resources,
        sandbox_certs::{TLS_FS_GROUP, TLS_MODE},
        sandbox_names::{SandboxNames, sandbox_labels},
    },
    labels::common_annotations,
    spec::AssistantSpec,
};
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{PersistentVolumeClaim, Service},
};
use serde_json::{Value, json};

fn api_image(spec: &AssistantSpec) -> String {
    format!("ghcr.io/n8n-io/n8n-sandbox-service-api:{}", spec.sandbox.version)
}

fn runner_image(spec: &AssistantSpec) -> String {
    format!(
        "ghcr.io/n8n-io/n8n-sandbox-service-runner-dind:{}",
        spec.sandbox.version
    )
}

fn env(name: &str, value: impl Into<String>) -> Value {
    json!({ "name": name, "value": value.into() })
}

/// Selector labels for a sandbox workload. Distinct per component so the API
/// and runner Services don't select each other's pods.
fn selector(cr_ns: &str, cr_name: &str, component: &str) -> Value {
    json!({
        "app.kubernetes.io/name": "n8n-sandbox",
        "app.kubernetes.io/instance": cr_name,
        "app.kubernetes.io/component": component,
        "n8n.slys.dev/assistant-namespace": cr_ns,
    })
}

fn deployment(
    name: &str,
    sbx_ns: &str,
    cr_ns: &str,
    cr_name: &str,
    component: &str,
    tls_revision: &str,
    container: Value,
    volumes: Vec<Value>,
    pod_cfg: Option<&crate::spec::PodConfig>,
) -> Deployment {
    let labels = sandbox_labels(cr_ns, cr_name, component);
    let mut annotations = common_annotations();
    annotations.insert("n8n.slys.dev/tls-revision".to_string(), tls_revision.to_string());
    let pod_spec = json!({ "volumes": volumes, "containers": [container] });
    let mut dep = json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": name,
            "namespace": sbx_ns,
            "labels": labels,
            "annotations": common_annotations(),
        },
        "spec": {
            "replicas": 1,
            // A second pod would register a duplicate runner identity, so never
            // surge: replace in place.
            "strategy": { "type": "Recreate" },
            "selector": { "matchLabels": selector(cr_ns, cr_name, component) },
            "template": {
                "metadata": { "labels": labels, "annotations": annotations },
                "spec": pod_spec,
            }
        }
    });
    if let Some(pc) = pod_cfg {
        apply_pod_config(&mut dep["spec"]["template"], pc);
    }
    // The operator owns `fsGroup`: the TLS Secret is mounted 0440 owned by
    // group 101, and the image runs as uid=100/gid=101, so without it the
    // process can't read its own private key. `apply_pod_config` may have
    // just done a wholesale `securityContext` assignment from the user's
    // `pod.securityContext` above — applied AFTER that call, merged into
    // whatever's there (never replacing it), so a user-supplied
    // securityContext keeps its own fields alongside the required fsGroup.
    if let Some(fsg) = TLS_FS_GROUP {
        let sc = &mut dep["spec"]["template"]["spec"]["securityContext"];
        if !sc.is_object() {
            *sc = json!({});
        }
        sc["fsGroup"] = json!(fsg);
    }
    serde_json::from_value(dep).expect("static sandbox deployment schema is valid")
}

pub fn build_sandbox_api(
    names: &SandboxNames,
    spec: &AssistantSpec,
    cr_ns: &str,
    cr_name: &str,
    sbx_ns: &str,
    tls_revision: &str,
) -> Deployment {
    let mut container = json!({
        "name": "sandbox-api",
        "image": api_image(spec),
        "ports": [
            { "containerPort": 8080, "name": "http" },
            { "containerPort": 9090, "name": "grpc" }
        ],
        "envFrom": [{ "secretRef": { "name": names.secrets } }],
        "env": [
            env("SANDBOX_API_GRPC_TLS_CERT_FILE", "/tls/grpc-server.crt"),
            env("SANDBOX_API_GRPC_TLS_KEY_FILE", "/tls/grpc-server.key"),
            env("SANDBOX_API_GRPC_TLS_CLIENT_CA_FILE", "/tls/ca.crt"),
            env("SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_CA_FILE", "/tls/ca.crt"),
            env(
                "SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_CERT_FILE",
                "/tls/control-grpc-api-client.crt"
            ),
            env(
                "SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_KEY_FILE",
                "/tls/control-grpc-api-client.key"
            ),
            // The cert is issued for the short name; the address is an FQDN.
            env("SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_SERVER_NAME", names.runner.clone()),
        ],
        "volumeMounts": [{ "name": "tls", "mountPath": "/tls", "readOnly": true }],
        "readinessProbe": {
            "httpGet": { "path": "/healthz", "port": "http" },
            "initialDelaySeconds": 10,
            "periodSeconds": 5
        },
        "livenessProbe": {
            "httpGet": { "path": "/healthz", "port": "http" },
            "initialDelaySeconds": 30,
            "periodSeconds": 10
        }
    });
    if let Some(r) = &spec.sandbox.api.resources {
        container["resources"] = resources(r);
    }
    let volumes = vec![json!({
        "name": "tls",
        "secret": { "secretName": names.tls_api, "defaultMode": TLS_MODE }
    })];
    deployment(
        &names.api,
        sbx_ns,
        cr_ns,
        cr_name,
        "sandbox-api",
        tls_revision,
        container,
        volumes,
        spec.sandbox.api.pod.as_ref(),
    )
}

pub(crate) fn docker_pvc_name(names: &SandboxNames) -> String {
    format!("{}-docker", names.runner)
}

pub fn build_sandbox_runner(
    names: &SandboxNames,
    spec: &AssistantSpec,
    cr_ns: &str,
    cr_name: &str,
    sbx_ns: &str,
    tls_revision: &str,
) -> Deployment {
    let store = &spec.sandbox.runner.docker_storage;
    let docker_volume = match &store.persistence {
        Some(_) => json!({
            "name": "docker",
            "persistentVolumeClaim": { "claimName": docker_pvc_name(names) }
        }),
        None => json!({ "name": "docker", "emptyDir": { "sizeLimit": store.size } }),
    };
    let mut container = json!({
        "name": "sandbox-runner",
        "image": runner_image(spec),
        // Docker-in-Docker: equivalent to root on the node. Never expose.
        "securityContext": { "privileged": true },
        "ports": [
            { "containerPort": 9091, "name": "control" },
            { "containerPort": 8080, "name": "http" }
        ],
        "envFrom": [{ "secretRef": { "name": names.secrets } }],
        "env": [
            env("SANDBOX_RUNNER_API_GRPC_ADDR", format!("{}.{sbx_ns}.svc:9090", names.api)),
            env("SANDBOX_RUNNER_REGISTRATION_GRPC_SERVER_NAME", names.api.clone()),
            env("SANDBOX_RUNNER_CONTROL_GRPC_LISTEN_ADDR", ":9091"),
            env(
                "SANDBOX_RUNNER_CONTROL_GRPC_ADVERTISE_ADDR",
                format!("{}.{sbx_ns}.svc:9091", names.runner)
            ),
            env(
                "SANDBOX_RUNNER_HTTP_BASE_URL",
                format!("http://{}.{sbx_ns}.svc:8080", names.runner)
            ),
            env("SANDBOX_RUNNER_ID", "runner-1"),
            env("SANDBOX_RUNNER_DOCKER_SANDBOX_IMAGE", spec.sandbox.sandbox_image.clone()),
            env("SANDBOX_RUNNER_REGISTRATION_GRPC_CA_FILE", "/tls/ca.crt"),
            env("SANDBOX_RUNNER_REGISTRATION_GRPC_CERT_FILE", "/tls/grpc-client.crt"),
            env("SANDBOX_RUNNER_REGISTRATION_GRPC_KEY_FILE", "/tls/grpc-client.key"),
            env("SANDBOX_RUNNER_CONTROL_GRPC_TLS_CERT_FILE", "/tls/control-grpc-server.crt"),
            env("SANDBOX_RUNNER_CONTROL_GRPC_TLS_KEY_FILE", "/tls/control-grpc-server.key"),
            env("SANDBOX_RUNNER_CONTROL_GRPC_TLS_CLIENT_CA_FILE", "/tls/ca.crt"),
        ],
        "volumeMounts": [
            { "name": "tls", "mountPath": "/tls", "readOnly": true },
            { "name": "docker", "mountPath": "/var/lib/docker" }
        ]
    });
    if let Some(r) = &spec.sandbox.runner.resources {
        container["resources"] = resources(r);
    }
    let volumes = vec![
        json!({
            "name": "tls",
            "secret": { "secretName": names.tls_runner, "defaultMode": TLS_MODE }
        }),
        docker_volume,
    ];
    deployment(
        &names.runner,
        sbx_ns,
        cr_ns,
        cr_name,
        "sandbox-runner",
        tls_revision,
        container,
        volumes,
        spec.sandbox.runner.pod.as_ref(),
    )
}

pub fn build_docker_pvc(
    names: &SandboxNames,
    spec: &AssistantSpec,
    cr_ns: &str,
    cr_name: &str,
    sbx_ns: &str,
) -> Option<PersistentVolumeClaim> {
    let p = spec.sandbox.runner.docker_storage.persistence.as_ref()?;
    let json = json!({
        "apiVersion": "v1",
        "kind": "PersistentVolumeClaim",
        "metadata": {
            "name": docker_pvc_name(names),
            "namespace": sbx_ns,
            "labels": sandbox_labels(cr_ns, cr_name, "sandbox-runner"),
            "annotations": common_annotations(),
        },
        "spec": {
            "accessModes": [p.access_mode],
            "resources": { "requests": { "storage": p.size } },
            "storageClassName": p.storage_class_name,
        }
    });
    Some(serde_json::from_value(json).expect("static docker pvc schema is valid"))
}

pub fn build_sandbox_service(
    name: &str,
    cr_ns: &str,
    cr_name: &str,
    sbx_ns: &str,
    component: &str,
    ports: &[(&str, i32)],
) -> Service {
    let ports: Vec<Value> = ports
        .iter()
        .map(|(n, p)| json!({ "name": n, "port": p, "targetPort": n }))
        .collect();
    let json = json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {
            "name": name,
            "namespace": sbx_ns,
            "labels": sandbox_labels(cr_ns, cr_name, component),
            "annotations": common_annotations(),
        },
        "spec": {
            "type": "ClusterIP",
            "selector": selector(cr_ns, cr_name, component),
            "ports": ports,
        }
    });
    serde_json::from_value(json).expect("static sandbox service schema is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{AssistantSpec, ModelConfig, PersistenceConfig, SandboxConfig, TargetRef};

    fn spec() -> AssistantSpec {
        AssistantSpec {
            target_ref: TargetRef {
                kind: "Cluster".into(),
                name: "n8n".into(),
            },
            model: ModelConfig {
                name: "anthropic/claude-opus-4-8".into(),
                api_key_secret: None,
                url: Some("http://x/v1".into()),
            },
            search: None,
            sandbox: SandboxConfig::default(),
        }
    }

    fn names() -> SandboxNames {
        SandboxNames::new("n8n-cluster", "n8n", "n8n")
    }

    fn env_of(v: &serde_json::Value, name: &str) -> String {
        v["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == name)
            .unwrap_or_else(|| panic!("env {name} not found"))["value"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn api_points_tls_paths_at_the_flat_secret_mount() {
        let d = build_sandbox_api(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "42");
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(
            env_of(&v, "SANDBOX_API_GRPC_TLS_CERT_FILE"),
            "/tls/grpc-server.crt"
        );
        assert_eq!(env_of(&v, "SANDBOX_API_GRPC_TLS_CLIENT_CA_FILE"), "/tls/ca.crt");
        assert_eq!(
            env_of(&v, "SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_CERT_FILE"),
            "/tls/control-grpc-api-client.crt"
        );
    }

    #[test]
    fn api_verifies_the_runner_by_its_short_san() {
        let d = build_sandbox_api(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "42");
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(
            env_of(&v, "SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_SERVER_NAME"),
            "n8n-cluster-n8n-sandbox-runner-1"
        );
    }

    #[test]
    fn api_pulls_shared_secrets_and_probes_healthz() {
        let d = build_sandbox_api(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "42");
        let v = serde_json::to_value(&d).unwrap();
        let c = &v["spec"]["template"]["spec"]["containers"][0];
        assert_eq!(
            c["envFrom"][0]["secretRef"]["name"],
            "n8n-cluster-n8n-sandbox-secrets"
        );
        assert_eq!(c["readinessProbe"]["httpGet"]["path"], "/healthz");
        assert_eq!(v["spec"]["strategy"]["type"], "Recreate");
        assert_eq!(v["spec"]["replicas"], 1);
    }

    #[test]
    fn tls_revision_annotation_forces_a_rollout() {
        let d = build_sandbox_api(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "42");
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(
            v["spec"]["template"]["metadata"]["annotations"]["n8n.slys.dev/tls-revision"],
            "42"
        );
    }

    #[test]
    fn tls_secret_is_mounted_read_only_with_restricted_mode() {
        let d = build_sandbox_api(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "42");
        let v = serde_json::to_value(&d).unwrap();
        let vol = &v["spec"]["template"]["spec"]["volumes"][0];
        assert_eq!(vol["secret"]["secretName"], "n8n-cluster-n8n-sandbox-tls-api");
        assert_eq!(vol["secret"]["defaultMode"], 0o440);
        assert_eq!(
            v["spec"]["template"]["spec"]["containers"][0]["volumeMounts"][0]["readOnly"],
            true
        );
    }

    #[test]
    fn runner_is_privileged_and_advertises_its_own_service() {
        let d = build_sandbox_runner(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "7");
        let v = serde_json::to_value(&d).unwrap();
        let c = &v["spec"]["template"]["spec"]["containers"][0];
        assert_eq!(c["securityContext"]["privileged"], true);
        assert_eq!(
            env_of(&v, "SANDBOX_RUNNER_CONTROL_GRPC_ADVERTISE_ADDR"),
            "n8n-cluster-n8n-sandbox-runner-1.n8n-sandbox.svc:9091"
        );
        assert_eq!(
            env_of(&v, "SANDBOX_RUNNER_API_GRPC_ADDR"),
            "n8n-cluster-n8n-sandbox-api.n8n-sandbox.svc:9090"
        );
        assert_eq!(
            env_of(&v, "SANDBOX_RUNNER_REGISTRATION_GRPC_SERVER_NAME"),
            "n8n-cluster-n8n-sandbox-api"
        );
        assert_eq!(env_of(&v, "SANDBOX_RUNNER_ID"), "runner-1");
    }

    #[test]
    fn runner_defaults_docker_storage_to_a_capped_emptydir() {
        let d = build_sandbox_runner(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "7");
        let v = serde_json::to_value(&d).unwrap();
        let vols = v["spec"]["template"]["spec"]["volumes"].as_array().unwrap();
        let docker = vols.iter().find(|x| x["name"] == "docker").unwrap();
        assert_eq!(docker["emptyDir"]["sizeLimit"], "20Gi");
        assert!(build_docker_pvc(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox").is_none());
    }

    #[test]
    fn runner_uses_a_pvc_when_persistence_is_set() {
        let mut s = spec();
        s.sandbox.runner.docker_storage.persistence = Some(PersistenceConfig {
            size: "40Gi".into(),
            storage_class_name: Some("longhorn".into()),
            access_mode: "ReadWriteOnce".into(),
        });
        let d = build_sandbox_runner(&names(), &s, "n8n-cluster", "n8n", "n8n-sandbox", "7");
        let v = serde_json::to_value(&d).unwrap();
        let vols = v["spec"]["template"]["spec"]["volumes"].as_array().unwrap();
        let docker = vols.iter().find(|x| x["name"] == "docker").unwrap();
        assert_eq!(
            docker["persistentVolumeClaim"]["claimName"],
            "n8n-cluster-n8n-sandbox-runner-1-docker"
        );
        let pvc = build_docker_pvc(&names(), &s, "n8n-cluster", "n8n", "n8n-sandbox").unwrap();
        assert_eq!(
            pvc.metadata.name.as_deref(),
            Some("n8n-cluster-n8n-sandbox-runner-1-docker")
        );
    }

    #[test]
    fn service_exposes_the_named_ports_and_selects_the_workload() {
        let s = build_sandbox_service(
            "n8n-cluster-n8n-sandbox-api",
            "n8n-cluster",
            "n8n",
            "n8n-sandbox",
            "sandbox-api",
            &[("http", 8080), ("grpc", 9090)],
        );
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["spec"]["ports"][0]["name"], "http");
        assert_eq!(v["spec"]["ports"][1]["port"], 9090);
        assert_eq!(v["spec"]["type"], "ClusterIP");
        assert_eq!(
            v["spec"]["selector"]["app.kubernetes.io/component"],
            "sandbox-api"
        );
    }

    #[test]
    fn fsgroup_is_merged_into_a_user_supplied_security_context_not_replaced() {
        let mut s = spec();
        s.sandbox.api.pod = Some(crate::spec::PodConfig {
            security_context: Some(json!({ "runAsNonRoot": true })),
            ..Default::default()
        });
        let d = build_sandbox_api(&names(), &s, "n8n-cluster", "n8n", "n8n-sandbox", "42");
        let v = serde_json::to_value(&d).unwrap();
        let sc = &v["spec"]["template"]["spec"]["securityContext"];
        assert_eq!(sc["runAsNonRoot"], true);
        assert_eq!(sc["fsGroup"], 101);
    }
}
