use super::*;
use serde_json::json;
use std::{
    cell::Cell,
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

pub(crate) const RUNTIME: &str = "616fed3e-ae7f-41a0-99e5-80546d659d76";
pub(crate) const INSTANCE: &str = "e3c0e2ce-544c-4f1e-a489-5d3e0cf1978e";
pub(crate) const SAMPLED: u64 = 1_792_861_200_000;
pub(crate) fn fixture() -> Value {
    json!({"schema_version":1,"contract":NATIVE_CONTRACT,"runtime_id":RUNTIME,"sequence":1,
        "sampled_at_ms":SAMPLED,"valid_for_ms":3000,"source":"legacy_session_run_state",
        "activity_scope":"legacy_runner_ownership","capabilities":{"activity":true,"idle":false,
        "catalog_complete":false,"decision_state":false,"retry_state":false,"durable_lifecycle":false,"job_completion":false},
        "authorization":"none","dispatchable":false,"native_lifecycle":"unestablished","job_completion":"unestablished",
        "instances":[{"instance_id":INSTANCE,"sessions":[{"session_id":"selected","activity":"busy"}]}]})
}

struct TestClock {
    base: Instant,
    elapsed: Cell<u64>,
    wall: Cell<f64>,
    final_delay: u64,
}
impl TestClock {
    fn new(delay: u64) -> Self {
        Self {
            base: Instant::now(),
            elapsed: Cell::new(0),
            wall: Cell::new(SAMPLED as f64 + 100.0),
            final_delay: delay,
        }
    }
}
impl Clock for TestClock {
    fn monotonic(&self) -> Instant {
        self.base + Duration::from_millis(self.elapsed.get())
    }
    fn wall_ms(&self) -> f64 {
        self.wall.get()
    }
    fn timestamp(&self) -> Result<String> {
        self.elapsed.set(self.elapsed.get() + self.final_delay);
        Ok("2026-10-24T12:00:00+00:00".into())
    }
}

fn source() -> (tempfile::TempDir, Request) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.path().join(format!("{RUNTIME}.json"));
    std::fs::write(&path, serde_json::to_vec(&fixture()).unwrap()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    (
        dir,
        Request {
            file: path.to_str().unwrap().into(),
            runtime: RUNTIME.into(),
            instance: INSTANCE.into(),
            session: "selected".into(),
            timeout: 1.0,
        },
    )
}

#[test]
fn selected_only_busy_and_unknown_are_success_not_authority() {
    let (_dir, request) = source();
    for activity in ["busy", "unknown"] {
        let mut value = fixture();
        value["instances"][0]["sessions"][0]["activity"] = json!(activity);
        value["instances"][0]["sessions"]
            .as_array_mut()
            .unwrap()
            .push(json!({"session_id":"PRIVATE_OTHER_ID","activity":"unknown"}));
        std::fs::write(&request.file, serde_json::to_vec(&value).unwrap()).unwrap();
        let report = inspect_with_clock(&request, &TestClock::new(0)).unwrap();
        assert_eq!(report["assessment"], "observed");
        assert_eq!(report["activity"], activity);
        assert_eq!(report["expires_in_ms"], 2900);
        assert_eq!(report["dispatchable"], false);
        assert!(!report.to_string().contains("PRIVATE_OTHER_ID"));
        assert_eq!(report["source"].as_object().unwrap().len(), 10);
    }
}

#[test]
fn final_clock_covers_post_read_work_and_never_uses_mtime() {
    let (_dir, request) = source();
    let report = inspect_with_clock(&request, &TestClock::new(100)).unwrap();
    assert_eq!(report["expires_in_ms"], 2700);
    assert_eq!(
        inspect_with_clock(&request, &TestClock::new(500)).unwrap()["diagnostics"],
        json!(["timeout"])
    );
    for (wall, code) in [
        (SAMPLED as f64 - 1.0, "future_sample"),
        (SAMPLED as f64 + 3000.0, "stale_sample"),
    ] {
        let clock = TestClock::new(0);
        clock.wall.set(wall);
        let report = inspect_with_clock(&request, &clock).unwrap();
        assert_eq!(report["diagnostics"], json!([code]));
        assert!(report["source"].is_null());
        assert_eq!(report["expires_in_ms"], 0);
    }
}

#[test]
fn late_wall_clock_work_counts_towards_deadline_and_clock_errors_are_safe_values() {
    let (_dir, request) = source();
    struct WallDelay(TestClock);
    impl Clock for WallDelay {
        fn monotonic(&self) -> Instant {
            self.0.monotonic()
        }
        fn wall_ms(&self) -> f64 {
            self.0.elapsed.set(1000);
            self.0.wall_ms()
        }
        fn timestamp(&self) -> Result<String> {
            self.0.timestamp()
        }
    }
    let report = inspect_with_clock(&request, &WallDelay(TestClock::new(0))).unwrap();
    assert_eq!(report["diagnostics"], json!(["timeout"]));
    struct FailedTimestamp(TestClock);
    impl Clock for FailedTimestamp {
        fn monotonic(&self) -> Instant {
            self.0.monotonic()
        }
        fn wall_ms(&self) -> f64 {
            self.0.wall_ms()
        }
        fn timestamp(&self) -> Result<String> {
            Err(Error("private_clock_failure"))
        }
    }
    let report = inspect_with_clock(&request, &FailedTimestamp(TestClock::new(0))).unwrap();
    assert_eq!(report["diagnostics"], json!(["io_error"]));
    assert!(report["source"].is_null());
    assert!(!report.to_string().contains("private_clock_failure"));
}

#[test]
fn caller_errors_are_errors_before_any_artifact_access() {
    let (_, mut request) = source(); // Deliberately remove source before validation.
    for timeout in [0.0, -1.0, 1.01, f64::NAN, f64::INFINITY] {
        request.timeout = timeout;
        assert_eq!(
            inspect(&request).unwrap_err().code(),
            "invalid_live_arguments"
        );
    }
    request.timeout = 1.0;
    for invalid in [
        "relative",
        "/a/../b",
        "/a\n.json",
        "/a\u{85}.json",
        "/wrong.json",
    ] {
        request.file = invalid.into();
        assert!(inspect(&request).is_err());
    }
}

#[test]
fn closed_whole_schema_types_duplicates_bounds_and_unselected_corruption() {
    let (_dir, request) = source();
    let original = fixture();
    let mut cases = Vec::new();
    for key in original.as_object().unwrap().keys() {
        let mut v = original.clone();
        v.as_object_mut().unwrap().remove(key);
        cases.push(v);
    }
    for (key, v) in [
        ("sequence", json!(true)),
        ("sequence", json!(0)),
        ("sequence", json!(9_007_199_254_740_992_u64)),
        ("sampled_at_ms", json!(-1)),
        ("sampled_at_ms", json!(1.0)),
        ("dispatchable", json!(0)),
        ("valid_for_ms", json!(3000.0)),
    ] {
        let mut value = original.clone();
        value[key] = v;
        cases.push(value);
    }
    for key in original["capabilities"].as_object().unwrap().keys() {
        let mut v = original.clone();
        v["capabilities"][key] = json!(i32::from(key == "activity"));
        cases.push(v);
    }
    let mut v = original.clone();
    v["instances"]
        .as_array_mut()
        .unwrap()
        .push(json!({"instance_id":INSTANCE,"sessions":[]}));
    cases.push(v);
    let mut v = original.clone();
    v["instances"][0]["sessions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"session_id":"other","activity":"idle"}));
    cases.push(v);
    let mut v = original.clone();
    v["instances"][0]["sessions"] = json!(
        (0..129)
            .map(|i| json!({"session_id":format!("s{i}"),"activity":"busy"}))
            .collect::<Vec<_>>()
    );
    cases.push(v);
    for value in cases {
        std::fs::write(&request.file, serde_json::to_vec(&value).unwrap()).unwrap();
        let report = inspect_with_clock(&request, &TestClock::new(0)).unwrap();
        assert_ne!(report["assessment"], "observed", "{value}");
        assert!(report["source"].is_null());
    }
    for (bytes, code) in [
        (b"{\"x\":1,\"x\":2}".as_slice(), "duplicate_key"),
        (b"[NaN]", "nonfinite_json"),
        (b"\xff", "invalid_utf8"),
        (b"{}\n{}", "invalid_json"),
    ] {
        std::fs::write(&request.file, bytes).unwrap();
        assert_eq!(
            inspect_with_clock(&request, &TestClock::new(0)).unwrap()["diagnostics"],
            json!([code])
        );
    }
}

#[test]
fn python_b_closed_oracle_schema_and_observer_semantics() {
    // Python is a test oracle only. No application startup, source discovery, or provider.
    let (_dir, request) = source();
    let script = r#"
import json,sys
from unittest.mock import patch
from runner import djinn_live_observation as live
class Clock:
    def monotonic(self): return 0
    def time(self): return 1792861200.1
with patch.object(live, 'time', Clock()):
    print(json.dumps(live.inspect(*sys.argv[1:])))
"#;
    let mut cases = Vec::new();
    for activity in ["busy", "unknown", "idle"] {
        let mut value = fixture();
        value["instances"][0]["sessions"][0]["activity"] = json!(activity);
        cases.push(value);
    }
    for (key, value) in [
        ("schema_version", json!(2)),
        ("schema_version", json!(true)),
        ("schema_version", json!(1.0)),
        ("contract", json!("future")),
        ("contract", Value::Null),
        ("runtime_id", json!(INSTANCE)),
        ("sampled_at_ms", json!(SAMPLED + 101)),
        ("sampled_at_ms", json!(SAMPLED - 2900)),
        ("sequence", json!(true)),
        ("sequence", json!(9_007_199_254_740_992_u64)),
        ("instances", json!([])),
    ] {
        let mut case = fixture();
        case[key] = value;
        cases.push(case);
    }
    let mut case = fixture();
    case["instances"][0]["sessions"] = json!([]);
    cases.push(case);
    let mut case = fixture();
    case["instances"][0]["sessions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"session_id":"other","activity":"idle"}));
    cases.push(case);
    for value in cases {
        std::fs::write(&request.file, serde_json::to_vec(&value).unwrap()).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .find(|p| p.join("runner").is_dir())
            .unwrap();
        let output = std::process::Command::new("python3")
            .args([
                "-B",
                "-c",
                script,
                &request.file,
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
        let mut python: Value = serde_json::from_slice(&output.stdout).unwrap();
        let mut native = inspect_with_clock(&request, &TestClock::new(0)).unwrap();
        for field in ["started_at", "observed_at", "duration_ms"] {
            python.as_object_mut().unwrap().remove(field);
            native.as_object_mut().unwrap().remove(field);
        }
        assert_eq!(native, python);
    }
    // Policy failures preserve the same checked file metadata without decoding.
    for mode in [0o600, 0o640] {
        std::fs::write(&request.file, b"{}").unwrap();
        std::fs::set_permissions(&request.file, std::fs::Permissions::from_mode(mode)).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .find(|p| p.join("runner").is_dir())
            .unwrap();
        let output = std::process::Command::new("python3")
            .args([
                "-B",
                "-c",
                script,
                &request.file,
                RUNTIME,
                INSTANCE,
                "selected",
            ])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success());
        let python: Value = serde_json::from_slice(&output.stdout).unwrap();
        let native = inspect_with_clock(&request, &TestClock::new(0)).unwrap();
        for key in [
            "file",
            "assessment",
            "diagnostics",
            "activity",
            "source",
            "expires_in_ms",
        ] {
            assert_eq!(native[key], python[key]);
        }
    }
}

#[test]
fn closed_source_rejects_oversized_integers_including_unknown_fields_as_schema_faults() {
    let (_dir, request) = source();
    let huge = "1234567890".repeat(10);
    for field in ["sequence", "sampled_at_ms", "unexpected_integer"] {
        let mut source = crate::values::parse(
            &serde_json::to_vec(&fixture()).unwrap(),
            crate::values::Profile::Authority,
        )
        .unwrap();
        source
            .set_field(field, crate::values::Value::integer(&huge).unwrap())
            .unwrap();
        std::fs::write(&request.file, source.compact().unwrap()).unwrap();
        let report = inspect_with_clock(&request, &TestClock::new(0)).unwrap();
        assert_eq!(report["assessment"], "unavailable");
        assert_eq!(report["diagnostics"], json!(["schema_error"]));
        assert!(report["source"].is_null());
        assert_eq!(report["expires_in_ms"], 0);
    }
}
