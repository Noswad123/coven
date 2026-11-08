use super::*;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

fn endpoint() -> wire::Endpoint {
    wire::Endpoint {
        selected: "/private/test.sock".into(),
        resolved: "/private/test.sock".into(),
        device: 1,
        inode: 2,
    }
}
fn request() -> Request {
    Request {
        socket: endpoint().selected,
        timeout: 3.0,
        max_response_bytes: 1048576,
    }
}
fn pane(id: usize, family: Value) -> Value {
    json!({"pane_id":format!("p{id}"),"terminal_id":format!("t{id}"),"workspace_id":"w","tab_id":"tab",
        "focused":false,"agent_status":"done","revision":1,"agent":family,
        "name":"secret-display","agent_session":{"source":"native","agent":"djinn","kind":"path","value":"/secret/ref"}})
}
fn snapshot(panes: Vec<Value>, agents: Vec<Value>) -> Value {
    json!({"type":"session_snapshot","snapshot":{"version":"test","protocol":18,
        "panes":panes,"agents":agents,"workspaces":[],"tabs":[],"layouts":[]}})
}
fn process(id: usize, argv: Value) -> Value {
    json!({"type":"pane_process_info","process_info":{"pane_id":format!("p{id}"),
        "foreground_processes":[{"pid":8,"name":"secret-process","argv":argv}]}})
}
struct Fake {
    replies: RefCell<VecDeque<wire::Reply>>,
    calls: RefCell<Vec<(ReadMethod, Value)>>,
    replacement: Cell<bool>,
}
impl Fake {
    fn new(results: Vec<Value>) -> Self {
        Self {
            replies: RefCell::new(
                results
                    .into_iter()
                    .map(|result| wire::Reply {
                        payload: serde_json::to_vec(&json!({"id":"test","result":result})).unwrap(),
                        request_id: "test".into(),
                        endpoint: endpoint(),
                    })
                    .collect(),
            ),
            calls: RefCell::new(vec![]),
            replacement: Cell::new(false),
        }
    }
}
impl Transport for Fake {
    fn resolve(&self, _: &str) -> WireResult<String> {
        Ok(endpoint().resolved)
    }
    fn select(&self, _: &str) -> WireResult<wire::Endpoint> {
        let mut e = endpoint();
        if self.replacement.get() {
            e.inode += 1;
        }
        Ok(e)
    }
    fn read(
        &self,
        path: &str,
        method: ReadMethod,
        params: &Value,
        _: Instant,
        budget: &mut ReadBudget,
        _: &dyn Cancel,
    ) -> WireResult<wire::Reply> {
        assert_eq!(path, endpoint().selected);
        self.calls.borrow_mut().push((method, params.clone()));
        let reply = self
            .replies
            .borrow_mut()
            .pop_front()
            .expect("unexpected read");
        budget.charge(reply.payload.len() + 1)?;
        Ok(reply)
    }
}
struct TestClock {
    now: Cell<Instant>,
    stamps: Cell<usize>,
    expire: bool,
}
impl TestClock {
    fn new() -> Self {
        Self {
            now: Cell::new(Instant::now()),
            stamps: Cell::new(0),
            expire: false,
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
    fn timestamp(&self) -> Result<String> {
        self.stamps.set(self.stamps.get() + 1);
        if self.expire && self.stamps.get() == 2 {
            self.now.set(self.now.get() + Duration::from_secs(60));
        }
        Ok("2026-11-08T00:00:00Z".into())
    }
}
fn run(fake: &Fake) -> Value {
    inspect_with(&request(), &TestClock::new(), fake, &wire::NoCancel).unwrap()
}
fn failed(report: &Value, code: &str) {
    assert_ne!(report["assessment"], "observed");
    assert_eq!(report["diagnostics"], json!([code]));
    assert_eq!(report["rows"], json!([]));
    assert!(report["counts"].is_null());
    assert!(report["target_count"].is_null());
}

#[test]
fn brackets_all_panes_preserves_initial_order_and_redacts() {
    let p0 = pane(0, json!("djinn"));
    let p1 = pane(1, json!("private-unknown-agent"));
    let a = snapshot(vec![p1.clone(), p0.clone()], vec![p0.clone(), p1.clone()]);
    let b = snapshot(vec![p0.clone(), p1.clone()], vec![p1, p0]);
    let f = Fake::new(vec![
        a,
        process(1, json!(["node", "secret-argv"])),
        process(0, json!(["/secret/djinn", "--help"])),
        b,
    ]);
    let r = run(&f);
    assert_eq!(r["assessment"], "observed");
    assert_eq!(r["target_count"], 2);
    assert_eq!(r["rows"][0]["locators"]["pane_id"], "p1");
    assert_eq!(r["rows"][0]["family_comparison"], "other_builtin_family");
    assert_eq!(r["rows"][1]["family_comparison"], "both_djinn");
    assert_eq!(r["rows"][1]["kitsune"]["before"]["status"], "done");
    assert_eq!(r["counts"]["both_djinn"], 1);
    let text = r.to_string();
    for secret in [
        "secret-display",
        "private-unknown-agent",
        "/secret",
        "secret-argv",
        "secret-process",
    ] {
        assert!(!text.contains(secret), "{secret}");
    }
    assert!(matches!(f.calls.borrow()[0].0, ReadMethod::Snapshot));
    assert!(matches!(f.calls.borrow()[1].0, ReadMethod::ProcessInfo));
    assert!(matches!(f.calls.borrow()[2].0, ReadMethod::ProcessInfo));
    assert!(matches!(f.calls.borrow()[3].0, ReadMethod::Snapshot));
    assert_eq!(f.calls.borrow()[1].1, json!({"pane_id":"p1"}));
}

#[test]
fn all_categories_are_djinn_scoped_and_legacy_changes_are_primary() {
    for (family, listed, recognized, expected) in [
        (json!("djinn"), true, true, "both_djinn"),
        (json!("djinn"), true, false, "builtin_only"),
        (Value::Null, false, true, "generic_only"),
        (Value::Null, false, false, "neither"),
        (json!("other-secret"), true, false, "other_builtin_family"),
        (
            json!("other-secret"),
            true,
            true,
            "conflicting_family_hints",
        ),
    ] {
        let p = pane(0, family);
        let s = snapshot(
            vec![p.clone()],
            if listed { vec![p.clone()] } else { vec![] },
        );
        let r = run(&Fake::new(vec![
            s.clone(),
            process(
                0,
                if recognized {
                    json!(["djinn-ui"])
                } else {
                    json!(["DJINN"])
                },
            ),
            s,
        ]));
        assert_eq!(r["rows"][0]["family_comparison"], expected);
        for (key, value) in [
            ("state_change_seq", json!(1)),
            ("launch_pending", json!(true)),
            ("interactive_ready", json!(true)),
            ("screen_detection_skipped", json!(true)),
            (
                "agent_session",
                json!({"source":"native","agent":"djinn","kind":"id","value":"changed-secret"}),
            ),
        ] {
            let before = snapshot(vec![p.clone()], vec![p.clone()]);
            let mut after = before.clone();
            after["snapshot"]["agents"][0][key] = value.clone();
            if key == "agent_session" {
                after["snapshot"]["panes"][0][key] = value;
            }
            let r = run(&Fake::new(vec![
                before,
                process(0, json!(["djinn"])),
                after,
            ]));
            assert_eq!(r["rows"][0]["family_comparison"], "legacy_changed", "{key}");
            assert!(!r.to_string().contains("changed-secret"));
        }
    }
}

#[test]
fn no_process_queries_after_pane_limit_or_malformed_strict_data() {
    let panes: Vec<_> = (0..65).map(|i| pane(i, Value::Null)).collect();
    let f = Fake::new(vec![snapshot(panes, vec![])]);
    failed(&run(&f), "pane_limit");
    assert_eq!(f.calls.borrow().len(), 1);
    let p = pane(0, Value::Null);
    for key in ["agent_status", "revision", "focused", "terminal_id"] {
        let mut s = snapshot(vec![p.clone()], vec![]);
        s["snapshot"]["panes"][0][key] = json!("bad value");
        let f = Fake::new(vec![s]);
        failed(&run(&f), "schema_error");
        assert_eq!(f.calls.borrow().len(), 1);
    }
}

#[test]
fn cohort_version_protocol_and_endpoint_drift_fail_closed() {
    let p = pane(0, json!("djinn"));
    let s = snapshot(vec![p], vec![]);
    for key in LOCATORS {
        let mut after = s.clone();
        after["snapshot"]["panes"][0][key] = json!("different");
        failed(
            &run(&Fake::new(vec![
                s.clone(),
                process(0, json!(["djinn"])),
                after,
            ])),
            "pane_mapping_changed",
        );
    }
    let mut after = s.clone();
    after["snapshot"]["version"] = json!("next");
    failed(
        &run(&Fake::new(vec![s.clone(), process(0, json!([])), after])),
        "version_changed",
    );
    let mut unsupported = s.clone();
    unsupported["snapshot"]["protocol"] = json!(19);
    let r = run(&Fake::new(vec![unsupported]));
    failed(&r, "unsupported_protocol");
    assert_eq!(r["assessment"], "incompatible");
    let f = Fake::new(vec![s]);
    f.replies.borrow_mut()[0].endpoint.inode += 1;
    failed(&run(&f), "endpoint_replaced");
}

#[test]
fn shared_budget_late_failure_and_final_deadline_clear_all_evidence() {
    let p = pane(0, json!("djinn"));
    let s = snapshot(vec![p.clone()], vec![p]);
    let values = vec![s.clone(), process(0, json!(["djinn"])), s.clone()];
    let f = Fake::new(values.clone());
    let mut req = request();
    req.max_response_bytes = 1024;
    failed(
        &inspect_with(&req, &TestClock::new(), &f, &wire::NoCancel).unwrap(),
        "response_limit",
    );
    let mut clock = TestClock::new();
    clock.expire = true;
    failed(
        &inspect_with(&request(), &clock, &Fake::new(values), &wire::NoCancel).unwrap(),
        "timeout",
    );
    let f = Fake::new(vec![
        s,
        process(0, json!(["djinn"])),
        json!({"type":"wrong"}),
    ]);
    failed(&run(&f), "schema_error");
}

#[test]
fn ignored_large_integers_remain_lossless_and_unprojected_in_every_reply() {
    let p = pane(0, json!("djinn"));
    let s = snapshot(vec![p.clone()], vec![p]);
    let results = vec![s.clone(), process(0, json!(["djinn"])), s];
    let expected = run(&Fake::new(results.clone()));
    assert_eq!(expected["assessment"], "observed");
    for digits in [400, 4300] {
        let integer = "9".repeat(digits);
        for position in 0..results.len() {
            let f = Fake::new(results.clone());
            let payload = format!(
                "{{\"id\":\"test\",\"result\":{},\"ignored\":{integer}}}",
                results[position]
            )
            .into_bytes();
            let root = crate::observation_wire::decode_native(&payload).unwrap();
            let crate::values::Value::Number(crate::values::Number::Integer(n)) =
                root.as_object().unwrap().get("ignored").unwrap()
            else {
                panic!("integer was narrowed");
            };
            assert_eq!(n.as_str(), integer);
            f.replies.borrow_mut()[position].payload = payload;
            let report = run(&f);
            assert_eq!(report, expected, "digits={digits}, position={position}");
            assert!(!report.to_string().contains(&integer));
            assert_eq!(f.calls.borrow().len(), 3);
        }
    }
}

#[test]
fn empty_cohort_still_reads_two_snapshots_and_invalid_requests_never_read() {
    let s = snapshot(vec![], vec![]);
    let f = Fake::new(vec![s.clone(), s]);
    let r = run(&f);
    assert_eq!(r["target_count"], 0);
    assert_eq!(f.calls.borrow().len(), 2);
    for timeout in [0.0, -1.0, 30.1, f64::NAN, f64::INFINITY] {
        let mut req = request();
        req.timeout = timeout;
        assert!(inspect_with(&req, &TestClock::new(), &f, &wire::NoCancel).is_err());
    }
}

#[test]
fn canonical_matches_and_all_legacy_statuses_remain_exact() {
    for status in ["idle", "working", "blocked", "done", "unknown"] {
        let mut p = pane(0, json!("djinn"));
        p["agent_status"] = json!(status);
        let s = snapshot(vec![p.clone()], vec![p]);
        let mut info = process(0, json!(["/private/djinn", "--help"]));
        info["process_info"]["foreground_processes"] = json!([
            {"pid":99,"name":"secret","argv":["djinn-ui"]},
            {"pid":3,"name":"secret","argv":["/secret/djinn","--help"]},
            {"pid":8,"name":"secret","argv":["node","djinn"]}
        ]);
        let r = run(&Fake::new(vec![s.clone(), info, s]));
        assert_eq!(r["assessment"], "observed");
        assert_eq!(r["rows"][0]["kitsune"]["before"]["status"], status);
        assert_eq!(
            r["rows"][0]["coven"]["matches"],
            json!([
                {"pid":3,"executable_family":"djinn","rule":"exact_posix_argv0_basename"},
                {"pid":99,"executable_family":"djinn-ui","rule":"exact_posix_argv0_basename"}
            ])
        );
    }
}

#[test]
fn unused_extensions_reject_nonfinite_malformed_and_out_of_profile_values() {
    let p = pane(0, json!("djinn"));
    let s = snapshot(vec![p.clone()], vec![p]);
    let results = vec![s.clone(), process(0, json!(["djinn"])), s.clone()];
    for (extension, code) in [
        ("1e999".to_owned(), "schema_error"),
        ("NaN".to_owned(), "nonfinite_json"),
        ("Infinity".to_owned(), "nonfinite_json"),
        ("{\"x\":1,\"x\":2}".to_owned(), "duplicate_key"),
        ("{\"x\":}".to_owned(), "invalid_json"),
        ("9".repeat(4301), "schema_error"),
    ] {
        for position in 0..results.len() {
            let f = Fake::new(results.clone());
            f.replies.borrow_mut()[position].payload = format!(
                "{{\"id\":\"test\",\"result\":{},\"ignored\":{extension}}}",
                results[position]
            )
            .into_bytes();
            failed(&run(&f), code);
        }
    }
    let f = Fake::new(vec![s, process(0, json!(["djinn"]))]);
    f.replies.borrow_mut()[1].payload =
        br#"{"id":"test","error":{"code":"private-code","message":"private-message"}}"#.to_vec();
    let r = run(&f);
    failed(&r, "server_error");
    assert!(!r.to_string().contains("private-message"));
    assert!(!r.to_string().contains("private-code"));
}

#[test]
fn interruption_and_invalid_budget_do_not_read() {
    struct Interrupted;
    impl Cancel for Interrupted {
        fn interrupted(&self) -> bool {
            true
        }
    }
    let f = Fake::new(vec![]);
    failed(
        &inspect_with(&request(), &TestClock::new(), &f, &Interrupted).unwrap(),
        "interrupted",
    );
    for budget in [0, 1023, 8388609] {
        let mut req = request();
        req.max_response_bytes = budget;
        assert!(inspect_with(&req, &TestClock::new(), &f, &wire::NoCancel).is_err());
    }
    assert!(f.calls.borrow().is_empty());
}
