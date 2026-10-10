//! Bounded candidate production and verification. Publishing is a separate action.
pub mod archive;
pub mod build;
pub mod demo_record;
pub mod inventory;
pub mod manifest;
pub mod sign;
pub mod verify;

pub type Result<T> = std::result::Result<T, ReleaseError>;
#[derive(Debug, thiserror::Error)]
pub enum ReleaseError {
    #[error("release I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("release JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid release: {0}")]
    Invalid(&'static str),
    #[error("release: {0}")]
    Message(String),
    #[error("release tool: {0}")]
    Tool(String),
}
