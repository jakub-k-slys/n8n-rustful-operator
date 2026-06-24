use crate::{
    env::{database::build_db_env, env_secret, env_str},
    spec::{Cluster, RedisConfig, SecretKeyRef},
};
use serde_json::{Value, json};

pub fn build_redis_env(redis: &RedisConfig) -> Vec<Value> {
    let mut out = vec![json!({ "name": "QUEUE_BULL_REDIS_HOST", "value": redis.host })];
    if let Some(p) = redis.port {
        out.push(env_str("QUEUE_BULL_REDIS_PORT", p.to_string()));
    }
    if let Some(d) = redis.db {
        out.push(env_str("QUEUE_BULL_REDIS_DB", d.to_string()));
    }
    if let Some(s) = &redis.password_secret {
        out.push(env_secret("QUEUE_BULL_REDIS_PASSWORD", s));
    }
    if let Some(s) = &redis.username_secret {
        out.push(env_secret("QUEUE_BULL_REDIS_USERNAME", s));
    }
    if let Some(t) = redis.tls {
        out.push(env_str("QUEUE_BULL_REDIS_TLS", t.to_string()));
    }
    if let Some(p) = &redis.prefix {
        out.push(json!({ "name": "QUEUE_BULL_PREFIX", "value": p }));
    }
    out
}

pub fn build_cluster_common_env(c: &Cluster, key_secret: &SecretKeyRef) -> Vec<Value> {
    let mut env = vec![
        env_str("EXECUTIONS_MODE", "queue"),
        json!({
            "name": "N8N_ENCRYPTION_KEY",
            "valueFrom": { "secretKeyRef": { "name": key_secret.name, "key": key_secret.key } }
        }),
    ];
    env.extend(build_db_env(&c.spec.database));
    env.extend(build_redis_env(&c.spec.redis));
    if let Some(bd) = &c.spec.binary_data {
        env.extend(crate::env::storage::build_binary_data_env(bd));
    }
    env
}
