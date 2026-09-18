use crate::spec::{Assistant, Cluster, Single};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::ResourceExt;

pub fn assistant_owner(a: &Assistant) -> OwnerReference {
    OwnerReference {
        api_version: "n8n.slys.dev/v1".to_string(),
        kind: "Assistant".to_string(),
        name: a.name_any(),
        uid: a.uid().expect("Assistant lacks uid"),
        controller: Some(true),
        block_owner_deletion: Some(true),
    }
}

pub fn single_owner(s: &Single) -> OwnerReference {
    OwnerReference {
        api_version: "n8n.slys.dev/v1".to_string(),
        kind: "Single".to_string(),
        name: s.name_any(),
        uid: s.uid().expect("Single lacks uid"),
        controller: Some(true),
        block_owner_deletion: Some(true),
    }
}

pub fn cluster_owner(c: &Cluster) -> OwnerReference {
    OwnerReference {
        api_version: "n8n.slys.dev/v1".to_string(),
        kind: "Cluster".to_string(),
        name: c.name_any(),
        uid: c.uid().expect("Cluster lacks uid"),
        controller: Some(true),
        block_owner_deletion: Some(true),
    }
}
