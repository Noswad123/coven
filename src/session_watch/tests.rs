use super::*;
use crate::observation_socket::NoCancel;
use std::cell::{Cell, RefCell};

struct FakeTimer {
    value: Cell<f64>,
    stalled: bool,
}
impl FakeTimer {
    fn new(value: f64) -> Self {
        Self {
            value: Cell::new(value),
            stalled: false,
        }
    }
}
impl Timer for FakeTimer {
    fn now(&self) -> WireResult<f64> {
        Ok(self.value.get())
    }
    fn sleep(&self, seconds: f64) -> WireResult<()> {
        if !self.stalled {
            self.value.set(self.value.get() + seconds);
        }
        Ok(())
    }
}
fn report(assessment: &str) -> Value {
    json!({"schema_version":1,"contract":"coven.session-inspection.v1","assessment":assessment,"diagnostics":[],
        "authorization":"none","dispatchable":false,"native_catalog_complete":false,"runtime_fact_freshness":"unknown",
        "native_identity":"unestablished","native_lifecycle":"unestablished","native_worker_acceptance":"unestablished","job_completion":"unestablished",
        "targets":[],"target_count":if assessment == "observed"{json!(0)}else{Value::Null},"recognized_target_count":if assessment == "observed"{json!(0)}else{Value::Null},
        "declarations":if assessment == "observed"{json!({"assessment":"uninitialized","unmatched":[]})}
            else{json!({"assessment":"unavailable","unmatched":[],"revision":null,"authority_schema_version":null,"other_endpoint_count":null,"outside_inspection_count":null})}})
}

#[test]
fn immediate_origin_anchored_ticks_include_emit_cost_and_remaining_budget() {
    for (cost, expected) in [
        (0., vec![100., 102., 104., 106., 108.]),
        (2., vec![100., 104., 108.]),
        (3., vec![100., 104., 108.]),
    ] {
        let timer = FakeTimer::new(100.);
        let starts = RefCell::new(Vec::new());
        let budgets = RefCell::new(Vec::new());
        let summary = watch(
            &mut |remaining| {
                starts.borrow_mut().push(timer.value.get());
                budgets.borrow_mut().push(remaining);
                Ok(report("observed"))
            },
            &mut |_| {
                timer.value.set(timer.value.get() + cost);
                Ok(())
            },
            10.,
            2.,
            &timer,
            &NoCancel,
        );
        assert_eq!(*starts.borrow(), expected);
        assert_eq!(summary["outcome"], "completed");
        for (start, budget) in starts.borrow().iter().zip(budgets.borrow().iter()) {
            assert_eq!(110. - start, *budget);
        }
    }
}

#[test]
fn collection_cost_skips_ticks_and_can_emit_after_deadline() {
    for (cost, duration, expected) in [
        (2., 5., vec![0., 4.]),
        (5., 12., vec![0., 6.]),
        (7., 5., vec![0.]),
    ] {
        let timer = FakeTimer::new(0.);
        let starts = RefCell::new(Vec::new());
        let elapsed = RefCell::new(Vec::new());
        let summary = watch(
            &mut |remaining| {
                let start = timer.value.get();
                assert_eq!(remaining, duration - start);
                starts.borrow_mut().push(start);
                timer.value.set(start + cost);
                Ok(report("observed"))
            },
            &mut |event| {
                elapsed
                    .borrow_mut()
                    .push(event["elapsed_ms"].as_f64().unwrap());
                Ok(())
            },
            duration,
            2.,
            &timer,
            &NoCancel,
        );
        assert_eq!(summary["outcome"], "completed");
        assert_eq!(*starts.borrow(), expected);
        assert_eq!(
            *elapsed.borrow(),
            expected
                .iter()
                .map(|start| (start + cost) * 1000.)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            summary["poll_attempts"].as_u64(),
            Some(expected.len() as u64)
        );
    }
}

#[test]
fn early_wakes_recheck_target_and_oversleep_never_polls_late() {
    struct WakingTimer {
        value: Cell<f64>,
        oversleep: bool,
        sleeps: Cell<usize>,
    }
    impl Timer for WakingTimer {
        fn now(&self) -> WireResult<f64> {
            Ok(self.value.get())
        }
        fn sleep(&self, seconds: f64) -> WireResult<()> {
            self.sleeps.set(self.sleeps.get() + 1);
            self.value.set(
                self.value.get()
                    + if self.oversleep {
                        seconds + 10.
                    } else {
                        seconds.min(0.5)
                    },
            );
            Ok(())
        }
    }
    for oversleep in [false, true] {
        let timer = WakingTimer {
            value: Cell::new(0.),
            oversleep,
            sleeps: Cell::new(0),
        };
        let starts = RefCell::new(Vec::new());
        let summary = watch(
            &mut |_| {
                starts.borrow_mut().push(timer.value.get());
                Ok(report("observed"))
            },
            &mut |_| Ok(()),
            5.,
            2.,
            &timer,
            &NoCancel,
        );
        assert_eq!(summary["outcome"], "completed");
        assert_eq!(
            *starts.borrow(),
            if oversleep {
                vec![0.]
            } else {
                vec![0., 2., 4.]
            }
        );
        assert_eq!(timer.sleeps.get(), if oversleep { 1 } else { 10 });
    }
}

#[test]
fn emitter_interruption_uses_failure_or_cancel_flag_without_counting_delivery() {
    struct Cancellation(Cell<bool>);
    impl Cancel for Cancellation {
        fn interrupted(&self) -> bool {
            self.0.get()
        }
    }
    for cancel_flag in [false, true] {
        let cancel = Cancellation(Cell::new(false));
        let summary = watch(
            &mut |_| Ok(report("observed")),
            &mut |_| {
                cancel.0.set(cancel_flag);
                Err(Failure::new(if cancel_flag {
                    "output_failed"
                } else {
                    "interrupted"
                }))
            },
            5.,
            2.,
            &FakeTimer::new(0.),
            &cancel,
        );
        assert_eq!(exit_code(&summary), 130);
        assert_eq!(summary["outcome"], "interrupted");
        assert_eq!(summary["poll_attempts"], 1);
        assert_eq!(summary["emitted"], 0);
        assert_eq!(summary["observed_samples"], 0);
    }
}

#[test]
fn fractional_roundoff_and_deadline_do_not_catch_up() {
    for start in [0., 100.] {
        let timer = FakeTimer::new(start);
        let starts = RefCell::new(Vec::new());
        let summary = watch(
            &mut |_| {
                starts.borrow_mut().push(timer.value.get());
                Ok(report("observed"))
            },
            &mut |_| Ok(()),
            10.,
            0.1,
            &timer,
            &NoCancel,
        );
        assert_eq!(summary["outcome"], "completed");
        assert_eq!(starts.borrow().len(), 100);
        assert!(starts.borrow().iter().all(|s| *s < start + 10.));
    }
}

#[test]
fn failures_clear_without_stale_success_and_collection_failure_stops() {
    let timer = FakeTimer::new(0.);
    let events = RefCell::new(Vec::new());
    let mut reports = vec![
        Ok(report("observed")),
        Ok(report("unavailable")),
        Ok(report("incompatible")),
        Err(Failure::new("PRIVATE")),
    ]
    .into_iter();
    let summary = watch(
        &mut |_| reports.next().unwrap(),
        &mut |e| {
            events.borrow_mut().push(e.clone());
            Ok(())
        },
        10.,
        2.,
        &timer,
        &NoCancel,
    );
    assert_eq!(summary["outcome"], "collector_failed");
    assert_eq!(summary["emitted"], 4);
    assert_eq!(summary["unavailable_samples"], 2);
    assert!(events.borrow()[3]["observation"].is_null());
    assert_eq!(
        events.borrow()[3]["diagnostics"],
        json!(["collection_failed"])
    );
    assert!(!summary.to_string().contains("PRIVATE"));
    for index in [1, 2] {
        assert_eq!(events.borrow()[index]["observation"]["targets"], json!([]));
    }
}

#[test]
fn invalid_envelopes_clear_and_output_failure_takes_precedence() {
    for field in [
        "authorization",
        "dispatchable",
        "target_count",
        "declarations",
        "native_identity",
        "runtime_fact_freshness",
    ] {
        let timer = FakeTimer::new(0.);
        let mut bad = report("observed");
        bad[field] = json!("bad");
        let summary = watch(
            &mut |_| Ok(bad.clone()),
            &mut |e| {
                assert!(e["observation"].is_null());
                Ok(())
            },
            5.,
            2.,
            &timer,
            &NoCancel,
        );
        assert_eq!(summary["outcome"], "collector_failed");
    }
    let attempts = Cell::new(0);
    let writes = Cell::new(0);
    let summary = watch(
        &mut |_| {
            attempts.set(attempts.get() + 1);
            Err(Failure::new("PRIVATE"))
        },
        &mut |_| {
            writes.set(writes.get() + 1);
            Err(Failure::new("output_failed"))
        },
        5.,
        2.,
        &FakeTimer::new(0.),
        &NoCancel,
    );
    assert_eq!(summary["outcome"], "output_failed");
    assert_eq!(summary["emitted"], 0);
    assert_eq!(attempts.get(), 1);
    assert_eq!(writes.get(), 1);
}

#[test]
fn clock_faults_and_interrupt_fail_closed() {
    for value in [f64::NAN, f64::INFINITY, 1e308] {
        let summary = watch(
            &mut |_| panic!("collection"),
            &mut |_| panic!("emit"),
            5.,
            2.,
            &FakeTimer::new(value),
            &NoCancel,
        );
        assert_eq!(summary["outcome"], "clock_failed");
        assert_eq!(summary["poll_attempts"], 0);
    }
    let timer = FakeTimer {
        value: Cell::new(0.),
        stalled: true,
    };
    let summary = watch(
        &mut |_| Ok(report("observed")),
        &mut |_| Ok(()),
        5.,
        2.,
        &timer,
        &NoCancel,
    );
    assert_eq!(summary["outcome"], "clock_failed");
    assert_eq!(summary["emitted"], 1);
    let timer = FakeTimer::new(0.);
    let summary = watch(
        &mut |_| {
            timer.value.set(-1.);
            Ok(report("observed"))
        },
        &mut |_| panic!("regression must prevent emit"),
        5.,
        2.,
        &timer,
        &NoCancel,
    );
    assert_eq!(summary["outcome"], "clock_failed");
    assert_eq!(summary["emitted"], 0);
    let summary = watch(
        &mut |_| Err(Failure::new("interrupted")),
        &mut |_| panic!("interrupted"),
        5.,
        2.,
        &FakeTimer::new(0.),
        &NoCancel,
    );
    assert_eq!(exit_code(&summary), 130);
    assert_eq!(summary["poll_attempts"], 1);
}

#[test]
fn every_record_flushes_and_summary_failure_is_not_retried() {
    struct Writer {
        bytes: Vec<u8>,
        flushes: usize,
        fail: bool,
    }
    impl Write for Writer {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if self.fail {
                return Err(std::io::ErrorKind::BrokenPipe.into());
            }
            self.bytes.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }
    let mut writer = Writer {
        bytes: vec![],
        flushes: 0,
        fail: false,
    };
    emit(&mut writer, &json!({"contract":"event"}), true, false).unwrap();
    emit(&mut writer, &json!({"contract":"summary"}), true, true).unwrap();
    assert_eq!(writer.flushes, 2);
    assert_eq!(writer.bytes.iter().filter(|b| **b == b'\n').count(), 2);
    writer.fail = true;
    assert!(emit(&mut writer, &json!({}), true, true).is_err());
    assert_eq!(writer.flushes, 2);
}

#[test]
fn interrupted_flush_preserves_interruption_without_a_signal_flag() {
    struct InterruptedFlush;
    impl Write for InterruptedFlush {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::Interrupted.into())
        }
    }
    let error = emit(&mut InterruptedFlush, &json!({}), true, false).unwrap_err();
    assert_eq!(error.code, "interrupted");
    assert!(error.interrupted);
}

#[cfg(unix)]
#[test]
fn output_sigint_is_process_isolated() {
    let test = format!(
        "{}::output_sigint_subprocess_case",
        module_path!().split_once("::").unwrap().1
    );
    for case in [
        "successful_flush",
        "interrupted_flush",
        "broken_flush",
        "summary_sigint",
        "summary_interrupted",
        "summary_broken",
    ] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test, "--nocapture"])
            .env("COVEN_SESSION_WATCH_OUTPUT_CASE", case)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{case}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(unix)]
#[test]
fn output_sigint_subprocess_case() {
    let Ok(case) = std::env::var("COVEN_SESSION_WATCH_OUTPUT_CASE") else {
        return;
    };
    struct Writer {
        bytes: Vec<u8>,
        flushes: usize,
        case: String,
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes += 1;
            if self.case.starts_with("summary_") {
                assert!(self.flushes <= 2, "summary delivery must not be retried");
                if self.flushes == 2 {
                    if self.case == "summary_sigint" {
                        assert_eq!(unsafe { libc::raise(libc::SIGINT) }, 0);
                    }
                    return Err(if self.case == "summary_broken" {
                        std::io::ErrorKind::BrokenPipe
                    } else {
                        std::io::ErrorKind::Interrupted
                    }
                    .into());
                }
                return Ok(());
            }
            if self.flushes == 1 {
                if self.case == "broken_flush" {
                    return Err(std::io::ErrorKind::BrokenPipe.into());
                }
                assert_eq!(unsafe { libc::raise(libc::SIGINT) }, 0);
                if self.case == "interrupted_flush" {
                    return Err(std::io::ErrorKind::Interrupted.into());
                }
            }
            Ok(())
        }
    }
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join("state")).unwrap();
    std::fs::write(
        workspace.path().join("coven.json"),
        b"not parsed by readers",
    )
    .unwrap();
    let request = Request {
        workspace: workspace.path().into(),
        socket: workspace
            .path()
            .join("missing.sock")
            .to_str()
            .unwrap()
            .into(),
        pane: None,
        expected_terminal: None,
        runtime: "djinn".into(),
        timeout: 3.,
        max_response_bytes: 1048576,
        group: None,
        member: None,
        recognized_only: false,
    };
    let mut writer = Writer {
        bytes: Vec::new(),
        flushes: 0,
        case: case.clone(),
    };
    let duration = if case.starts_with("summary_") {
        0.1
    } else {
        5.
    };
    let result = arguments(&request, duration, 2., true, &mut writer).unwrap();
    assert!(result.stdout.is_empty());
    let records: Vec<Value> = std::str::from_utf8(&writer.bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    if case.starts_with("summary_") {
        assert_eq!(
            result.exit_code,
            if case == "summary_broken" { 1 } else { 130 }
        );
        assert_eq!(writer.flushes, 2);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["sequence"], 1);
        let summary = &records[1];
        assert_eq!(summary["contract"], "coven.session-watch-summary.v1");
        assert_eq!(summary["outcome"], "completed");
        assert_eq!(summary["poll_attempts"], 1);
        assert_eq!(summary["emitted"], 1);
        assert_eq!(summary["unavailable_samples"], 1);
        assert_eq!(summary["observed_samples"], 0);
        assert_eq!(summary["incompatible_samples"], 0);
    } else if case == "broken_flush" {
        assert_eq!(result.exit_code, 1);
        assert_eq!(writer.flushes, 1);
        assert_eq!(records.len(), 1);
    } else {
        assert_eq!(result.exit_code, 130);
        assert_eq!(writer.flushes, 2);
        assert_eq!(records.len(), 2);
        let summary = &records[1];
        assert_eq!(summary["outcome"], "interrupted");
        assert_eq!(summary["diagnostics"], json!(["interrupted"]));
        assert_eq!(summary["poll_attempts"], 1);
        let delivered = if case == "successful_flush" { 1 } else { 0 };
        assert_eq!(summary["emitted"], delivered);
        assert_eq!(summary["unavailable_samples"], delivered);
        assert_eq!(summary["observed_samples"], 0);
        assert_eq!(summary["incompatible_samples"], 0);
    }
}
