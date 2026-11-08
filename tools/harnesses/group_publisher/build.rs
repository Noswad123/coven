//! Adapt unwired sibling modules' implicit child paths for this standalone harness.
//! Generated copies live only in Cargo OUT_DIR; application sources are never edited.
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn emit(path: &Path, out: &Path, name: &str) -> PathBuf {
    println!("cargo:rerun-if-changed={}", path.display());
    let mut source = fs::read_to_string(path).expect("sibling source");
    // Focused tests belong to this domain, not the imported storage/authority domains.
    if let Some(at) = ["#[cfg(test)]", "#[cfg(all(test, unix))]"]
        .iter()
        .filter_map(|marker| source.find(marker))
        .min()
    {
        source.truncate(at);
    }
    source = source.replace("//!", "//");
    let directory = path.with_extension("");
    let mut result = String::new();
    for line in source.lines() {
        let declaration = line.trim();
        let bare = declaration
            .strip_prefix("pub(crate) ")
            .or_else(|| declaration.strip_prefix("pub "))
            .unwrap_or(declaration);
        if let Some(module) = bare.strip_prefix("mod ").and_then(|m| m.strip_suffix(';')) {
            let child = directory.join(format!("{module}.rs"));
            let generated = emit(&child, out, &format!("{name}_{module}"));
            result.push_str(&format!("#[path = {:?}] ", generated));
        }
        result.push_str(line);
        result.push('\n');
    }
    let generated = out.join(format!("{name}.rs"));
    fs::write(&generated, result).expect("generated harness module");
    generated
}

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest"))
        .join("../../../src")
        .canonicalize()
        .expect("parent src");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("out"));
    for name in ["storage", "groups", "djinn_live_observation"] {
        emit(&root.join(format!("{name}.rs")), &out, name);
    }
    emit(&root.join("platform/unix/init.rs"), &out, "init_platform");
}
