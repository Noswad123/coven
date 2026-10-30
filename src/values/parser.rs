use super::*;
use crate::Error;
use std::collections::HashMap;

struct Parser<'a> {
    raw: &'a [u8],
    at: usize,
    nodes: usize,
    profile: Profile,
}

pub(super) fn parse(raw: &[u8], profile: Profile) -> Result<Value> {
    require(raw.len() <= MAX_INPUT, "unsupported_json_size_profile")?;
    std::str::from_utf8(raw)?;
    let mut parser = Parser {
        raw,
        at: 0,
        nodes: 0,
        profile,
    };
    let value = parser.value(0)?;
    parser.whitespace();
    require(parser.at == raw.len(), "invalid_json_trailing_data")?;
    Ok(value)
}

impl Parser<'_> {
    fn whitespace(&mut self) {
        while self
            .raw
            .get(self.at)
            .is_some_and(|c| matches!(c, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.at += 1;
        }
    }
    fn take(&mut self, byte: u8) -> Result<()> {
        require(self.raw.get(self.at) == Some(&byte), "invalid_compat_json")?;
        self.at += 1;
        Ok(())
    }
    fn literal(&mut self, bytes: &[u8], value: Value) -> Result<Value> {
        require(
            self.raw.get(self.at..self.at + bytes.len()) == Some(bytes),
            "invalid_compat_json",
        )?;
        self.at += bytes.len();
        Ok(value)
    }
    fn value(&mut self, depth: usize) -> Result<Value> {
        require(depth <= MAX_DEPTH, "unsupported_json_depth_profile")?;
        self.nodes += 1;
        require(self.nodes <= MAX_NODES, "unsupported_json_node_profile")?;
        self.whitespace();
        match self.raw.get(self.at) {
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b'N' | b'I') => Err(Error("unsupported_nonfinite_json")),
            _ => Err(Error("invalid_compat_json")),
        }
    }
    fn string(&mut self) -> Result<String> {
        let start = self.at;
        self.take(b'"')?;
        loop {
            match self.raw.get(self.at) {
                Some(b'"') => {
                    self.at += 1;
                    break;
                }
                Some(b'\\') => {
                    self.at += 1;
                    require(self.at < self.raw.len(), "invalid_compat_json")?;
                    self.at += 1;
                }
                Some(byte) if *byte >= 32 => self.at += 1,
                _ => return Err(Error("invalid_compat_json_string")),
            }
        }
        let value: String = serde_json::from_slice(&self.raw[start..self.at])
            .map_err(|_| Error("unsupported_or_invalid_json_scalar"))?;
        require(value.len() <= MAX_STRING, "unsupported_json_string_profile")?;
        Ok(value)
    }
    fn digits(&mut self, start: usize) -> Result<()> {
        let first = self.at;
        while self.raw.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
            require(
                self.at - start <= MAX_NUMBER,
                "unsupported_number_token_profile",
            )?;
        }
        require(self.at > first, "invalid_json_number")
    }
    fn number(&mut self) -> Result<Value> {
        let start = self.at;
        if self.raw.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        if self.raw.get(self.at) == Some(&b'0') {
            self.at += 1;
        } else {
            require(
                self.raw
                    .get(self.at)
                    .is_some_and(|c| (b'1'..=b'9').contains(c)),
                "invalid_json_number",
            )?;
            self.digits(start)?;
        }
        let mut float = false;
        if self.raw.get(self.at) == Some(&b'.') {
            float = true;
            self.at += 1;
            self.digits(start)?;
        }
        if self
            .raw
            .get(self.at)
            .is_some_and(|c| matches!(c, b'e' | b'E'))
        {
            float = true;
            self.at += 1;
            if self
                .raw
                .get(self.at)
                .is_some_and(|c| matches!(c, b'+' | b'-'))
            {
                self.at += 1;
            }
            self.digits(start)?;
        }
        require(
            self.at - start <= MAX_NUMBER,
            "unsupported_number_token_profile",
        )?;
        let token = std::str::from_utf8(&self.raw[start..self.at])?;
        if float {
            Value::float(
                token
                    .parse::<f64>()
                    .map_err(|_| Error("invalid_json_number"))?,
            )
        } else {
            Value::integer(token)
        }
    }
    fn array(&mut self, depth: usize) -> Result<Value> {
        require(depth < MAX_DEPTH, "unsupported_json_depth_profile")?;
        self.take(b'[')?;
        self.whitespace();
        let mut values = Vec::new();
        if self.raw.get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(Value::Array(values));
        }
        loop {
            require(values.len() < MAX_ITEMS, "unsupported_json_array_profile")?;
            values.push(self.value(depth + 1)?);
            self.whitespace();
            if self.raw.get(self.at) == Some(&b']') {
                self.at += 1;
                break;
            }
            self.take(b',')?;
            self.whitespace();
        }
        Ok(Value::Array(values))
    }
    fn object(&mut self, depth: usize) -> Result<Value> {
        require(depth < MAX_DEPTH, "unsupported_json_depth_profile")?;
        self.take(b'{')?;
        self.whitespace();
        let mut entries: Vec<(String, Value)> = Vec::new();
        let mut indexes: HashMap<String, usize> = HashMap::new();
        let mut members = 0;
        if self.raw.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Value::Object(Object { entries }));
        }
        loop {
            members += 1;
            require(members <= MAX_MEMBERS, "unsupported_json_object_profile")?;
            let key = self.string()?;
            self.whitespace();
            self.take(b':')?;
            let duplicate = indexes.get(&key).copied();
            require(
                duplicate.is_none() || matches!(self.profile, Profile::LegacySnapshot),
                "duplicate_json_key",
            )?;
            let value = self.value(depth + 1)?;
            if let Some(index) = duplicate {
                entries[index].1 = value;
            } else {
                indexes.insert(key.clone(), entries.len());
                entries.push((key, value));
            }
            self.whitespace();
            if self.raw.get(self.at) == Some(&b'}') {
                self.at += 1;
                break;
            }
            self.take(b',')?;
            self.whitespace();
        }
        Ok(Value::Object(Object { entries }))
    }
}
