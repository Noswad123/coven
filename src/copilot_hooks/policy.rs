use super::native::{LIMIT, PIN, parse};
use crate::{
    Error, Result,
    claude_hooks::single_file::Policy,
    error::require,
    hook_files::{Root, Spec},
    task_mutation::TaskOutput,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

pub(super) const KIT_SHA: &str = "40dee35dc22f1486267789e499deab23f382bd78ee92edd940f47e369421afbe";
const MARKER: &str = "coven-managed:copilot-hooks:v1";
pub(super) const POLICY: Policy = Policy {
    spec: Spec::copilot(),
    receipt: "coven-copilot-hooks.json",
    lock: ".coven-copilot-hooks.lock",
    parse,
    install,
    uninstall,
    verify_kit,
    output,
    error,
};

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

fn owned(exe: &str, socket: &str) -> Value {
    json!({"type":"command", "bash":format!(
        "{} agent report --provider copilot --socket {} --event SessionStart >/dev/null 2>&1 || true # {MARKER}",
        quote(exe), quote(socket)), "timeoutSec":10})
}

fn encoded(v: &Value) -> Result<Vec<u8>> {
    let mut raw = serde_json::to_vec_pretty(v)?;
    raw.push(b'\n');
    require(raw.len() <= LIMIT, "copilot_hooks_size_limit")?;
    Ok(raw)
}

pub(super) fn install(
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
    let path = root.join("hooks/kitsune-agent-state.sh");
    let legacy = format!(
        "bash {}",
        quote(path.to_str().ok_or(Error("non_unicode_path_unsupported"))?)
    );
    let mut migrated = false;
    for (event, entries) in hooks.iter_mut() {
        let entries = entries.as_array_mut().unwrap();
        let mut remove = Vec::new();
        for (index, h) in entries.iter().enumerate() {
            if h.get("type").and_then(Value::as_str).unwrap_or("command") != "command" {
                continue;
            }
            // Only direct command slots are authority positions. Arguments are
            // inspected as ambiguity signals for exec, never treated as shell.
            let commands = ["bash", "powershell", "command", "exec"]
                .iter()
                .filter_map(|k| h.get(*k).and_then(Value::as_str));
            let args = h
                .get("args")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str);
            let mut kit = false;
            for command in commands.chain(args) {
                require(
                    !command.contains("coven-managed:"),
                    "copilot_hooks_unreceipted_ownership",
                )?;
                kit |= command.contains("kitsune-agent-state");
            }
            if kit {
                // Exact v2 POSIX handler only. Extra execution/platform fields,
                // matchers or metadata could change intent: refuse, never delete.
                require(
                    event == "SessionStart"
                        && *h == json!({"type":"command","bash":legacy,"timeoutSec":10}),
                    "copilot_hooks_ambiguous_kitsune",
                )?;
                require(consent, "copilot_hooks_migration_consent_required")?;
                migrated = true;
                remove.push(index);
            }
        }
        for index in remove.into_iter().rev() {
            entries.remove(index);
        }
    }
    hooks
        .entry("SessionStart")
        .or_insert(json!([]))
        .as_array_mut()
        .unwrap()
        .push(owned(exe, socket));
    Ok((encoded(&v)?, migrated))
}

fn uninstall(post: &[u8], exe: &str, socket: &str) -> Result<Vec<u8>> {
    let mut v = parse(post)?;
    let entries = v["hooks"]["SessionStart"]
        .as_array_mut()
        .ok_or(Error("copilot_hooks_invalid_receipt"))?;
    let owned = owned(exe, socket);
    require(
        entries.iter().filter(|h| **h == owned).count() == 1,
        "copilot_hooks_conflict",
    )?;
    entries.retain(|h| *h != owned);
    encoded(&v)
}

fn verify_kit(root: &Root) -> Result<()> {
    let image = root
        .read_relative("hooks/kitsune-agent-state.sh", LIMIT)?
        .ok_or(Error("copilot_hooks_ambiguous_kitsune"))?;
    require(
        digest(&image.bytes) == KIT_SHA,
        "copilot_hooks_ambiguous_kitsune",
    )
}

fn output(
    json_output: bool,
    status: &str,
    before: Option<&[u8]>,
    after: Option<&[u8]>,
    migrated: bool,
) -> Result<TaskOutput> {
    let value = json!({"provider":"copilot", "status":status, "settings":"settings.json",
        "events":["SessionStart"], "before_digest":before.map(digest), "after_digest":after.map(digest),
        "migrate_kitsune":migrated, "native_contract":PIN,
        "native_hook_approval":"user-managed; may be required; not granted by this operation",
        "native_policy":"disableAllHooks, sandbox, trust and approval preserved; execution not guaranteed"});
    Ok(TaskOutput {
        stdout: if json_output {
            serde_json::to_string(&value)?
        } else {
            format!("Copilot hooks: {status}")
        },
        warnings: vec![],
        exit_code: 0,
    })
}

// Translate the shared runner's original fixed codes without allocating/leaking
// strings. Native/storage policy already emits the Copilot namespace unchanged.
fn error(e: Error) -> Error {
    macro_rules! codes {
        ($($suffix:literal),* $(,)?) => {
            match e.code() {
                $(concat!("claude_hooks_", $suffix) => Error(concat!("copilot_hooks_", $suffix)),)*
                _ => e,
            }
        };
    }
    codes!(
        "absolute_path_required",
        "executable_required",
        "socket_required",
        "apply_unsupported_platform",
        "unsafe_leaf",
        "invalid_receipt",
        "conflict",
        "receipt_required",
        "receipt_stage_conflict",
        "pending_recovery_required",
        "durability_uncertain",
        "recovery_required"
    )
}
