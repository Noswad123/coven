//! Explicit, offline Claude settings ownership. No provider discovery or execution.
//! Pending receipts are resumed only by applying the same operation. Uninstall is
//! intentionally conservative: edits after installation require manual resolution.
//! Linux applies retain displaced entries until verified, not strict filesystem
//! CAS. Writers holding old file descriptors can still mutate an inode after
//! verification. Detected races/crashes leave `.coven-claude-hooks-recovery/`
//! (0700; fixed `expected`, `replacement`, `displaced`, `transaction.json` names)
//! for manual recovery. Generated copies are bounded; displaced external data is
//! retained without assuming its type or size. The manifest is never code authority;
//! never blindly exchange back over a potentially newer edit. No further apply
//! proceeds until that directory is resolved. Installed settings use 0600;
//! rollback restores the original mode. Other Unix platforms support preview only.
use crate::{Error, Result, task_mutation::TaskOutput};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Plan,
    Install,
    Uninstall,
    Rollback,
}

pub struct Request {
    pub config_root: PathBuf,
    pub executable: Option<String>,
    pub socket: Option<String>,
    pub migrate_kitsune: bool,
    pub apply: bool,
    pub operation: Operation,
}

/// Returns exit code zero on a successful plan, mutation, or unchanged result.
/// Refusals use fixed `Error::code()` values, never settings or OS diagnostics.
/// `Plan` ignores `apply`; all other operations require it to write anything.
/// A pending receipt can be inspected with Plan and resumed with the same
/// operation plus apply. There is no automatic rollback across user edits.
pub fn arguments(request: &Request, json: bool) -> Result<TaskOutput> {
    #[cfg(unix)]
    {
        unix::run(request, json)
    }
    #[cfg(not(unix))]
    {
        let _ = (request, json);
        Err(Error("claude_hooks_unsupported_platform"))
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::error::require;
    use crate::hook_files::{Root, Spec};
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::path::{Component, Path};

    const SETTINGS: &str = "settings.json";
    const RECEIPT: &str = "coven-claude-hooks.json";
    const LOCK: &str = ".coven-claude-hooks.lock";
    const LIMIT: usize = 1024 * 1024;
    const MARKER: &str = "coven-managed:claude-hooks:v1";
    const KIT_SHA: &str = "00b90511a1eb781776952232309a8db3f6ed20a54ece679339cf655b9f6a6ff9";
    const EVENTS: [&str; 6] = [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "Stop",
        "SessionEnd",
    ];

    // The receipt contains data only. No path or command from it is executed.
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Receipt {
        version: u32,
        stage: String,
        executable: String,
        socket: String,
        migrated: bool,
        preimage: Option<String>,
        pre_mode: Option<u32>,
        post_mode: u32,
        post_digest: String,
    }

    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
    fn quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    }
    fn absolute(s: &Path) -> Result<()> {
        require(
            s.is_absolute()
                && s.components()
                    .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
                && s.to_str().is_some_and(|s| !s.chars().any(char::is_control)),
            "claude_hooks_absolute_path_required",
        )
    }
    fn command(exe: &str, socket: &str, event: &str) -> String {
        format!(
            "{} agent report --provider claude --socket {} --event {} >/dev/null 2>&1 || true # {MARKER}",
            quote(exe),
            quote(socket),
            event
        )
    }
    fn block(exe: &str, socket: &str, event: &str) -> Value {
        json!({"matcher":"", "hooks":[{"type":"command", "command":command(exe,socket,event), "timeout":2}]})
    }
    fn parse(raw: &[u8]) -> Result<Value> {
        require(raw.len() <= LIMIT, "claude_hooks_size_limit")?;
        let v = crate::strict_json::decode(raw).map_err(|_| Error("claude_hooks_invalid_json"))?;
        require(v.is_object(), "claude_hooks_invalid_settings")?;
        if let Some(hooks) = v.get("hooks") {
            let hooks = hooks
                .as_object()
                .ok_or(Error("claude_hooks_invalid_settings"))?;
            for entries in hooks.values() {
                let entries = entries
                    .as_array()
                    .ok_or(Error("claude_hooks_invalid_settings"))?;
                for entry in entries {
                    require(
                        entry.is_object() && entry.get("hooks").is_some_and(Value::is_array),
                        "claude_hooks_invalid_settings",
                    )?;
                }
            }
        }
        Ok(v)
    }
    fn encoded(v: &Value) -> Result<Vec<u8>> {
        let mut raw = serde_json::to_vec_pretty(v)?;
        raw.push(b'\n');
        require(raw.len() <= LIMIT, "claude_hooks_size_limit")?;
        Ok(raw)
    }
    fn kit_actions(event: &str) -> &'static [&'static str] {
        match event {
            "SessionStart" => &["session", "idle"],
            "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure"
            | "SubagentStop" => &["working"],
            "PermissionRequest" => &["blocked"],
            "Stop" => &["idle"],
            "SessionEnd" => &["release"],
            _ => &[],
        }
    }
    // Public native format facts, independently implemented; no Kit source/assets shipped.
    fn install_image(
        pre: Option<&[u8]>,
        root: &Path,
        exe: &str,
        socket: &str,
        consent: bool,
    ) -> Result<(Vec<u8>, bool)> {
        let mut v = parse(pre.unwrap_or(b"{}"))?;
        let hooks = v
            .as_object_mut()
            .unwrap()
            .entry("hooks")
            .or_insert(json!({}))
            .as_object_mut()
            .unwrap();
        let kit_path = root.join("hooks/kitsune-agent-state.sh");
        let kit_prefix = format!(
            "bash {} ",
            quote(
                kit_path
                    .to_str()
                    .ok_or(Error("non_unicode_path_unsupported"))?
            )
        );
        let mut migrated = false;
        for (event, entries) in hooks.iter_mut() {
            let entries = entries.as_array_mut().unwrap();
            let mut empty_owned_entries = Vec::new();
            for (entry_index, entry) in entries.iter_mut().enumerate() {
                let children = entry["hooks"].as_array_mut().unwrap();
                let mut remove = Vec::new();
                for (index, hook) in children.iter().enumerate() {
                    if let Some(cmd) = hook.get("command").and_then(Value::as_str) {
                        // Suspicious strings are conflict signals, never evidence of ownership.
                        require(!cmd.contains(MARKER), "claude_hooks_unreceipted_ownership")?;
                        if cmd.contains("kitsune-agent-state") {
                            require(
                                kit_actions(event)
                                    .iter()
                                    .any(|action| cmd == format!("{kit_prefix}{action}"))
                                    && hook.get("type").and_then(Value::as_str) == Some("command"),
                                "claude_hooks_ambiguous_kitsune",
                            )?;
                            require(consent, "claude_hooks_migration_consent_required")?;
                            migrated = true;
                            remove.push(index);
                        }
                    }
                }
                let changed = !remove.is_empty();
                for index in remove.into_iter().rev() {
                    children.remove(index);
                }
                if changed && children.is_empty() {
                    empty_owned_entries.push(entry_index);
                }
            }
            for index in empty_owned_entries.into_iter().rev() {
                entries.remove(index);
            }
        }
        for event in EVENTS {
            hooks
                .entry(event)
                .or_insert(json!([]))
                .as_array_mut()
                .unwrap()
                .push(block(exe, socket, event));
        }
        Ok((encoded(&v)?, migrated))
    }

    fn uninstalled(post: &[u8], exe: &str, socket: &str) -> Result<Vec<u8>> {
        let mut v = parse(post)?;
        for event in EVENTS {
            let entries = v["hooks"][event]
                .as_array_mut()
                .ok_or(Error("claude_hooks_invalid_receipt"))?;
            let owned = block(exe, socket, event);
            require(
                entries.iter().filter(|e| **e == owned).count() == 1,
                "claude_hooks_conflict",
            )?;
            entries.retain(|e| *e != owned);
        }
        encoded(&v)
    }
    fn verify_kit(root: &Root) -> Result<()> {
        let script = root
            .read_relative("hooks/kitsune-agent-state.sh", LIMIT)?
            .ok_or(Error("claude_hooks_ambiguous_kitsune"))?;
        require(
            digest(&script.bytes) == KIT_SHA,
            "claude_hooks_ambiguous_kitsune",
        )
    }
    fn output(
        json_output: bool,
        status: &str,
        before: Option<&[u8]>,
        after: Option<&[u8]>,
        migrated: bool,
    ) -> Result<TaskOutput> {
        let value = json!({"provider":"claude", "status":status, "settings":"settings.json", "events":EVENTS,
            "before_digest":before.map(digest), "after_digest":after.map(digest), "migrate_kitsune":migrated});
        Ok(TaskOutput {
            stdout: if json_output {
                serde_json::to_string(&value)?
            } else {
                format!("Claude hooks: {status}")
            },
            warnings: vec![],
            exit_code: 0,
        })
    }

    pub(super) fn run(request: &Request, json_output: bool) -> Result<TaskOutput> {
        let root = Root::open(&request.config_root, Spec::claude())?;
        if let Some(exe) = &request.executable {
            absolute(Path::new(exe))?;
        }
        if let Some(socket) = &request.socket {
            absolute(Path::new(socket))?;
        }
        if request.operation == Operation::Install {
            require(
                request.executable.is_some(),
                "claude_hooks_executable_required",
            )?;
            require(request.socket.is_some(), "claude_hooks_socket_required")?;
        }
        let writing = request.apply && request.operation != Operation::Plan;
        require(
            !writing || cfg!(target_os = "linux"),
            "claude_hooks_apply_unsupported_platform",
        )?;
        root.recovery_absent()?;
        // Read-only paths never create/open a lock for writing.
        let _lock = if writing {
            Some(root.lock(LOCK)?)
        } else {
            None
        };
        let current = root.read(SETTINGS, LIMIT)?;
        if let Some(image) = &current {
            parse(&image.bytes)?;
            require(image.mode & 0o7000 == 0, "claude_hooks_unsafe_leaf")?;
        }
        let receipt_image = root.read(RECEIPT, LIMIT * 8)?;
        let before = current.as_ref().map(|i| i.bytes.as_slice());
        let before_mode = current.as_ref().map(|i| i.mode & 0o7777);
        let matches =
            |bytes: Option<&[u8]>, mode: Option<u32>| before == bytes && before_mode == mode;
        if receipt_image.is_none()
            && request.operation == Operation::Plan
            && (request.executable.is_none() || request.socket.is_none())
        {
            if let Some(exe) = &request.executable {
                absolute(Path::new(exe))?;
            }
            if let Some(socket) = &request.socket {
                absolute(Path::new(socket))?;
            }
            let (_, migrated) = install_image(
                before,
                &request.config_root,
                "/unconfigured-coven",
                "/unconfigured-receiver",
                request.migrate_kitsune,
            )?;
            if migrated {
                verify_kit(&root)?;
            }
            return output(
                json_output,
                "planned_requires_executable_and_socket",
                before,
                None,
                migrated,
            );
        }
        let mut receipt;
        let post;
        if let Some(image) = &receipt_image {
            let v = crate::strict_json::decode(&image.bytes)
                .map_err(|_| Error("claude_hooks_invalid_receipt"))?;
            receipt = serde_json::from_value::<Receipt>(v)
                .map_err(|_| Error("claude_hooks_invalid_receipt"))?;
            require(
                receipt.version == 2
                    && receipt.post_mode == 0o600
                    && receipt.preimage.is_some() == receipt.pre_mode.is_some()
                    && receipt
                        .pre_mode
                        .is_none_or(|mode| mode <= 0o777 && mode & 0o022 == 0),
                "claude_hooks_invalid_receipt",
            )?;
            absolute(Path::new(&receipt.executable))?;
            absolute(Path::new(&receipt.socket))?;
            let (rebuilt, migrated) = install_image(
                receipt.preimage.as_deref().map(str::as_bytes),
                &request.config_root,
                &receipt.executable,
                &receipt.socket,
                receipt.migrated,
            )?;
            require(
                migrated == receipt.migrated && digest(&rebuilt) == receipt.post_digest,
                "claude_hooks_invalid_receipt",
            )?;
            if matches!(request.operation, Operation::Install | Operation::Plan)
                && matches!(receipt.stage.as_str(), "uninstalled" | "rolled_back")
            {
                let previous_target = if receipt.stage == "rolled_back" {
                    receipt
                        .preimage
                        .as_deref()
                        .map(str::as_bytes)
                        .map(Vec::from)
                } else {
                    Some(uninstalled(&rebuilt, &receipt.executable, &receipt.socket)?)
                };
                require(
                    matches(
                        previous_target.as_deref(),
                        if receipt.stage == "rolled_back" {
                            receipt.pre_mode
                        } else {
                            Some(receipt.post_mode)
                        },
                    ),
                    "claude_hooks_conflict",
                )?;
                let (built, migrated) = install_image(
                    before,
                    &request.config_root,
                    &receipt.executable,
                    &receipt.socket,
                    request.migrate_kitsune,
                )?;
                if migrated {
                    verify_kit(&root)?;
                }
                receipt.preimage = before.map(|b| String::from_utf8(b.to_vec()).unwrap());
                receipt.pre_mode = before_mode;
                receipt.post_digest = digest(&built);
                receipt.migrated = migrated;
                receipt.stage = "new".into();
                post = built;
            } else {
                post = rebuilt;
            }
        } else {
            require(
                matches!(request.operation, Operation::Plan | Operation::Install),
                "claude_hooks_receipt_required",
            )?;
            let exe = request
                .executable
                .as_deref()
                .ok_or(Error("claude_hooks_executable_required"))?;
            let socket = request
                .socket
                .as_deref()
                .ok_or(Error("claude_hooks_socket_required"))?;
            absolute(Path::new(exe))?;
            absolute(Path::new(socket))?;
            let (built, migrated) = install_image(
                before,
                &request.config_root,
                exe,
                socket,
                request.migrate_kitsune,
            )?;
            if migrated {
                verify_kit(&root)?;
            }
            receipt = Receipt {
                version: 2,
                stage: "new".into(),
                executable: exe.into(),
                socket: socket.into(),
                migrated,
                preimage: before.map(|b| String::from_utf8(b.to_vec()).unwrap()),
                pre_mode: before_mode,
                post_mode: 0o600,
                post_digest: digest(&built),
            };
            post = built;
        }
        if let Some(exe) = &request.executable {
            require(*exe == receipt.executable, "claude_hooks_conflict")?;
        }
        if let Some(socket) = &request.socket {
            require(*socket == receipt.socket, "claude_hooks_conflict")?;
        }
        let uninstall = uninstalled(&post, &receipt.executable, &receipt.socket)?;
        let pre = receipt.preimage.as_deref().map(str::as_bytes);
        if request.operation == Operation::Plan
            && matches!(
                receipt.stage.as_str(),
                "pending_uninstall" | "pending_rollback"
            )
        {
            let target = if receipt.stage == "pending_uninstall" {
                Some(uninstall.as_slice())
            } else {
                pre
            };
            let target_mode = if receipt.stage == "pending_uninstall" {
                Some(receipt.post_mode)
            } else {
                receipt.pre_mode
            };
            require(
                matches(Some(post.as_slice()), Some(receipt.post_mode))
                    || matches(target, target_mode),
                "claude_hooks_conflict",
            )?;
            return output(
                json_output,
                if receipt.stage == "pending_uninstall" {
                    "recovery_required_apply_uninstall"
                } else {
                    "recovery_required_apply_rollback"
                },
                before,
                target,
                receipt.migrated,
            );
        }
        let operation = if request.operation == Operation::Plan {
            Operation::Install
        } else {
            request.operation
        };
        let (pending, done, source, target): (&str, &str, Option<&[u8]>, Option<&[u8]>) =
            match operation {
                Operation::Install => ("pending_install", "installed", pre, Some(&post)),
                Operation::Uninstall => (
                    "pending_uninstall",
                    "uninstalled",
                    Some(&post),
                    Some(&uninstall),
                ),
                Operation::Rollback => ("pending_rollback", "rolled_back", Some(&post), pre),
                Operation::Plan => unreachable!(),
            };
        let source_mode = if operation == Operation::Install {
            receipt.pre_mode
        } else {
            Some(receipt.post_mode)
        };
        let target_mode = if operation == Operation::Rollback {
            receipt.pre_mode
        } else {
            Some(receipt.post_mode)
        };
        require(
            matches!(
                receipt.stage.as_str(),
                "new"
                    | "installed"
                    | "uninstalled"
                    | "rolled_back"
                    | "pending_install"
                    | "pending_uninstall"
                    | "pending_rollback"
            ),
            "claude_hooks_invalid_receipt",
        )?;
        if receipt.stage == done {
            require(matches(target, target_mode), "claude_hooks_conflict")?;
            return output(json_output, "unchanged", before, target, receipt.migrated);
        }
        let recovering = receipt.stage == pending;
        require(
            recovering
                || (operation == Operation::Install && receipt.stage == "new")
                || (operation != Operation::Install && receipt.stage == "installed"),
            "claude_hooks_receipt_stage_conflict",
        )?;
        require(
            matches(source, source_mode) || (recovering && matches(target, target_mode)),
            "claude_hooks_conflict",
        )?;
        // Consent was recorded before interruption, but the asset may have been
        // edited since. Only an already committed target can retire this check.
        if recovering
            && operation == Operation::Install
            && receipt.migrated
            && !matches(target, target_mode)
        {
            verify_kit(&root)?;
        }
        if !writing {
            return output(
                json_output,
                if recovering {
                    "recovery_required"
                } else {
                    "planned"
                },
                before,
                target,
                receipt.migrated,
            );
        }
        root.fence(SETTINGS, &current)?;
        root.fence(RECEIPT, &receipt_image)?;
        let migrated = receipt.migrated;
        receipt.stage = pending.into();
        let pending_bytes = serde_json::to_vec_pretty(&receipt)?;
        root.replace(RECEIPT, &receipt_image, Some(&pending_bytes), 0o600)?;
        let pending_image = root
            .read(RECEIPT, LIMIT * 8)
            .map_err(|_| Error("claude_hooks_pending_recovery_required"))?;
        require(
            pending_image
                .as_ref()
                .is_some_and(|i| i.bytes == pending_bytes),
            "claude_hooks_pending_recovery_required",
        )?;
        if !matches(target, target_mode) {
            root.replace(SETTINGS, &current, target, target_mode.unwrap_or(0o600))
                .map_err(|e| {
                    if matches!(
                        e.code(),
                        "claude_hooks_durability_uncertain" | "claude_hooks_recovery_required"
                    ) {
                        e
                    } else {
                        Error("claude_hooks_pending_recovery_required")
                    }
                })?;
        }
        receipt.stage = done.into();
        root.replace(
            RECEIPT,
            &pending_image,
            Some(&serde_json::to_vec_pretty(&receipt)?),
            0o600,
        )
        .map_err(|error| {
            if matches!(
                error.code(),
                "claude_hooks_durability_uncertain" | "claude_hooks_recovery_required"
            ) {
                error
            } else {
                Error("claude_hooks_pending_recovery_required")
            }
        })?;
        output(json_output, done, before, target, migrated)
    }
}
