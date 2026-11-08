//! Bounded Python str/repr for finite legacy JSON, not terminal sanitization.
use crate::{
    Result,
    error::require,
    unicode::printable,
    values::{self, Number, Value},
};

pub(super) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::String(value) => !value.is_empty(),
        Value::Number(Number::Integer(value)) => value.as_str() != "0",
        Value::Number(Number::Float(value)) => value.value() != 0.0,
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.entries().is_empty(),
    }
}

#[derive(Default)]
pub(crate) struct Output(String);

impl Output {
    pub(crate) fn finish(self) -> String {
        self.0
    }

    pub(crate) fn push(&mut self, text: &str) -> Result<()> {
        require(
            text.len() <= values::MAX_OUTPUT.saturating_sub(self.0.len()),
            "unsupported_history_output_profile",
        )?;
        self.0.push_str(text);
        Ok(())
    }

    pub(crate) fn value(&mut self, value: &Value) -> Result<()> {
        if let Value::String(text) = value {
            self.push(text)
        } else {
            self.repr(value, 0)
        }
    }

    fn string(&mut self, text: &str) -> Result<()> {
        // CPython prefers single quotes, unless double quotes avoid escaping.
        let quote = if text.contains('\'') && !text.contains('"') {
            '"'
        } else {
            '\''
        };
        let mut buffer = [0; 4];
        self.push(quote.encode_utf8(&mut buffer))?;
        for ch in text.chars() {
            match ch {
                '\\' => self.push("\\\\")?,
                '\t' => self.push("\\t")?,
                '\n' => self.push("\\n")?,
                '\r' => self.push("\\r")?,
                ch if ch == quote => {
                    self.push("\\")?;
                    self.push(ch.encode_utf8(&mut buffer))?;
                }
                ch if printable(ch) => self.push(ch.encode_utf8(&mut buffer))?,
                ch => {
                    let code = ch as u32;
                    let escaped = if code <= 0xff {
                        format!("\\x{code:02x}")
                    } else if code <= 0xffff {
                        format!("\\u{code:04x}")
                    } else {
                        format!("\\U{code:08x}")
                    };
                    self.push(&escaped)?;
                }
            }
        }
        self.push(quote.encode_utf8(&mut buffer))
    }

    fn repr(&mut self, value: &Value, depth: usize) -> Result<()> {
        require(depth <= values::MAX_DEPTH, "unsupported_json_depth_profile")?;
        match value {
            Value::Null => self.push("None"),
            Value::Bool(value) => self.push(if *value { "True" } else { "False" }),
            Value::String(text) => self.string(text),
            Value::Number(Number::Integer(value)) => self.push(value.as_str()),
            Value::Number(Number::Float(_)) => self.push(std::str::from_utf8(&value.compact()?)?),
            Value::Array(items) => {
                require(depth < values::MAX_DEPTH, "unsupported_json_depth_profile")?;
                self.push("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index != 0 {
                        self.push(", ")?;
                    }
                    self.repr(item, depth + 1)?;
                }
                self.push("]")
            }
            Value::Object(object) => {
                require(depth < values::MAX_DEPTH, "unsupported_json_depth_profile")?;
                self.push("{")?;
                for (index, (key, item)) in object.entries().iter().enumerate() {
                    if index != 0 {
                        self.push(", ")?;
                    }
                    self.string(key)?;
                    self.push(": ")?;
                    self.repr(item, depth + 1)?;
                }
                self.push("}")
            }
        }
    }
}

#[cfg(test)]
#[path = "python/tests.rs"]
mod tests;
