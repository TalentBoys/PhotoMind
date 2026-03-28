use thiserror::Error;

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    NotFound(String),

    #[error("Tool disabled: {0}")]
    Disabled(String),

    #[error("Tool execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Confirmation required for tool: {0}")]
    ConfirmationRequired(String),

    #[error("Delete operations are not allowed")]
    DeleteNotAllowed,

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
