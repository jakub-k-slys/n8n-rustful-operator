use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::{
    Client, Resource,
    api::{Api, PatchParams},
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::fmt::Debug;

/// Bundle of mutable apply parameters threaded through every reconciler.
/// Replaces the (client, ns, owner, ps) tuple that was duplicated everywhere.
pub struct ApplyCtx<'a> {
    pub client: &'a Client,
    pub ns: &'a str,
    pub owner: &'a OwnerReference,
    pub patch: &'a PatchParams,
}

impl<'a> ApplyCtx<'a> {
    pub fn api<K>(&self) -> Api<K>
    where
        K: Resource<Scope = k8s_openapi::NamespaceResourceScope> + Clone + DeserializeOwned + Debug,
        K::DynamicType: Default,
    {
        Api::namespaced(self.client.clone(), self.ns)
    }
}

/// Environment-variable and volume payload shared across all cluster roles.
#[derive(Default)]
pub struct Bundle {
    pub env: Vec<Value>,
    pub volumes: Vec<Value>,
    pub mounts: Vec<Value>,
}
