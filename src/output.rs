//! Presentation escaping, never persisted canonical encoding.
use crate::{Error, Result};
use std::fmt::Write;

pub(crate) fn ascii_json(json: &str) -> Result<String> {
    let mut escaped = String::new();
    for character in json.chars() {
        let additional = if character < '\u{7f}' {
            1
        } else {
            6 * character.len_utf16()
        };
        crate::error::require(
            additional <= crate::values::MAX_OUTPUT.saturating_sub(escaped.len()),
            "unsupported_json_output_profile",
        )?;
        if character < '\u{7f}' {
            escaped.push(character);
        } else {
            let mut units = [0_u16; 2];
            for unit in character.encode_utf16(&mut units) {
                write!(&mut escaped, "\\u{unit:04x}")
                    .map_err(|_| Error("report_encoding_failed"))?;
            }
        }
    }
    Ok(escaped)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ascii_expansion_has_its_own_complete_output_bound() {
        let text = "\u{7f}".repeat(crate::values::MAX_OUTPUT / 6 + 1);
        assert_eq!(
            ascii_json(&text).unwrap_err().code(),
            "unsupported_json_output_profile"
        );
    }
}
