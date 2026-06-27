use crate::spec::common::SharedStorage;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Community-node handling for a `Cluster`. In queue mode a node installed on
/// one pod isn't visible to the others, so pick one strategy to make the set
/// consistent across roles: declarative `packages` (GitOps), a shared `nodes`
/// volume (`sharedStorage`), or the legacy `reinstallMissing`. They are
/// mutually exclusive.
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct CommunityNodesConfig {
    /// `N8N_COMMUNITY_PACKAGES_ENABLED`. Omit for the n8n default (true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Declaratively managed package set. Sets `N8N_COMMUNITY_PACKAGES` and
    /// `N8N_COMMUNITY_PACKAGES_MANAGED_BY_ENV=true`, so every role reconciles to
    /// this exact list on startup and the UI install page becomes read-only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<CommunityPackage>,
    /// Shared `ReadWriteMany` volume mounted at `~/.n8n/nodes` on every role, so
    /// a UI install propagates. Mutually exclusive with `packages`.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "sharedStorage")]
    pub shared_storage: Option<SharedStorage>,
    /// `N8N_REINSTALL_MISSING_PACKAGES`. Legacy fallback (each pod reinstalls
    /// missing packages from the DB on boot) — discouraged by the n8n docs as it
    /// can slow startup, fail health checks, and double-load packages. Defaults
    /// to off; prefer `packages` or `sharedStorage`.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "reinstallMissing")]
    pub reinstall_missing: Option<bool>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct CommunityPackage {
    /// npm package name, e.g. `n8n-nodes-foo`.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}
