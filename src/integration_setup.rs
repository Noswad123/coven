//! One-time Kit configuration ownership. No live API, provider installation, or
//! runtime startup is performed here. The journal lives beside config.toml.
//! Pending intents resume only against exact recorded pre/post bytes and modes.
//! A retained hook_files recovery directory requires inspection, not an automatic
//! exchange/rollback that could overwrite a newer user edit.
#![cfg(unix)]

use crate::{
    Error, Result,
    error::require,
    hook_files::{Image, Root, Spec},
    task_mutation::TaskOutput,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use toml_edit::{Array, DocumentMut, Item, Table, value};

mod paths;
#[cfg(test)]
mod tests;

const CONFIG: &str = "config.toml";
const RECEIPT: &str = "coven-kit-integration.json";
const LOCK: &str = ".coven-kit-integration.lock";
const CONFIG_LIMIT: usize = 1024 * 1024;
const JOURNAL_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Plan,
    Enable,
    Disable,
}

pub struct Request {
    pub operation: Operation,
    pub kitsune_config: Option<PathBuf>,
    pub executable: Option<PathBuf>,
    pub state_root: Option<PathBuf>,
    pub apply: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    PendingEnable,
    Active,
    PendingDisable,
    Inactive,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Preimage {
    bytes: Vec<u8>,
    mode: u32,
}

/// v1 journal: exact preimage/absence and mode, exact postimage plus SHA-256,
/// selected paths, and a durable intent/complete stage. Inactive is retained.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    config: PathBuf,
    executable: PathBuf,
    state_root: PathBuf,
    preimage: Option<Preimage>,
    postimage: Vec<u8>,
    post_digest: String,
    stage: Stage,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn generated(bytes: &[u8], executable: &Path, state_root: &Path) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error("integration_setup_invalid_toml"))?;
    let mut doc = text
        .parse::<DocumentMut>()
        .map_err(|_| Error("integration_setup_invalid_toml"))?;
    if !doc.contains_key("integrations") {
        doc["integrations"] = Item::Table(Table::new());
    }
    let integrations = doc["integrations"]
        .as_table_mut()
        .ok_or(Error("integration_setup_incompatible_shape"))?;
    require(
        !integrations.contains_key("runtime"),
        "integration_setup_unmanaged_runtime",
    )?;
    if !integrations.contains_key("sources") {
        integrations["sources"] = value(Array::new());
    }
    let sources = integrations["sources"]
        .as_array_mut()
        .ok_or(Error("integration_setup_incompatible_shape"))?;
    let mut seen = std::collections::HashSet::new();
    for source in sources.iter() {
        let id = source
            .as_str()
            .ok_or(Error("integration_setup_invalid_sources"))?;
        require(
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
                && seen.insert(id.to_owned()),
            "integration_setup_invalid_sources",
        )?;
    }
    require(sources.len() <= 8, "integration_setup_source_capacity")?;
    if !seen.contains("coven") {
        require(sources.len() < 8, "integration_setup_source_capacity")?;
        sources.push("coven");
    }
    let exe = paths::text(executable)?;
    let root = paths::text(state_root)?;
    let mut command = Array::new();
    for arg in [exe, "integration", "runtime", "--state-root", root] {
        command.push(arg);
    }
    let mut reporter = Array::new();
    for arg in [
        exe,
        "agent",
        "report-native",
        "--from-kitsune",
        "--state-root",
        root,
    ] {
        reporter.push(arg);
    }
    let mut runtime = Table::new();
    runtime["source"] = value("coven");
    runtime["command"] = value(command);
    runtime["report_command"] = value(reporter);
    runtime["external_agent_management"] = value(true);
    integrations["runtime"] = Item::Table(runtime);
    let output = doc.to_string().into_bytes();
    require(output.len() <= CONFIG_LIMIT, "integration_setup_size_limit")?;
    Ok(output)
}

impl Receipt {
    fn validate(&self, config: &Path) -> Result<()> {
        require(
            self.version == 1 && self.config == config,
            "integration_setup_invalid_receipt",
        )?;
        paths::absolute(&self.executable)?;
        paths::absolute(&self.state_root)?;
        if let Some(pre) = &self.preimage {
            require(
                pre.bytes.len() <= CONFIG_LIMIT && pre.mode & !0o7777 == 0 && pre.mode & 0o022 == 0,
                "integration_setup_invalid_receipt",
            )?;
        }
        let pre = self
            .preimage
            .as_ref()
            .map_or(&[][..], |p| p.bytes.as_slice());
        require(
            self.postimage.len() <= CONFIG_LIMIT
                && digest(&self.postimage) == self.post_digest
                && generated(pre, &self.executable, &self.state_root)? == self.postimage,
            "integration_setup_invalid_receipt",
        )
    }

    fn pre_matches(&self, image: &Option<Image>) -> bool {
        match (&self.preimage, image) {
            (None, None) => true,
            (Some(pre), Some(image)) => pre.bytes == image.bytes && pre.mode == image.mode & 0o7777,
            _ => false,
        }
    }

    fn post_matches(&self, image: &Option<Image>) -> bool {
        image.as_ref().is_some_and(|image| {
            image.bytes == self.postimage
                && image.mode & 0o7777 == self.preimage.as_ref().map_or(0o600, |p| p.mode)
        })
    }
}

fn journal(root: &Root, image: &mut Option<Image>, receipt: &Receipt) -> Result<()> {
    let bytes = serde_json::to_vec(receipt)?;
    require(bytes.len() <= JOURNAL_LIMIT, "integration_setup_size_limit")?;
    root.replace(RECEIPT, image, Some(&bytes), 0o600)?;
    *image = root.read(RECEIPT, JOURNAL_LIMIT)?;
    require(
        image.as_ref().is_some_and(|i| i.bytes == bytes),
        "integration_setup_conflict",
    )
}

pub fn arguments(request: &Request, json_output: bool) -> Result<TaskOutput> {
    let config = paths::config(request.kitsune_config.as_deref())?;
    let parent = config
        .parent()
        .ok_or(Error("integration_setup_absolute_path_required"))?;
    let apply = request.apply && request.operation != Operation::Plan;
    let parent_exists = paths::physical(parent)?;
    // A missing configuration hierarchy is normal on a fresh install. Preview
    // never creates it, and applies validate TOML before creating any lock.
    let mut root = if parent_exists {
        Some(Root::open(parent, Spec::integration_setup())?)
    } else {
        None
    };
    let mut config_image = match &root {
        Some(root) => root.read(CONFIG, CONFIG_LIMIT)?,
        None => None,
    };
    let mut receipt_image = match &root {
        Some(root) => {
            root.recovery_absent()?;
            // Inspect an existing lock's privacy without creating or locking it.
            root.read(LOCK, JOURNAL_LIMIT)?;
            root.read(RECEIPT, JOURNAL_LIMIT)?
        }
        None => None,
    };
    let mut receipt: Option<Receipt> = receipt_image
        .as_ref()
        .map(|i| {
            serde_json::from_slice(&i.bytes).map_err(|_| Error("integration_setup_invalid_receipt"))
        })
        .transpose()?;
    if let Some(receipt) = &receipt {
        receipt.validate(&config)?;
    }
    if let Some(receipt) = &receipt {
        let valid = match receipt.stage {
            Stage::Active => receipt.post_matches(&config_image),
            Stage::Inactive => receipt.pre_matches(&config_image),
            Stage::PendingEnable | Stage::PendingDisable => {
                receipt.pre_matches(&config_image) || receipt.post_matches(&config_image)
            }
        };
        require(valid, "integration_setup_config_edited")?;
        require(
            request
                .executable
                .as_ref()
                .is_none_or(|path| path == &receipt.executable)
                && request
                    .state_root
                    .as_ref()
                    .is_none_or(|path| path == &receipt.state_root),
            "integration_setup_spec_conflict",
        )?;
        require(
            request.operation == Operation::Plan
                || !matches!(
                    (receipt.stage, request.operation),
                    (Stage::PendingEnable, Operation::Disable)
                        | (Stage::PendingDisable, Operation::Enable)
                ),
            "integration_setup_pending_operation_required",
        )?;
    }
    // Ownership precedes selection: omitted pins describe the managed install,
    // not the invoking binary or today's runtime defaults. Undo never uses them.
    let (executable, state_root) = if let Some(receipt) = &receipt {
        (
            Some(receipt.executable.clone()),
            Some(receipt.state_root.clone()),
        )
    } else if request.operation == Operation::Disable {
        for path in [&request.executable, &request.state_root]
            .into_iter()
            .flatten()
        {
            paths::absolute(path)?;
        }
        (None, None)
    } else {
        (
            Some(
                request
                    .executable
                    .clone()
                    .map_or_else(std::env::current_exe, Ok)?,
            ),
            Some(
                request
                    .state_root
                    .clone()
                    .map(Ok)
                    .unwrap_or_else(crate::integration_runtime::default_state_root)?,
            ),
        )
    };
    let mut runtime_error = None;
    if request.operation != Operation::Disable {
        let readiness = paths::executable(executable.as_deref().unwrap())
            .and_then(|_| paths::state_root(state_root.as_deref().unwrap()));
        if request.operation == Operation::Plan
            && receipt.as_ref().is_some_and(|r| r.stage == Stage::Active)
        {
            runtime_error = readiness.err();
        } else {
            readiness?;
        }
    }
    let mut next = None;
    if receipt.is_none() && request.operation != Operation::Disable {
        next = Some(generated(
            config_image.as_ref().map_or(&[][..], |i| &i.bytes),
            executable.as_deref().unwrap(),
            state_root.as_deref().unwrap(),
        )?);
    }
    let noop = matches!(
        (request.operation, receipt.as_ref().map(|r| r.stage)),
        (Operation::Disable, None | Some(Stage::Inactive))
            | (Operation::Plan | Operation::Enable, Some(Stage::Active))
    );
    if apply && !noop {
        require(
            crate::hook_files::supports_apply(),
            "integration_setup_apply_unsupported_platform",
        )?;
        if root.is_none() {
            crate::integration_runtime::ensure_private_directory(parent)?;
            root = Some(Root::open(parent, Spec::integration_setup())?);
        }
        let root = root.as_ref().unwrap();
        let _lock = root.lock(LOCK)?;
        root.recovery_absent()?;
        root.fence(CONFIG, &config_image)?;
        root.fence(RECEIPT, &receipt_image)?;
        let receipt = receipt.get_or_insert_with(|| {
            let postimage = next.take().unwrap();
            Receipt {
                version: 1,
                config: config.clone(),
                executable: executable.clone().unwrap(),
                state_root: state_root.clone().unwrap(),
                preimage: config_image.as_ref().map(|i| Preimage {
                    bytes: i.bytes.clone(),
                    mode: i.mode & 0o7777,
                }),
                post_digest: digest(&postimage),
                postimage,
                stage: Stage::PendingEnable,
            }
        });
        let enabling = request.operation == Operation::Enable;
        receipt.stage = if enabling {
            Stage::PendingEnable
        } else {
            Stage::PendingDisable
        };
        // Commit intent durably first. Failure never triggers a blind rollback.
        journal(root, &mut receipt_image, receipt)?;
        #[cfg(test)]
        tests::checkpoint("intent")?;
        let already_target = if enabling {
            receipt.post_matches(&config_image)
        } else {
            receipt.pre_matches(&config_image)
        };
        if !already_target {
            let bytes = if enabling {
                Some(receipt.postimage.as_slice())
            } else {
                receipt.preimage.as_ref().map(|p| p.bytes.as_slice())
            };
            root.fence(RECEIPT, &receipt_image)?;
            root.replace(
                CONFIG,
                &config_image,
                bytes,
                receipt.preimage.as_ref().map_or(0o600, |p| p.mode),
            )?;
            config_image = root.read(CONFIG, CONFIG_LIMIT)?;
        }
        require(
            if enabling {
                receipt.post_matches(&config_image)
            } else {
                receipt.pre_matches(&config_image)
            },
            "integration_setup_conflict",
        )?;
        root.fence(CONFIG, &config_image)?;
        #[cfg(test)]
        tests::checkpoint("config")?;
        receipt.stage = if enabling {
            Stage::Active
        } else {
            Stage::Inactive
        };
        journal(root, &mut receipt_image, receipt)?;
    }
    let pins = executable.as_ref().zip(state_root.as_ref());
    let runtime_command =
        pins.map(|(exe, root)| json!([exe, "integration", "runtime", "--state-root", root]));
    let report_command = pins.map(|(exe, root)| {
        json!([
            exe,
            "agent",
            "report-native",
            "--from-kitsune",
            "--state-root",
            root
        ])
    });
    let development = executable
        .as_ref()
        .is_some_and(|exe| exe.components().any(|c| c.as_os_str() == "target"));
    let mut warnings = if development {
        vec!["Selected executable is a development build path; rebuilding or removing it affects future Kit starts.".to_owned()]
    } else {
        Vec::new()
    };
    if let Some(error) = &runtime_error {
        warnings.push(format!("Recorded runtime is not ready: {}. Disable remains available without runtime readiness.", error.code()));
    }
    let report = json!({"schema_version":1,"operation":format!("{:?}", request.operation).to_lowercase(),"applied":apply && !noop,"noop":noop,"kitsune_config":config,"executable":executable,"state_root":state_root,"receipt":parent.join(RECEIPT),"stage":receipt.as_ref().map(|r| r.stage),"command":runtime_command,"report_command":report_command,"runtime_ready":(request.operation != Operation::Disable).then_some(runtime_error.is_none()),"runtime_readiness_error":runtime_error.as_ref().map(Error::code),"source":"coven","external_agent_management":true,"development_binary_warning":development,"warnings":warnings,"kit_restart_required":true,"new_panes_required":true,"provider_reload_required":true,"provider_setup_required":true,"other_providers_setup_required":true,"automatic_provider_install":false,"live_api_used":false});
    let stdout = if json_output {
        format!("{}\n", serde_json::to_string_pretty(&report)?)
    } else {
        format!(
            "Integration {:?}: {}\nKit config: {}\nExecutable: {}\nState root: {}\nRuntime argv: {}\nReporter argv: {}\nRestart Kit, open new panes, and reload providers. Provider hooks/extensions and other providers require separate explicit setup; no live API or restart was performed.\n",
            request.operation,
            if noop {
                "already in requested state"
            } else if apply {
                "applied"
            } else {
                "preview (no changes)"
            },
            config.display(),
            executable
                .as_ref()
                .map_or_else(|| "(not managed)".to_owned(), |p| p.display().to_string()),
            state_root
                .as_ref()
                .map_or_else(|| "(not managed)".to_owned(), |p| p.display().to_string()),
            serde_json::to_string(&runtime_command)?,
            serde_json::to_string(&report_command)?
        )
    };
    Ok(TaskOutput {
        stdout,
        warnings,
        exit_code: 0,
    })
}
