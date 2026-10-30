use super::*;
use crate::Error;

#[derive(Clone, Copy)]
pub(super) enum Mode {
    Canonical,
    Storage,
    Compact,
}
struct Encoder {
    text: String,
    mode: Mode,
}

pub(super) fn encode(value: &Value, mode: Mode) -> Result<Vec<u8>> {
    let mut output = Encoder {
        text: String::new(),
        mode,
    };
    output.value(value, 0)?;
    if matches!(mode, Mode::Storage) {
        output.push("\n")?;
    }
    Ok(output.text.into_bytes())
}

impl Encoder {
    fn push(&mut self, value: &str) -> Result<()> {
        require(
            value.len() <= MAX_OUTPUT.saturating_sub(self.text.len()),
            "unsupported_json_output_profile",
        )?;
        self.text.push_str(value);
        Ok(())
    }
    fn indent(&mut self, depth: usize) -> Result<()> {
        self.push(&"  ".repeat(depth))
    }
    fn string(&mut self, value: &str) -> Result<()> {
        require(value.len() <= MAX_STRING, "unsupported_json_string_profile")?;
        self.push(&serde_json::to_string(value)?)
    }
    fn value(&mut self, value: &Value, depth: usize) -> Result<()> {
        require(depth <= MAX_DEPTH, "unsupported_json_depth_profile")?;
        let pretty = matches!(self.mode, Mode::Storage);
        match value {
            Value::Null => self.push("null"),
            Value::Bool(value) => self.push(if *value { "true" } else { "false" }),
            Value::String(value) => self.string(value),
            Value::Number(Number::Integer(value)) => self.push(value.as_str()),
            Value::Number(Number::Float(value)) => self.push(&python_float(*value)?),
            Value::Array(values) => {
                require(depth < MAX_DEPTH, "unsupported_json_depth_profile")?;
                require(values.len() <= MAX_ITEMS, "unsupported_json_array_profile")?;
                self.push("[")?;
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        self.push(",")?;
                    }
                    if pretty {
                        self.push("\n")?;
                        self.indent(depth + 1)?;
                    }
                    self.value(value, depth + 1)?;
                }
                if pretty && !values.is_empty() {
                    self.push("\n")?;
                    self.indent(depth)?;
                }
                self.push("]")
            }
            Value::Object(object) => {
                require(depth < MAX_DEPTH, "unsupported_json_depth_profile")?;
                require(
                    object.entries.len() <= MAX_MEMBERS,
                    "unsupported_json_object_profile",
                )?;
                let mut entries = object.entries.iter().collect::<Vec<_>>();
                if matches!(self.mode, Mode::Canonical) {
                    entries.sort_by(|a, b| a.0.cmp(&b.0));
                }
                self.push("{")?;
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        self.push(",")?;
                    }
                    if pretty {
                        self.push("\n")?;
                        self.indent(depth + 1)?;
                    }
                    self.string(key)?;
                    self.push(if pretty { ": " } else { ":" })?;
                    self.value(value, depth + 1)?;
                }
                if pretty && !entries.is_empty() {
                    self.push("\n")?;
                    self.indent(depth)?;
                }
                self.push("}")
            }
        }
    }
}

fn python_float(value: FiniteFloat) -> Result<String> {
    let value = value.value();
    if value == 0.0 {
        return Ok(if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .to_owned());
    }
    // Ryu supplies shortest round-to-even digits; the codec separately applies
    // Python's exponent thresholds/spelling. Exact digit/parsing parity is gated
    // by the independent Python bit-pattern/midpoint corpus, not assumed by name.
    let mut buffer = ryu::Buffer::new();
    let rendered = buffer.format_finite(value.abs());
    let (mantissa, exp) = match rendered.split_once('e') {
        Some((mantissa, exp)) => (
            mantissa,
            exp.parse::<i32>()
                .map_err(|_| Error("float_encoding_failed"))?,
        ),
        None => (rendered, 0),
    };
    let point = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let all = mantissa.replace('.', "");
    let leading = all.bytes().take_while(|c| *c == b'0').count();
    let exponent = point - leading as i32 - 1 + exp;
    let digits = all[leading..].trim_end_matches('0');
    require(!digits.is_empty(), "float_encoding_failed")?;
    let mut output = if value.is_sign_negative() {
        "-".to_owned()
    } else {
        String::new()
    };
    if !(-4..16).contains(&exponent) {
        output.push_str(&digits[..1]);
        if digits.len() > 1 {
            output.push('.');
            output.push_str(&digits[1..]);
        }
        output.push_str(&format!("e{exponent:+03}"));
    } else {
        let decimal = exponent + 1;
        if decimal <= 0 {
            output.push_str("0.");
            output.push_str(&"0".repeat((-decimal) as usize));
            output.push_str(digits);
        } else if decimal as usize >= digits.len() {
            output.push_str(digits);
            output.push_str(&"0".repeat(decimal as usize - digits.len()));
            output.push_str(".0");
        } else {
            let at = decimal as usize;
            output.push_str(&digits[..at]);
            output.push('.');
            output.push_str(&digits[at..]);
        }
    }
    Ok(output)
}
