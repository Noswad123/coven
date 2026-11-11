use super::{NATIVE_PIN, Operation, Request, asset};
use crate::{
    Error, Result,
    error::require,
    hook_files::{Image, Root, Spec},
    task_mutation::TaskOutput,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

const LEAVES: [&str; 2] = ["coven-agent-reference.ts", "kitsune-agent-state.ts"];
const RECEIPT: &str = "coven-pi-extension.json";
const LOCK: &str = ".coven-pi-extension.lock";
const LIMIT: usize = 1024 * 1024;
const JOURNAL_LIMIT: usize = LIMIT * 8;
// Kit fabc9f265e3a8faec86b17a0102e4dd649768902, integration v7; inert bytes only.
const KIT: &[u8] = include_bytes!("../../tests/fixtures/pi_extension/kitsune-agent-state.ts");
const KIT_SHA: &str = "cce258cccccd2fdf66549cc89f1eee72b3293f558a7da96b8b87fe9071b3f247";

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn absolute(path: &Path) -> Result<()> {
    require(
        path.is_absolute()
            && path
                .components()
                .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
            && path
                .to_str()
                .is_some_and(|s| !s.chars().any(char::is_control)),
        "pi_extension_absolute_path_required",
    )
}

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
        require(bytes.len() <= LIMIT, "pi_extension_size_limit")?;
        Ok(Self {
            text: std::str::from_utf8(bytes)
                .map_err(|_| Error("pi_extension_invalid_utf8"))?
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
            "pi_extension_invalid_receipt",
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
    fn order(self) -> [usize; 2] {
        if self == Self::Rollback {
            [1, 0]
        } else {
            [0, 1]
        }
    }
}

// Positions name fixed leaves. No receipt-controlled path is ever opened.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: String,
    version: u32,
    native_pin: String,
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
fn installed(exe: &str, socket: &str) -> Result<Pair> {
    Ok([
        Some(Stored::new(&asset::generate(exe, socket)?, 0o600)?),
        None,
    ])
}
impl Receipt {
    fn validate(&self) -> Result<()> {
        require(
            self.schema == "coven-pi-extension"
                && self.version == 1
                && self.native_pin == NATIVE_PIN,
            "pi_extension_invalid_receipt",
        )?;
        absolute(Path::new(&self.executable))?;
        absolute(Path::new(&self.socket))?;
        for pair in [&self.original, &self.installed, &self.source, &self.target] {
            for leaf in pair.iter().flatten() {
                leaf.validate()?;
            }
        }
        require(
            self.original[0].is_none() && self.migrated == self.original[1].is_some(),
            "pi_extension_invalid_receipt",
        )?;
        if let Some(old) = &self.original[1] {
            require(
                old.text.as_bytes() == KIT && digest(KIT) == KIT_SHA,
                "pi_extension_invalid_receipt",
            )?;
        }
        let rebuilt = installed(&self.executable, &self.socket)?;
        require(self.installed == rebuilt, "pi_extension_invalid_receipt")?;
        let absent = [None, None];
        let valid = match self.operation {
            Op::Install => {
                (self.source == self.original || self.source == absent) && self.target == rebuilt
            }
            Op::Uninstall => self.source == rebuilt && self.target == absent,
            Op::Rollback => {
                (self.source == rebuilt || self.source == absent) && self.target == self.original
            }
        };
        require(valid, "pi_extension_invalid_receipt")
    }
}
fn prefix(current: &Pair, receipt: &Receipt) -> bool {
    let order = receipt.operation.order();
    (0..=2).any(|split| {
        order.iter().enumerate().all(|(position, &i)| {
            &current[i]
                == if position < split {
                    &receipt.target[i]
                } else {
                    &receipt.source[i]
                }
        })
    })
}
struct Prepared {
    images: [Option<Image>; 2],
    receipt_image: Option<Image>,
    receipt: Receipt,
    status: &'static str,
    unchanged: bool,
    missing_arguments: bool,
}
fn prepare(root: &Root, request: &Request) -> Result<Prepared> {
    root.recovery_absent()?;
    let images = [root.read(LEAVES[0], LIMIT)?, root.read(LEAVES[1], LIMIT)?];
    for i in images.iter().flatten() {
        require(i.mode & 0o7000 == 0, "pi_extension_unsafe_leaf")?;
    }
    let current = stored(&images)?;
    let receipt_image = root.read(RECEIPT, JOURNAL_LIMIT)?;
    let op = match request.operation {
        Operation::Plan | Operation::Install => Op::Install,
        Operation::Uninstall => Op::Uninstall,
        Operation::Rollback => Op::Rollback,
    };
    let prior = if let Some(image) = &receipt_image {
        let value = crate::strict_json::decode(&image.bytes)
            .map_err(|_| Error("pi_extension_invalid_receipt"))?;
        let r: Receipt =
            serde_json::from_value(value).map_err(|_| Error("pi_extension_invalid_receipt"))?;
        r.validate()?;
        if let Some(exe) = &request.executable {
            require(*exe == r.executable, "pi_extension_conflict")?;
        }
        if let Some(socket) = &request.socket {
            require(*socket == r.socket, "pi_extension_conflict")?;
        }
        Some(r)
    } else {
        None
    };
    if let Some(r) = &prior {
        if r.pending {
            require(
                request.operation == Operation::Plan || op == r.operation,
                "pi_extension_receipt_stage_conflict",
            )?;
            require(prefix(&current, r), "pi_extension_conflict")?;
            return Ok(Prepared {
                images,
                receipt_image,
                receipt: r.clone(),
                status: "recovery_required",
                unchanged: false,
                missing_arguments: false,
            });
        }
        require(current == r.target, "pi_extension_conflict")?;
        if op == r.operation {
            return Ok(Prepared {
                images,
                receipt_image,
                receipt: r.clone(),
                status: "unchanged",
                unchanged: true,
                missing_arguments: false,
            });
        }
    }
    let missing_arguments =
        prior.is_none() && (request.executable.is_none() || request.socket.is_none());
    // Reinstall changes the transition source, never the first original images.
    let mut receipt = if let Some(r) = prior {
        require(
            op == Op::Install
                || r.operation == Op::Install
                || (r.operation == Op::Uninstall && op == Op::Rollback),
            "pi_extension_receipt_stage_conflict",
        )?;
        r
    } else {
        require(op == Op::Install, "pi_extension_receipt_required")?;
        require(current[0].is_none(), "pi_extension_unreceipted_ownership")?;
        if let Some(old) = &current[1] {
            require(
                old.text.as_bytes() == KIT && digest(KIT) == KIT_SHA,
                "pi_extension_ambiguous_kitsune",
            )?;
            require(
                request.migrate_kitsune,
                "pi_extension_migration_consent_required",
            )?;
        }
        require(
            request.operation == Operation::Plan || request.executable.is_some(),
            "pi_extension_executable_required",
        )?;
        require(
            request.operation == Operation::Plan || request.socket.is_some(),
            "pi_extension_socket_required",
        )?;
        let exe = request
            .executable
            .as_deref()
            .unwrap_or("/unconfigured-coven");
        let socket = request
            .socket
            .as_deref()
            .unwrap_or("/unconfigured-receiver");
        Receipt {
            schema: "coven-pi-extension".into(),
            version: 1,
            native_pin: NATIVE_PIN.into(),
            executable: exe.into(),
            socket: socket.into(),
            migrated: current[1].is_some(),
            original: current.clone(),
            installed: installed(exe, socket)?,
            operation: op,
            pending: false,
            source: current.clone(),
            target: [None, None],
        }
    };
    if op == Op::Install && current[1].is_some() {
        require(
            request.migrate_kitsune,
            "pi_extension_migration_consent_required",
        )?;
    }
    receipt.operation = op;
    receipt.source = current;
    receipt.target = match op {
        Op::Install => receipt.installed.clone(),
        Op::Uninstall => [None, None],
        Op::Rollback => receipt.original.clone(),
    };
    receipt.validate()?;
    Ok(Prepared {
        images,
        receipt_image,
        receipt,
        status: "planned",
        unchanged: false,
        missing_arguments,
    })
}
fn journal_bytes(receipt: &Receipt) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(receipt)?;
    require(bytes.len() <= JOURNAL_LIMIT, "pi_extension_size_limit")?;
    Ok(bytes)
}
fn fence(root: &Root, images: &[Option<Image>; 2], receipt: &Option<Image>) -> Result<()> {
    for (name, image) in LEAVES.iter().zip(images) {
        root.fence(name, image)?;
    }
    root.fence(RECEIPT, receipt)
}
pub(super) fn run(request: &Request, json_output: bool) -> Result<TaskOutput> {
    run_inner(request, json_output, &mut |_| Ok(()))
}
fn run_inner(
    request: &Request,
    json_output: bool,
    step: &mut dyn FnMut(usize) -> Result<()>,
) -> Result<TaskOutput> {
    let root = Root::open(&request.extension_root, Spec::pi())?;
    for path in [&request.executable, &request.socket].into_iter().flatten() {
        absolute(Path::new(path))?;
    }
    let writing = request.apply && request.operation != Operation::Plan;
    require(
        !writing || cfg!(target_os = "linux"),
        "pi_extension_apply_unsupported_platform",
    )?;
    // Validate both leaves and every reconstructed journal image before artifacts.
    let mut p = prepare(&root, request)?;
    journal_bytes(&p.receipt)?;
    let _lock = if writing && !p.unchanged {
        let lock = root.lock(LOCK)?;
        p = prepare(&root, request)?;
        journal_bytes(&p.receipt)?;
        Some(lock)
    } else {
        None
    };
    let mut status = p.status;
    if writing && !p.unchanged {
        fence(&root, &p.images, &p.receipt_image)?;
        p.receipt.pending = true;
        let bytes = journal_bytes(&p.receipt)?;
        root.replace(RECEIPT, &p.receipt_image, Some(&bytes), 0o600)?;
        let pending = root.read(RECEIPT, JOURNAL_LIMIT)?;
        require(
            pending
                .as_ref()
                .is_some_and(|i| i.bytes == bytes && i.mode & 0o7777 == 0o600),
            "pi_extension_pending_recovery_required",
        )?;
        step(0)?;
        for (position, i) in p.receipt.operation.order().into_iter().enumerate() {
            fence(&root, &p.images, &pending)?;
            let target = &p.receipt.target[i];
            if stored(&p.images)?[i] != *target {
                root.replace(
                    LEAVES[i],
                    &p.images[i],
                    target.as_ref().map(|s| s.text.as_bytes()),
                    target.as_ref().map_or(0o600, |s| s.mode),
                )?;
            }
            p.images[i] = root.read(LEAVES[i], LIMIT)?;
            require(
                stored(&p.images)?[i] == *target,
                "pi_extension_pending_recovery_required",
            )?;
            step(position + 1)?;
        }
        fence(&root, &p.images, &pending)?;
        require(
            stored(&p.images)? == p.receipt.target,
            "pi_extension_pending_recovery_required",
        )?;
        p.receipt.pending = false;
        let bytes = journal_bytes(&p.receipt)?;
        root.replace(RECEIPT, &pending, Some(&bytes), 0o600)?;
        let done = root.read(RECEIPT, JOURNAL_LIMIT)?;
        require(
            done.as_ref().is_some_and(|i| i.bytes == bytes),
            "pi_extension_pending_recovery_required",
        )?;
        step(3)?;
        fence(&root, &p.images, &done)?;
        status = p.receipt.operation.done();
    }
    if p.missing_arguments {
        status = "planned_requires_executable_and_socket";
    }
    let r = &p.receipt;
    let warning = "Migration drops Kit working/blocked/idle reporting; Coven reports session references only.";
    let placement =
        "File placement is not proof of native activation; no approvals or settings are changed.";
    let value = json!({"provider":"pi", "status":status, "files":LEAVES, "operation":r.operation.name(),
        "pending":r.pending, "migrate_kitsune":r.migrated, "native_contract":NATIVE_PIN,
        "before_digests":r.source.each_ref().map(|s| s.as_ref().map(|v| &v.digest)),
        "after_digests":r.target.each_ref().map(|s| s.as_ref().map(|v| &v.digest)),
        "activation":placement, "migration_warning":if r.migrated { Some(warning) } else { None }});
    Ok(TaskOutput {
        stdout: if json_output {
            serde_json::to_string(&value)?
        } else {
            format!(
                "Pi extension: {status}; {placement}{}",
                if r.migrated {
                    format!(" {warning}")
                } else {
                    String::new()
                }
            )
        },
        warnings: if r.migrated {
            vec![warning.into()]
        } else {
            vec![]
        },
        exit_code: 0,
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
