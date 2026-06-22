//! Coverage for `gateway_v1_served` — the predicate that decides whether the
//! operator should touch HTTPRoutes. Pure function over an APIGroupList, so no
//! cluster is needed (the live `/apis` lookup is exercised by the e2e suite).

use k8s_openapi::apimachinery::pkg::apis::meta::v1::{APIGroup, APIGroupList, GroupVersionForDiscovery};
use n8n_rustful_operator::builders::http_route::gateway_v1_served;

fn group(name: &str, versions: &[&str]) -> APIGroup {
    APIGroup {
        name: name.to_string(),
        versions: versions
            .iter()
            .map(|v| GroupVersionForDiscovery {
                group_version: format!("{name}/{v}"),
                version: v.to_string(),
            })
            .collect(),
        ..Default::default()
    }
}

fn group_list(groups: Vec<APIGroup>) -> APIGroupList {
    APIGroupList { groups }
}

#[test]
fn served_when_group_has_v1() {
    let gl = group_list(vec![
        group("apps", &["v1"]),
        group("gateway.networking.k8s.io", &["v1beta1", "v1"]),
    ]);
    assert!(gateway_v1_served(&gl));
}

#[test]
fn not_served_when_group_absent() {
    let gl = group_list(vec![group("apps", &["v1"])]);
    assert!(!gateway_v1_served(&gl));
}

#[test]
fn not_served_when_group_present_without_v1() {
    let gl = group_list(vec![group("gateway.networking.k8s.io", &["v1alpha2"])]);
    assert!(!gateway_v1_served(&gl));
}

#[test]
fn not_served_on_empty_discovery() {
    assert!(!gateway_v1_served(&group_list(vec![])));
}
