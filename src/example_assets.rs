//! Relocatable, read-only example assets. Explicit roots are authoritative,
//! including for the demo; implicit discovery never replaces the embedded demo.
use super::{InitExample, init::embedded_example, slug};
use crate::{
    Error, Result,
    error::require,
    read_fs,
    values::{self, Number, Value},
};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

const TEXT_LIMIT: usize = 1_048_576;
const JSON_LIMIT: usize = 8 * TEXT_LIMIT;

pub(crate) struct AssetBundle {
    root: Option<PathBuf>,
    explicit: bool,
}

impl AssetBundle {
    pub(crate) fn environment() -> Result<Self> {
        Self::resolve(
            std::env::var_os("COVEN_EXAMPLES_DIR"),
            std::env::var_os("COVEN_HOME"),
            &std::env::current_exe()?,
        )
    }

    // Inputs are explicit so tests need not mutate the process environment.
    fn resolve(examples: Option<OsString>, home: Option<OsString>, exe: &Path) -> Result<Self> {
        if let Some(root) = examples
            .map(PathBuf::from)
            .or_else(|| home.map(|p| PathBuf::from(p).join("examples")))
        {
            require(root.is_absolute(), "example_root_not_absolute")?;
            // Strip insignificant separators/dots before the no-follow leaf check.
            // Keep `..` and ancestor symlinks in filesystem traversal order.
            let root: PathBuf = root.components().collect();
            require(fs::symlink_metadata(&root)?.is_dir(), "unsafe_example_root")?;
            return Ok(Self {
                root: Some(root),
                explicit: true,
            });
        }
        let exe = exe.canonicalize()?;
        let bin = exe.parent().ok_or(Error("example_location_unavailable"))?;
        let prefix = bin.parent().ok_or(Error("example_location_unavailable"))?;
        let mut candidates = vec![
            prefix.join("share/coven/examples"),
            prefix.join("libexec/examples"),
        ];
        // Only an executable actually in a Cargo target tree may use its checkout.
        let target = if matches!(
            prefix.file_name().and_then(|s| s.to_str()),
            Some("debug" | "release")
        ) {
            prefix.parent()
        } else {
            Some(prefix)
        };
        if let Some(target) = target.filter(|p| p.file_name().is_some_and(|s| s == "target")) {
            if let Some(source) = target.parent() {
                candidates.push(source.join("examples"));
            }
        }
        let mut root = None;
        for candidate in candidates {
            match fs::symlink_metadata(&candidate) {
                Ok(meta) => {
                    require(meta.is_dir(), "unsafe_example_root")?;
                    root = Some(candidate);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(Self {
            root,
            explicit: false,
        })
    }

    fn directory(&self, selector: &str) -> Result<(String, PathBuf)> {
        let id = slug(selector).map_err(|_| Error("unsupported_init_example_profile"))?;
        require(id.len() <= 255, "unsupported_example_filename_profile")?;
        let root = self
            .root
            .as_ref()
            .ok_or(Error("unsupported_init_example_profile"))?;
        let path = read_fs::managed(root, Path::new(&id), true)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error("unsupported_init_example_profile")
            } else {
                error.into()
            }
        })?;
        require(metadata.is_dir(), "unsafe_example_directory")?;
        Ok((id, path))
    }

    fn text(directory: &Path, relative: &str, default: &str, limit: usize) -> Result<String> {
        let path = read_fs::managed(directory, Path::new(relative), false)?;
        match fs::symlink_metadata(&path) {
            Ok(meta) => require(meta.is_file(), "unsafe_example_file")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(default.into()),
            Err(error) => return Err(error.into()),
        }
        let bytes = read_fs::read_regular(&path, limit)?;
        Ok(std::str::from_utf8(&bytes)?
            .replace("\r\n", "\n")
            .replace('\r', "\n"))
    }

    pub(crate) fn context(&self, selector: Option<&str>) -> Result<String> {
        let Some(selector) = selector else {
            return Ok(String::new());
        };
        // Empty metadata is invalid (unlike absence), just as validated teams are.
        require(!selector.is_empty(), "unsupported_refresh_example_profile")?;
        if !self.explicit && slug(selector).is_ok_and(|s| s == "interactive-demo") {
            return Ok(embedded_example()?.context);
        }
        let (_, directory) = self.directory(selector).map_err(|error| {
            if error.code() == "unsupported_init_example_profile" {
                Error("unsupported_refresh_example_profile")
            } else {
                error
            }
        })?;
        // Refresh must validate the manifest, but never loads unrelated seed files.
        self.manifest(&directory)?;
        Self::text(&directory, "prompt-context.md", "", TEXT_LIMIT)
    }

    fn manifest(&self, directory: &Path) -> Result<Value> {
        let path = read_fs::managed(directory, Path::new("example.json"), false)?;
        require(
            fs::symlink_metadata(&path)?.is_file(),
            "invalid_example_manifest",
        )?;
        let text = Self::text(directory, "example.json", "", JSON_LIMIT)?;
        let value = values::parse(text.as_bytes(), values::Profile::LegacySnapshot)?;
        require(value.as_object().is_some(), "invalid_example_manifest")?;
        Ok(value)
    }

    pub(crate) fn load(&self, selector: &str) -> Result<InitExample> {
        if !self.explicit && slug(selector).is_ok_and(|s| s == "interactive-demo") {
            return embedded_example();
        }
        let (fallback_id, directory) = self.directory(selector)?;
        let manifest = self.manifest(&directory)?;
        let fields = manifest
            .as_object()
            .ok_or(Error("invalid_example_manifest"))?;
        let id = metadata_string(fields.get("id"), &fallback_id)?;
        let seed_message =
            metadata_string(fields.get("seed_message"), &format!("Seeded example: {id}"))?;
        let mut goal = Self::text(&directory, "goal.md", super::default_goal(), TEXT_LIMIT)?;
        if !goal.ends_with('\n') {
            goal.push('\n');
        }
        require(goal.len() <= TEXT_LIMIT, "unsupported_example_text_profile")?;
        let tasks = Self::text(&directory, "tasks.json", "[]", JSON_LIMIT)?;
        Ok(InitExample {
            id,
            seed_message,
            goal,
            spec: Self::text(
                &directory,
                "spec.md",
                "# Spec\n\nDescribe the desired behavior here.\n",
                TEXT_LIMIT,
            )?,
            work: Self::text(
                &directory,
                "work/README.md",
                super::default_work(),
                TEXT_LIMIT,
            )?,
            output: Self::text(
                &directory,
                "output/README.md",
                super::default_output(),
                TEXT_LIMIT,
            )?,
            context: Self::text(&directory, "prompt-context.md", "", TEXT_LIMIT)?,
            tasks: values::parse(tasks.as_bytes(), values::Profile::LegacySnapshot)?,
        })
    }
}

fn metadata_string(value: Option<&Value>, fallback: &str) -> Result<String> {
    let truthy = value.is_some_and(|value| match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::String(value) => !value.is_empty(),
        Value::Number(Number::Integer(value)) => value.as_str() != "0",
        Value::Number(Number::Float(value)) => value.value() != 0.0,
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.entries().is_empty(),
    });
    if !truthy {
        return Ok(fallback.into());
    }
    let mut output = crate::workspace_commands::python::Output::default();
    output.value(value.ok_or(Error("invalid_example_manifest"))?)?;
    Ok(output.finish())
}

#[cfg(test)]
#[path = "example_assets/tests.rs"]
mod tests;
