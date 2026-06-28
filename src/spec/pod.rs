use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Schema for a free-form field: `x-kubernetes-preserve-unknown-fields: true`,
/// which the apiserver requires for an untyped object in a structural schema.
fn preserve_arbitrary(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({ "x-kubernetes-preserve-unknown-fields": true })
}

/// Pod-level scheduling and metadata applied to a role's pod template. Every
/// field is optional; omit the whole block to keep the defaults.
#[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
pub struct PodConfig {
    /// ServiceAccount the pods run as (also where image-pull Secrets attached
    /// to the SA are honoured).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "serviceAccountName"
    )]
    pub service_account_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "nodeSelector")]
    pub node_selector: Option<BTreeMap<String, String>>,
    /// Free-form pod tolerations (k8s `[]Toleration`), passed through verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "preserve_arbitrary")]
    pub tolerations: Option<serde_json::Value>,
    /// Free-form pod affinity (k8s `Affinity`), passed through verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "preserve_arbitrary")]
    pub affinity: Option<serde_json::Value>,
    /// Free-form pod security context (k8s `PodSecurityContext`), passed through
    /// verbatim — e.g. `fsGroup` so a mounted PVC is writable by the n8n user
    /// (uid/gid 1000).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "securityContext")]
    #[schemars(schema_with = "preserve_arbitrary")]
    pub security_context: Option<serde_json::Value>,
    /// Extra labels merged onto the pod template metadata.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "podLabels")]
    pub pod_labels: Option<BTreeMap<String, String>>,
    /// Extra annotations merged onto the pod template metadata.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "podAnnotations")]
    pub pod_annotations: Option<BTreeMap<String, String>>,
}
