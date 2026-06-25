//! Shared error type for the Hermes install manager.

use std::path::PathBuf;

/// Result alias used by manager modules.
pub type Result<T> = std::result::Result<T, ManagerError>;

/// Errors surfaced by install, repair, and uninstall management commands.
#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON error at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("unsafe path outside Hermes home: {0}")]
    UnsafePath(PathBuf),
}

impl ManagerError {
    /// Wrap an I/O error with the path being operated on.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Wrap a JSON error with the path being parsed.
    pub fn json(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.into(),
            source,
        }
    }
}
