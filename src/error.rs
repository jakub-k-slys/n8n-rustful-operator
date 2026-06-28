use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("SerializationError: {0}")]
    SerializationError(#[source] serde_json::Error),

    #[error("Kube Error: {0}")]
    KubeError(#[source] kube::Error),

    #[error("Finalizer Error: {0}")]
    FinalizerError(#[source] Box<kube::runtime::finalizer::Error<Error>>),

    #[error("IllegalSingle")]
    IllegalSingle,

    #[error(
        "ConflictingNetworking: spec.networking.ingress and spec.networking.httpRoute are mutually exclusive"
    )]
    ConflictingNetworking,

    #[error("IllegalDatabase: {0}")]
    IllegalDatabase(String),

    #[error("IllegalCluster: {0}")]
    IllegalCluster(String),

    #[error("IllegalEnv: {0}")]
    IllegalEnv(String),

    #[error("IllegalSmtp: {0}")]
    IllegalSmtp(String),

    #[error("IllegalStrategy: {0}")]
    IllegalStrategy(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub fn metric_label(&self) -> String {
        format!("{self:?}").to_lowercase()
    }
}
