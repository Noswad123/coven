use super::{Operation, Request, policy::*};
use crate::{
    Error, Result,
    error::require,
    hook_files::{Image, Lock, Root, Spec},
    task_mutation::TaskOutput,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

const LEAVES: [&str; 2] = ["hooks.json", "config.toml"];
const RECEIPT: &str = "coven-codex-hooks.json";
const LOCK: &str = ".coven-codex-hooks.lock";
const JOURNAL_LIMIT: usize = LIMIT * 8;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    text: String,
    mode: u32,
    digest: String,
}
type Pair = [Option<Stored>; 2];
impl Stored {
    fn new(bytes: &[u8], mode: u32) -> Result<Self> {
        Ok(Self {
            text: std::str::from_utf8(bytes)
                .map_err(|_| Error("codex_hooks_invalid_receipt"))?
                .into(),
            mode,
            digest: digest(bytes),
        })
    }
    fn validate(&self) -> Result<()> {
        require(
            self.text.len() <= LIMIT
                && self.mode <= 0o777
                && self.mode & 0o022 == 0
                && self.digest == digest(self.text.as_bytes()),
            "codex_hooks_invalid_receipt",
        )
    }
}
fn stored(images: &[Option<Image>; 2]) -> Result<Pair> {
    Ok([
        images[0]
            .as_ref()
            .map(|i| Stored::new(&i.bytes, i.mode & 0o7777))
            .transpose()?,
        images[1]
            .as_ref()
            .map(|i| Stored::new(&i.bytes, i.mode & 0o7777))
            .transpose()?,
    ])
}
fn raw(pair: &Pair, index: usize) -> Option<&[u8]> {
    pair[index].as_ref().map(|s| s.text.as_bytes())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Op {
    Install,
    Uninstall,
    Rollback,
}
impl Op {
    fn name(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Uninstall => "uninstall",
            Self::Rollback => "rollback",
        }
    }
    fn done(self) -> &'static str {
        match self {
            Self::Install => "installed",
            Self::Uninstall => "uninstalled",
            Self::Rollback => "rolled_back",
        }
    }
}

// Array positions, never deserialized paths, name the two fixed leaves. Targets
// are reconstructed from provider policy before any journal is accepted.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: String,
    version: u32,
    executable: String,
    socket: String,
    migrated: bool,
    original: Pair,
    installed: Pair,
    operation: Op,
    pending: bool,
    source: Pair,
    target: Pair,
}
fn installed(
    original: &Pair,
    request: &Request,
    exe: &str,
    socket: &str,
    consent: bool,
) -> Result<(Pair, bool)> {
    let (hooks, a) = install_json(raw(original, 0), &request.config_root, exe, socket, consent)?;
    let (config, b) = install_toml(raw(original, 1), &request.config_root, consent)?;
    Ok((
        [
            Some(Stored::new(&hooks, 0o600)?),
            Some(Stored::new(&config, 0o600)?),
        ],
        a || b,
    ))
}
fn uninstalled(receipt: &Receipt) -> Result<Pair> {
    let hooks = uninstall_json(
        raw(&receipt.installed, 0).ok_or(Error("codex_hooks_invalid_receipt"))?,
        &receipt.executable,
        &receipt.socket,
    )?;
    Ok([
        Some(Stored::new(&hooks, 0o600)?),
        receipt.installed[1].clone(),
    ])
}
impl Receipt {
    fn validate(&self, request: &Request) -> Result<()> {
        require(
            self.schema == "coven-codex-hooks" && self.version == 1,
            "codex_hooks_invalid_receipt",
        )?;
        absolute(Path::new(&self.executable))?;
        absolute(Path::new(&self.socket))?;
        for pair in [&self.original, &self.installed, &self.source, &self.target] {
            for leaf in pair.iter().flatten() {
                leaf.validate()?;
            }
        }
        let (rebuilt, migrated) = installed(
            &self.original,
            request,
            &self.executable,
            &self.socket,
            self.migrated,
        )?;
        require(
            rebuilt == self.installed && migrated == self.migrated,
            "codex_hooks_invalid_receipt",
        )?;
        let uninstall = uninstalled(self)?;
        let valid = match self.operation {
            Op::Install => self.source == self.original && self.target == self.installed,
            Op::Uninstall => self.source == self.installed && self.target == uninstall,
            Op::Rollback => {
                (self.source == self.installed || self.source == uninstall)
                    && self.target == self.original
            }
        };
        require(valid, "codex_hooks_invalid_receipt")
    }
}
struct Prepared {
    images: [Option<Image>; 2],
    kit_image: Option<Image>,
    receipt_image: Option<Image>,
    receipt: Receipt,
    status: &'static str,
    unchanged: bool,
    missing_arguments: bool,
}
fn verify_kit(root: &Root) -> Result<Image> {
    let asset = root
        .read_relative("kitsune-agent-state.sh", LIMIT)?
        .ok_or(Error("codex_hooks_ambiguous_kitsune"))?;
    require(
        digest(&asset.bytes) == KIT_SHA,
        "codex_hooks_ambiguous_kitsune",
    )?;
    Ok(asset)
}
fn prefix(current: &Pair, source: &Pair, target: &Pair) -> bool {
    (0..=2)
        .any(|split| (0..2).all(|i| current[i] == *if i < split { &target[i] } else { &source[i] }))
}
fn prepare(root: &Root, request: &Request) -> Result<Prepared> {
    root.recovery_absent()?;
    let images = [root.read(LEAVES[0], LIMIT)?, root.read(LEAVES[1], LIMIT)?];
    for image in images.iter().flatten() {
        require(image.mode & 0o7000 == 0, "codex_hooks_unsafe_leaf")?;
    }
    if let Some(image) = &images[0] {
        parse_json(&image.bytes)?;
    }
    if let Some(image) = &images[1] {
        parse_toml(&image.bytes)?;
    }
    let current = stored(&images)?;
    let receipt_image = root.read(RECEIPT, JOURNAL_LIMIT)?;
    let op = match request.operation {
        Operation::Plan | Operation::Install => Op::Install,
        Operation::Uninstall => Op::Uninstall,
        Operation::Rollback => Op::Rollback,
    };
    let mut prior = if let Some(image) = &receipt_image {
        let value = crate::strict_json::decode(&image.bytes)
            .map_err(|_| Error("codex_hooks_invalid_receipt"))?;
        let r: Receipt =
            serde_json::from_value(value).map_err(|_| Error("codex_hooks_invalid_receipt"))?;
        r.validate(request)?;
        if let Some(exe) = &request.executable {
            require(*exe == r.executable, "codex_hooks_conflict")?;
        }
        if let Some(socket) = &request.socket {
            require(*socket == r.socket, "codex_hooks_conflict")?;
        }
        Some(r)
    } else {
        None
    };
    if let Some(r) = &prior {
        if r.pending {
            require(
                request.operation == Operation::Plan || op == r.operation,
                "codex_hooks_receipt_stage_conflict",
            )?;
            require(
                prefix(&current, &r.source, &r.target),
                "codex_hooks_conflict",
            )?;
            let mut kit_image = None;
            if r.operation == Op::Install && r.migrated && current != r.target {
                // Committed leaves no longer depend on the old script. Only
                // source leaves still containing verified Kit commands need it.
                let hooks_need_asset = current[0] != r.target[0]
                    && install_json(
                        raw(&r.original, 0),
                        &request.config_root,
                        &r.executable,
                        &r.socket,
                        true,
                    )?
                    .1;
                let config_needs_asset = current[1] != r.target[1]
                    && install_toml(raw(&r.original, 1), &request.config_root, true)?.1;
                if hooks_need_asset || config_needs_asset {
                    kit_image = Some(verify_kit(root)?);
                }
            }
            return Ok(Prepared {
                images,
                kit_image,
                receipt_image,
                receipt: r.clone(),
                status: "recovery_required",
                unchanged: false,
                missing_arguments: false,
            });
        }
        require(current == r.target, "codex_hooks_conflict")?;
        if op == r.operation {
            return Ok(Prepared {
                images,
                kit_image: None,
                receipt_image,
                receipt: r.clone(),
                status: "unchanged",
                unchanged: true,
                missing_arguments: false,
            });
        }
    }
    let missing_arguments = request.executable.is_none() || request.socket.is_none();
    let mut kit_image = None;
    let mut receipt = if prior.is_none() || op == Op::Install {
        require(op == Op::Install, "codex_hooks_receipt_required")?;
        let exe = request
            .executable
            .as_deref()
            .or_else(|| prior.as_ref().map(|r| r.executable.as_str()));
        let socket = request
            .socket
            .as_deref()
            .or_else(|| prior.as_ref().map(|r| r.socket.as_str()));
        require(
            request.operation == Operation::Plan || exe.is_some(),
            "codex_hooks_executable_required",
        )?;
        require(
            request.operation == Operation::Plan || socket.is_some(),
            "codex_hooks_socket_required",
        )?;
        let exe = exe.unwrap_or("/unconfigured-coven");
        let socket = socket.unwrap_or("/unconfigured-receiver");
        let (target, migrated) =
            installed(&current, request, exe, socket, request.migrate_kitsune)?;
        if migrated {
            kit_image = Some(verify_kit(root)?);
        }
        Receipt {
            schema: "coven-codex-hooks".into(),
            version: 1,
            executable: exe.into(),
            socket: socket.into(),
            migrated,
            original: current.clone(),
            installed: target.clone(),
            operation: op,
            pending: false,
            source: current.clone(),
            target,
        }
    } else {
        let r = prior.take().unwrap();
        require(
            r.operation == Op::Install || (r.operation == Op::Uninstall && op == Op::Rollback),
            "codex_hooks_receipt_stage_conflict",
        )?;
        r
    };
    receipt.operation = op;
    receipt.source = current;
    receipt.target = match op {
        Op::Install => receipt.installed.clone(),
        Op::Uninstall => uninstalled(&receipt)?,
        Op::Rollback => receipt.original.clone(),
    };
    Ok(Prepared {
        images,
        kit_image,
        receipt_image,
        receipt,
        status: "planned",
        unchanged: false,
        missing_arguments: missing_arguments && request.operation == Operation::Plan,
    })
}

pub(super) fn run(request: &Request, json_output: bool) -> Result<TaskOutput> {
    run_inner(request, json_output, &mut |_| Ok(()))
}
fn run_inner(
    request: &Request,
    json_output: bool,
    step: &mut dyn FnMut(usize) -> Result<()>,
) -> Result<TaskOutput> {
    let root = Root::open(&request.config_root, Spec::codex())?;
    for path in [&request.executable, &request.socket].into_iter().flatten() {
        absolute(Path::new(path))?;
    }
    let writing = request.apply && request.operation != Operation::Plan;
    require(
        !writing || crate::hook_files::supports_apply(),
        "codex_hooks_apply_unsupported_platform",
    )?;
    // Invalid second files, receipts and preimages must not even create a lock.
    let mut prepared = prepare(&root, request)?;
    journal_bytes(&prepared.receipt)?;
    let _lock: Option<Lock> = if writing && !prepared.unchanged {
        let lock = root.lock(LOCK)?;
        prepared = prepare(&root, request)?;
        journal_bytes(&prepared.receipt)?;
        Some(lock)
    } else {
        None
    };
    let mut status = prepared.status;
    if writing && !prepared.unchanged {
        for (name, image) in LEAVES.iter().zip(&prepared.images) {
            root.fence(name, image)?;
        }
        root.fence(RECEIPT, &prepared.receipt_image)?;
        if prepared.kit_image.is_some() {
            root.fence("kitsune-agent-state.sh", &prepared.kit_image)?;
        }
        prepared.receipt.pending = true;
        let bytes = journal_bytes(&prepared.receipt)?;
        root.replace(RECEIPT, &prepared.receipt_image, Some(&bytes), 0o600)?;
        let pending = root.read(RECEIPT, JOURNAL_LIMIT)?;
        require(
            pending
                .as_ref()
                .is_some_and(|i| i.bytes == bytes && i.mode & 0o7777 == 0o600),
            "codex_hooks_pending_recovery_required",
        )?;
        step(0)?;
        for (i, name) in LEAVES.iter().enumerate() {
            // Recheck the entire observed prefix after journaling and between
            // leaves. This detects cross-leaf edits without claiming global CAS.
            for (leaf, image) in LEAVES.iter().zip(&prepared.images) {
                root.fence(leaf, image)?;
            }
            root.fence(RECEIPT, &pending)?;
            let current = prepared.images[i]
                .as_ref()
                .map(|v| Stored::new(&v.bytes, v.mode & 0o7777))
                .transpose()?;
            let target = &prepared.receipt.target[i];
            if current != *target {
                root.replace(
                    name,
                    &prepared.images[i],
                    target.as_ref().map(|v| v.text.as_bytes()),
                    target.as_ref().map_or(0o600, |v| v.mode),
                )?;
            } else {
                root.fence(name, &prepared.images[i])?;
            }
            // Pin each committed result, then check all results again before finalizing.
            prepared.images[i] = root.read(name, LIMIT)?;
            require(
                stored(&prepared.images)?[i] == *target,
                "codex_hooks_pending_recovery_required",
            )?;
            step(i + 1)?;
        }
        for (name, image) in LEAVES.iter().zip(&prepared.images) {
            root.fence(name, image)?;
        }
        require(
            stored(&prepared.images)? == prepared.receipt.target,
            "codex_hooks_pending_recovery_required",
        )?;
        prepared.receipt.pending = false;
        root.replace(
            RECEIPT,
            &pending,
            Some(&journal_bytes(&prepared.receipt)?),
            0o600,
        )?;
        status = prepared.receipt.operation.done();
    }
    if prepared.missing_arguments {
        status = "planned_requires_executable_and_socket";
    }
    let r = &prepared.receipt;
    let value = json!({"provider":"codex", "status":status, "events":["SessionStart"],
        "files":LEAVES, "operation":r.operation.name(), "pending":r.pending,
        "before_digests":r.source.each_ref().map(|s| s.as_ref().map(|v| &v.digest)),
        "after_digests":r.target.each_ref().map(|s| s.as_ref().map(|v| &v.digest)),
        "features":feature_change(raw(&r.source, 1), raw(&r.target, 1))?, "migrate_kitsune":r.migrated,
        "native_hook_approval":"user-managed; may be required; not granted by this operation",
        "native_contract":"openai/codex rust-v0.167.1 0a94398fa60c4fac0e7659e4213a0a36e613dd46"});
    Ok(TaskOutput {
        stdout: if json_output {
            serde_json::to_string(&value)?
        } else {
            format!(
                "Codex hooks: {status}; global features.hooks: {} -> {}; legacy alias removal: {}; native hook approval is user-managed, may be required, and is not granted",
                value["features"]["hooks_before"],
                value["features"]["hooks_after"],
                value["features"]["remove_legacy_alias"]
            )
        },
        warnings: vec![],
        exit_code: 0,
    })
}
fn journal_bytes(receipt: &Receipt) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(receipt)?;
    require(bytes.len() <= JOURNAL_LIMIT, "codex_hooks_size_limit")?;
    Ok(bytes)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
