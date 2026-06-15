pub mod labels;
pub mod reconcile;

pub use labels::{ErrorLabels, TraceLabel};
pub use reconcile::{ReconcileMeasurer, ReconcileMetrics};

use prometheus_client::registry::Registry;
use std::sync::Arc;

#[derive(Clone)]
pub struct Metrics {
    pub reconcile: ReconcileMetrics,
    pub registry: Arc<Registry>,
}

impl Default for Metrics {
    fn default() -> Self {
        let mut registry = Registry::with_prefix("n8n_operator_reconcile");
        let reconcile = ReconcileMetrics::default().register(&mut registry);
        Self {
            registry: Arc::new(registry),
            reconcile,
        }
    }
}
