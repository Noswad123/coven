//! Private fixed two-leaf journal. No receipt-controlled paths.
//! Detected races retain recovery artifacts; this is not global filesystem CAS.
use crate::claude_hooks::Operation;
use std::path::PathBuf;
pub struct Request {
    pub extension_root: PathBuf,
    pub executable: Option<String>,
    pub socket: Option<String>,
    pub migrate_kitsune: bool,
    pub apply: bool,
    pub operation: Operation,
}
#[cfg(unix)]
pub(crate) struct Policy {
    pub leaves: [&'static str; 2],
    pub receipt: &'static str,
    pub lock: &'static str,
    pub spec: crate::hook_files::Spec,
    pub generate: fn(&str, &str) -> crate::Result<Vec<u8>>,
    pub native_pin: &'static str,
    pub kit: &'static [u8],
    pub kit_sha: &'static str,
    pub schema: &'static str,
    pub provider: &'static str,
    pub label: &'static str,
    pub errors: Errors,
}
#[cfg(unix)]
pub(crate) struct Errors {
    pub absolute_path_required: &'static str,
    pub size_limit: &'static str,
    pub invalid_utf8: &'static str,
    pub invalid_receipt: &'static str,
    pub unsafe_leaf: &'static str,
    pub conflict: &'static str,
    pub receipt_stage_conflict: &'static str,
    pub receipt_required: &'static str,
    pub unreceipted_ownership: &'static str,
    pub ambiguous_kitsune: &'static str,
    pub migration_consent_required: &'static str,
    pub executable_required: &'static str,
    pub socket_required: &'static str,
    pub apply_unsupported_platform: &'static str,
    pub pending_recovery_required: &'static str,
}
#[cfg(unix)]
macro_rules! errors {
    ($prefix:literal) => {
        crate::extension_assets::Errors {
            absolute_path_required: concat!($prefix, "_absolute_path_required"),
            size_limit: concat!($prefix, "_size_limit"),
            invalid_utf8: concat!($prefix, "_invalid_utf8"),
            invalid_receipt: concat!($prefix, "_invalid_receipt"),
            unsafe_leaf: concat!($prefix, "_unsafe_leaf"),
            conflict: concat!($prefix, "_conflict"),
            receipt_stage_conflict: concat!($prefix, "_receipt_stage_conflict"),
            receipt_required: concat!($prefix, "_receipt_required"),
            unreceipted_ownership: concat!($prefix, "_unreceipted_ownership"),
            ambiguous_kitsune: concat!($prefix, "_ambiguous_kitsune"),
            migration_consent_required: concat!($prefix, "_migration_consent_required"),
            executable_required: concat!($prefix, "_executable_required"),
            socket_required: concat!($prefix, "_socket_required"),
            apply_unsupported_platform: concat!($prefix, "_apply_unsupported_platform"),
            pending_recovery_required: concat!($prefix, "_pending_recovery_required"),
        }
    };
}
#[cfg(unix)]
pub(crate) use errors;
#[cfg(unix)]
mod engine;
#[cfg(unix)]
pub(crate) use engine::run;
