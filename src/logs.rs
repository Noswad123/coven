//! Bounded, strict JSONL storage. Callers own workspace preflight and lock epochs.
use crate::{
    Error, Result,
    error::require,
    platform, read_fs, records,
    storage::WorkspaceGuard,
    values::{self, Profile, Value},
};
use std::{fs::File, io::Write, path::Path};

pub(crate) const MAX_LOG: usize = 8 * 1_048_576;
pub(crate) const MAX_LINES: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LogKind {
    Events,
    Messages,
}

impl LogKind {
    pub(crate) fn relative(self) -> &'static str {
        match self {
            Self::Events => "logs/events.jsonl",
            Self::Messages => "logs/messages.jsonl",
        }
    }
}

fn bytes(root: &Path, relative: &str) -> Result<Vec<u8>> {
    let path = read_fs::managed(root, Path::new(relative), false)?;
    match std::fs::symlink_metadata(&path) {
        Ok(_) => read_fs::read_regular(&path, MAX_LOG).map_err(|error| match error.code() {
            "handoff_size_limit" => Error("unsupported_event_log_size_profile"),
            _ => error,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn validate_log(bytes: &[u8]) -> Result<usize> {
    require(bytes.len() <= MAX_LOG, "unsupported_event_log_size_profile")?;
    require(
        bytes.is_empty() || bytes.last() == Some(&b'\n'),
        "event_log_unterminated_line",
    )?;
    let mut count = 0;
    // Split LF only: CR remains JSON whitespace, never a record separator.
    for raw in bytes.split_inclusive(|c| *c == b'\n') {
        count += 1;
        require(count <= MAX_LINES, "unsupported_event_log_line_profile")?;
        let text =
            std::str::from_utf8(&raw[..raw.len() - 1]).map_err(|_| Error("invalid_jsonl_utf8"))?;
        if records::trimmed(text).is_empty() {
            continue;
        }
        // Do not trim nonblank records: json.loads accepts JSON whitespace only.
        let record =
            values::parse(text.as_bytes(), Profile::LegacySnapshot).map_err(|error| match error
                .code()
            {
                "invalid_compat_json"
                | "invalid_compat_json_string"
                | "invalid_json_trailing_data"
                | "invalid_json_number"
                | "unsupported_nonfinite_json"
                | "unsupported_or_invalid_json_scalar" => Error("invalid_jsonl_record"),
                _ => error,
            })?;
        require(record.as_object().is_some(), "invalid_jsonl_record")?;
    }
    Ok(count)
}

pub(crate) fn read_log(root: &Path, relative: &str) -> Result<(usize, usize)> {
    let bytes = bytes(root, relative)?;
    Ok((bytes.len(), validate_log(&bytes)?))
}

pub(crate) fn read(root: &Path, kind: LogKind) -> Result<Vec<Value>> {
    let bytes = bytes(root, kind.relative())?;
    // Admit the entire log before exposing any records or inspecting references.
    validate_log(&bytes)?;
    let mut records = Vec::new();
    for raw in bytes.split_inclusive(|byte| *byte == b'\n') {
        let text = std::str::from_utf8(&raw[..raw.len() - 1])?;
        if !crate::records::trimmed(text).is_empty() {
            records.push(values::parse(text.as_bytes(), Profile::LegacySnapshot)?);
        }
    }
    Ok(records)
}

fn validate_encoded(encoded: &[u8]) -> Result<()> {
    require(validate_log(encoded)? == 1, "invalid_encoded_event")?;
    let record = values::parse(encoded, Profile::LegacySnapshot)
        .map_err(|_| Error("invalid_encoded_event"))?;
    require(record.as_object().is_some(), "invalid_encoded_event")
}

pub(crate) fn encode_record(record: &Value) -> Result<Vec<u8>> {
    let mut encoded = record.compact()?;
    encoded.push(b'\n');
    validate_encoded(&encoded)?;
    Ok(encoded)
}

pub(crate) trait AppendIo {
    fn open(&self, path: &Path) -> Result<File> {
        platform::open_append(path)
    }
    fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
        file.write_all(bytes)
    }
    fn sync(&self, file: &File) -> std::io::Result<()> {
        file.sync_all()
    }
}
pub(crate) struct SystemAppendIo;
impl AppendIo for SystemAppendIo {}

pub(crate) fn append_encoded(
    root: &Path,
    kind: LogKind,
    encoded: &[u8],
    guard: &WorkspaceGuard,
) -> Result<()> {
    append_encoded_with(root, kind, encoded, guard, &SystemAppendIo)
}

pub(crate) fn append_encoded_with(
    root: &Path,
    kind: LogKind,
    encoded: &[u8],
    guard: &WorkspaceGuard,
    io: &impl AppendIo,
) -> Result<()> {
    guard.validate_for(root)?;
    validate_encoded(encoded)?;
    let (bytes, lines) = read_log(root, kind.relative())?;
    require(
        encoded.len() <= MAX_LOG - bytes,
        "unsupported_event_log_size_profile",
    )?;
    require(lines < MAX_LINES, "unsupported_event_log_line_profile")?;
    // Only this managed parent is created, after admission, with default umask.
    let logs = read_fs::managed(root, Path::new("logs"), true)?;
    std::fs::create_dir_all(&logs)?;
    read_fs::managed(root, Path::new("logs"), true)?;
    let path = read_fs::managed(root, Path::new(kind.relative()), false)?;
    // Never truncate, repair, deduplicate, or roll back partial evidence.
    let mut file = io.open(&path)?;
    io.write(&mut file, encoded)
        .and_then(|()| io.sync(&file))
        .map_err(|_| {
            Error(match kind {
                LogKind::Events => "event_append_may_be_partial",
                LogKind::Messages => "message_append_may_be_partial",
            })
        })
}

#[cfg(test)]
mod tests;
