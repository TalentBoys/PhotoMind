use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Storage error: {0}")]
    Storage(#[from] photomind_storage::StorageError),

    #[error("Tool error: {0}")]
    Tool(#[from] photomind_tools::ToolError),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Scanner error: {0}")]
    Scanner(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
