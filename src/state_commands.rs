//! Explicit state CLI coordination. SDK reports retain operation-local knowledge;
//! secondary delivery and presentation can never turn publication into failure.
use crate::{
    Error, Result,
    error::require,
    ledger,
    records::TaskRecords,
    storage::{Delivery, WorkspaceGuard},
    task_admission, task_delivery,
    task_mutation::TaskOutput,
    values::{self, Profile, Value},
    workspace,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryMode {
    RepairProjection,
    ResumeImport,
    AbortImport,
}

fn object(fields: Vec<(&str, Value)>) -> Result<Value> {
    Value::object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}
fn text(value: impl Into<String>) -> Value {
    Value::String(value.into())
}
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .as_object()
        .and_then(|v| v.get(key))
        .ok_or(Error("invalid_state_result"))
}
fn root_argument(path: &Path) -> Result<PathBuf> {
    crate::platform::ensure_supported()?;
    let root = crate::platform::workspace_argument(path)?
        .canonicalize()
        .map_err(|_| Error("invalid_task_workspace"))?;
    require(root.is_dir(), "invalid_task_workspace")?;
    root.to_str().ok_or(Error("non_unicode_path_unsupported"))?;
    Ok(root)
}
fn render(value: &Value, json: bool) -> Result<String> {
    let bytes = if json {
        value.compact()?
    } else {
        value.storage()?
    };
    let output = std::str::from_utf8(&bytes)?;
    crate::output::ascii_json(if json {
        output
    } else {
        output.strip_suffix('\n').unwrap_or(output)
    })
}
fn output(stdout: String, uncertain: bool) -> TaskOutput {
    TaskOutput {
        stdout,
        warnings: Vec::new(),
        exit_code: if uncertain { 2 } else { 0 },
    }
}
fn snapshot(root: &Path) -> Result<(String, Value)> {
    let raw = ledger::optional_bytes(root, "state/tasks.json")?;
    let tasks = raw
        .as_deref()
        .map(|v| values::parse(v, Profile::LegacySnapshot))
        .transpose()?
        .unwrap_or(Value::Array(Vec::new()));
    Ok((
        ledger::snapshot_revision(raw.as_deref()),
        TaskRecords::validate(tasks)?.into_value(),
    ))
}

pub fn inspect_arguments(workspace: &Path, json: bool) -> Result<TaskOutput> {
    inspect(&root_argument(workspace)?, json, &StateIo::default())
}
fn inspect(root: &Path, json: bool, io: &StateIo) -> Result<TaskOutput> {
    workspace::diagnostic(root)?;
    let view = ledger::read(root)?;
    let mut pending = false;
    let report = if let Some(view) = &view {
        view.report(root)?
    } else {
        pending = ledger::pending(root)?;
        match snapshot(root) {
            Ok((revision, tasks)) => object(vec![
                ("authority", text("snapshot-v1")),
                ("revision", text(revision)),
                ("tasks", tasks),
                ("import_pending", Value::Bool(pending)),
            ])?,
            Err(error) if pending => object(vec![
                ("authority", text("snapshot-v1")),
                ("import_pending", Value::Bool(true)),
                ("source_error", text(task_admission::warning_code(&error))),
            ])?,
            Err(error) => return Err(error),
        }
    };
    let stdout = render(&report, json)?;
    io.at("inspect_captured", root)?;
    crate::actions::validate_workspace_control(root)?;
    if let Some(view) = view {
        view.fence(root)?;
    } else {
        require(
            ledger::pending(root)? == pending,
            "task_authority_changed_during_read",
        )?;
    }
    Ok(output(stdout, false))
}

fn pin(expected: &str) -> Result<()> {
    require(
        expected == "tasks-missing:v1"
            || expected.strip_prefix("tasks-sha256:").is_some_and(|v| {
                v.len() == 64
                    && v.bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            }),
        "invalid_snapshot_revision",
    )
}

// This is only response-size admission, never a candidate authority token. Include
// actual paths before reserving the envelope: Unicode roots can expand sixfold.
fn admit(root: &Path, tasks: Value, view: Option<&ledger::View>) -> Result<()> {
    let value = admission_value(root, tasks, view)?;
    task_admission::response(&value)?;
    for json in [false, true] {
        require(
            render(&value, json)?.len() <= values::MAX_OUTPUT - task_admission::RESPONSE_RESERVE,
            "unsupported_task_response_profile",
        )?;
    }
    Ok(())
}
fn admission_value(root: &Path, tasks: Value, view: Option<&ledger::View>) -> Result<Value> {
    let projection = unreadable("projection_health_unavailable")?;
    let mut value = if let Some(view) = view {
        view.report_with_projection(projection)?
    } else {
        object(vec![
            ("authority", text("task-ledger-v1")),
            (
                "revision",
                text(format!(
                    "tasks-ledger:v1:{}:{}:18446744073709551615:{}",
                    "0".repeat(32),
                    "0".repeat(32),
                    "0".repeat(64)
                )),
            ),
            ("tasks", tasks),
            ("workspace_id", text("0".repeat(32))),
            ("epoch_id", text("0".repeat(32))),
            ("sequence", Value::integer("18446744073709551615")?),
            ("frame_digest", text("0".repeat(64))),
            ("projection", projection),
        ])?
    };
    value.set_field("outcome", text("durability_uncertain"))?;
    value.set_field("warnings", Value::Array(Vec::new()))?;
    // Both public SDK path families have fixed ASCII children and UUID widths.
    value.set_field(
        "path_budget",
        Value::Array(vec![
            text(
                root.join(format!("state/task-ledger-aborted-{}", "0".repeat(32)))
                    .to_str()
                    .ok_or(Error("non_unicode_path_unsupported"))?,
            ),
            text(
                root.join(format!("state/task-recovery/{}", "0".repeat(32)))
                    .to_str()
                    .ok_or(Error("non_unicode_path_unsupported"))?,
            ),
        ]),
    )?;
    Ok(value)
}

fn import_preflight(root: &Path, expected: Option<&str>, resume: bool) -> Result<()> {
    let state = workspace::load(root)?;
    let expected = expected.ok_or(Error("resume_import_requires_revision"))?;
    pin(expected)?;
    require(
        ledger::read(root)?.is_none(),
        if resume {
            "no_resumable_pending_import"
        } else {
            "active_or_pending_ledger_exists"
        },
    )?;
    require(
        ledger::pending(root)? == resume,
        if resume {
            "no_resumable_pending_import"
        } else {
            "active_or_pending_ledger_exists"
        },
    )?;
    let (revision, tasks) = snapshot(root)?;
    require(revision == expected, "snapshot_revision_mismatch")?;
    // The captured raw source is pinned again by the SDK under this same guard.
    // Both representations must fit, including the full WorkspaceState task load.
    admit(root, state.tasks, None)?;
    admit(root, tasks, None)
}

pub fn import_arguments(workspace: &Path, expected: &str, json: bool) -> Result<TaskOutput> {
    activate(
        &root_argument(workspace)?,
        Some(expected),
        false,
        json,
        &StateIo::default(),
    )
}
fn activate(
    root: &Path,
    expected: Option<&str>,
    resume: bool,
    json: bool,
    io: &StateIo,
) -> Result<TaskOutput> {
    import_preflight(root, expected, resume)?;
    io.at("before_lock", root)?;
    let guard = WorkspaceGuard::acquire(root, Duration::from_secs(5))?;
    io.at("held", root)?;
    import_preflight(root, expected, resume)?;
    let expected = expected.ok_or(Error("resume_import_requires_revision"))?;
    io.at("before_sdk", root)?;
    let report = match io.activate(root, expected, resume, &guard) {
        Ok(report) => report,
        Err(failure) => return import_failure(root, failure),
    };
    let uncertain = report.durability() == ledger::ActivationDurability::SynchronizationUncertain
        || io.uncertain("activation_uncertain");
    let detail = if resume { Some(Detail::Resumed) } else { None };
    Ok(finish(
        root,
        report.view(),
        if resume { "recovered" } else { "committed" },
        detail,
        None,
        uncertain,
        &guard,
        json,
        io,
    ))
}

pub fn recover_arguments(
    workspace: &Path,
    mode: RecoveryMode,
    expected: Option<&str>,
    json: bool,
) -> Result<TaskOutput> {
    recover(
        &root_argument(workspace)?,
        mode,
        expected,
        json,
        &StateIo::default(),
    )
}
fn repair_preflight(root: &Path, expected: Option<&str>) -> Result<()> {
    let state = workspace::load(root)?;
    require(expected.is_none(), "revision_only_with_resume_import")?;
    let view = ledger::read(root)?.ok_or(Error("no_active_ledger"))?;
    admit(root, state.tasks, Some(&view))
}
fn abort_preflight(root: &Path) -> Result<()> {
    workspace::diagnostic(root)?;
    require(ledger::read(root)?.is_none(), "cannot_abort_active_ledger")?;
    require(ledger::pending(root)?, "no_pending_import")?;
    admit(root, Value::Array(Vec::new()), None)
}
fn recover(
    root: &Path,
    mode: RecoveryMode,
    expected: Option<&str>,
    json: bool,
    io: &StateIo,
) -> Result<TaskOutput> {
    if mode == RecoveryMode::ResumeImport {
        return activate(root, expected, true, json, io);
    }
    if mode == RecoveryMode::AbortImport {
        require(expected.is_none(), "revision_only_with_resume_import")?;
        abort_preflight(root)?;
        io.at("before_lock", root)?;
        let guard = WorkspaceGuard::acquire(root, Duration::from_secs(5))?;
        io.at("held", root)?;
        abort_preflight(root)?;
        let report = io.archive(root, &guard)?;
        let uncertain = report.durability() == ledger::ArchiveDurability::SynchronizationUncertain
            || io.uncertain("archive_uncertain");
        let outcome = if uncertain {
            "durability_uncertain"
        } else {
            "aborted"
        };
        let mut warnings = Vec::new();
        if uncertain {
            warnings.push("Pending import archived; durability uncertain".to_owned());
        }
        // Path conversion cannot fail after rename: root was UTF-8 admitted and
        // the SDK appends only a fixed ASCII prefix and a validated hex UUID.
        let path = report.path().to_string_lossy();
        let value = object(vec![
            ("outcome", text(outcome)),
            ("authority", text("snapshot-v1")),
            ("preserved_artifacts", text(path.as_ref())),
        ])
        .and_then(|mut value| {
            if uncertain {
                value.set_field("warnings", warning_values(&warnings))?;
            }
            io.render(&value, json)
        });
        let stdout = value.unwrap_or_else(|_| {
            warnings.push("Full report unavailable; compact known metadata retained".to_owned());
            format!("{{\"outcome\":{},\"authority\":\"snapshot-v1\",\"preserved_artifacts\":{},\"warnings\":[{}]}}", quote(outcome), quote(&path), warnings.iter().map(|s| quote(s)).collect::<Vec<_>>().join(","))
        });
        return Ok(output(stdout, uncertain));
    }
    repair_preflight(root, expected)?;
    io.at("before_lock", root)?;
    let guard = WorkspaceGuard::acquire(root, Duration::from_secs(5))?;
    io.at("held", root)?;
    repair_preflight(root, expected)?;
    let report = match io.repair(root, &guard) {
        Ok(report) => report,
        Err(failure) => {
            if let Some(path) = failure.backup_path() {
                return Ok(preserved_failure(failure.code(), path));
            }
            return Err(Error(failure.code()));
        }
    };
    let (detail, preserved, uncertain) =
        repair_delivery(report.status(), report.durability(), report.backup_path());
    Ok(finish(
        root,
        report.view(),
        "recovered",
        detail,
        preserved,
        uncertain || io.uncertain("repair_uncertain"),
        &guard,
        json,
        io,
    ))
}

fn repair_delivery(
    status: ledger::RepairStatus,
    durability: ledger::RepairDurability,
    backup_path: Option<&Path>,
) -> (Option<Detail<'_>>, Option<&Path>, bool) {
    let uncertain = durability == ledger::RepairDurability::SynchronizationUncertain
        || matches!(
            status,
            ledger::RepairStatus::AuthorityUnconfirmed | ledger::RepairStatus::BackupUnconfirmed
        );
    let detail = match status {
        ledger::RepairStatus::Current => Some(Detail::Projection("current", None)),
        ledger::RepairStatus::Repaired => Some(Detail::Projection("repaired", backup_path)),
        _ => None,
    };
    let preserved = if status == ledger::RepairStatus::BackupUnconfirmed {
        backup_path
    } else {
        None
    };
    (detail, preserved, uncertain)
}

fn import_failure(root: &Path, failure: ledger::ImportFailure) -> Result<TaskOutput> {
    match failure.progress() {
        ledger::ImportProgress::NoNewArtifacts => Err(Error(failure.code())),
        ledger::ImportProgress::Pending { last_published, .. } => Ok(pending_failure(
            root,
            failure.code(),
            last_published.as_deref(),
        )),
    }
}
fn pending_failure(root: &Path, code: &'static str, last_published: Option<&Path>) -> TaskOutput {
    preserved_failure(
        code,
        &root.join(last_published.unwrap_or(Path::new(ledger::DIRECTORY))),
    )
}
fn preserved_failure(code: &'static str, path: &Path) -> TaskOutput {
    TaskOutput {
        stdout: String::new(),
        warnings: vec![format!(
            "[ERROR] {}; preserved artifacts: {}",
            task_admission::warning_code(&Error(code)),
            quote(&path.to_string_lossy())
        )],
        exit_code: 1,
    }
}

#[derive(Clone, Copy)]
enum Detail<'a> {
    Resumed,
    Projection(&'static str, Option<&'a Path>),
}
impl Detail<'_> {
    fn value(self) -> Result<Value> {
        match self {
            Self::Resumed => object(vec![("import_resumed", Value::Bool(true))]),
            Self::Projection(status, path) => object(vec![
                ("status", text(status)),
                (
                    "backup_path",
                    path.map_or(Value::Null, |p| text(p.to_string_lossy().as_ref())),
                ),
            ]),
        }
    }
    fn compact(self) -> String {
        match self {
            Self::Resumed => "{\"import_resumed\":true}".to_owned(),
            Self::Projection(status, path) => format!(
                "{{\"status\":{},\"backup_path\":{}}}",
                quote(status),
                path.map_or_else(|| "null".to_owned(), |p| quote(&p.to_string_lossy()))
            ),
        }
    }
}
fn warning_values(warnings: &[String]) -> Value {
    Value::Array(warnings.iter().map(text).collect())
}
fn unreadable(code: &str) -> Result<Value> {
    object(vec![
        ("status", text("unreadable")),
        ("expected_revision", Value::Null),
        ("observed_revision", Value::Null),
        ("error", text(code)),
    ])
}
fn health(value: Value) -> Result<Value> {
    require(
        matches!(
            field(&value, "status")?.as_str(),
            Some("current" | "missing" | "diverged" | "unreadable")
        ),
        "invalid_projection_health",
    )?;
    let fields = value
        .as_object()
        .ok_or(Error("invalid_projection_health"))?
        .entries();
    require(fields.len() <= 4, "invalid_projection_health")?;
    for (key, value) in fields {
        require(
            matches!(
                key.as_str(),
                "status" | "expected_revision" | "observed_revision" | "error"
            ) && (matches!(value, Value::Null)
                || value.as_str().is_some_and(|v| {
                    v.len() <= 128 && v.is_ascii() && !v.bytes().any(|b| b.is_ascii_control())
                })),
            "invalid_projection_health",
        )?;
    }
    Ok(value)
}

#[allow(clippy::too_many_arguments)] // One operation-local delivery context, not a new framework.
fn finish(
    root: &Path,
    view: &ledger::View,
    normal: &'static str,
    detail: Option<Detail<'_>>,
    preserved: Option<&Path>,
    uncertain: bool,
    guard: &WorkspaceGuard,
    json: bool,
    io: &StateIo,
) -> TaskOutput {
    let outcome = if uncertain {
        "durability_uncertain"
    } else {
        normal
    };
    let mut warnings = Vec::new();
    if uncertain {
        warnings.push(
            "Ledger operation observed; durability uncertain. Inspect before retrying.".to_owned(),
        );
    } else {
        let projection = (|| {
            let status = health(io.health(root, view, false)?)?;
            if field(&status, "status")?.as_str() != Some("current") {
                require(
                    io.projection(root, view, guard)? == Delivery::Committed,
                    "projection_durability_uncertain",
                )?;
            }
            Ok(())
        })();
        if let Err(error) = projection {
            warnings.push(format!(
                "Ledger authority active; task projection stale: {}",
                task_admission::warning_code(&error)
            ));
        }
        // Independent even when the projection check/publication fails. No event.
        if let Err(error) = io.dashboard(root, guard) {
            warnings.push(format!(
                "Ledger authority active; dashboard stale: {}",
                task_admission::warning_code(&error)
            ));
        }
    }
    // AuthorityUnconfirmed must not access projection at all. Other uncertain
    // operations also avoid fresh projection I/O and all secondary effects.
    let projection = if uncertain {
        unreadable("durability_unconfirmed")
    } else {
        io.health(root, view, true).and_then(health)
    };
    let projection = match projection {
        Ok(value) => Ok(value),
        Err(error) => {
            warnings.push(format!(
                "Task projection health unavailable: {}",
                task_admission::warning_code(&error)
            ));
            unreadable(task_admission::warning_code(&error))
        }
    };
    let result = (|| {
        let mut value = io.report(view, projection?)?;
        value.set_field("outcome", text(outcome))?;
        if let Some(detail) = detail {
            value.set_field("recovery", detail.value()?)?;
        }
        if let Some(path) = preserved {
            value.set_field("preserved_artifacts", text(path.to_string_lossy().as_ref()))?;
        }
        value.set_field("warnings", warning_values(&warnings))?;
        io.render(&value, json)
    })();
    let stdout = result.unwrap_or_else(|_| {
        warnings.push("Full report unavailable; compact known metadata retained".to_owned());
        // Infallible ASCII emitter over admitted paths and bounded SDK metadata.
        // Never substitute tasks, a new revision, or rediscover authority here.
        let mut output = format!("{{\"authority\":\"task-ledger-v1\",\"revision\":{},\"workspace_id\":{},\"epoch_id\":{},\"sequence\":{},\"frame_digest\":{},\"outcome\":{}",
            quote(view.revision()), quote(view.workspace_id()), quote(view.epoch_id()), view.sequence(), quote(view.head_digest()), quote(outcome));
        if let Some(detail) = detail { output.push_str(&format!(",\"recovery\":{}", detail.compact())); }
        if let Some(path) = preserved { output.push_str(&format!(",\"preserved_artifacts\":{}", quote(&path.to_string_lossy()))); }
        output.push_str(",\"warnings\":[");
        output.push_str(&warnings.iter().map(|s| quote(s)).collect::<Vec<_>>().join(","));
        output.push_str("]}");
        output
    });
    output(stdout, uncertain)
}

// JSON string escaping with no fallible encoder/profile checks after an effect.
fn quote(value: &str) -> String {
    let mut output = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            c if c.is_ascii() && !c.is_ascii_control() => output.push(c),
            c => {
                for unit in c.encode_utf16(&mut [0; 2]) {
                    output.push_str("\\u");
                    for shift in [12, 8, 4, 0] {
                        output
                            .push(b"0123456789abcdef"[usize::from((*unit >> shift) & 15)] as char);
                    }
                }
            }
        }
    }
    output.push('"');
    output
}

#[derive(Default)]
struct StateIo {
    #[cfg(test)]
    faults: std::collections::BTreeMap<&'static str, Fault>,
    #[cfg(test)]
    trace: std::cell::RefCell<Vec<&'static str>>,
    #[cfg(test)]
    hooks: std::collections::BTreeMap<&'static str, fn(&Path)>,
}
#[cfg(test)]
#[derive(Clone, Copy)]
enum Fault {
    Error,
    Uncertain,
    BadHealth,
    Oversize,
}
impl StateIo {
    fn at(&self, name: &'static str, root: &Path) -> Result<()> {
        self.step(name)?;
        #[cfg(test)]
        if let Some(hook) = self.hooks.get(name) {
            hook(root);
        }
        let _ = root;
        Ok(())
    }
    fn step(&self, name: &'static str) -> Result<()> {
        #[cfg(test)]
        {
            self.trace.borrow_mut().push(name);
            if matches!(self.faults.get(name), Some(Fault::Error)) {
                return Err(Error("injected_state_io_failure"));
            }
        }
        let _ = name;
        Ok(())
    }
    fn uncertain(&self, name: &'static str) -> bool {
        #[cfg(test)]
        {
            matches!(self.faults.get(name), Some(Fault::Uncertain))
        }
        #[cfg(not(test))]
        {
            let _ = name;
            false
        }
    }
    fn activate(
        &self,
        root: &Path,
        expected: &str,
        resume: bool,
        guard: &WorkspaceGuard,
    ) -> std::result::Result<ledger::ActivationReport, ledger::ImportFailure> {
        // Faults after successful SDK calls alter delivery observations, never
        // fabricate an authority view or a preserved path.
        if resume {
            ledger::resume_import(root, expected, guard)
        } else {
            ledger::import_tasks(root, expected, guard)
        }
    }
    fn archive(&self, root: &Path, guard: &WorkspaceGuard) -> Result<ledger::ArchiveReport> {
        self.step("archive")?;
        ledger::abort_import(root, guard)
    }
    #[allow(clippy::result_large_err)] // Preserve the SDK's known-view/path failure unchanged.
    fn repair(
        &self,
        root: &Path,
        guard: &WorkspaceGuard,
    ) -> std::result::Result<ledger::RepairReport, ledger::RepairFailure> {
        ledger::repair_projection(root, guard)
    }
    fn health(&self, root: &Path, view: &ledger::View, final_read: bool) -> Result<Value> {
        let name = if final_read { "final_health" } else { "health" };
        self.step(name)?;
        #[cfg(test)]
        if matches!(self.faults.get(name), Some(Fault::BadHealth)) {
            return Ok(Value::Null);
        }
        ledger::projection_status(root, view)
    }
    fn projection(
        &self,
        root: &Path,
        view: &ledger::View,
        guard: &WorkspaceGuard,
    ) -> Result<Delivery> {
        self.step("projection")?;
        let delivery = ledger::publish_projection(root, view, guard)?;
        Ok(if self.uncertain("projection") {
            Delivery::DurabilityUncertain
        } else {
            delivery
        })
    }
    fn dashboard(&self, root: &Path, guard: &WorkspaceGuard) -> Result<()> {
        self.step("dashboard")?;
        task_delivery::refresh_dashboard(root, guard)
    }
    fn report(&self, view: &ledger::View, projection: Value) -> Result<Value> {
        self.step("report")?;
        let value = view.report_with_projection(projection)?;
        #[cfg(test)]
        if matches!(self.faults.get("report"), Some(Fault::Oversize)) {
            let mut value = value;
            value.set_field("oversized", text("x".repeat(values::MAX_OUTPUT)))?;
            return Ok(value);
        }
        Ok(value)
    }
    fn render(&self, value: &Value, json: bool) -> Result<String> {
        self.step("render")?;
        render(value, json)
    }
}

#[cfg(test)]
mod tests;
