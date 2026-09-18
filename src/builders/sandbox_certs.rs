use crate::{
    builders::sandbox_names::{SandboxNames, sandbox_labels},
    labels::common_annotations,
    spec::AssistantSpec,
};
use k8s_openapi::api::{
    batch::v1::Job,
    core::v1::ServiceAccount,
    rbac::v1::{Role, RoleBinding},
};
use serde_json::json;

/// GID the sandbox-api image runs as, used as the pod `fsGroup` so the mounted
/// key files are readable. `None` when the image runs as root.
/// Verified 2026-09-15 against `n8n-sandbox-service-api:1.2.0` via
/// `docker run --rm --entrypoint id <image>` → `uid=100(sandbox-api)
/// gid=101(sandbox-api)`.
pub const TLS_FS_GROUP: Option<i64> = Some(101);

/// Secret volume mode `0440` — private keys must not be world-readable.
pub const TLS_MODE: i32 = 0o440;

pub fn certs_image(spec: &AssistantSpec) -> String {
    format!("ghcr.io/n8n-io/n8n-sandbox-service-api:{}", spec.sandbox.version)
}

pub fn build_certs_rbac(
    names: &SandboxNames,
    cr_ns: &str,
    cr_name: &str,
    sbx_ns: &str,
) -> (ServiceAccount, Role, RoleBinding) {
    let labels = sandbox_labels(cr_ns, cr_name, "sandbox-certs");
    let meta = json!({
        "name": names.certs,
        "namespace": sbx_ns,
        "labels": labels,
        "annotations": common_annotations(),
    });
    let sa = json!({
        "apiVersion": "v1",
        "kind": "ServiceAccount",
        "metadata": meta,
    });
    let role = json!({
        "apiVersion": "rbac.authorization.k8s.io/v1",
        "kind": "Role",
        "metadata": meta,
        "rules": [{
            "apiGroups": [""],
            "resources": ["secrets"],
            "verbs": ["create", "patch"],
        }],
    });
    let rb = json!({
        "apiVersion": "rbac.authorization.k8s.io/v1",
        "kind": "RoleBinding",
        "metadata": meta,
        "roleRef": {
            "apiGroup": "rbac.authorization.k8s.io",
            "kind": "Role",
            "name": names.certs,
        },
        "subjects": [{
            "kind": "ServiceAccount",
            "name": names.certs,
            "namespace": sbx_ns,
        }],
    });
    (
        serde_json::from_value(sa).expect("static serviceaccount schema is valid"),
        serde_json::from_value(role).expect("static role schema is valid"),
        serde_json::from_value(rb).expect("static rolebinding schema is valid"),
    )
}

/// One-shot Job: `bootstrap-mtls.sh` into an emptyDir, then `tlspub` publishes
/// the whitelisted files as two Secrets. Created only when those Secrets are
/// missing — rerunning it would mint a new CA under the running pods.
///
/// This object (and its RBAC above) carry no `ownerReference`: the sandbox
/// stack lives in a different namespace than the `Assistant` CR, and Kubernetes
/// garbage-collects an object whose owner lives in another namespace as an
/// orphan — i.e. it would delete the Job/Secrets essentially at random. The
/// operator instead finds and cleans these up by `sandbox_labels`.
pub fn build_certs_job(
    names: &SandboxNames,
    spec: &AssistantSpec,
    cr_ns: &str,
    cr_name: &str,
    sbx_ns: &str,
    operator_image: &str,
) -> Job {
    let labels = sandbox_labels(cr_ns, cr_name, "sandbox-certs");
    let bootstrap = format!(
        "bootstrap-mtls.sh --out-dir /tls --api-san {} --control-san-prefix {}-sandbox-runner --world-readable",
        names.api, names.prefix
    );
    let job = json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {
            "name": names.certs,
            "namespace": sbx_ns,
            "labels": labels,
            "annotations": common_annotations(),
        },
        "spec": {
            "ttlSecondsAfterFinished": 3600,
            "backoffLimit": 4,
            "template": {
                "metadata": { "labels": labels },
                "spec": {
                    "restartPolicy": "Never",
                    "serviceAccountName": names.certs,
                    "volumes": [{ "name": "tls", "emptyDir": {} }],
                    "initContainers": [{
                        "name": "bootstrap",
                        "image": certs_image(spec),
                        "securityContext": { "runAsUser": 0 },
                        "command": ["sh", "-c"],
                        "args": [bootstrap],
                        "env": [{ "name": "NUM_RUNNERS", "value": "1" }],
                        "volumeMounts": [{ "name": "tls", "mountPath": "/tls" }],
                    }],
                    "containers": [{
                        "name": "tlspub",
                        "image": operator_image,
                        "command": ["/app/tlspub"],
                        "args": [
                            "--dir", "/tls",
                            "--namespace", sbx_ns,
                            "--api-secret", names.tls_api,
                            "--runner-secret", names.tls_runner,
                            "--label", format!("n8n.slys.dev/assistant={cr_name}"),
                            "--label", format!("n8n.slys.dev/assistant-namespace={cr_ns}"),
                            "--label", "app.kubernetes.io/managed-by=n8n-rustful-operator",
                        ],
                        "volumeMounts": [{ "name": "tls", "mountPath": "/tls", "readOnly": true }],
                    }],
                }
            }
        }
    });
    serde_json::from_value(job).expect("static job schema is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{AssistantSpec, ModelConfig, SandboxConfig, TargetRef};

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

    #[test]
    fn job_runs_bootstrap_then_tlspub() {
        let j = build_certs_job(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "op:1");
        let v = serde_json::to_value(&j).unwrap();
        let init = &v["spec"]["template"]["spec"]["initContainers"][0];
        assert_eq!(init["image"], "ghcr.io/n8n-io/n8n-sandbox-service-api:1.2.0");
        assert_eq!(init["securityContext"]["runAsUser"], 0);
        assert_eq!(init["env"][0]["name"], "NUM_RUNNERS");
        assert_eq!(init["env"][0]["value"], "1");
        let cmd = init["args"][0].as_str().unwrap();
        assert!(cmd.contains("--api-san n8n-cluster-n8n-sandbox-api"));
        assert!(cmd.contains("--control-san-prefix n8n-cluster-n8n-sandbox-runner"));
        let main = &v["spec"]["template"]["spec"]["containers"][0];
        assert_eq!(main["image"], "op:1");
        assert_eq!(main["command"][0], "/app/tlspub");
        let args: Vec<&str> = main["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        assert!(args.contains(&"--api-secret"));
        assert!(args.contains(&"n8n-cluster-n8n-sandbox-tls-api"));
        assert!(args.contains(&"n8n-cluster-n8n-sandbox-tls-runner"));
    }

    #[test]
    fn job_cleans_itself_up_and_does_not_retry_forever() {
        let j = build_certs_job(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "op:1");
        let v = serde_json::to_value(&j).unwrap();
        assert_eq!(v["spec"]["ttlSecondsAfterFinished"], 3600);
        assert_eq!(v["spec"]["backoffLimit"], 4);
        assert_eq!(v["spec"]["template"]["spec"]["restartPolicy"], "Never");
    }

    #[test]
    fn job_carries_no_owner_reference() {
        let j = build_certs_job(&names(), &spec(), "n8n-cluster", "n8n", "n8n-sandbox", "op:1");
        assert!(j.metadata.owner_references.is_none());
        let l = j.metadata.labels.unwrap();
        assert_eq!(l["n8n.slys.dev/assistant"], "n8n");
    }

    #[test]
    fn rbac_grants_only_secret_create_and_patch() {
        let (sa, role, rb) = build_certs_rbac(&names(), "n8n-cluster", "n8n", "n8n-sandbox");
        assert_eq!(sa.metadata.name.as_deref(), Some("n8n-cluster-n8n-sandbox-certs"));
        let rules = role.rules.unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].resources.as_ref().unwrap(), &vec!["secrets".to_string()]);
        let mut verbs = rules[0].verbs.clone();
        verbs.sort();
        assert_eq!(verbs, vec!["create".to_string(), "patch".to_string()]);
        assert_eq!(rb.subjects.unwrap()[0].name, "n8n-cluster-n8n-sandbox-certs");
    }
}
