use crate::{
    labels::{common_annotations, common_labels, selector_labels},
    spec::{ServiceConfig, SingleSpec, default_service_type},
};
use k8s_openapi::{
    api::core::v1::{Service, ServicePort, ServiceSpec},
    apimachinery::pkg::{apis::meta::v1::OwnerReference, util::intstr::IntOrString},
};
use kube::api::ObjectMeta;

pub fn build_service(name: &str, spec: &SingleSpec, owner: &OwnerReference) -> Service {
    let svc_type = spec
        .service
        .as_ref()
        .map(|s| s.type_.clone())
        .unwrap_or_else(default_service_type);
    service_with_labels(name, &spec.image, "workflow-engine", svc_type, owner)
}

pub fn build_cluster_service(
    name: &str,
    image: &str,
    component: &str,
    svc: Option<&ServiceConfig>,
    owner: &OwnerReference,
) -> Service {
    let svc_type = svc.map(|s| s.type_.clone()).unwrap_or_else(default_service_type);
    service_with_labels(name, image, component, svc_type, owner)
}

fn service_with_labels(
    name: &str,
    image: &str,
    component: &str,
    svc_type: String,
    owner: &OwnerReference,
) -> Service {
    let selector = selector_labels(name);
    let labels = common_labels(name, image, component);
    Service {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(labels),
            annotations: Some(common_annotations()),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(selector),
            ports: Some(vec![ServicePort {
                name: Some("http".to_string()),
                port: 5678,
                target_port: Some(IntOrString::String("http".to_string())),
                protocol: Some("TCP".to_string()),
                ..Default::default()
            }]),
            type_: Some(svc_type),
            ..Default::default()
        }),
        ..Default::default()
    }
}
