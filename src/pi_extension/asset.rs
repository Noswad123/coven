//! Reference-only extension for earendil-works/pi at
//! 6e11a00a01a568ef0ae0df5074acf02fc107c34b (package 1.5.2 on main, not a tag).

use crate::{Error, Result};

pub(super) fn generate(executable: &str, socket: &str) -> Result<Vec<u8>> {
    const LIMIT: usize = 1024 * 1024;
    const BODY: &str = include_str!("template.js");
    // Bound inputs before JSON encoding as well as the final generated asset.
    if executable.len().saturating_add(socket.len()) > LIMIT {
        return Err(Error("pi_extension_asset_too_large"));
    }
    let executable = serde_json::to_string(executable)?;
    let socket = serde_json::to_string(socket)?;
    let source = format!("const EXECUTABLE = {executable};\nconst SOCKET = {socket};\n{BODY}");
    if source.len() > LIMIT {
        return Err(Error("pi_extension_asset_too_large"));
    }
    Ok(source.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_json_literals_not_template_replacements() {
        let executable = "/quoted/\"\\\n${SOCKET}/__SOCKET__";
        let socket = "/socket/\";globalThis.injected=true;//";
        let source = String::from_utf8(generate(executable, socket).unwrap()).unwrap();
        let mut lines = source.lines();
        assert_eq!(
            lines.next().unwrap(),
            format!(
                "const EXECUTABLE = {};",
                serde_json::to_string(executable).unwrap()
            )
        );
        assert_eq!(
            lines.next().unwrap(),
            format!("const SOCKET = {};", serde_json::to_string(socket).unwrap())
        );
        assert!(source.ends_with(include_str!("template.js")));
    }

    #[test]
    fn generation_is_bounded_after_encoding() {
        assert!(generate(&"x".repeat(1024 * 1024), "x").is_err());
        assert!(generate(&"\n".repeat(600_000), "x").is_err());
        assert!(generate("/bin/coven", "/tmp/coven.sock").unwrap().len() < 1024 * 1024);
    }
}
