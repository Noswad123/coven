//! Bounded task-response admission before authoritative publication.
use crate::{Error, Result, error::require, values::Value};

pub(crate) const RESPONSE_RESERVE: usize = 8192;

pub(crate) fn response(value: &Value) -> Result<()> {
    let compact = value.compact()?;
    let ascii = crate::output::ascii_json(std::str::from_utf8(&compact)?)?;
    require(
        compact.len() <= crate::values::MAX_OUTPUT - RESPONSE_RESERVE
            && ascii.len() <= crate::values::MAX_OUTPUT - RESPONSE_RESERVE,
        "unsupported_task_response_profile",
    )
}

pub(crate) fn warning_code(error: &Error) -> &str {
    let code = error.code();
    if code.len() <= 128 && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        code
    } else {
        "secondary_operation_failed"
    }
}
