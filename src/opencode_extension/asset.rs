//! Reference-only plugin pinned to anomalyco/opencode v1.19.5,
//! 810c0eba1a7348036f8bdf61e6632d0c36c6876c. Not the changed dev 611cee9 API.

use crate::{Error, Result};

pub(super) fn generate(executable: &str, socket: &str) -> Result<Vec<u8>> {
    const LIMIT: usize = 1024 * 1024;
    const BODY: &str = include_str!("template.js");
    if executable.len().saturating_add(socket.len()) > LIMIT {
        return Err(Error("opencode_extension_asset_too_large"));
    }
    let executable = serde_json::to_string(executable)?;
    let socket = serde_json::to_string(socket)?;
    let source = format!("const EXECUTABLE = {executable};\nconst SOCKET = {socket};\n{BODY}");
    if source.len() > LIMIT {
        return Err(Error("opencode_extension_asset_too_large"));
    }
    Ok(source.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_json_literals_and_generation_is_deterministic() {
        let executable = "/quoted/\"\\\n${SOCKET}/__SOCKET__";
        let socket = "/socket/\";globalThis.injected=true;//${EXECUTABLE}/__EXECUTABLE__";
        let bytes = generate(executable, socket).unwrap();
        assert_eq!(bytes, generate(executable, socket).unwrap());
        let source = String::from_utf8(bytes).unwrap();
        assert_eq!(
            source,
            format!(
                "const EXECUTABLE = {};\nconst SOCKET = {};\n{}",
                serde_json::to_string(executable).unwrap(),
                serde_json::to_string(socket).unwrap(),
                include_str!("template.js")
            )
        );
    }

    #[test]
    fn generation_is_bounded_before_and_after_encoding() {
        assert!(generate(&"x".repeat(1024 * 1024), "x").is_err());
        assert!(generate(&"\n".repeat(600_000), "x").is_err());
        assert!(generate("/bin/coven", "/tmp/coven.sock").unwrap().len() < 1024 * 1024);
    }
}
