use crate::{Error, Result};

fn generate(executable: &str, socket: &str, provider: &str, body: &str) -> Result<Vec<u8>> {
    const LIMIT: usize = 1024 * 1024;
    if executable.len().saturating_add(socket.len()) > LIMIT {
        return Err(Error("native_extension_asset_too_large"));
    }
    let route = crate::integration_route::argv(socket)?;
    let profile = if provider == "pi" {
        "pi-context.v1"
    } else {
        "opencode-root.v1"
    };
    let source = format!(
        "const EXECUTABLE = {};\nconst ROUTE = {};\nconst PROVIDER = {};\nconst PROFILE = {};\n{}\n{}",
        serde_json::to_string(executable)?,
        serde_json::to_string(&route)?,
        serde_json::to_string(provider)?,
        serde_json::to_string(profile)?,
        include_str!("emitter.js"),
        body,
    );
    if source.len() > LIMIT {
        return Err(Error("native_extension_asset_too_large"));
    }
    Ok(source.into_bytes())
}

pub(super) fn pi(executable: &str, socket: &str) -> Result<Vec<u8>> {
    generate(executable, socket, "pi", include_str!("pi.js"))
}

pub(super) fn opencode(executable: &str, socket: &str) -> Result<Vec<u8>> {
    generate(
        executable,
        socket,
        "opencode",
        &format!(
            "{}\n{}",
            include_str!("session.js"),
            include_str!("opencode.js")
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_json_literals_bounded_and_deterministic() {
        for generate in [pi, opencode] {
            let exe = "/literal/\"\\\n${EXECUTABLE}";
            let socket = "/literal/\";injected();//${SOCKET}";
            let source = generate(exe, socket).unwrap();
            assert_eq!(source, generate(exe, socket).unwrap());
            let source = String::from_utf8(source).unwrap();
            assert!(source.starts_with(&format!(
                "const EXECUTABLE = {};\nconst ROUTE = {};\n",
                serde_json::to_string(exe).unwrap(),
                serde_json::to_string(&["--socket", socket]).unwrap(),
            )));
            assert!(generate(&"x".repeat(1024 * 1024), "x").is_err());
            assert!(generate(&"\n".repeat(600_000), "x").is_err());
        }
    }
}
