use std::io;
use std::path::Path;

#[derive(Debug)]
pub(crate) enum MkaError {
    Usage(String),
    Runtime(String),
}

impl MkaError {
    pub(crate) fn usage(message: impl Into<String>) -> Self {
        Self::Usage(message.into())
    }

    pub(crate) fn runtime(message: impl Into<String>) -> Self {
        Self::Runtime(message.into())
    }

    pub(crate) fn io(action: &str, path: &Path, error: io::Error) -> Self {
        Self::Runtime(format!("{action} {path:?}: {error}"))
    }
}

pub(crate) type Result<T> = std::result::Result<T, MkaError>;
