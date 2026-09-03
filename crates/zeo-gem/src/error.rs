//! One error type for every format this crate reads.
//!
//! The variants say WHERE the input went wrong, because a caller's report
//! ("reading Gemfile.lock: ...") already says which file. `message` is the
//! whole text for a caller that carries its own error type.

use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The bytes are not the format they claim to be.
    #[error("{0}")]
    Format(String),
}

impl Error {
    pub fn format(message: impl Into<String>) -> Self {
        Error::Format(message.into())
    }

    pub fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// The whole error as text, for a caller that wraps it in its own type.
    pub fn message(&self) -> String {
        self.to_string()
    }
}

/// `std::fs::read` that names the file it could not read.
pub(crate) fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| Error::io(path, e))
}
