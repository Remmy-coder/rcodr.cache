use thiserror::Error;

#[derive(Error, Debug)]
pub enum CacheError {
    #[error("Cache size limit exceeded")]
    SizeLimitExceeded,

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    #[error("Persistence error: {0}")]
    PersistenceError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Lock acquisition failed")]
    LockError,

    #[error("Background task error: {0}")]
    BackgroundTaskError(String),
}


pub type Result<T> = std::result::Result<T, CacheError>;
