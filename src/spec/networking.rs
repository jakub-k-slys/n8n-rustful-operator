use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct NetworkingSpec {
    /// Provision a `networking.k8s.io/v1` Ingress. Mutually exclusive with `httpRoute`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingress: Option<IngressConfig>,
    /// Provision a `gateway.networking.k8s.io/v1` HTTPRoute. Mutually exclusive with `ingress`.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "httpRoute")]
    pub http_route: Option<HttpRouteConfig>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct IngressConfig {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "className")]
    pub class_name: Option<String>,
    /// Name of a TLS Secret in the same namespace.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "tlsSecretName")]
    pub tls_secret_name: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct HttpRouteConfig {
    /// Gateway to attach this HTTPRoute to (parentRefs[0]).
    pub gateway: GatewayRef,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct GatewayRef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}
