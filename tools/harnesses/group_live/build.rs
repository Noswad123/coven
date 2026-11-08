use std::{fs, path::Path};

fn copy(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == "verify" {
            continue;
        }
        let to = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), to).unwrap();
        }
    }
}
fn main() {
    let source = Path::new("../../../src").canonicalize().unwrap();
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed=template.rs");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("native");
    copy(&source, &out);
    // Do not compile unrelated owner suites/bootstrap through this focused crate.
    let storage = fs::read_to_string(out.join("storage.rs")).unwrap();
    fs::write(
        out.join("storage.rs"),
        storage.replace("pub(crate) mod bootstrap;", ""),
    )
    .unwrap();
    let groups = fs::read_to_string(out.join("groups.rs")).unwrap();
    fs::write(
        out.join("groups.rs"),
        groups.split("\n#[cfg(test)]").next().unwrap(),
    )
    .unwrap();
    let template = fs::read_to_string("template.rs").unwrap();
    let mut entry = String::new();
    for line in template.lines() {
        if line.starts_with("#!") {
            continue;
        }
        if line.starts_with("//!") {
            entry.push_str(&line.replacen("//!", "//", 1));
        } else if line.starts_with("#[path") {
            if line.contains("values/mod.rs") || line.contains("live_artifact.rs") {
                entry.push_str(&line.replace("../../", ""));
            } else {
                if let Some((_, body)) = line.split_once("] ") {
                    entry.push_str(body);
                }
            }
        } else {
            entry.push_str(line);
        }
        entry.push('\n');
    }
    fs::write(out.join("entry.rs"), entry).unwrap();
}
