use opentelemetry::trace::TraceId;
use prometheus_client::encoding::EncodeLabelSet;

#[derive(Clone, Hash, PartialEq, Eq, EncodeLabelSet, Debug, Default)]
pub struct TraceLabel {
    pub trace_id: String,
}

impl TryFrom<&TraceId> for TraceLabel {
    type Error = anyhow::Error;

    fn try_from(id: &TraceId) -> Result<TraceLabel, Self::Error> {
        if std::matches!(id, &TraceId::INVALID) {
            anyhow::bail!("invalid trace id")
        } else {
            Ok(Self {
                trace_id: id.to_string(),
            })
        }
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct ErrorLabels {
    pub instance: String,
    pub error: String,
}
