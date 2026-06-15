use crate::{
    Error,
    metrics::labels::{ErrorLabels, TraceLabel},
};
use kube::ResourceExt;
use opentelemetry::trace::TraceId;
use prometheus_client::{
    metrics::{counter::Counter, exemplar::HistogramWithExemplars, family::Family},
    registry::{Registry, Unit},
};
use tokio::time::Instant;

#[derive(Clone)]
pub struct ReconcileMetrics {
    pub runs: Counter,
    pub failures: Family<ErrorLabels, Counter>,
    pub duration: HistogramWithExemplars<TraceLabel>,
}

impl Default for ReconcileMetrics {
    fn default() -> Self {
        Self {
            runs: Counter::default(),
            failures: Family::<ErrorLabels, Counter>::default(),
            duration: HistogramWithExemplars::new([0.01, 0.1, 0.25, 0.5, 1., 5., 15., 60.].into_iter()),
        }
    }
}

impl ReconcileMetrics {
    pub fn register(self, r: &mut Registry) -> Self {
        r.register_with_unit(
            "duration",
            "reconcile duration",
            Unit::Seconds,
            self.duration.clone(),
        );
        r.register("failures", "reconciliation errors", self.failures.clone());
        r.register("runs", "reconciliations", self.runs.clone());
        self
    }

    pub fn set_failure<R: ResourceExt>(&self, obj: &R, e: &Error) {
        self.failures
            .get_or_create(&ErrorLabels {
                instance: obj.name_any(),
                error: e.metric_label(),
            })
            .inc();
    }

    pub fn count_and_measure(&self, trace_id: &TraceId) -> ReconcileMeasurer {
        self.runs.inc();
        ReconcileMeasurer {
            start: Instant::now(),
            labels: trace_id.try_into().ok(),
            metric: self.duration.clone(),
        }
    }
}

/// Records duration via `Drop`.
pub struct ReconcileMeasurer {
    start: Instant,
    labels: Option<TraceLabel>,
    metric: HistogramWithExemplars<TraceLabel>,
}

impl Drop for ReconcileMeasurer {
    fn drop(&mut self) {
        #[allow(clippy::cast_precision_loss)]
        let duration = self.start.elapsed().as_millis() as f64 / 1000.0;
        self.metric
            .observe(duration, self.labels.take(), Some(std::time::SystemTime::now()));
    }
}
