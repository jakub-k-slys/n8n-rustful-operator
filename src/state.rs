use crate::Metrics;
use jiff::Timestamp;
use kube::{
    client::Client,
    runtime::events::{Recorder, Reporter},
};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct Context {
    pub client: Client,
    pub recorder: Recorder,
    pub diagnostics: Arc<RwLock<Diagnostics>>,
    pub metrics: Arc<Metrics>,
}

#[derive(Clone, Serialize)]
pub struct Diagnostics {
    pub last_event: Timestamp,
    #[serde(skip)]
    pub reporter: Reporter,
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            last_event: Timestamp::now(),
            reporter: "n8n-rustful-operator".into(),
        }
    }
}

impl Diagnostics {
    pub fn recorder(&self, client: Client) -> Recorder {
        Recorder::new(client, self.reporter.clone())
    }
}

#[derive(Clone, Default)]
pub struct State {
    pub diagnostics: Arc<RwLock<Diagnostics>>,
    pub metrics: Arc<Metrics>,
}

impl State {
    pub fn metrics(&self) -> String {
        let mut buffer = String::new();
        let registry = &*self.metrics.registry;
        prometheus_client::encoding::text::encode(&mut buffer, registry).unwrap();
        buffer
    }

    pub async fn diagnostics(&self) -> Diagnostics {
        self.diagnostics.read().await.clone()
    }

    pub async fn to_context(&self, client: Client) -> Arc<Context> {
        Arc::new(Context {
            client: client.clone(),
            recorder: self.diagnostics.read().await.recorder(client),
            metrics: self.metrics.clone(),
            diagnostics: self.diagnostics.clone(),
        })
    }
}
