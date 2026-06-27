//! Coverage for `destination_rule_v1_served` — the predicate that decides
//! whether the operator should manage Istio DestinationRules (multi-main sticky
//! sessions). Pure function over an APIGroupList; the live `/apis` lookup is
//! exercised by the e2e suite.

use k8s_openapi::apimachinery::pkg::apis::meta::v1::{APIGroup, APIGroupList, GroupVersionForDiscovery};
use n8n_rustful_operator::builders::destination_rule::destination_rule_v1_served;

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
        group("networking.istio.io", &["v1beta1", "v1"]),
    ]);
    assert!(destination_rule_v1_served(&gl));
}

#[test]
fn not_served_when_group_absent() {
    let gl = group_list(vec![group("gateway.networking.k8s.io", &["v1"])]);
    assert!(!destination_rule_v1_served(&gl));
}

#[test]
fn not_served_when_group_present_without_v1() {
    let gl = group_list(vec![group("networking.istio.io", &["v1beta1", "v1alpha3"])]);
    assert!(!destination_rule_v1_served(&gl));
}

#[test]
fn not_served_on_empty_discovery() {
    assert!(!destination_rule_v1_served(&group_list(vec![])));
}
