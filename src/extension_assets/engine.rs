use super::{Policy, Request};
use crate::claude_hooks::Operation;
use crate::{
    Error, Result,
    error::require,
    hook_files::{Image, Root},
    task_mutation::TaskOutput,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

const LIMIT: usize = 1024 * 1024;
const JOURNAL_LIMIT: usize = LIMIT * 8;

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn absolute(policy: &Policy, path: &Path) -> Result<()> {
    require(
        path.is_absolute()
            && path
                .components()
                .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
            && path
                .to_str()
                .is_some_and(|s| !s.chars().any(char::is_control)),
        policy.errors.absolute_path_required,
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
    fn new(policy: &Policy, bytes: &[u8], mode: u32) -> Result<Self> {
        require(bytes.len() <= LIMIT, policy.errors.size_limit)?;
        Ok(Self {
            text: std::str::from_utf8(bytes)
                .map_err(|_| Error(policy.errors.invalid_utf8))?
                .into(),
            mode,
            digest: digest(bytes),
        })
    }
    fn validate(&self, policy: &Policy) -> Result<()> {
        require(
            self.text.len() <= LIMIT
                && self.mode <= 0o777
                && self.mode & 0o022 == 0
                && self.digest == digest(self.text.as_bytes()),
            policy.errors.invalid_receipt,
        )
    }
}
fn stored(policy: &Policy, images: &[Option<Image>; 2]) -> Result<Pair> {
    Ok([
        images[0]
            .as_ref()
            .map(|i| Stored::new(policy, &i.bytes, i.mode & 0o7777))
            .transpose()?,
        images[1]
            .as_ref()
            .map(|i| Stored::new(policy, &i.bytes, i.mode & 0o7777))
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    route: Option<crate::integration_route::Route>,
    migrated: bool,
    original: Pair,
    installed: Pair,
    operation: Op,
    pending: bool,
    source: Pair,
    target: Pair,
}
fn installed(policy: &Policy, exe: &str, socket: &str) -> Result<Pair> {
    Ok([
        Some(Stored::new(
            policy,
            &(policy.generate)(exe, socket)?,
            0o600,
        )?),
        None,
    ])
}
impl Receipt {
    fn validate(&self, policy: &Policy) -> Result<()> {
        require(
            self.schema == policy.schema
                && matches!(self.version, 1 | 2)
                && self.native_pin == policy.native_pin,
            policy.errors.invalid_receipt,
        )?;
        absolute(policy, Path::new(&self.executable))?;
        crate::integration_route::validate(&self.socket, self.version, 1, &self.route)
            .map_err(|_| Error(policy.errors.invalid_receipt))?;
        for pair in [&self.original, &self.installed, &self.source, &self.target] {
            for leaf in pair.iter().flatten() {
                leaf.validate(policy)?;
            }
        }
        require(
            self.original[0].is_none() && self.migrated == self.original[1].is_some(),
            policy.errors.invalid_receipt,
        )?;
        if let Some(old) = &self.original[1] {
            require(
                old.text.as_bytes() == policy.kit && digest(policy.kit) == policy.kit_sha,
                policy.errors.invalid_receipt,
            )?;
        }
        let rebuilt = installed(policy, &self.executable, &self.socket)?;
        require(self.installed == rebuilt, policy.errors.invalid_receipt)?;
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
        require(valid, policy.errors.invalid_receipt)
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
fn prepare(policy: &Policy, root: &Root, request: &Request) -> Result<Prepared> {
    root.recovery_absent()?;
    let images = [
        root.read(policy.leaves[0], LIMIT)?,
        root.read(policy.leaves[1], LIMIT)?,
    ];
    for i in images.iter().flatten() {
        require(i.mode & 0o7000 == 0, policy.errors.unsafe_leaf)?;
    }
    let current = stored(policy, &images)?;
    let receipt_image = root.read(policy.receipt, JOURNAL_LIMIT)?;
    let op = match request.operation {
        Operation::Plan | Operation::Install => Op::Install,
        Operation::Uninstall => Op::Uninstall,
        Operation::Rollback => Op::Rollback,
    };
    let prior = if let Some(image) = &receipt_image {
        let value = crate::strict_json::decode(&image.bytes)
            .map_err(|_| Error(policy.errors.invalid_receipt))?;
        let r: Receipt =
            serde_json::from_value(value).map_err(|_| Error(policy.errors.invalid_receipt))?;
        r.validate(policy)?;
        if let Some(exe) = &request.executable {
            require(*exe == r.executable, policy.errors.conflict)?;
        }
        if let Some(socket) = &request.socket {
            require(
                *socket == r.socket
                    || (!r.pending
                        && op == Op::Install
                        && r.operation != Op::Install
                        && (r.route.is_some()
                            || crate::integration_route::decode(socket)?.is_some())),
                policy.errors.conflict,
            )?;
        }
        Some(r)
    } else {
        None
    };
    if let Some(r) = &prior {
        if r.pending {
            require(
                request.operation == Operation::Plan || op == r.operation,
                policy.errors.receipt_stage_conflict,
            )?;
            require(prefix(&current, r), policy.errors.conflict)?;
            return Ok(Prepared {
                images,
                receipt_image,
                receipt: r.clone(),
                status: "recovery_required",
                unchanged: false,
                missing_arguments: false,
            });
        }
        require(current == r.target, policy.errors.conflict)?;
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
            policy.errors.receipt_stage_conflict,
        )?;
        r
    } else {
        require(op == Op::Install, policy.errors.receipt_required)?;
        require(current[0].is_none(), policy.errors.unreceipted_ownership)?;
        if let Some(old) = &current[1] {
            require(
                old.text.as_bytes() == policy.kit && digest(policy.kit) == policy.kit_sha,
                policy.errors.ambiguous_kitsune,
            )?;
            require(
                request.migrate_kitsune,
                policy.errors.migration_consent_required,
            )?;
        }
        require(
            request.operation == Operation::Plan || request.executable.is_some(),
            policy.errors.executable_required,
        )?;
        require(
            request.operation == Operation::Plan || request.socket.is_some(),
            policy.errors.socket_required,
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
            schema: policy.schema.into(),
            version: 1 + u32::from(crate::integration_route::decode(socket)?.is_some()),
            route: crate::integration_route::decode(socket)?,
            native_pin: policy.native_pin.into(),
            executable: exe.into(),
            socket: socket.into(),
            migrated: current[1].is_some(),
            original: current.clone(),
            installed: installed(policy, exe, socket)?,
            operation: op,
            pending: false,
            source: current.clone(),
            target: [None, None],
        }
    };
    if op == Op::Install {
        if let Some(pin) = &request.socket {
            if *pin != receipt.socket {
                receipt.socket = pin.clone();
                receipt.route = crate::integration_route::decode(pin)?;
                receipt.version = 1 + u32::from(receipt.route.is_some());
                receipt.installed = installed(policy, &receipt.executable, pin)?;
            }
        }
    }
    if op == Op::Install && current[1].is_some() {
        require(
            request.migrate_kitsune,
            policy.errors.migration_consent_required,
        )?;
    }
    receipt.operation = op;
    receipt.source = current;
    receipt.target = match op {
        Op::Install => receipt.installed.clone(),
        Op::Uninstall => [None, None],
        Op::Rollback => receipt.original.clone(),
    };
    receipt.validate(policy)?;
    Ok(Prepared {
        images,
        receipt_image,
        receipt,
        status: "planned",
        unchanged: false,
        missing_arguments,
    })
}
fn journal_bytes(policy: &Policy, receipt: &Receipt) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(receipt)?;
    require(bytes.len() <= JOURNAL_LIMIT, policy.errors.size_limit)?;
    Ok(bytes)
}
fn fence(
    policy: &Policy,
    root: &Root,
    images: &[Option<Image>; 2],
    receipt: &Option<Image>,
) -> Result<()> {
    for (name, image) in policy.leaves.iter().zip(images) {
        root.fence(name, image)?;
    }
    root.fence(policy.receipt, receipt)
}
pub(crate) fn run(
    policy: &'static Policy,
    request: &Request,
    json_output: bool,
) -> Result<TaskOutput> {
    run_inner(policy, request, json_output, &mut |_| Ok(()))
}
fn run_inner(
    policy: &'static Policy,
    request: &Request,
    json_output: bool,
    step: &mut dyn FnMut(usize) -> Result<()>,
) -> Result<TaskOutput> {
    let root = Root::open(&request.extension_root, policy.spec)?;
    if let Some(exe) = &request.executable {
        absolute(policy, Path::new(exe))?;
    }
    if let Some(pin) = &request.socket {
        if crate::integration_route::decode(pin)?.is_none() {
            absolute(policy, Path::new(pin))?;
        }
    }
    let writing = request.apply && request.operation != Operation::Plan;
    require(
        !writing || crate::hook_files::supports_apply(),
        policy.errors.apply_unsupported_platform,
    )?;
    // Validate both leaves and every reconstructed journal image before artifacts.
    let mut p = prepare(policy, &root, request)?;
    journal_bytes(policy, &p.receipt)?;
    let _lock = if writing && !p.unchanged {
        step(4)?;
        let lock = root.lock(policy.lock)?;
        step(5)?;
        p = prepare(policy, &root, request)?;
        journal_bytes(policy, &p.receipt)?;
        Some(lock)
    } else {
        None
    };
    let mut status = p.status;
    if writing && !p.unchanged {
        fence(policy, &root, &p.images, &p.receipt_image)?;
        p.receipt.pending = true;
        let bytes = journal_bytes(policy, &p.receipt)?;
        root.replace(policy.receipt, &p.receipt_image, Some(&bytes), 0o600)?;
        let pending = root.read(policy.receipt, JOURNAL_LIMIT)?;
        require(
            pending
                .as_ref()
                .is_some_and(|i| i.bytes == bytes && i.mode & 0o7777 == 0o600),
            policy.errors.pending_recovery_required,
        )?;
        step(0)?;
        for (position, i) in p.receipt.operation.order().into_iter().enumerate() {
            fence(policy, &root, &p.images, &pending)?;
            let target = &p.receipt.target[i];
            if stored(policy, &p.images)?[i] != *target {
                root.replace(
                    policy.leaves[i],
                    &p.images[i],
                    target.as_ref().map(|s| s.text.as_bytes()),
                    target.as_ref().map_or(0o600, |s| s.mode),
                )?;
            }
            p.images[i] = root.read(policy.leaves[i], LIMIT)?;
            require(
                stored(policy, &p.images)?[i] == *target,
                policy.errors.pending_recovery_required,
            )?;
            step(position + 1)?;
        }
        fence(policy, &root, &p.images, &pending)?;
        require(
            stored(policy, &p.images)? == p.receipt.target,
            policy.errors.pending_recovery_required,
        )?;
        p.receipt.pending = false;
        let bytes = journal_bytes(policy, &p.receipt)?;
        root.replace(policy.receipt, &pending, Some(&bytes), 0o600)?;
        let done = root.read(policy.receipt, JOURNAL_LIMIT)?;
        require(
            done.as_ref().is_some_and(|i| i.bytes == bytes),
            policy.errors.pending_recovery_required,
        )?;
        step(3)?;
        fence(policy, &root, &p.images, &done)?;
        status = p.receipt.operation.done();
    }
    if p.missing_arguments {
        status = "planned_requires_executable_and_socket";
    }
    let r = &p.receipt;
    let warning = "Migration drops Kit working/blocked/idle reporting; Coven reports session references only.";
    let placement =
        "File placement is not proof of native activation; no approvals or settings are changed.";
    let mut value = json!({"provider":policy.provider, "status":status, "files":policy.leaves, "operation":r.operation.name(),
        "pending":r.pending, "migrate_kitsune":r.migrated, "native_contract":policy.native_pin,
        "before_digests":r.source.each_ref().map(|s| s.as_ref().map(|v| &v.digest)),
        "after_digests":r.target.each_ref().map(|s| s.as_ref().map(|v| &v.digest)),
        "activation":placement, "migration_warning":if r.migrated { Some(warning) } else { None }});
    if let Some(route) = &r.route {
        value["route"] = serde_json::to_value(route)?;
    }
    Ok(TaskOutput {
        stdout: if json_output {
            serde_json::to_string(&value)?
        } else {
            format!(
                "{} extension: {status}; {placement}{}",
                policy.label,
                if r.migrated {
                    format!(" {warning}")
                } else {
                    String::new()
                }
            ) + if r.route.is_some() {
                "; route: from-kitsune"
            } else {
                ""
            }
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
mod tests;
