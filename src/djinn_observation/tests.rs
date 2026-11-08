use super::*;
use crate::process::{ProbeResult, ProbeStatus};
use serde_json::{Value as Json, json};
use std::{cell::Cell, fs, process::Command};

fn fixture(folder: &str) -> Json {
    json!({
        "schema_version":1,"contract":"djinn.session.observation.v1",
        "selected_folder":folder,"resolved_folder":"/persisted/folder",
        "started_at":"2026-10-24T12:00:00Z","observed_at":"2026-10-24T12:00:01+00:00",
        "assessment":"observed","diagnostics":[],
        "capabilities":{"persisted_folder_events":true,"live_status":false,"ui_sessions":false,"native_store":false},
        "authorization":"none","dispatchable":false,"runtime_fact_freshness":"unknown",
        "native_lifecycle":"unestablished","job_completion":"unestablished","activity":"unknown",
        "observation":{
            "recorded_native_session_id":"agt_test_123","recorded_created_at":"2026-10-24T11:00:00Z",
            "sources":{"manifest":{"path":"djinn.toml","bytes":150,"sha256":"a".repeat(64)},
                "folder_events":{"path":"events.jsonl","status":"observed","bytes":400,"sha256":"b".repeat(64)}},
            "event_count":2,
            "last_event":{"kind":"session_lifecycle_updated","event_id":"evt-2","session_id":"agt_test_123","recorded_at":"2026-10-24T11:30:00Z"},
            "last_lifecycle":{"state":"completed","mode":"background","event_id":"evt-2","session_id":"agt_test_123","recorded_at":"2026-10-24T11:30:00Z"},
            "consistency":"stable_per_file_not_transactional"}})
}
fn decode_json(raw: &[u8], folder: &str) -> Json {
    match schema::decode(raw, folder) {
        Ok((assessment, facts, producer)) => json!({"assessment":assessment,
            "observation":serde_json::from_slice::<Json>(&facts.compact().unwrap()).unwrap(),
            "producer":serde_json::from_slice::<Json>(&producer.compact().unwrap()).unwrap()}),
        Err(error) => json!({"error":error.code()}),
    }
}
fn rejected(v: &Json) {
    assert_eq!(
        decode_json(&serde_json::to_vec(v).unwrap(), "/selected"),
        json!({"error":"schema_error"})
    );
}

#[test]
fn schema_differential_oracle_corpus() {
    let folder = "/selected";
    let base = fixture(folder);
    let mut cases = vec![base.clone()];
    // Remove every required envelope, fact, source and event field.
    for pointer in [
        "",
        "/observation",
        "/observation/sources/manifest",
        "/observation/sources/folder_events",
        "/observation/last_event",
        "/observation/last_lifecycle",
    ] {
        for key in base.pointer(pointer).unwrap().as_object().unwrap().keys() {
            let mut v = base.clone();
            v.pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key);
            cases.push(v);
        }
    }
    for (pointer, values) in [
        (
            "/schema_version",
            vec![json!(true), json!("1"), json!(1.0), json!(2), Json::Null],
        ),
        ("/contract", vec![json!("unknown"), Json::Null]),
        ("/selected_folder", vec![json!("/other"), Json::Null]),
        (
            "/resolved_folder",
            vec![
                json!("/a//b"),
                json!("/a/./b"),
                json!("///a"),
                json!("//a/b"),
                json!("/"),
                json!("relative"),
                Json::Null,
            ],
        ),
        (
            "/started_at",
            vec![
                json!("2026-02-30T12:00:00Z"),
                json!("2026-01-01t00:00:60.12345678901234567890z"),
                json!("0000-02-29T00:00:00+00:00"),
                json!("2026-01-01T00:00:00-00:00"),
                json!("2026-01-01T00:00:00+01:00"),
            ],
        ),
        ("/dispatchable", vec![json!(0), json!(true), Json::Null]),
        (
            "/diagnostics",
            vec![json!(["x"]), json!(["X"]), json!(vec!["x"; 17]), Json::Null],
        ),
        (
            "/observation/event_count",
            vec![
                json!(-1),
                json!(true),
                json!(1.0),
                json!(16385),
                json!(16384),
                json!(1),
                json!(0),
                Json::Null,
            ],
        ),
        (
            "/observation/last_lifecycle/state",
            vec![
                json!("done"),
                json!("created"),
                json!("running"),
                json!("paused"),
                json!("failed"),
                json!("cancelled"),
            ],
        ),
        (
            "/observation/last_lifecycle/mode",
            vec![json!("foreground"), json!("daemon"), Json::Null],
        ),
        (
            "/observation/last_event/event_id",
            vec![
                json!(""),
                json!("x\u{85}"),
                json!("x".repeat(257)),
                json!("different"),
                Json::Null,
            ],
        ),
        (
            "/observation/recorded_native_session_id",
            vec![
                json!("agt_other"),
                json!("agt_"),
                json!("agt_é"),
                Json::Null,
            ],
        ),
        (
            "/observation/recorded_created_at",
            vec![
                json!("2026-02-29T00:00:00Z"),
                json!("2024-02-29T23:59:60.1234567890123456789-23:59"),
                Json::Null,
            ],
        ),
        (
            "/observation/last_event/kind",
            vec![json!("message_recorded"), json!("Session"), json!("")],
        ),
        (
            "/observation/consistency",
            vec![json!("transactional"), Json::Null],
        ),
    ] {
        for val in values {
            let mut v = base.clone();
            *v.pointer_mut(pointer).unwrap() = val;
            cases.push(v);
        }
    }
    for (key, b) in [
        ("persisted_folder_events", true),
        ("live_status", false),
        ("ui_sessions", false),
        ("native_store", false),
    ] {
        for val in [json!(i32::from(b)), json!(!b), Json::Null, json!("true")] {
            let mut v = base.clone();
            v["capabilities"][key] = val;
            cases.push(v);
        }
    }
    for (key, cap) in [("manifest", 65536), ("folder_events", 4194304)] {
        for (field, values) in [
            (
                "bytes",
                vec![json!(-1), json!(true), json!(cap + 1), json!(cap), json!(0)],
            ),
            (
                "sha256",
                vec![
                    json!("A".repeat(64)),
                    json!("g".repeat(64)),
                    json!("a".repeat(63)),
                    Json::Null,
                ],
            ),
            ("path", vec![json!("/events.jsonl"), json!("../djinn.toml")]),
        ] {
            for val in values {
                let mut v = base.clone();
                v["observation"]["sources"][key][field] = val;
                cases.push(v);
            }
        }
    }
    for status in ["missing", "observed"] {
        let mut v = base.clone();
        v["observation"]["event_count"] = if status == "missing" {
            Json::Null
        } else {
            json!(0)
        };
        v["observation"]["last_event"] = Json::Null;
        v["observation"]["last_lifecycle"] = Json::Null;
        let e = &mut v["observation"]["sources"]["folder_events"];
        e["status"] = json!(status);
        e["bytes"] = if status == "missing" {
            Json::Null
        } else {
            json!(0)
        };
        e["sha256"] = if status == "missing" {
            Json::Null
        } else {
            json!("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        };
        cases.push(v);
    }
    let mut unavailable = base.clone();
    unavailable["assessment"] = json!("unavailable");
    unavailable["observation"] = Json::Null;
    unavailable["resolved_folder"] = Json::Null;
    unavailable["diagnostics"] = json!(["folder_missing"]);
    cases.push(unavailable);
    let mut payloads: Vec<String> = cases
        .iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect();
    payloads.extend(
        [
            "",
            "{} {}",
            "[]",
            "null",
            "{\"x\":1,\"x\":2}",
            "{\"extension\":{\"x\":1,\"x\":2}}",
            "{\"x\":NaN}",
            "{\"x\":Infinity}",
            "{\"x\":-Infinity}",
            "{\"x\":1e999}",
            "{\"x\":1e999,\"text\":\"NaN\"}",
        ]
        .map(str::to_owned),
    );
    payloads.push(format!("{}0{}", "[".repeat(33), "]".repeat(33)));
    for extension in [
        json!([]),
        json!("x".repeat(4097)),
        json!(vec![0; 4097]),
        json!({"path":"é".repeat(8192)}),
        json!({"path":"é".repeat(8193)}),
    ] {
        let mut v = base.clone();
        v["extension"] = extension;
        payloads.push(v.to_string());
    }
    // Unknown bigint must remain an integer; never deserialize through serde Value.
    payloads.push(format!(
        "{{\"extension\":{},{}",
        "9".repeat(4300),
        &base.to_string()[1..]
    ));
    let input: Vec<Json> = payloads
        .iter()
        .map(|p| json!({"payload":p,"folder":folder}))
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cases.json");
    fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
    let out = Command::new("python3")
        .args([
            "-B",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/djinn_observation/oracle.py"
            ),
        ])
        .arg(path)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("PYTHONPATH")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected: Vec<Json> = serde_json::from_slice(&out.stdout).unwrap();
    for (i, (p, e)) in payloads.iter().zip(expected).enumerate() {
        assert_eq!(
            decode_json(p.as_bytes(), folder),
            e,
            "oracle case {i}: {}",
            &p[..p.len().min(100)]
        );
    }
}

#[test]
fn unknown_extensions_are_validated_before_contract_and_projection() {
    let mut v = fixture("/selected");
    for pointer in [
        "",
        "/observation",
        "/observation/last_event",
        "/observation/last_lifecycle",
        "/observation/sources/manifest",
    ] {
        v.pointer_mut(pointer).unwrap()["transcript"] =
            json!({"secret":"PRIVATE_PRODUCER_TRANSCRIPT"});
    }
    let result = decode_json(&serde_json::to_vec(&v).unwrap(), "/selected");
    assert_eq!(result["assessment"], "observed");
    assert!(!result.to_string().contains("PRIVATE_PRODUCER_TRANSCRIPT"));
    v["contract"] = json!("unknown");
    v["extension"] = json!({"nested":"x".repeat(4097)});
    rejected(&v);
    assert_eq!(
        decode_json(b"\xff", "/selected"),
        json!({"error":"invalid_utf8"})
    );
    let mut deep = Json::Null;
    for _ in 0..31 {
        deep = json!([deep]);
    }
    let mut v = fixture("/selected");
    v["extension"] = deep.clone();
    assert_eq!(
        decode_json(v.to_string().as_bytes(), "/selected")["assessment"],
        "observed"
    );
    v["extension"] = json!([deep]);
    assert_eq!(
        decode_json(v.to_string().as_bytes(), "/selected")["error"],
        "nesting_limit"
    );
}

#[test]
fn wire_character_limits_and_native_finite_profile_are_explicit() {
    for (key, limit) in [
        ("extension", 4096),
        ("value", 8192),
        ("cwd", 8192),
        ("foreground_cwd", 8192),
        ("path", 8192),
        ("extension_path", 8192),
    ] {
        let mut v = fixture("/selected");
        // Array entries inherit their enclosing key's string limit.
        v[key] = json!(["é".repeat(limit)]);
        assert_eq!(
            decode_json(v.to_string().as_bytes(), "/selected")["assessment"],
            "observed"
        );
        v[key] = json!(["é".repeat(limit + 1)]);
        rejected(&v);
    }
    for n in [4096, 4097] {
        let mut v = fixture("/selected");
        let members: serde_json::Map<String, Json> =
            (0..n).map(|i| (format!("key{i}"), Json::Null)).collect();
        v["extension"] = Json::Object(members);
        let decoded = decode_json(v.to_string().as_bytes(), "/selected");
        if n == 4096 {
            assert_eq!(decoded["assessment"], "observed");
        } else {
            assert_eq!(decoded["error"], "schema_error");
        }
        let mut v = fixture("/selected");
        v["extension"] = json!({("é".repeat(n)):null});
        let decoded = decode_json(v.to_string().as_bytes(), "/selected");
        if n == 4096 {
            assert_eq!(decoded["assessment"], "observed");
        } else {
            assert_eq!(decoded["error"], "schema_error");
        }
    }
    let mut v = fixture("/selected");
    v["extension"] = json!(vec![vec![0; 4096]; 64]);
    assert_eq!(
        decode_json(v.to_string().as_bytes(), "/selected")["error"],
        "schema_error"
    );
    let payload = format!(
        "{{\"extension\":{},{}",
        "9".repeat(4301),
        &fixture("/selected").to_string()[1..]
    );
    assert_eq!(
        decode_json(payload.as_bytes(), "/selected")["error"],
        "schema_error"
    );
}

struct FakeRunner<F> {
    calls: Cell<usize>,
    run: F,
}
impl<F: Fn(&ProbeRequest) -> ProbeResult> ProbeRunner for FakeRunner<F> {
    fn run(&self, r: &ProbeRequest) -> ProbeResult {
        self.calls.set(self.calls.get() + 1);
        (self.run)(r)
    }
}
fn result(payload: &Json) -> ProbeResult {
    ProbeResult {
        status: ProbeStatus::Ok,
        returncode: Some(0),
        stdout: serde_json::to_vec(payload).unwrap(),
        stderr: vec![0xff, 0xfe],
        duration_ms: 0.0,
    }
}
#[cfg(unix)]
fn executable(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("producer");
    fs::write(&path, b"#!/bin/sh\nexit 99\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn output_json(out: &TaskOutput) -> Json {
    serde_json::from_str(&out.stdout).unwrap()
}

#[test]
fn selectors_and_limits_reject_without_probe() {
    let fake = FakeRunner {
        calls: Cell::new(0),
        run: |_: &ProbeRequest| panic!("no process"),
    };
    for s in [
        "",
        "relative",
        "/",
        "/a/",
        "/a/../b",
        "/a\n",
        "/a\u{7f}",
        "/a\u{85}",
        &format!("/{}", "é".repeat(2048)),
    ] {
        assert!(arguments_with_runner("/explicit", s, 1.0, 1024, true, &fake).is_err());
        assert!(arguments_with_runner(s, "/folder", 1.0, 1024, true, &fake).is_err());
    }
    for t in [0.0, -1.0, 31.0, f64::NAN, f64::INFINITY] {
        assert!(arguments_with_runner("/explicit", "/folder", t, 1024, true, &fake).is_err());
    }
    for cap in [1023, 1048577, -1] {
        assert!(arguments_with_runner("/explicit", "/folder", 1.0, cap, true, &fake).is_err());
    }
    assert_eq!(fake.calls.get(), 0);
    for s in ["/a//b/./c", "//a", "/.", &format!("/{}", "x".repeat(4095))] {
        schema::selector(s).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn one_fixed_probe_original_selector_safe_environment_ascii_and_uncertainty() {
    let dir = tempfile::tempdir().unwrap();
    let exe = executable(dir.path());
    let folder = "/absent//folder/./é";
    let payload = fixture(folder);
    let fake = FakeRunner {
        calls: Cell::new(0),
        run: |r: &ProbeRequest| {
            assert_eq!(r.executable, exe.canonicalize().unwrap());
            assert_eq!(
                r.arguments,
                vec!["session", "observe", folder, "--json"]
                    .into_iter()
                    .map(OsString::from)
                    .collect::<Vec<_>>()
            );
            assert_eq!(r.cwd, Path::new("/"));
            assert_eq!(r.timeout, std::time::Duration::from_secs(1));
            assert_eq!(r.max_output_bytes, 131072);
            assert_eq!(r.environment, identity::adapter_environment());
            for k in r.environment.keys() {
                assert!(
                    ["PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"]
                        .iter()
                        .any(|s| k == *s)
                );
            }
            result(&payload)
        },
    };
    for compact in [false, true] {
        let out = arguments_with_runner(exe.to_str().unwrap(), folder, 1.0, 131072, compact, &fake)
            .unwrap();
        assert_eq!(out.exit_code, 0);
        assert!(out.warnings.is_empty());
        assert!(out.stdout.is_ascii());
        assert!(out.stdout.contains("\\u00e9"));
        assert_eq!(out.stdout.contains('\n'), !compact);
        assert!(!out.stdout.ends_with('\n'));
        assert!(out.stdout.ends_with('}'));
        let v = output_json(&out);
        assert_eq!(v["authorization"], "none");
        assert_eq!(v["dispatchable"], false);
        for k in ["runtime_fact_freshness", "activity"] {
            assert_eq!(v[k], "unknown");
        }
        for k in ["native_lifecycle", "job_completion"] {
            assert_eq!(v[k], "unestablished");
        }
        assert_eq!(v["observation"], payload["observation"]);
        assert_eq!(v["limitations"], json!(LIMITATIONS));
    }
    assert_eq!(fake.calls.get(), 2);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn all_transport_statuses_and_combined_cap_precede_payload() {
    let dir = tempfile::tempdir().unwrap();
    let exe = executable(dir.path());
    for status in [
        ProbeStatus::Ok,
        ProbeStatus::Nonzero,
        ProbeStatus::Timeout,
        ProbeStatus::OutputLimit,
        ProbeStatus::SpawnError,
    ] {
        for overflow in [false, true] {
            let fake = FakeRunner {
                calls: Cell::new(0),
                run: |_: &ProbeRequest| ProbeResult {
                    status,
                    returncode: Some(9),
                    stdout: b"PRIVATE BAD PAYLOAD".to_vec(),
                    stderr: vec![0xff; if overflow { 1024 } else { 0 }],
                    duration_ms: 0.0,
                },
            };
            let out =
                arguments_with_runner(exe.to_str().unwrap(), "/selected", 1.0, 1024, true, &fake)
                    .unwrap();
            let expected = if overflow {
                "output_limit"
            } else {
                match status {
                    ProbeStatus::Timeout => "timeout",
                    ProbeStatus::OutputLimit => "output_limit",
                    ProbeStatus::SpawnError => "spawn_error",
                    _ => "producer_error",
                }
            };
            assert_eq!(output_json(&out)["diagnostics"], json!([expected]));
            assert_eq!(out.exit_code, 1);
            assert!(!out.stdout.contains("PRIVATE"));
            assert_eq!(fake.calls.get(), 1);
        }
    }
    let payload = fixture("/selected");
    let size = serde_json::to_vec(&payload).unwrap().len();
    for extra in [0, 1] {
        let fake = FakeRunner {
            calls: Cell::new(0),
            run: |_: &ProbeRequest| {
                let mut r = result(&payload);
                r.stderr = vec![0xff; 4096 - size + extra];
                r
            },
        };
        let out = arguments_with_runner(exe.to_str().unwrap(), "/selected", 1.0, 4096, true, &fake)
            .unwrap();
        assert_eq!(out.exit_code, if extra == 0 { 0 } else { 1 });
    }
}

#[cfg(unix)]
#[test]
fn native_unavailable_retains_metadata_and_contract_exits_two() {
    let dir = tempfile::tempdir().unwrap();
    let exe = executable(dir.path());
    for assessment in ["observed", "unavailable", "incompatible"] {
        let mut payload = fixture("/selected");
        if assessment == "unavailable" {
            payload["assessment"] = json!("unavailable");
            payload["observation"] = Json::Null;
            payload["diagnostics"] = json!(["folder_missing"]);
        }
        if assessment == "incompatible" {
            payload["contract"] = json!("v2");
        }
        let fake = FakeRunner {
            calls: Cell::new(0),
            run: |_: &ProbeRequest| result(&payload),
        };
        let out =
            arguments_with_runner(exe.to_str().unwrap(), "/selected", 1.0, 131072, true, &fake)
                .unwrap();
        let v = output_json(&out);
        assert_eq!(v["assessment"], assessment);
        assert_eq!(
            out.exit_code,
            match assessment {
                "observed" => 0,
                "unavailable" => 1,
                _ => 2,
            }
        );
        if assessment == "unavailable" {
            assert!(v["producer"].is_object());
            assert!(v["observation"].is_null());
            assert_eq!(v["diagnostics"], json!(["native_unavailable"]));
        }
    }
}

#[cfg(unix)]
#[test]
fn postprobe_content_rewrite_and_alias_retarget_dominate_every_status() {
    use std::os::unix::fs::symlink;
    for status in [
        ProbeStatus::Ok,
        ProbeStatus::Nonzero,
        ProbeStatus::Timeout,
        ProbeStatus::OutputLimit,
        ProbeStatus::SpawnError,
    ] {
        for alias_change in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let exe = executable(dir.path());
            let alias = dir.path().join("alias");
            symlink(&exe, &alias).unwrap();
            let replacement = dir.path().join("replacement");
            fs::copy(&exe, &replacement).unwrap();
            let fake = FakeRunner {
                calls: Cell::new(0),
                run: |_: &ProbeRequest| {
                    if alias_change {
                        fs::remove_file(&alias).unwrap();
                        symlink(&replacement, &alias).unwrap();
                    } else {
                        let metadata = fs::metadata(&exe).unwrap();
                        fs::write(&exe, vec![b'X'; metadata.len() as usize]).unwrap();
                        fs::File::options()
                            .write(true)
                            .open(&exe)
                            .unwrap()
                            .set_times(
                                fs::FileTimes::new().set_modified(metadata.modified().unwrap()),
                            )
                            .unwrap();
                    }
                    ProbeResult {
                        status,
                        returncode: Some(0),
                        stdout: vec![b'x'; 1025],
                        stderr: vec![],
                        duration_ms: 0.0,
                    }
                },
            };
            let out =
                arguments_with_runner(alias.to_str().unwrap(), "/selected", 1.0, 1024, true, &fake)
                    .unwrap();
            assert_eq!(
                output_json(&out)["diagnostics"],
                json!(["executable_changed"])
            );
            assert_eq!(fake.calls.get(), 1);
        }
    }
}

#[cfg(unix)]
#[test]
fn unavailable_executable_never_probes() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = executable(dir.path());
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o600)).unwrap();
    let fake = FakeRunner {
        calls: Cell::new(0),
        run: |_: &ProbeRequest| panic!("must not probe"),
    };
    for p in [exe, dir.path().to_owned(), dir.path().join("missing")] {
        let out = arguments_with_runner(p.to_str().unwrap(), "/selected", 1.0, 1024, true, &fake)
            .unwrap();
        assert_eq!(out.exit_code, 1);
        assert_eq!(
            output_json(&out)["diagnostics"],
            json!(["executable_unavailable"])
        );
    }
}

#[cfg(unix)]
#[test]
fn controlled_real_producer_supervisor_and_source_independence() {
    use std::os::unix::fs::PermissionsExt;
    // Unit tests are not supervisor bootstraps. Select the actual same-binary
    // private supervisor explicitly through the transport's test seam.
    struct Supervised(PathBuf);
    impl ProbeRunner for Supervised {
        fn run(&self, r: &ProbeRequest) -> ProbeResult {
            crate::process::run_with_supervisor(r, &self.0)
        }
    }
    let binary = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("coven");
    assert!(
        binary.is_file(),
        "build the native coven binary before unit supervisor tests"
    );
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("controlled producer");
    // There is deliberately no selected folder on disk. The adapter may only
    // consume the producer response, not inspect its alleged persisted sources.
    let folder = "/missing//folder/./é";
    let payload = fixture(folder).to_string();
    let script = format!(
        "#!/bin/sh\n[ \"$#\" = 4 ] || exit 91\n[ \"$1\" = session ] || exit 92\n[ \"$2\" = observe ] || exit 93\n[ \"$3\" = '{folder}' ] || exit 94\n[ \"$4\" = --json ] || exit 95\n[ \"$PWD\" = / ] || exit 96\nread ignored && exit 97\nprintf '%s' '{payload}'\nprintf '\\377' >&2\n"
    );
    fs::write(&exe, script).unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
    let before = fs::read(&exe).unwrap();
    let metadata = fs::metadata(&exe).unwrap();
    let runner = Supervised(binary);
    let out =
        arguments_with_runner(exe.to_str().unwrap(), folder, 3.0, 131072, true, &runner).unwrap();
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(
        output_json(&out)["observation"],
        fixture(folder)["observation"]
    );
    assert_eq!(fs::read(&exe).unwrap(), before);
    assert_eq!(
        fs::metadata(&exe).unwrap().modified().unwrap(),
        metadata.modified().unwrap()
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}
