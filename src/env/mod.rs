pub mod database;
pub mod redis;

use crate::spec::SecretKeyRef;
use serde_json::{Value, json};

pub fn env_str(name: &str, value: impl Into<Value>) -> Value {
    json!({
        "name": name,
        "value": value.into().to_string().trim_matches('"').to_string()
    })
}

pub fn env_secret(name: &str, sec: &SecretKeyRef) -> Value {
    json!({
        "name": name,
        "valueFrom": { "secretKeyRef": { "name": sec.name, "key": sec.key } }
    })
}
