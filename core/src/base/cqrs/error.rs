use std::error;

#[derive(Debug, thiserror::Error)]
pub enum AggregateError<T: error::Error> {
    #[error("{0}")]
    UserError(T),

    #[error("aggregate conflict")]
    AggregateConflict,

    #[error("{0}")]
    DatabaseConnectionError(Box<dyn error::Error + Send + Sync + 'static>),

    #[error("{0}")]
    DeserializationError(Box<dyn error::Error + Send + Sync + 'static>),

    #[error("{0}")]
    UnexpectedError(Box<dyn error::Error + Send + Sync + 'static>),
}
