use thiserror::Error;

#[derive(Error, Debug)]
pub enum DeviceOpsError {
    #[error("IPC connection failed: {0}")]
    IpcError(String),

    #[error("Job execution failed: {0}")]
    ExecutionError(String),

    #[error("Security validation failed: {0}")]
    SecurityError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Timeout: command exceeded {0} seconds")]
    TimeoutError(u64),

    #[error("Invalid job document: {0}")]
    InvalidJobDocument(String),
}

pub type Result<T> = std::result::Result<T, DeviceOpsError>;
