#![allow(dead_code)] // The focused crate intentionally omits unrelated domain callers.
include!(concat!(env!("OUT_DIR"), "/native/entry.rs"));
