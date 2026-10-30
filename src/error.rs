use std::fmt;

/// Fixed codes deliberately exclude untrusted input and OS/native error bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub(crate) &'static str);

impl Error {
    pub fn code(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for Error {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self("input_or_storage_unavailable")
    }
}

impl From<serde_json::Error> for Error {
    fn from(_: serde_json::Error) -> Self {
        Self("invalid_handoff_json_or_schema")
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn require(condition: bool, code: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(Error(code)) }
}
