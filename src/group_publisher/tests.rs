use super::{
    Request,
    engine::{self, Ops},
    model,
};
use crate::observation_socket::{self, Cancel, Delivery, Failure, ReadBudget, WireResult};
use crate::observation_time::Clock;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    time::{Duration, Instant},
};

pub(super) struct TestClock {
    pub base: Instant,
    pub elapsed: Cell<Duration>,
    pub wall: Cell<f64>,
}
impl TestClock {
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            elapsed: Cell::new(Duration::ZERO),
            wall: Cell::new(100_000.0),
        }
    }
    pub fn advance(&self, ms: u64) {
        self.elapsed
            .set(self.elapsed.get() + Duration::from_millis(ms));
        self.wall.set(self.wall.get() + ms as f64);
    }
}
impl Clock for TestClock {
    fn monotonic(&self) -> Instant {
        self.base + self.elapsed.get()
    }
    fn wall_ms(&self) -> f64 {
        self.wall.get()
    }
    fn timestamp(&self) -> crate::Result<String> {
        Ok("2026-10-23T00:00:00+00:00".into())
    }
}

#[derive(Default)]
pub(super) struct Stop {
    pub after: Cell<Option<usize>>,
}
impl Cancel for Stop {
    fn interrupted(&self) -> bool {
        match self.after.get() {
            None => false,
            Some(0) => true,
            Some(n) => {
                self.after.set(Some(n - 1));
                false
            }
        }
    }
}

pub(super) fn request(live: bool) -> Request {
    Request {
        workspace: "/unused".into(),
        group: "team".into(),
        socket: "/unused/socket".into(),
        source: "coven".into(),
        duration: 1,
        live,
    }
}
pub(super) fn group(count: usize) -> Value {
    json!({"group_id": "group-id", "key": "team", "name": "Review Team", "members": (0..count).map(|i| json!({
        "participant_id": format!("participant-{i}"), "key": format!("member-{i}"), "role": format!("Role {i}"),
        "binding": {"presentation": {"adapter": "kitsune", "endpoint": {"path": "/unused/socket", "device": 1, "inode": 2},
            "terminal_id": format!("terminal-{i}"), "pane_id_at_binding": format!("pane-{i}")},
            "evidence": {"provenance": "kitsune.session.snapshot", "observed_at": "2026-10-23T00:00:00+00:00",
                "agent_kind_hint": "test-agent", "native_reference": null, "native_binding": "unestablished"}}
    })).collect::<Vec<_>>()})
}
pub(super) fn source(sequence: u64, sample: u64) -> Value {
    json!({"contract": "djinn.live-observation.v1", "runtime_id": "runtime", "instance_id": "instance",
        "session_id": "session", "sequence": sequence, "sampled_at_ms": sample, "valid_for_ms": 3000,
        "source": "legacy_session_run_state", "activity_scope": "legacy_runner_ownership",
        "capabilities": {"activity": true, "idle": false, "catalog_complete": false,
            "decision_state": false, "retry_state": false, "durable_lifecycle": false, "job_completion": false}})
}
pub(super) fn attach(group: &mut Value) {
    for member in group["members"].as_array_mut().unwrap() {
        member["live_attachment"] = json!({"adapter": "djinn_live", "source_contract": "djinn.live-observation.v1",
            "runtime_id": "runtime", "instance_id": "instance", "session_id": "session",
            "artifact_path": "/unused/runtime.json", "attached_at": "2026-10-23T00:00:00+00:00"});
    }
}
pub(super) fn status(group: &Value) -> Value {
    json!({"schema_version": 1, "contract": "coven.group.status.v1", "assessment": "observed", "revision": "revision",
        "group": {"group_id": group["group_id"], "key": group["key"], "name": group["name"]},
        "authorization": "none", "dispatchable": false, "native_lifecycle": "unestablished", "job_completion": "unestablished",
        "participants": group["members"].as_array().unwrap().iter().map(|m| json!({
            "participant_id": m["participant_id"], "key": m["key"], "role": m["role"], "live_attachment": m["live_attachment"],
            "association_basis": if m["live_attachment"].is_null() {"unattached"} else {"explicit_declaration"},
            "assessment": "observed", "activity": "busy", "activity_scope": "legacy_runner_ownership",
            "freshness": "fresh_sample", "expires_in_ms": 3000, "source": source(1,100_000),
            "authorization": "none", "dispatchable": false, "native_lifecycle": "unestablished", "job_completion": "unestablished"
        })).collect::<Vec<_>>()})
}

pub(super) struct Fixture {
    pub clock: TestClock,
    pub stop: Stop,
    pub group: Value,
    pub fresh: Value,
    pub report: Value,
    pub reservation: Option<Value>,
    pub fault: Option<(&'static str, Failure)>,
    pub signal: Option<(&'static str, usize)>,
    pub delay: Option<(&'static str, u64)>,
    pub writes: RefCell<Vec<Value>>,
    pub events: RefCell<Vec<String>>,
    pub reserve_calls: Cell<usize>,
    pub group_calls: Cell<usize>,
    pub drift: Option<Value>,
    pub socket: bool,
    pub process_bytes: usize,
}
impl Fixture {
    pub fn new(count: usize, live: bool) -> Self {
        let mut group = group(count);
        if live {
            attach(&mut group);
        }
        let panes = (0..count)
            .map(|i| {
                json!({"pane_id": format!("pane-{i}"), "terminal_id": format!("terminal-{i}"),
            "workspace_id": "w", "tab_id": "t", "agent": "test-agent", "agent_session": null})
            })
            .collect::<Vec<_>>();
        let fresh = json!({"endpoint": {"path": "/unused/socket", "device": 1, "inode": 2}, "epoch": "epoch-1",
            "sources": ["coven"], "valid_until_unix_ms": true, "panes": panes, "agents": panes});
        Self {
            clock: TestClock::new(),
            stop: Stop::default(),
            report: status(&group),
            group,
            fresh,
            reservation: None,
            fault: None,
            signal: None,
            delay: None,
            writes: RefCell::new(vec![]),
            events: RefCell::new(vec![]),
            reserve_calls: Cell::new(0),
            group_calls: Cell::new(0),
            drift: None,
            socket: false,
            process_bytes: 100,
        }
    }
    fn boundary(&self, stage: &str) -> WireResult<()> {
        self.events.borrow_mut().push(stage.into());
        if let Some((name, ms)) = self.delay {
            if name == stage {
                self.clock.advance(ms);
            }
        }
        if let Some((name, checks)) = self.signal {
            if name == stage {
                self.stop.after.set(Some(checks));
            }
        }
        if let Some((name, failure)) = &self.fault {
            if *name == stage {
                return Err(failure.clone());
            }
        }
        Ok(())
    }
    pub fn run(&self, request: &Request) -> Value {
        let report = engine::run(request, self, &self.clock, &self.stop);
        let n = |key| report[key].as_u64().unwrap();
        assert_eq!(
            n("acknowledged") + n("rejected") + n("unsent") + n("uncertain"),
            n("reservation_attempted_targets")
        );
        assert_eq!(
            n("reserved_targets") + n("reservation_uncertain"),
            n("reservation_attempted_targets")
        );
        assert!(n("completed_cycles") <= n("cycles"));
        assert!(n("cycles") <= n("reservation_attempts"));
        report
    }
    pub fn process_member(&mut self, index: usize) {
        let evidence = &mut self.group["members"][index]["binding"]["evidence"];
        evidence["provenance"] = json!("kitsune.pane.process_info");
        evidence["agent_kind_hint"] = json!("djinn");
    }
}
impl Ops for Fixture {
    fn group(&self, _: &Request) -> WireResult<(Value, Value)> {
        self.boundary("group")?;
        let count = self.group_calls.get();
        self.group_calls.set(count + 1);
        Ok((
            if count > 0 {
                self.drift.clone().unwrap_or(json!("revision"))
            } else {
                json!("revision")
            },
            self.group.clone(),
        ))
    }
    fn snapshot(
        &self,
        _: &Request,
        _: Instant,
        budget: &mut ReadBudget,
        process_only: bool,
        _: &dyn Cancel,
    ) -> WireResult<Value> {
        self.boundary(if process_only { "generic" } else { "strict" })?;
        budget.charge(100)?;
        Ok(self.fresh.clone())
    }
    fn process(
        &self,
        _: &Request,
        _: &str,
        terminal: &str,
        _: &Value,
        _: Instant,
        budget: &mut ReadBudget,
        _: &dyn Cancel,
    ) -> WireResult<Value> {
        self.boundary("process")?;
        // Three separately charged bracket reads share the initial snapshot budget.
        for _ in 0..3 {
            budget.charge(self.process_bytes)?;
        }
        Ok(self.group["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["binding"]["presentation"]["terminal_id"] == terminal)
            .unwrap()["binding"]
            .clone())
    }
    fn stable(&self, binding: &Value) -> WireResult<Value> {
        self.boundary("stable")?;
        Ok(binding.clone())
    }
    fn status(&self, _: &Request, timeout: f64) -> WireResult<Value> {
        assert!(timeout > 0.0 && timeout <= 1.0);
        self.boundary("status")?;
        Ok(self.report.clone())
    }
    fn accepted(&self, report: &Value, _: &Value) -> WireResult<Value> {
        self.boundary("accepted")?;
        assert_eq!(
            report["adapter_contract"],
            "coven.djinn.live-observation.v1"
        );
        Ok(report["source"].clone())
    }
    fn reserve(
        &self,
        _: &Request,
        revision: &Value,
        _: &Value,
        _: &str,
        terminals: &[String],
        first: bool,
    ) -> WireResult<Value> {
        let count = self.reserve_calls.get();
        self.reserve_calls.set(count + 1);
        assert_eq!(first, count == 0);
        self.boundary("reserve")?;
        Ok(self.reservation.clone().unwrap_or_else(|| json!({"delivery": {"outcome": "durably_committed"},
            "revision": revision, "reservations": terminals.iter().map(|t| (t.clone(), json!(count+1))).collect::<serde_json::Map<_,_>>()})))
    }
    fn annotate(
        &self,
        request: &Request,
        endpoint: &Value,
        params: &mut Value,
        deadline: Instant,
        cancel: &dyn Cancel,
        prepare: &mut dyn FnMut(&mut Value) -> WireResult<()>,
    ) -> WireResult<()> {
        self.boundary("connect")?;
        if self.socket {
            return observation_socket::annotate(
                &request.socket,
                endpoint,
                params,
                deadline,
                engine::BUDGET,
                cancel,
                prepare,
            );
        }
        prepare(params)?;
        self.boundary("serialize")?;
        prepare(params)?;
        self.boundary("send")?;
        self.writes.borrow_mut().push(params.clone());
        self.boundary("ack")
    }
    fn sleep(&self, duration: Duration, _: &dyn Cancel) {
        self.clock.advance(duration.as_millis() as u64);
        let _ = self.boundary("sleep");
    }
}

fn assert_failure(f: &Fixture, code: &str) -> Value {
    let report = f.run(&request(false));
    assert_eq!(report["outcome"], "failed");
    assert!(
        report["diagnostics"]
            .as_array()
            .unwrap()
            .contains(&json!(code)),
        "{report}"
    );
    assert_eq!(f.reserve_calls.get(), 0);
    assert!(f.writes.borrow().is_empty());
    report
}

mod group_publisher42 {
    use super::*;
    #[test]
    fn static_payload() {
        let f = Fixture::new(1, false);
        let report = f.run(&request(false));
        assert_eq!(report["outcome"], "completed");
        assert_eq!(report["completed_cycles"], 1);
        assert_eq!(
            f.writes.borrow()[0],
            json!({"pane_id":"pane-0", "expected_terminal_id":"terminal-0",
            "annotation_epoch":"epoch-1", "source":"coven", "text":"Review Team \u{b7} Role 0", "ttl_ms":6000,"revision":1})
        );
    }
    #[test]
    fn zero_attachment_reads() {
        let mut f = Fixture::new(1, false);
        f.group["members"][0]["live_attachment"] = json!({"artifact_path":"/never/read"});
        assert_eq!(f.run(&request(false))["outcome"], "completed");
        assert!(
            !f.events
                .borrow()
                .iter()
                .any(|e| e == "status" || e == "accepted")
        );
    }
    #[test]
    fn caller_validation() {
        for (field, value) in [
            ("group", ""),
            ("group", "UPPER"),
            ("source", "bad source"),
            ("source", ""),
            ("socket", "relative"),
            ("socket", "/x/../y"),
        ] {
            let mut r = request(false);
            match field {
                "group" => r.group = value.into(),
                "source" => r.source = value.into(),
                _ => r.socket = value.into(),
            };
            assert!(model::validate(&r).is_err());
        }
        for duration in [0, 3601, u64::MAX] {
            let mut r = request(false);
            r.duration = duration;
            assert!(model::validate(&r).is_err());
        }
    }
    #[test]
    fn endpoint_replaced() {
        let mut f = Fixture::new(1, false);
        f.fresh["endpoint"]["inode"] = json!(3);
        assert_failure(&f, "endpoint_replaced");
    }
    #[test]
    fn unsupported_annotations() {
        let mut f = Fixture::new(1, false);
        f.fresh["epoch"] = Value::Null;
        assert_failure(&f, "annotations_unsupported");
    }
    #[test]
    fn source_disabled() {
        let mut f = Fixture::new(1, false);
        f.fresh["sources"] = json!([]);
        assert_failure(&f, "source_disabled");
    }
    #[test]
    fn content_drift() {
        let mut f = Fixture::new(1, false);
        f.drift = Some(json!("other"));
        assert_failure(&f, "group_content_changed");
    }
    #[test]
    fn missing_agent() {
        let mut f = Fixture::new(1, false);
        f.fresh["agents"] = json!([]);
        assert_failure(&f, "bound_agent_missing");
    }
    #[test]
    fn changed_hint() {
        let mut f = Fixture::new(1, false);
        f.fresh["agents"][0]["agent"] = json!("new");
        assert_failure(&f, "rebind_required");
    }
    #[test]
    fn changed_native() {
        let mut f = Fixture::new(1, false);
        f.fresh["agents"][0]["agent_session"] = json!({"value":"new"});
        assert_failure(&f, "rebind_required");
    }
    #[test]
    fn empty_group() {
        let f = Fixture::new(0, false);
        assert_failure(&f, "group_size");
    }
    #[test]
    fn too_many_members() {
        let f = Fixture::new(17, false);
        assert_failure(&f, "group_size");
    }
    #[test]
    fn mixed_endpoints() {
        let mut f = Fixture::new(2, false);
        f.group["members"][1]["binding"]["presentation"]["endpoint"]["inode"] = json!(3);
        assert_failure(&f, "group_endpoint_mismatch");
    }
    #[test]
    fn duplicate_terminal() {
        let mut f = Fixture::new(2, false);
        f.group["members"][1]["binding"]["presentation"]["terminal_id"] = json!("terminal-0");
        assert_failure(&f, "duplicate_terminal");
    }
    #[test]
    fn duplicate_inventory() {
        let mut f = Fixture::new(1, false);
        let p = f.fresh["agents"][0].clone();
        f.fresh["agents"].as_array_mut().unwrap().push(p);
        assert_failure(&f, "schema_error");
    }
    #[test]
    fn process_only_generic() {
        let mut f = Fixture::new(1, false);
        f.process_member(0);
        assert_eq!(f.run(&request(false))["outcome"], "completed");
        assert!(f.events.borrow().iter().any(|e| e == "generic"));
    }
    #[test]
    fn mixed_strict_initial() {
        let mut f = Fixture::new(2, false);
        f.process_member(0);
        assert_eq!(f.run(&request(false))["outcome"], "completed");
        assert!(f.events.borrow().iter().any(|e| e == "strict"));
    }
    #[test]
    fn all_verified_before_reserve() {
        let mut f = Fixture::new(2, false);
        f.process_member(1);
        f.fault = Some(("process", Failure::new("process_not_recognized")));
        assert_failure(&f, "process_not_recognized");
    }
    #[test]
    fn strict_no_fallback() {
        let mut f = Fixture::new(2, false);
        f.process_member(0);
        f.fault = Some(("strict", Failure::new("schema_error")));
        assert_failure(&f, "schema_error");
        assert!(!f.events.borrow().iter().any(|e| e == "generic"));
    }
    #[test]
    fn shared_budget_failure() {
        let mut f = Fixture::new(2, false);
        f.process_member(0);
        f.process_bytes = engine::BUDGET / 3;
        assert_failure(&f, "response_limit");
    }
    #[test]
    fn shared_deadline() {
        let mut f = Fixture::new(2, false);
        f.delay = Some(("strict", 1000));
        assert_failure(&f, "timeout");
    }
    #[test]
    fn same_terminal_pane_move() {
        let mut f = Fixture::new(1, false);
        f.fresh["agents"][0]["pane_id"] = json!("moved");
        f.run(&request(false));
        assert_eq!(f.writes.borrow()[0]["pane_id"], "moved");
    }
    #[test]
    fn reservation_error_possible_burn() {
        let mut f = Fixture::new(2, false);
        f.fault = Some(("reserve", Failure::new("io_error")));
        let r = f.run(&request(false));
        assert_eq!(r["outcome"], "uncertain");
        assert_eq!(r["reservation_uncertain"], 2);
        assert_eq!(r["uncertain"], 0);
        assert_eq!(r["unsent"], 2);
        assert!(f.writes.borrow().is_empty());
    }
    #[test]
    fn reservation_uncertain_zero_writes() {
        let mut f = Fixture::new(2, false);
        f.reservation = Some(json!({"delivery":{"outcome":"durability_uncertain"}}));
        let r = f.run(&request(false));
        assert_eq!(r["cycles"], 0);
        assert_eq!(r["unsent"], 2);
        assert!(f.writes.borrow().is_empty());
    }
    #[test]
    fn invalid_reservation_matrix() {
        for numbers in [
            json!({}),
            json!({"terminal-0":0}),
            json!({"terminal-0":true}),
            json!({"terminal-0":1.0}),
            json!({"terminal-0":-1}),
            json!({"other":1}),
            json!({"terminal-0":1,"extra":1}),
        ] {
            let mut f = Fixture::new(1, false);
            f.reservation = Some(
                json!({"revision":"revision","delivery":{"outcome":"durably_committed"},"reservations":numbers}),
            );
            let r = f.run(&request(false));
            assert_eq!(r["outcome"], "uncertain");
            assert_eq!(r["cycles"], 0);
            assert!(f.writes.borrow().is_empty());
        }
    }
    #[test]
    fn reservation_revision_mismatch() {
        let mut f = Fixture::new(1, false);
        f.reservation = Some(
            json!({"revision":"changed","delivery":{"outcome":"durably_committed"},"reservations":{"terminal-0":1}}),
        );
        assert_eq!(f.run(&request(false))["cycles"], 0);
        assert!(f.writes.borrow().is_empty());
    }
    #[test]
    fn max_u64_revision() {
        let mut f = Fixture::new(1, false);
        f.reservation = Some(
            json!({"revision":"revision","delivery":{"outcome":"durably_committed"},"reservations":{"terminal-0":u64::MAX}}),
        );
        assert_eq!(f.run(&request(false))["outcome"], "completed");
        assert_eq!(f.writes.borrow()[0]["revision"], u64::MAX);
    }
    #[test]
    fn rejection_stops_no_retry() {
        let mut f = Fixture::new(2, false);
        f.fault = Some((
            "ack",
            Failure {
                code: "server_error:stale_revision".into(),
                delivery: Delivery::Rejected,
                interrupted: false,
            },
        ));
        let r = f.run(&request(false));
        assert_eq!(r["rejected"], 1);
        assert_eq!(r["unsent"], 1);
        assert_eq!(f.writes.borrow().len(), 1);
    }
    #[test]
    fn partial_send() {
        let mut f = Fixture::new(2, false);
        f.fault = Some((
            "send",
            Failure {
                code: "io_error".into(),
                delivery: Delivery::Uncertain,
                interrupted: false,
            },
        ));
        let r = f.run(&request(false));
        assert_eq!(r["uncertain"], 1);
        assert_eq!(r["unsent"], 1);
        assert_eq!(r["cycles"], 1);
    }
    #[test]
    fn lost_ack() {
        let mut f = Fixture::new(2, false);
        f.fault = Some((
            "ack",
            Failure {
                code: "incomplete_response".into(),
                delivery: Delivery::Uncertain,
                interrupted: false,
            },
        ));
        let r = f.run(&request(false));
        assert_eq!(r["uncertain"], 1);
        assert_eq!(f.writes.borrow().len(), 1);
    }
    #[test]
    fn unsent_connect() {
        let mut f = Fixture::new(2, false);
        f.fault = Some(("connect", Failure::new("io_error")));
        let r = f.run(&request(false));
        assert_eq!(r["unsent"], 2);
        assert_eq!(r["uncertain"], 0);
    }
    #[test]
    fn interrupt_before_reserve() {
        let mut f = Fixture::new(2, false);
        f.signal = Some(("strict", 0));
        let r = f.run(&request(false));
        assert_eq!(r["outcome"], "interrupted");
        assert_eq!(r["reservation_attempts"], 0);
    }
    #[test]
    fn interrupt_during_reserve() {
        let mut f = Fixture::new(2, false);
        f.signal = Some(("reserve", 0));
        let r = f.run(&request(false));
        assert_eq!(r["outcome"], "interrupted");
        assert_eq!(r["reservation_uncertain"], 2);
        assert_eq!(r["unsent"], 2);
        assert!(f.writes.borrow().is_empty());
    }
    #[test]
    fn interrupt_ack_return_gap() {
        let mut f = Fixture::new(2, false);
        f.signal = Some(("ack", 0));
        let r = f.run(&request(false));
        assert_eq!(r["outcome"], "interrupted");
        assert_eq!(r["uncertain"], 1);
        assert_eq!(r["acknowledged"], 0);
    }
    #[test]
    fn interrupt_after_ack_phase() {
        let mut f = Fixture::new(2, false);
        f.signal = Some(("ack", 1));
        let r = f.run(&request(false));
        assert_eq!(r["outcome"], "interrupted");
        assert_eq!(r["acknowledged"], 1);
        assert_eq!(r["unsent"], 1);
    }
    #[test]
    fn interrupt_sleep() {
        let mut f = Fixture::new(1, false);
        f.signal = Some(("sleep", 0));
        let r = f.run(&request(false));
        assert_eq!(r["outcome"], "interrupted");
        assert_eq!(r["acknowledged"], 1);
    }
    #[test]
    fn two_second_cadence() {
        let f = Fixture::new(1, false);
        let mut r = request(false);
        r.duration = 5;
        let report = f.run(&r);
        assert_eq!(report["cycles"], 3);
        assert_eq!(f.clock.elapsed.get(), Duration::from_secs(5));
    }
    #[test]
    fn skip_missed_ticks() {
        let mut f = Fixture::new(1, false);
        f.delay = Some(("ack", 2100));
        let mut r = request(false);
        r.duration = 7;
        assert_eq!(f.run(&r)["cycles"], 2);
    }
    #[test]
    fn bounded_phase_ledger() {
        let f = Fixture::new(16, false);
        let mut r = request(false);
        r.duration = 3600;
        let report = f.run(&r);
        assert_eq!(report["cycles"], 1800);
        assert_eq!(report["acknowledged"], 28800);
        assert!(serde_json::to_vec(&report).unwrap().len() < 1500);
    }
    #[test]
    fn python_byte_oracle_static() {
        oracle(false);
    }
    #[test]
    fn real_unix_ack_eof_matrix() {
        unix_matrix(false);
    }
    #[test]
    fn real_unix_live_expired_rejection() {
        unix_matrix(true);
    }
}

pub(super) fn oracle(live: bool) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    for mode in [
        "success",
        "reservation_error",
        "reservation_uncertain",
        "invalid_reservation",
        "reject",
        "lost_ack",
        "unsent",
        "unknown",
    ] {
        let mut f = Fixture::new(2, live);
        match mode {
            "reservation_error" => f.fault = Some(("reserve", Failure::new("io_error"))),
            "reservation_uncertain" => {
                f.reservation = Some(json!({"delivery":{"outcome":"durability_uncertain"}}))
            }
            "invalid_reservation" => {
                f.reservation = Some(
                    json!({"revision":"revision","delivery":{"outcome":"durably_committed"},"reservations":{}}),
                )
            }
            "reject" => {
                f.fault = Some((
                    "ack",
                    Failure {
                        code: "server_error:annotation_expired".into(),
                        delivery: Delivery::Rejected,
                        interrupted: false,
                    },
                ))
            }
            "lost_ack" => {
                f.fault = Some((
                    "ack",
                    Failure {
                        code: "incomplete_response".into(),
                        delivery: Delivery::Uncertain,
                        interrupted: false,
                    },
                ))
            }
            "unsent" => f.fault = Some(("connect", Failure::new("io_error"))),
            "unknown" => {
                for row in f.report["participants"].as_array_mut().unwrap() {
                    row["activity"] = json!("unknown");
                }
            }
            _ => {}
        }
        let report = f.run(&request(live));
        let input =
            json!({"mode":mode,"live":live,"group":f.group,"fresh":f.fresh,"status":f.report});
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .map(|root| root.join("src/group_publisher/oracle.py"))
            .find(|path| path.is_file())
            .expect("checkout publisher oracle");
        let mut child = Command::new("python3")
            .arg("-B")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&input).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let actual = json!({"report":report,"writes":*f.writes.borrow()});
        assert_eq!(
            serde_json::to_vec(&actual).unwrap(),
            output.stdout,
            "mode={mode} live={live}"
        );
    }
}

pub(super) fn unix_effects() {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::{fs::PermissionsExt, net::UnixListener},
        thread,
    };
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("s");
    let listener = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = thread::spawn(move || {
        let mut effects = Vec::new();
        for wall in [100000u64, 102000, 104000] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            let p = &request["params"];
            // A minimal fake multiplexer mutation: relative TTL is capped by the
            // absolute deadline at acceptance. There is exactly one visible frame.
            let ttl = p["ttl_ms"].as_u64().unwrap();
            let expiry = (wall + ttl).min(p["valid_until_unix_ms"].as_u64().unwrap_or(u64::MAX));
            assert!(expiry > wall);
            effects.push((
                p["text"].as_str().unwrap().to_owned(),
                expiry,
                p["revision"].as_u64().unwrap(),
            ));
            let reply = json!({"id":request["id"],"result":{"type":"ok"}});
            stream
                .write_all(&serde_json::to_vec(&reply).unwrap())
                .unwrap();
            stream.write_all(b"\n").unwrap();
        }
        effects
    });
    let mut f = Fixture::new(1, true);
    f.socket = true;
    let mut r = request(true);
    r.socket = path.to_str().unwrap().into();
    r.duration = 5;
    let endpoint = observation_socket::select(&r.socket)
        .unwrap()
        .identity()
        .unwrap();
    f.group["members"][0]["binding"]["presentation"]["endpoint"] = endpoint.clone();
    f.fresh["endpoint"] = endpoint;
    f.report = status(&f.group);
    let report = f.run(&r);
    assert_eq!(report["completed_cycles"], 3);
    let effects = server.join().unwrap();
    assert_eq!(effects[0].1, 103000);
    assert_eq!(effects[1].1, 103000);
    // At 103000 the entire busy frame, including group and role, is gone.
    let visible_at = |now: u64, frame: &(String, u64, u64)| now < frame.1;
    assert!(visible_at(102999, &effects[1]));
    assert!(!visible_at(103000, &effects[1]));
    // Only a later ordinary, higher-revision refresh restores role-only text.
    assert_eq!(effects[2], ("Review Team \u{b7} Role 0".into(), 110000, 3));
}

fn unix_matrix(live: bool) {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::{fs::PermissionsExt, net::UnixListener},
        thread,
    };
    for mode in if live {
        vec!["reject"]
    } else {
        vec!["ok", "eof", "wrong_id", "trailing", "no_newline", "no_eof"]
    } {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("s");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "pane.annotate");
            if mode == "eof" {
                return;
            }
            let mut response = if mode == "reject" {
                json!({"id":request["id"],"error":{"code":"annotation_expired","message":"private"}})
            } else {
                json!({"id":request["id"],"result":{"type":"ok"}})
            };
            if mode == "wrong_id" {
                response["id"] = json!("wrong");
            }
            let mut bytes = serde_json::to_vec(&response).unwrap();
            if mode != "no_newline" {
                bytes.push(b'\n');
            }
            if mode == "trailing" {
                bytes.extend_from_slice(b"trailing");
            }
            let _ = stream.write_all(&bytes);
            if mode == "no_eof" {
                thread::sleep(Duration::from_millis(1100));
            }
        });
        let mut f = Fixture::new(1, live);
        f.socket = true;
        let mut r = request(live);
        r.socket = path.to_str().unwrap().into();
        let endpoint = observation_socket::select(&r.socket)
            .unwrap()
            .identity()
            .unwrap();
        f.group["members"][0]["binding"]["presentation"]["endpoint"] = endpoint.clone();
        f.fresh["endpoint"] = endpoint;
        if live {
            f.report = status(&f.group);
        }
        let report = f.run(&r);
        assert_eq!(report["cycles"], 1);
        match mode {
            "ok" => assert_eq!(report["acknowledged"], 1),
            "reject" => assert_eq!(report["rejected"], 1),
            _ => assert_eq!(report["uncertain"], 1),
        }
        server.join().unwrap();
    }
}
