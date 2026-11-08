use super::*;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

fn endpoint() -> observation_socket::Endpoint {
    observation_socket::Endpoint {
        selected: "/private/api.sock".into(),
        resolved: "/private/api.sock".into(),
        device: 1,
        inode: 2,
    }
}
fn mapping() -> Value {
    json!({"pane_id":"opaque:p","terminal_id":"terminal","workspace_id":"workspace","tab_id":"tab"})
}
fn generic() -> Value {
    json!({"version":"test","protocol":18,"panes":[mapping()]})
}
fn strict() -> Value {
    let mut p = mapping();
    p["focused"] = json!(false);
    p["agent_status"] = json!("done");
    p["revision"] = json!(4);
    p["agent"] = json!("djinn");
    p["agent_session"] =
        json!({"source":"native","agent":"djinn","kind":"id","value":"private-id"});
    json!({"version":"test","protocol":18,"panes":[p.clone()],"agents":[p],"workspaces":[],"tabs":[],"layouts":[]})
}
fn snapshot(s: Value) -> Value {
    json!({"type":"session_snapshot","snapshot":s})
}
fn process(argv: Value) -> Value {
    json!({"type":"pane_process_info","process_info":{"pane_id":"opaque:p","foreground_processes":[{"pid":7,"name":"unused","argv":argv}]}})
}
fn reply(result: Value) -> observation_socket::Reply {
    observation_socket::Reply {
        payload: serde_json::to_vec(&json!({"id":"test","result":result})).unwrap(),
        request_id: "test".into(),
        endpoint: endpoint(),
    }
}
struct Fake {
    replies: RefCell<VecDeque<Value>>,
    calls: RefCell<Vec<ReadMethod>>,
    replacement: Cell<bool>,
}
impl Fake {
    fn new(values: Vec<Value>) -> Self {
        Self {
            replies: RefCell::new(values.into()),
            calls: RefCell::new(vec![]),
            replacement: Cell::new(false),
        }
    }
}
impl Transport for Fake {
    fn resolve(&self, _: &str) -> WireResult<String> {
        Ok(endpoint().resolved)
    }
    fn select(&self, _: &str) -> WireResult<observation_socket::Endpoint> {
        let mut e = endpoint();
        if self.replacement.get() {
            e.inode += 1;
        }
        Ok(e)
    }
    fn read(
        &self,
        _: &str,
        method: ReadMethod,
        _: &Value,
        _: Instant,
        budget: &mut ReadBudget,
        _: &dyn Cancel,
    ) -> WireResult<observation_socket::Reply> {
        self.calls.borrow_mut().push(method);
        let r = reply(
            self.replies
                .borrow_mut()
                .pop_front()
                .expect("unexpected read"),
        );
        budget.charge(r.payload.len() + 1)?;
        Ok(r)
    }
}
struct TestClock {
    now: Cell<Instant>,
    stamp_calls: Cell<usize>,
    expire_on_stamp: Option<usize>,
}
impl TestClock {
    fn new() -> Self {
        Self {
            now: Cell::new(Instant::now()),
            stamp_calls: Cell::new(0),
            expire_on_stamp: None,
        }
    }
}
impl Clock for TestClock {
    fn monotonic(&self) -> Instant {
        self.now.get()
    }
    fn wall_ms(&self) -> f64 {
        0.0
    }
    fn timestamp(&self) -> crate::Result<String> {
        let n = self.stamp_calls.get() + 1;
        self.stamp_calls.set(n);
        if self.expire_on_stamp == Some(n) {
            self.now.set(self.now.get() + Duration::from_secs(60));
        }
        Ok("2026-11-05T00:00:00+00:00".into())
    }
}
fn request(process: bool) -> Request {
    Request {
        socket: endpoint().selected,
        process_pane: process.then(|| "opaque:p".into()),
        timeout: 3.0,
        max_response_bytes: 1048576,
    }
}
fn run_process(values: Vec<Value>) -> Value {
    inspect_with(&request(true), &TestClock::new(), &Fake::new(values)).unwrap()
}

#[test]
fn anchor01_strict_inventory_and_native_projection() {
    let report = inspect_with(
        &request(false),
        &TestClock::new(),
        &Fake::new(vec![snapshot(strict())]),
    )
    .unwrap();
    assert_eq!(report["assessment"], "observed");
    assert_eq!(report["agents"][0]["activity"], "idle");
    assert_eq!(report["agents"][0]["attention"], "unacknowledged");
    assert_eq!(report["agents"][0]["native_session"]["value"], "private-id");
    let mut s = strict();
    s["agents"][0]["terminal_id"] = json!("other");
    assert_eq!(
        schema::snapshot(&reply(snapshot(s)), false)
            .unwrap_err()
            .code,
        "schema_error"
    );
}
#[test]
fn anchor02_generic_strict_exact_invalid_annotation_differences() {
    let mut s = generic();
    s["annotations"] = Value::Null;
    s["agents"] = json!({"malformed":true});
    let r = reply(snapshot(s.clone()));
    assert!(schema::snapshot(&r, false).is_err());
    let g = schema::snapshot(&r, true).unwrap();
    assert!(matches!(g.annotations, Some(crate::values::Value::Null)));
    let values = vec![
        snapshot(s.clone()),
        process(json!(["djinn"])),
        snapshot(s.clone()),
    ];
    assert_eq!(run_process(values)["assessment"], "observed");
    let f = Fake::new(vec![snapshot(s)]);
    let c = TestClock::new();
    assert_eq!(
        bindings::publication_with(
            "/private/api.sock",
            c.monotonic() + Duration::from_secs(3),
            &mut ReadBudget::new(10000),
            true,
            &NoCancel,
            &c,
            &f
        )
        .unwrap_err()
        .code,
        "schema_error"
    );
}
#[test]
fn anchor03_mixed_projection_never_falls_back() {
    for pure_process in [false, true] {
        let f = Fake::new(vec![snapshot(generic())]);
        let c = TestClock::new();
        assert_eq!(
            bindings::publication_with(
                "/private/api.sock",
                c.monotonic() + Duration::from_secs(3),
                &mut ReadBudget::new(10000),
                pure_process,
                &NoCancel,
                &c,
                &f
            )
            .is_ok(),
            pure_process
        );
        assert_eq!(f.calls.borrow().len(), 1);
    }
}
#[test]
fn anchor04_global_wire_ignored_fields() {
    let s = serde_json::to_string(&snapshot(generic())).unwrap();
    let prefix = format!("{{\"id\":\"test\",\"result\":{s},\"ignored\":");
    for (suffix, code) in [
        ("{\"x\":1,\"x\":2}}".to_owned(), "duplicate_key"),
        ("NaN}".into(), "nonfinite_json"),
        (
            format!("{}0{}}}", "[".repeat(33), "]".repeat(33)),
            "nesting_limit",
        ),
        (format!("\"{}\"}}", "x".repeat(4097)), "schema_error"),
    ] {
        let mut r = reply(snapshot(generic()));
        r.payload = format!("{prefix}{suffix}").into_bytes();
        assert_eq!(schema::snapshot(&r, true).unwrap_err().code, code);
    }
    let mut r = reply(snapshot(generic()));
    r.payload = format!("{prefix}{}{}", "9".repeat(400), "}").into_bytes();
    assert!(schema::snapshot(&r, true).is_ok());
}
#[test]
fn anchor05_opaque_ids_global_uniqueness() {
    let mut s = generic();
    s["panes"][0]["pane_id"] = json!("\u{e9}:opaque");
    assert!(schema::snapshot(&reply(snapshot(s.clone())), true).is_ok());
    for bad in ["", "has space", "tab\t", "\u{200b}"] {
        s["panes"][0]["pane_id"] = json!(bad);
        assert!(schema::snapshot(&reply(snapshot(s.clone())), true).is_err());
    }
    let mut s = generic();
    s["panes"].as_array_mut().unwrap().push(mapping());
    assert!(schema::snapshot(&reply(snapshot(s)), true).is_err());
}
#[test]
fn anchor06_process_basename_privacy_and_no_fallback() {
    for (argv, recognized) in [
        (json!(["/private/djinn", "secret"]), true),
        (json!(["djinn-ui"]), true),
        (json!(["node", "djinn"]), false),
        (json!(["DJINN"]), false),
        (json!(["djinn "]), false),
        (Value::Null, false),
    ] {
        let r = run_process(vec![
            snapshot(generic()),
            process(argv),
            snapshot(generic()),
        ]);
        assert_eq!(r["assessment"], "observed");
        assert_eq!(
            r["observations"][0]["recognition"],
            if recognized {
                "djinn_executable_family"
            } else {
                "unknown"
            }
        );
        let output = r.to_string();
        assert!(!output.contains("secret"));
        assert!(!output.contains("/private/djinn"));
        assert!(!output.contains("unused"));
    }
}
#[test]
fn anchor07_process_pids_and_argv_validation() {
    for pid in [
        json!(0),
        json!(-1),
        json!(4294967296u64),
        json!(true),
        json!(1.0),
    ] {
        let mut p = process(json!(["djinn"]));
        p["process_info"]["foreground_processes"][0]["pid"] = pid;
        assert!(schema::processes(&reply(p), "opaque:p").is_err());
    }
    let mut p = process(json!(["djinn"]));
    let duplicate = p["process_info"]["foreground_processes"][0].clone();
    p["process_info"]["foreground_processes"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert!(schema::processes(&reply(p), "opaque:p").is_err());
    assert!(schema::processes(&reply(process(json!(["djinn", "nul\0"]))), "opaque:p").is_err());
}
#[test]
fn anchor08_mapping_all_four_and_expected_terminal_before_query() {
    for k in schema::LOCATORS {
        let mut after = generic();
        after["panes"][0][k] = json!("changed");
        let r = run_process(vec![
            snapshot(generic()),
            process(json!(["djinn"])),
            snapshot(after),
        ]);
        assert_eq!(r["assessment"], "unavailable");
        assert_eq!(r["observations"], json!([]));
    }
    let f = Fake::new(vec![snapshot(generic())]);
    let c = TestClock::new();
    let err = bindings::process_binding_with(
        ProcessRequest {
            socket: "/private/api.sock",
            pane: "opaque:p",
            deadline: c.monotonic() + Duration::from_secs(3),
            frozen_endpoint: None,
            expected_terminal: Some("other"),
        },
        &mut ReadBudget::new(10000),
        &NoCancel,
        &c,
        &f,
    )
    .unwrap_err();
    assert_eq!(err.code, "pane_mapping_changed");
    assert_eq!(f.calls.borrow().len(), 1);
}
#[test]
fn anchor09_byte_budget_aggregate_includes_lf() {
    let values = vec![
        snapshot(generic()),
        process(json!(["djinn"])),
        snapshot(generic()),
    ];
    let bytes: usize = values
        .iter()
        .map(|v| reply(v.clone()).payload.len() + 1)
        .sum();
    for (budget, ok) in [(bytes, true), (bytes - 1, false)] {
        let f = Fake::new(values.clone());
        let c = TestClock::new();
        let e = endpoint();
        assert_eq!(
            evidence(
                &ProcessRequest {
                    socket: &e.selected,
                    pane: "opaque:p",
                    deadline: c.monotonic() + Duration::from_secs(3),
                    frozen_endpoint: None,
                    expected_terminal: None
                },
                &e,
                &mut ReadBudget::new(budget),
                &NoCancel,
                &c,
                &f
            )
            .is_ok(),
            ok
        );
    }
}
#[test]
fn anchor10_deadline_final_postvalidation_guard() {
    let mut c = TestClock::new();
    c.expire_on_stamp = Some(1);
    let f = Fake::new(vec![snapshot(generic())]);
    assert_eq!(
        bindings::publication_with(
            "/private/api.sock",
            c.monotonic() + Duration::from_secs(3),
            &mut ReadBudget::new(10000),
            true,
            &NoCancel,
            &c,
            &f
        )
        .unwrap_err()
        .code,
        "timeout"
    );
}
#[test]
fn anchor11_publication_descriptor_and_bindings() {
    for sources in [json!(["coven", "coven"]), json!(["bad source"]), json!([1])] {
        let mut s = generic();
        s["annotations"] = json!({"epoch":"e","sources":sources});
        let f = Fake::new(vec![snapshot(s)]);
        let c = TestClock::new();
        assert!(
            bindings::publication_with(
                "/private/api.sock",
                c.monotonic() + Duration::from_secs(3),
                &mut ReadBudget::new(10000),
                true,
                &NoCancel,
                &c,
                &f
            )
            .is_err()
        );
    }
    let mut s = strict();
    s["annotations"] = json!({"epoch":"e","sources":["coven"],"valid_until_unix_ms":1});
    let f = Fake::new(vec![snapshot(s)]);
    let c = TestClock::new();
    let s = bindings::publication_with(
        "/private/api.sock",
        c.monotonic() + Duration::from_secs(3),
        &mut ReadBudget::new(10000),
        false,
        &NoCancel,
        &c,
        &f,
    )
    .unwrap();
    assert_eq!(s["valid_until_unix_ms"], false);
    assert_eq!(
        binding_from_snapshot(&s, "opaque:p").unwrap()["evidence"]["native_binding"],
        "observed_reference"
    );
}
#[test]
fn anchor12_process_binding_canonical_order_and_frozen_endpoint() {
    let mut p = process(json!(["djinn"]));
    let mut second = p["process_info"]["foreground_processes"][0].clone();
    second["pid"] = json!(2);
    p["process_info"]["foreground_processes"]
        .as_array_mut()
        .unwrap()
        .push(second);
    let f = Fake::new(vec![snapshot(generic()), p, snapshot(generic())]);
    let c = TestClock::new();
    let id = identity(&endpoint()).unwrap();
    let b = bindings::process_binding_with(
        ProcessRequest {
            socket: "/private/api.sock",
            pane: "opaque:p",
            deadline: c.monotonic() + Duration::from_secs(3),
            frozen_endpoint: Some(&id),
            expected_terminal: Some("terminal"),
        },
        &mut ReadBudget::new(10000),
        &NoCancel,
        &c,
        &f,
    )
    .unwrap();
    assert_eq!(b["evidence"]["process_evidence"]["matches"][0]["pid"], 2);
    f.replacement.set(true);
    assert_eq!(
        bindings::process_binding_with(
            ProcessRequest {
                socket: "/private/api.sock",
                pane: "opaque:p",
                deadline: c.monotonic() + Duration::from_secs(3),
                frozen_endpoint: Some(&id),
                expected_terminal: None
            },
            &mut ReadBudget::new(10000),
            &NoCancel,
            &c,
            &f
        )
        .unwrap_err()
        .code,
        "endpoint_replaced"
    );
}
#[test]
fn anchor13_caller_errors_and_protocol_assessments() {
    let f = Fake::new(vec![]);
    let c = TestClock::new();
    let mut r = request(false);
    r.timeout = f64::NAN;
    assert!(inspect_with(&r, &c, &f).is_err());
    assert!(f.calls.borrow().is_empty());
    let mut s = generic();
    s["protocol"] = json!(19);
    let report = run_process(vec![snapshot(s)]);
    assert_eq!(report["assessment"], "incompatible");
}

#[cfg(unix)]
#[test]
fn native_api_socket_fixture() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("api.sock");
    let listener = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let thread = std::thread::spawn(move || {
        for result in [
            snapshot(generic()),
            process(json!(["/private/djinn", "secret"])),
            snapshot(generic()),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            let mut bytes =
                serde_json::to_vec(&json!({"id":request["id"],"result":result})).unwrap();
            bytes.push(b'\n');
            stream.write_all(&bytes).unwrap();
        }
    });
    let mut r = request(true);
    r.socket = path.to_str().unwrap().into();
    let report = inspect(&r).unwrap();
    thread.join().unwrap();
    assert_eq!(report["assessment"], "observed");
    assert_eq!(
        report["observations"][0]["recognition"],
        "djinn_executable_family"
    );
    assert!(!report.to_string().contains("secret"));
}

#[cfg(unix)]
#[test]
fn native_strict_api_socket_fixture() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("api.sock");
    let listener = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], "session.snapshot");
        assert_eq!(request["params"], json!({}));
        let mut bytes =
            serde_json::to_vec(&json!({"id":request["id"],"result":snapshot(strict())})).unwrap();
        bytes.push(b'\n');
        stream.write_all(&bytes).unwrap();
    });
    let mut r = request(false);
    r.socket = path.to_str().unwrap().into();
    let report = inspect(&r).unwrap();
    thread.join().unwrap();
    assert_eq!(report["assessment"], "observed");
    assert_eq!(report["pane_count"], 1);
    assert_eq!(report["agents"][0]["native_session"]["value"], "private-id");
    assert_eq!(report["agents"][0]["attention"], "unacknowledged");
}

#[test]
fn python_b_parent_oracle_contract_and_process_parity() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut cases = vec![];
    for mut s in [strict(), generic()] {
        for malformed in [false, true] {
            if malformed {
                s["agents"] = json!("malformed");
                s["annotations"] = Value::Null;
            }
            for generic in [false, true] {
                cases.push(
                    json!({"kind":"snapshot","generic":generic,"result":snapshot(s.clone())}),
                );
            }
        }
    }
    for argv in [
        json!(["/a/djinn", "secret"]),
        json!(["djinn-ui"]),
        json!(["node", "djinn"]),
        json!(["djinn/"]),
        json!(["DJINN"]),
        json!(null),
        json!(["djinn", "\0"]),
    ] {
        cases.push(json!({"kind":"process","result":process(argv)}));
    }
    let mut bad = strict();
    bad["agents"][0]["revision"] = json!(1.0);
    cases.push(json!({"kind":"snapshot","generic":false,"result":snapshot(bad)}));
    let expected: Vec<Value> = cases
        .iter()
        .map(|case| {
            let r = reply(case["result"].clone());
            if case["kind"] == "process" {
                match schema::processes(&r, "opaque:p") {
                    Ok(v) => json!(v),
                    Err(e) => json!({"error":e.code}),
                }
            } else {
                match schema::snapshot(&r, case["generic"].as_bool().unwrap()) {
                    Ok(_) => json!("ok"),
                    Err(e) => json!({"error":e.code}),
                }
            }
        })
        .collect();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|path| path.join("runner/kitsune_observation.py").is_file())
        .expect("checkout observation oracle")
        .canonicalize()
        .unwrap();
    let mut child = Command::new("python3")
        .args([
            "-B",
            "-c",
            r#"
import json,sys
sys.path.insert(0,sys.argv[1])
from runner import kitsune_observation as o, kitsune_process_observation as p
out=[]
for case in json.load(sys.stdin):
    payload=json.dumps({'id':'test','result':case['result']}).encode()
    try:
        if case['kind']=='process': value=p._processes(payload,'test','opaque:p')
        else:
            (o._pane_snapshot if case['generic'] else o._snapshot)(payload,'test')
            value='ok'
        out.append(value)
    except o._Failure as e: out.append({'error':e.code})
json.dump(out,sys.stdout)
"#,
        ])
        .arg(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&cases).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!(expected)
    );
}

fn reply_with_native_integer(result: Value, integer: &str) -> observation_socket::Reply {
    let mut r = reply(result);
    r.payload = String::from_utf8(r.payload)
        .unwrap()
        .replace("\"WIRE_INTEGER\"", integer)
        .into_bytes();
    r
}

#[test]
fn native_projection_ignores_large_integer_extensions_at_every_consumed_boundary() {
    for integer in ["18446744073709551616".to_owned(), "9".repeat(4000)] {
        let mut s = strict();
        s["ignored"] = json!("WIRE_INTEGER");
        s["$serde_json::private::Number"] = json!("WIRE_INTEGER");
        s["panes"][0]["ignored"] = json!("WIRE_INTEGER");
        s["agents"][0]["agent_session"]["ignored"] = json!("WIRE_INTEGER");
        s["layouts"] = json!([{"workspace_id":"w","tab_id":"t","focused_pane_id":"p","zoomed":false,
            "area":{"ignored":"WIRE_INTEGER"},"panes":[{"ignored":"WIRE_INTEGER"}],"splits":[{"ignored":"WIRE_INTEGER"}]}]);
        s["workspaces"] = json!([{"workspace_id":"w","focused":false,"number":1,"pane_count":1,"label":"w",
            "agent_status":"idle","tab_count":1,"active_tab_id":"t","ignored":"WIRE_INTEGER"}]);
        s["annotations"] = json!({"epoch":"e","sources":["coven"],"valid_until_unix_ms":true,
            "ignored":"WIRE_INTEGER","$serde_json::private::Number":"WIRE_INTEGER"});
        for generic in [false, true] {
            let s = schema::snapshot(
                &reply_with_native_integer(snapshot(s.clone()), &integer),
                generic,
            )
            .unwrap();
            assert!(s.get("ignored").is_none());
            let a = schema::publication_annotations(s.annotations.as_ref().unwrap()).unwrap();
            assert_eq!(
                a,
                json!({"epoch":"e","sources":["coven"],"valid_until_unix_ms":true})
            );
        }
        let mut p = process(json!(["djinn"]));
        p["ignored"] = json!("WIRE_INTEGER");
        p["process_info"]["ignored"] = json!("WIRE_INTEGER");
        p["process_info"]["foreground_processes"][0]["ignored"] = json!("WIRE_INTEGER");
        let matches =
            schema::processes(&reply_with_native_integer(p, &integer), "opaque:p").unwrap();
        assert_eq!(matches[0]["pid"], 7);
    }
}

#[test]
fn native_known_large_integers_are_not_hidden_or_rounded() {
    for integer in ["18446744073709551616".to_owned(), "9".repeat(4000)] {
        for key in ["protocol", "revision", "pane_id"] {
            let mut s = strict();
            match key {
                "protocol" => s[key] = json!("WIRE_INTEGER"),
                _ => s["panes"][0][key] = json!("WIRE_INTEGER"),
            }
            let r = reply_with_native_integer(snapshot(s), &integer);
            assert_eq!(
                schema::snapshot(&r, false).unwrap_err().code,
                "schema_error"
            );
            if key != "revision" {
                assert_eq!(schema::snapshot(&r, true).unwrap_err().code, "schema_error");
            }
        }
        for key in ["pid", "shell_pid", "foreground_process_group_id"] {
            let mut p = process(json!(["djinn"]));
            if key == "pid" {
                p["process_info"]["foreground_processes"][0][key] = json!("WIRE_INTEGER");
            } else {
                p["process_info"][key] = json!("WIRE_INTEGER");
            }
            assert_eq!(
                schema::processes(&reply_with_native_integer(p, &integer), "opaque:p")
                    .unwrap_err()
                    .code,
                "schema_error"
            );
        }
        for (key, value) in [
            ("epoch", json!("WIRE_INTEGER")),
            ("sources", json!(["WIRE_INTEGER"])),
        ] {
            let mut s = generic();
            s["annotations"] = json!({"epoch":"e","sources":["coven"]});
            s["annotations"][key] = value;
            let s =
                schema::snapshot(&reply_with_native_integer(snapshot(s), &integer), true).unwrap();
            assert_eq!(
                schema::publication_annotations(s.annotations.as_ref().unwrap())
                    .unwrap_err()
                    .code,
                "schema_error"
            );
        }
        let mut s = generic();
        s["annotations"] = json!("WIRE_INTEGER");
        let s = schema::snapshot(&reply_with_native_integer(snapshot(s), &integer), true).unwrap();
        assert!(matches!(
            s.annotations.as_ref(),
            Some(crate::values::Value::Number(
                crate::values::Number::Integer(_)
            ))
        ));
        assert_eq!(
            schema::publication_annotations(s.annotations.as_ref().unwrap())
                .unwrap_err()
                .code,
            "schema_error"
        );
    }
}

#[test]
fn unused_layout_contents_are_ignored_only_after_shape_checks() {
    for (key, value) in [
        ("area", json!(0)),
        ("panes", json!([0])),
        ("splits", json!([null])),
    ] {
        let mut s = strict();
        s["layouts"] = json!([{"workspace_id":"w","tab_id":"t","focused_pane_id":"p","zoomed":false,"area":{},"panes":[],"splits":[]}]);
        s["layouts"][0][key] = value;
        assert_eq!(
            schema::snapshot(&reply(snapshot(s)), false)
                .unwrap_err()
                .code,
            "schema_error"
        );
    }
}

#[cfg(unix)]
#[test]
fn native_failed_endpoint_reports_match_python_parent_resolution() {
    use std::io::Write;
    use std::os::unix::{
        fs::{PermissionsExt, symlink},
        net::UnixListener,
    };
    use std::process::{Command, Stdio};

    let directory = tempfile::tempdir().unwrap();
    let real = directory.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let alias = directory.path().join("alias");
    symlink(&real, &alias).unwrap();
    std::fs::write(real.join("regular"), b"not a socket").unwrap();
    let private_listener = UnixListener::bind(real.join("private.sock")).unwrap();
    std::fs::set_permissions(
        real.join("private.sock"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    symlink(real.join("private.sock"), real.join("leaf-link")).unwrap();
    let unsafe_listener = UnixListener::bind(real.join("unsafe.sock")).unwrap();
    std::fs::set_permissions(
        real.join("unsafe.sock"),
        std::fs::Permissions::from_mode(0o660),
    )
    .unwrap();

    let mut cases = Vec::new();
    let mut expected = Vec::new();
    for parent in [&real, &alias] {
        for (leaf, diagnostic) in [
            ("missing.sock", "io_error"),
            ("regular", "unsafe_endpoint"),
            ("leaf-link", "unsafe_endpoint"),
            ("unsafe.sock", "unsafe_endpoint"),
            ("missing/./ancestors/socket", "io_error"),
        ] {
            for process_mode in [false, true] {
                let mut r = request(process_mode);
                r.socket = parent.join(leaf).to_str().unwrap().into();
                let report = inspect(&r).unwrap();
                let endpoint = json!({"selected":r.socket,"resolved":real.canonicalize().unwrap().join(leaf.replace("/./","/")).to_str().unwrap(),"device":null,"inode":null});
                assert_eq!(report["endpoint"], endpoint);
                assert_eq!(report["assessment"], "unavailable");
                assert_eq!(report["diagnostics"], json!([diagnostic]));
                assert_eq!(
                    report[if process_mode {
                        "observations"
                    } else {
                        "agents"
                    }],
                    json!([])
                );
                expected.push(json!({"endpoint":report["endpoint"],"assessment":report["assessment"],"diagnostics":report["diagnostics"]}));
                cases.push(json!({"socket":r.socket,"process":process_mode}));
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|path| path.join("runner/kitsune_observation.py").is_file())
        .expect("checkout observation oracle")
        .canonicalize()
        .unwrap();
    let mut child = Command::new("python3")
        .args([
            "-B",
            "-c",
            r#"
import json,sys
sys.path.insert(0,sys.argv[1])
from runner import kitsune_observation as o, kitsune_process_observation as p
reports=[]
for case in json.load(sys.stdin):
    report=p.inspect(case['socket'],'opaque:p') if case['process'] else o.inspect(case['socket'])
    reports.append({k:report[k] for k in ('endpoint','assessment','diagnostics')})
json.dump(reports,sys.stdout)
"#,
        ])
        .arg(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&cases).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!(expected)
    );
    private_listener.set_nonblocking(true).unwrap();
    unsafe_listener.set_nonblocking(true).unwrap();
    assert_eq!(
        private_listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        unsafe_listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn resolution_failure_does_not_select_or_read_an_endpoint() {
    struct ResolutionFailure(Cell<usize>);
    impl Transport for ResolutionFailure {
        fn resolve(&self, _: &str) -> WireResult<String> {
            self.0.set(self.0.get() + 1);
            Err(Failure::new("io_error"))
        }
        fn select(&self, _: &str) -> WireResult<observation_socket::Endpoint> {
            panic!("selection after resolution failure")
        }
        fn read(
            &self,
            _: &str,
            _: ReadMethod,
            _: &Value,
            _: Instant,
            _: &mut ReadBudget,
            _: &dyn Cancel,
        ) -> WireResult<observation_socket::Reply> {
            panic!("read after resolution failure")
        }
    }
    for process_mode in [false, true] {
        let transport = ResolutionFailure(Cell::new(0));
        let report = inspect_with(&request(process_mode), &TestClock::new(), &transport).unwrap();
        assert_eq!(report["endpoint"]["resolved"], Value::Null);
        assert_eq!(report["endpoint"]["device"], Value::Null);
        assert_eq!(report["endpoint"]["inode"], Value::Null);
        assert_eq!(report["diagnostics"], json!(["io_error"]));
        assert_eq!(transport.0.get(), 1);
    }
}
