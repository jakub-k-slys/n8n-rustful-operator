use std::collections::BTreeMap;

/// Stable subset used as `Deployment.spec.selector` and `Service.spec.selector`.
/// These two labels MUST NOT change — selectors are immutable after creation.
pub fn selector_labels(name: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("app.kubernetes.io/name".to_string(), "n8n".to_string());
    m.insert("app.kubernetes.io/instance".to_string(), name.to_string());
    m
}

/// Full label set put on `metadata.labels` of every managed object and on the
/// pod template. Superset of `selector_labels` plus the four other
/// recommended app.kubernetes.io labels.
pub fn common_labels(name: &str, image: &str, component: &str) -> BTreeMap<String, String> {
    let mut m = selector_labels(name);
    m.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "n8n-rustful-operator".to_string(),
    );
    m.insert("app.kubernetes.io/part-of".to_string(), "n8n".to_string());
    m.insert("app.kubernetes.io/component".to_string(), component.to_string());
    m.insert("app.kubernetes.io/version".to_string(), image_version(image));
    m
}

pub fn image_version(image: &str) -> String {
    let last = image.rsplit('/').next().unwrap_or(image);
    last.rsplit_once(':')
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| "latest".to_string())
}

pub fn common_annotations() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert(
        "n8n.slys.dev/operator-version".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    m
}
