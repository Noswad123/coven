use super::*;
use std::{
    cell::{Cell, RefCell},
    os::unix::fs::PermissionsExt,
    time::Duration,
};

const RUNTIME: &str = "616fed3e-ae7f-41a0-99e5-80546d659d76";
const INSTANCE: &str = "e3c0e2ce-544c-4f1e-a489-5d3e0cf1978e";
const PARTICIPANT: &str = "0c1a32bf-1a23-4da1-8234-123456789abc";
const SAMPLED: u64 = 1_792_861_200_000;

struct ClockHook {
    base: Instant,
    elapsed: Cell<u64>,
    wall: Cell<f64>,
    calls: Cell<usize>,
    hook: RefCell<Box<dyn FnMut(usize)>>,
}
impl ClockHook {
    fn new() -> Self {
        Self {
            base: Instant::now(),
            elapsed: Cell::new(0),
            wall: Cell::new(SAMPLED as f64 + 100.0),
            calls: Cell::new(0),
            hook: RefCell::new(Box::new(|_| {})),
        }
    }
}
impl Clock for ClockHook {
    fn monotonic(&self) -> Instant {
        self.base + Duration::from_millis(self.elapsed.get())
    }
    fn wall_ms(&self) -> f64 {
        self.wall.get()
    }
    fn timestamp(&self) -> Result<String> {
        let n = self.calls.get() + 1;
        self.calls.set(n);
        (self.hook.borrow_mut())(n);
        Ok("2026-10-24T12:00:00+00:00".into())
    }
}

fn workspace() -> (tempfile::TempDir, String) {
    workspace_with_stats(None)
}

fn workspace_with_stats(stats: Option<&str>) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::create_dir(dir.path().join("state")).unwrap();
    std::fs::write(dir.path().join("coven.json"), b"deliberately not parsed").unwrap();
    let path = dir
        .path()
        .join(format!("{RUNTIME}.json"))
        .to_str()
        .unwrap()
        .to_owned();
    let value = json!({"schema_version":1,"contract":live::NATIVE_CONTRACT,"runtime_id":RUNTIME,
        "sequence":1,"sampled_at_ms":SAMPLED,"valid_for_ms":3000,"source":"legacy_session_run_state",
        "activity_scope":"legacy_runner_ownership","capabilities":{"activity":true,"idle":false,"catalog_complete":false,
        "decision_state":false,"retry_state":false,"durable_lifecycle":false,"job_completion":false},
        "authorization":"none","dispatchable":false,"native_lifecycle":"unestablished","job_completion":"unestablished",
        "instances":[{"instance_id":INSTANCE,"sessions":[{"session_id":"selected","activity":"unknown"}]}]});
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    // Have the legacy oracle construct a valid declaration-only binding. No native calls.
    let script = r#"
import sys
from runner import groups
from pathlib import Path
root=Path(sys.argv[1])
groups.create_group(root,'review','Review')
binding={'adapter':'kitsune','endpoint':{'path':'/tmp/unused.sock','device':1,'inode':2},'terminal_id':'terminal',
 'pane_id_at_binding':'pane','native_agent_ref':None,'bound_at':'2026-10-24T12:00:00+00:00'}
if len(sys.argv)>2:
    binding['endpoint'].update(device=int(sys.argv[2]),inode=int(sys.argv[2]))
from runner import group_publisher as p
binding=p.binding_from_snapshot({'endpoint':binding['endpoint'],'observed_at':binding['bound_at'],
 'agents':[{'pane_id':'pane','terminal_id':'terminal','workspace_id':'w','tab_id':'t','agent':'djinn','agent_session':None}]},'pane')
groups.add_member(root,'review','member','Reviewer',binding)
"#;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|p| p.join("runner").is_dir())
        .unwrap();
    let output = std::process::Command::new("python3")
        .args(["-B", "-c", script, dir.path().to_str().unwrap()])
        .args(stats)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (dir, path)
}
fn request(file: &str) -> AttachRequest<'_> {
    AttachRequest {
        group: "review",
        member: "member",
        file,
        runtime: RUNTIME,
        instance: INSTANCE,
        session: "selected",
        reattach: false,
        expected_revision: None,
        timeout: 1.0,
    }
}
fn edit(workspace: &Path, edit: impl FnOnce(&mut Value)) {
    let path = workspace.join("state/groups.json");
    let mut v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    edit(&mut v);
    std::fs::write(path, serde_json::to_vec(&v).unwrap()).unwrap();
}

#[test]
fn attach_fresh_idempotent_preserves_timestamp_and_detach_ignores_unsafe_source() {
    let (dir, file) = workspace();
    let first = attach_with_clock(dir.path(), request(&file), &ClockHook::new()).unwrap();
    assert_eq!(
        authority_field(&first, "outcome").unwrap().as_str(),
        Some("committed")
    );
    let again = attach_with_clock(dir.path(), request(&file), &ClockHook::new()).unwrap();
    assert_eq!(
        authority_field(&again, "outcome").unwrap().as_str(),
        Some("unchanged")
    );
    assert_eq!(
        authority_field(&first, "member")
            .unwrap()
            .canonical()
            .unwrap(),
        authority_field(&again, "member")
            .unwrap()
            .canonical()
            .unwrap()
    );
    edit(dir.path(), |state| {
        state["groups"][0]["members"][0]["live_attachment"]["attached_at"] =
            json!("2026-10-23T12:00:00+00:00");
    });
    let mut same = request(&file);
    same.reattach = true;
    let unchanged = attach_with_clock(dir.path(), same, &ClockHook::new()).unwrap();
    assert_eq!(
        authority_field(&unchanged, "outcome").unwrap().as_str(),
        Some("unchanged")
    );
    let member = authority_field(&unchanged, "member").unwrap();
    let attachment = authority_field(member, "live_attachment").unwrap();
    assert_eq!(
        authority_field(attachment, "attached_at").unwrap().as_str(),
        Some("2026-10-23T12:00:00+00:00")
    );
    std::fs::remove_file(&file).unwrap();
    assert!(attach_with_clock(dir.path(), request(&file), &ClockHook::new()).is_err());
    assert_eq!(
        authority_field(
            &detach(dir.path(), "review", "member", None).unwrap(),
            "outcome"
        )
        .unwrap()
        .as_str(),
        Some("committed")
    );
}

#[test]
fn attach_always_cas_fences_content_and_remove_readd_aba() {
    for aba in [false, true] {
        let (dir, file) = workspace();
        let root = dir.path().to_owned();
        let clock = ClockHook::new();
        *clock.hook.borrow_mut() = Box::new(move |n| {
            if n == 2 {
                edit(&root, |v| {
                    if aba {
                        v["groups"][0]["members"][0]["participant_id"] = json!(PARTICIPANT);
                    } else {
                        v["content_revision"] = json!(v["content_revision"].as_u64().unwrap() + 1);
                    }
                });
            }
        });
        assert_eq!(
            attach_with_clock(dir.path(), request(&file), &clock)
                .unwrap_err()
                .code(),
            "groups_revision_conflict"
        );
        let member = crate::observation_wire::to_json(
            groups::read_groups(dir.path())
                .unwrap()
                .member("review", "member")
                .unwrap(),
        )
        .unwrap();
        assert!(member.get("live_attachment").is_none_or(Value::is_null));
    }
}

#[test]
fn status_observes_unknown_and_individual_faults_but_whole_content_drift_is_empty() {
    let (dir, file) = workspace();
    attach_with_clock(dir.path(), request(&file), &ClockHook::new()).unwrap();
    let report = status_with_clock(dir.path(), "review", None, 1.0, &ClockHook::new()).unwrap();
    assert_eq!(report["assessment"], "observed");
    assert_eq!(report["participants"][0]["activity"], "unknown");
    assert!(!report["participants"][0]["source"].is_null());
    let root = dir.path().to_owned();
    let clock = ClockHook::new();
    *clock.hook.borrow_mut() = Box::new(move |n| {
        if n == 3 {
            edit(&root, |v| v["groups"][0]["name"] = json!("Changed"));
        }
    });
    let report = status_with_clock(dir.path(), "review", None, 1.0, &clock).unwrap();
    assert_eq!(report["diagnostics"], json!(["groups_changed"]));
    assert_eq!(report["participants"], json!([]));
    std::fs::remove_file(&file).unwrap();
    let report = status_with_clock(dir.path(), "review", None, 1.0, &ClockHook::new()).unwrap();
    assert_eq!(report["assessment"], "observed");
    assert_eq!(report["participants"][0]["assessment"], "unavailable");
}

#[test]
fn final_expiry_clears_even_observed_unknown_source_and_future_wall() {
    let (dir, file) = workspace();
    attach_with_clock(dir.path(), request(&file), &ClockHook::new()).unwrap();
    for (wall, elapsed, code) in [
        (SAMPLED as f64 - 1.0, 0, "future_sample"),
        (SAMPLED as f64 + 3000.0, 0, "source_expired"),
        (SAMPLED as f64 + 100.0, 1000, "status_timeout"),
    ] {
        struct FinalClock {
            inner: ClockHook,
            wall: f64,
            elapsed: u64,
        }
        impl Clock for FinalClock {
            fn monotonic(&self) -> Instant {
                self.inner.monotonic()
            }
            fn wall_ms(&self) -> f64 {
                self.inner.wall_ms()
            }
            fn timestamp(&self) -> Result<String> {
                let stamp = self.inner.timestamp()?;
                if self.inner.calls.get() == 4 {
                    self.inner.wall.set(self.wall);
                    self.inner.elapsed.set(self.elapsed);
                }
                Ok(stamp)
            }
        }
        let report = status_with_clock(
            dir.path(),
            "review",
            None,
            1.0,
            &FinalClock {
                inner: ClockHook::new(),
                wall,
                elapsed,
            },
        )
        .unwrap();
        if code == "status_timeout" {
            assert_eq!(report["participants"], json!([]));
            assert_eq!(report["diagnostics"], json!([code]));
        } else {
            assert!(report["participants"][0]["source"].is_null());
            assert_eq!(report["participants"][0]["diagnostics"], json!([code]));
        }
    }
}

#[test]
fn obvious_pin_change_is_refused_before_missing_artifact_io() {
    let (dir, file) = workspace();
    attach_with_clock(dir.path(), request(&file), &ClockHook::new()).unwrap();
    std::fs::remove_file(&file).unwrap();
    let mut changed = request(&file);
    changed.session = "other";
    assert_eq!(
        attach_with_clock(dir.path(), changed, &ClockHook::new())
            .unwrap_err()
            .code(),
        "live_reattach_required"
    );
    let mut changed = request(&file);
    changed.expected_revision = Some(groups::UNINITIALIZED);
    assert_eq!(
        attach_with_clock(dir.path(), changed, &ClockHook::new())
            .unwrap_err()
            .code(),
        "groups_revision_conflict"
    );
}

#[test]
fn serial_reads_use_declared_sources_without_cache_and_allow_counter_only_updates() {
    let (dir, file) = workspace();
    attach_with_clock(dir.path(), request(&file), &ClockHook::new()).unwrap();
    edit(dir.path(), |state| {
        let mut second = state["groups"][0]["members"][0].clone();
        second["participant_id"] = json!(PARTICIPANT);
        second["key"] = json!("second");
        second["binding"]["presentation"]["terminal_id"] = json!("terminal-2");
        state["groups"][0]["members"]
            .as_array_mut()
            .unwrap()
            .push(second);
    });
    let root = dir.path().to_owned();
    let clock = ClockHook::new();
    let path = file.clone();
    *clock.hook.borrow_mut() = Box::new(move |n| {
        if n == 3 {
            let mut source: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            source["instances"][0]["sessions"][0]["activity"] = json!("busy");
            std::fs::write(&path, serde_json::to_vec(&source).unwrap()).unwrap();
            edit(&root, |v| {
                v["publication_counters"] = json!([{"endpoint":{"path":"/tmp/unused.sock","device":1,"inode":2},
            "source":"coven","epoch":"e","watermarks":{"terminal":1}}])
            });
        }
    });
    let report = status_with_clock(dir.path(), "review", None, 1.0, &clock).unwrap();
    assert_eq!(report["assessment"], "observed");
    assert_eq!(report["participants"][0]["activity"], "unknown");
    assert_eq!(report["participants"][1]["activity"], "busy");
    assert_eq!(clock.calls.get(), 6); // Start, two receipts per source, final projection.
}

#[test]
fn status_budget_includes_work_before_first_authority_read_and_recheck_failure() {
    let (dir, file) = workspace();
    attach_with_clock(dir.path(), request(&file), &ClockHook::new()).unwrap();
    struct BudgetClock {
        inner: ClockHook,
    }
    impl Clock for BudgetClock {
        fn monotonic(&self) -> Instant {
            self.inner.monotonic()
        }
        fn wall_ms(&self) -> f64 {
            self.inner.wall_ms()
        }
        fn timestamp(&self) -> Result<String> {
            let result = self.inner.timestamp();
            self.inner.elapsed.set(1000);
            result
        }
    }
    let clock = BudgetClock {
        inner: ClockHook::new(),
    };
    let report = status_with_clock(dir.path(), "review", None, 1.0, &clock).unwrap();
    assert_eq!(report["diagnostics"], json!(["status_timeout"]));
    assert_eq!(report["participants"], json!([]));
    assert_eq!(clock.inner.calls.get(), 2); // No source read after the initial budget elapsed.
    let root = dir.path().to_owned();
    let clock = ClockHook::new();
    *clock.hook.borrow_mut() = Box::new(move |n| {
        if n == 3 {
            std::fs::write(root.join("state/groups.json"), b"invalid").unwrap();
        }
    });
    let report = status_with_clock(dir.path(), "review", None, 1.0, &clock).unwrap();
    assert_eq!(report["diagnostics"], json!(["groups_unavailable"]));
    assert_eq!(report["participants"], json!([]));
}

#[test]
fn accepted_is_strict_about_adapter_row_and_whole_selected_source() {
    let (dir, file) = workspace();
    let attached = attach_with_clock(dir.path(), request(&file), &ClockHook::new()).unwrap();
    let member = authority_field(&attached, "member").unwrap();
    let attachment =
        crate::observation_wire::to_json(authority_field(member, "live_attachment").unwrap())
            .unwrap();
    let live_request = Request {
        file,
        runtime: RUNTIME.into(),
        instance: INSTANCE.into(),
        session: "selected".into(),
        timeout: 1.0,
    };
    let report = live::inspect_with_clock(&live_request, &ClockHook::new()).unwrap();
    assert!(accepted(&report, &attachment).is_ok());
    for (key, value) in [
        ("adapter", json!("other")),
        ("expires_in_ms", json!(true)),
        ("expires_in_ms", json!(3001)),
        ("dispatchable", json!(0)),
        ("attention", json!("ready")),
        ("schema_version", json!(1.0)),
    ] {
        let mut bad = report.clone();
        bad[key] = value;
        assert!(accepted(&bad, &attachment).is_err());
    }
    for key in ["runtime_id", "instance_id", "session_id"] {
        let mut bad = report.clone();
        bad["source"][key] = json!("different");
        assert!(accepted(&bad, &attachment).is_err());
    }
    let mut bad = report.clone();
    bad["source"]["extra"] = json!("PRIVATE");
    assert!(accepted(&bad, &attachment).is_err());
    let mut bad = report;
    bad["source"]["capabilities"]["idle"] = json!(true);
    assert!(accepted(&bad, &attachment).is_err());
}

#[test]
fn python_b_closed_oracle_status_semantics() {
    let (dir, file) = workspace();
    attach_with_clock(dir.path(), request(&file), &ClockHook::new()).unwrap();
    let script = r#"
import json,sys
from unittest.mock import patch
from pathlib import Path
from runner import djinn_live_observation as live, group_live
class Clock:
    def monotonic(self): return 0
    def time(self): return 1792861200.1
with patch.object(live,'time',Clock()), patch.object(group_live,'time',Clock()):
    print(json.dumps(group_live.status(Path(sys.argv[1]),'review')))
"#;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|p| p.join("runner").is_dir())
        .unwrap();
    let output = std::process::Command::new("python3")
        .args(["-B", "-c", script, dir.path().to_str().unwrap()])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut python: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut native = status_with_clock(dir.path(), "review", None, 1.0, &ClockHook::new()).unwrap();
    for key in ["started_at", "observed_at", "duration_ms"] {
        python.as_object_mut().unwrap().remove(key);
        native.as_object_mut().unwrap().remove(key);
    }
    assert_eq!(native, python);
}

#[test]
fn observed_unknown_receipt_budget_is_reduced_after_all_io_even_with_same_wall() {
    let (dir, file) = workspace();
    attach_with_clock(dir.path(), request(&file), &ClockHook::new()).unwrap();
    struct FinalElapsed(ClockHook);
    impl Clock for FinalElapsed {
        fn monotonic(&self) -> Instant {
            self.0.monotonic()
        }
        fn wall_ms(&self) -> f64 {
            self.0.wall_ms()
        }
        fn timestamp(&self) -> Result<String> {
            let result = self.0.timestamp();
            if self.0.calls.get() == 4 {
                self.0.elapsed.set(101);
            }
            result
        }
    }
    let clock = FinalElapsed(ClockHook::new());
    clock.0.wall.set(SAMPLED as f64 + 2900.0);
    let report = status_with_clock(dir.path(), "review", None, 1.0, &clock).unwrap();
    assert_eq!(report["assessment"], "observed");
    assert_eq!(
        report["participants"][0]["diagnostics"],
        json!(["source_expired"])
    );
    assert!(report["participants"][0]["source"].is_null());
    assert_eq!(report["participants"][0]["expires_in_ms"], 0);
}

#[test]
fn arbitrary_precision_unused_endpoint_stats_never_cross_report_or_mutation_codecs() {
    let huge = "1234567890".repeat(10);
    fn assert_stats(result: &AuthorityValue, expected: &str) {
        assert_eq!(
            authority_field(result, "outcome").unwrap().as_str(),
            Some("committed")
        );
        let group = authority_field(result, "group").unwrap();
        let group_member = &authority_field(group, "members")
            .unwrap()
            .as_array()
            .unwrap()[0];
        for member in [authority_field(result, "member").unwrap(), group_member] {
            let binding = authority_field(member, "binding").unwrap();
            let presentation = authority_field(binding, "presentation").unwrap();
            let endpoint = authority_field(presentation, "endpoint").unwrap();
            for key in ["device", "inode"] {
                let stat = authority_field(endpoint, key).unwrap();
                let AuthorityValue::Number(crate::values::Number::Integer(integer)) = stat else {
                    panic!("endpoint integer was narrowed or converted to float");
                };
                assert_eq!(integer.as_str(), expected);
                assert_eq!(stat.compact().unwrap(), expected.as_bytes());
            }
        }
    }
    // Verify the public API types, not a test-only compatibility conversion.
    let _: for<'a, 'b> fn(&'a Path, AttachRequest<'b>) -> Result<AuthorityValue> = attach;
    for unsafe_source in [false, true] {
        let (dir, file) = workspace_with_stats(Some(&huge));
        let before = groups::read_groups(dir.path()).unwrap();
        assert!(before.member("review", "member").is_ok());
        let unattached =
            status_with_clock(dir.path(), "review", None, 1.0, &ClockHook::new()).unwrap();
        assert_eq!(unattached["assessment"], "observed");
        assert_eq!(unattached["participants"][0]["assessment"], "unattached");
        let attached: AuthorityValue =
            attach_with_clock(dir.path(), request(&file), &ClockHook::new()).unwrap();
        assert_stats(&attached, &huge);
        for member in [None, Some("member")] {
            let status =
                status_with_clock(dir.path(), "review", member, 1.0, &ClockHook::new()).unwrap();
            assert_eq!(status["assessment"], "observed");
            assert_eq!(status["participants"][0]["assessment"], "observed");
            assert!(!status.to_string().contains(&huge));
        }
        if unsafe_source {
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        } else {
            std::fs::remove_file(&file).unwrap();
        }
        let status = status_with_clock(dir.path(), "review", None, 1.0, &ClockHook::new()).unwrap();
        assert_eq!(status["assessment"], "observed");
        assert_eq!(status["participants"][0]["assessment"], "unavailable");
        let detached: AuthorityValue = detach(dir.path(), "review", "member", None).unwrap();
        assert_stats(&detached, &huge);
        assert!(matches!(
            authority_field(
                authority_field(&detached, "member").unwrap(),
                "live_attachment"
            )
            .unwrap(),
            AuthorityValue::Null
        ));
        let bytes = detached.compact().unwrap();
        assert!(
            std::str::from_utf8(&bytes)
                .unwrap()
                .contains(&format!("\"device\":{huge}"))
        );
        assert!(
            std::str::from_utf8(&bytes)
                .unwrap()
                .contains(&format!("\"inode\":{huge}"))
        );
    }
}

#[test]
fn python_b_attach_exact_authority_and_raw_result_bytes_preserve_metadata_order() {
    let huge = "1234567890".repeat(10);
    for stats in [None, Some(huge.as_str())] {
        let (dir, file) = workspace_with_stats(stats);
        let before = groups::read_groups(dir.path()).unwrap();
        let binding = authority_field(before.member("review", "member").unwrap(), "binding")
            .unwrap()
            .clone();
        // Generate the participant natively, then give Python these exact bytes:
        // no UUID, field order, binding, endpoint integer, or timestamp masking.
        groups::remove_member(dir.path(), "review", "member", None).unwrap();
        groups::add_member(
            dir.path(),
            groups::AddMember {
                group: "review",
                member: "member",
                role: "Reviewer",
                binding: &binding,
                rebind: false,
                expected_revision: None,
            },
        )
        .unwrap();
        let oracle = tempfile::tempdir().unwrap();
        std::fs::create_dir(oracle.path().join("state")).unwrap();
        std::fs::write(oracle.path().join("coven.json"), b"not parsed by groups").unwrap();
        let state_path = dir.path().join("state/groups.json");
        std::fs::write(
            oracle.path().join("state/groups.json"),
            std::fs::read(&state_path).unwrap(),
        )
        .unwrap();
        let script = r#"
import datetime,json,sys
from pathlib import Path
from unittest.mock import patch
from runner import djinn_live_observation as live,group_live
class Clock:
    def monotonic(self): return 0
    def time(self): return 1792861200.1
class FixedDateTime(datetime.datetime):
    @classmethod
    def now(cls,tz=None):
        return cls(2026,10,24,12,0,0,tzinfo=datetime.timezone.utc)
with patch.object(live,'time',Clock()),patch.object(group_live.datetime,'datetime',FixedDateTime):
    result=group_live.attach(Path(sys.argv[1]),'review','member',*sys.argv[2:],reattach=True)
sys.stdout.buffer.write((json.dumps(result,ensure_ascii=False,allow_nan=False,indent=2)+'\n').encode())
"#;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .find(|p| p.join("runner").is_dir())
            .unwrap();
        // The second identical attach must preserve existing metadata bytes too.
        for _ in 0..2 {
            let output = std::process::Command::new("python3")
                .args([
                    "-B",
                    "-c",
                    script,
                    oracle.path().to_str().unwrap(),
                    &file,
                    RUNTIME,
                    INSTANCE,
                    "selected",
                ])
                .current_dir(root)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let mut selected = request(&file);
            selected.reattach = true;
            let native = attach_with_clock(dir.path(), selected, &ClockHook::new()).unwrap();
            assert_eq!(native.storage().unwrap(), output.stdout);
            assert_eq!(
                std::fs::read(&state_path).unwrap(),
                std::fs::read(oracle.path().join("state/groups.json")).unwrap()
            );
            let member = authority_field(&native, "member").unwrap();
            let attachment = authority_field(member, "live_attachment").unwrap();
            assert_eq!(
                attachment
                    .as_object()
                    .unwrap()
                    .entries()
                    .iter()
                    .map(|(key, _)| key.as_str())
                    .collect::<Vec<_>>(),
                [
                    "adapter",
                    "source_contract",
                    "artifact_path",
                    "runtime_id",
                    "instance_id",
                    "session_id",
                    "attached_at"
                ]
            );
        }
    }
}
