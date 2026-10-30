//! Bounded finite Python-compatible values, separate from closed handoff JSON.
//! Never route these extension values through serde_json::Value (large integers).
mod encode;
mod parser;

use crate::{Result, error::require};

pub const MAX_INPUT: usize = 8 * 1_048_576;
pub const MAX_OUTPUT: usize = 32 * 1_048_576;
pub const MAX_DEPTH: usize = 64;
pub const MAX_NODES: usize = 262_144;
pub const MAX_MEMBERS: usize = 16_384;
pub const MAX_ITEMS: usize = 65_536;
pub const MAX_STRING: usize = 1_048_576;
pub const MAX_NUMBER: usize = 8192;
pub const MAX_INTEGER_DIGITS: usize = 4300;

#[derive(Debug, Clone, Copy)]
pub enum Profile {
    LegacySnapshot,
    Authority,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecimalInteger(String);
impl DecimalInteger {
    pub fn parse(raw: &str) -> Result<Self> {
        let digits = raw.strip_prefix('-').unwrap_or(raw);
        require(
            !digits.is_empty()
                && digits.len() <= MAX_INTEGER_DIGITS
                && digits.bytes().all(|c| c.is_ascii_digit())
                && (digits.len() == 1 || !digits.starts_with('0')),
            "unsupported_integer_profile",
        )?;
        Ok(Self(if raw == "-0" {
            "0".to_owned()
        } else {
            raw.to_owned()
        }))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FiniteFloat(f64);
impl FiniteFloat {
    pub fn new(value: f64) -> Result<Self> {
        require(value.is_finite(), "unsupported_nonfinite_json")?;
        Ok(Self(value))
    }
    pub fn value(self) -> f64 {
        self.0
    }
    pub fn bits(self) -> u64 {
        self.0.to_bits()
    }
}

#[derive(Debug, Clone)]
pub enum Number {
    Integer(DecimalInteger),
    Float(FiniteFloat),
}

#[derive(Debug, Clone)]
pub struct Object {
    entries: Vec<(String, Value)>,
}
impl Object {
    pub fn entries(&self) -> &[(String, Value)] {
        &self.entries
    }
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.entries
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value))
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    String(String),
    Number(Number),
    Array(Vec<Value>),
    Object(Object),
}
impl Value {
    pub fn integer(raw: &str) -> Result<Self> {
        Ok(Self::Number(Number::Integer(DecimalInteger::parse(raw)?)))
    }
    pub fn float(value: f64) -> Result<Self> {
        Ok(Self::Number(Number::Float(FiniteFloat::new(value)?)))
    }
    pub fn object(entries: Vec<(String, Value)>) -> Result<Self> {
        let mut keys = std::collections::HashSet::new();
        require(
            entries.iter().all(|(key, _)| keys.insert(key)),
            "duplicate_json_key",
        )?;
        Ok(Self::Object(Object { entries }))
    }
    pub fn as_object(&self) -> Option<&Object> {
        if let Self::Object(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn as_array(&self) -> Option<&[Value]> {
        if let Self::Array(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn is_integer_one(&self) -> bool {
        matches!(self, Self::Number(Number::Integer(value)) if value.as_str()=="1")
    }
    pub fn canonical(&self) -> Result<Vec<u8>> {
        encode::encode(self, encode::Mode::Canonical)
    }
    pub fn storage(&self) -> Result<Vec<u8>> {
        encode::encode(self, encode::Mode::Storage)
    }
    pub fn compact(&self) -> Result<Vec<u8>> {
        encode::encode(self, encode::Mode::Compact)
    }
}

pub fn parse(raw: &[u8], profile: Profile) -> Result<Value> {
    parser::parse(raw, profile)
}
