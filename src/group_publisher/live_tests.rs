use super::*;
use crate::group_publisher::tests::{Fixture, Stop, TestClock, request, source};

mod live41 {
    use super::*;

    fn failed(f: &Fixture, code: &str) {
        let r = f.run(&request(true));
        assert_eq!(r["outcome"], "failed", "{r}");
        assert!(
            r["diagnostics"].as_array().unwrap().contains(&json!(code)),
            "{r}"
        );
        assert_eq!(f.reserve_calls.get(), 0);
        assert!(f.writes.borrow().is_empty());
    }
    #[test]
    fn busy_payload_and_whole_frame() {
        let f = Fixture::new(1, true);
        let r = f.run(&request(true));
        assert_eq!(r["outcome"], "completed");
        assert_eq!(r["ttl_ms"], Value::Null);
        let writes = f.writes.borrow();
        let p = &writes[0];
        assert_eq!(
            p["text"],
            "Review Team \u{b7} Role 0 \u{b7} legacy runner busy"
        );
        assert_eq!(p["ttl_ms"], 3000);
        assert_eq!(p["valid_until_unix_ms"], 103000);
        assert_eq!(writes.len(), 1);
    }
    #[test]
    fn unknown_role_only() {
        let mut f = Fixture::new(1, true);
        f.report["participants"][0]["activity"] = json!("unknown");
        let r = f.run(&request(true));
        assert_eq!(r["outcome"], "completed");
        assert_eq!(f.writes.borrow()[0]["ttl_ms"], 6000);
        assert!(f.writes.borrow()[0].get("valid_until_unix_ms").is_none());
    }
    #[test]
    fn unattached_role_only() {
        let mut f = Fixture::new(1, false);
        f.report["participants"][0]["assessment"] = json!("unattached");
        assert_eq!(f.run(&request(true))["outcome"], "completed");
        assert_eq!(f.writes.borrow()[0]["ttl_ms"], 6000);
    }
    #[test]
    fn literal_capability_even_unknown() {
        for capability in [Value::Null, json!(false), json!(1), json!("true")] {
            let mut f = Fixture::new(1, false);
            f.fresh["valid_until_unix_ms"] = capability;
            failed(&f, "annotation_deadline_unsupported");
        }
    }
    #[test]
    fn live_status_unavailable() {
        let mut f = Fixture::new(1, true);
        f.fault = Some(("status", Failure::new("io_error")));
        failed(&f, "live_status_unavailable");
    }
    #[test]
    fn invalid_individual_source_unknown() {
        let mut f = Fixture::new(1, true);
        f.fault = Some(("accepted", Failure::new("schema_error")));
        let r = f.run(&request(true));
        assert_eq!(r["outcome"], "completed");
        assert_eq!(f.writes.borrow()[0]["ttl_ms"], 6000);
    }
    macro_rules! corruption {
        ($name:ident, $field:expr, $value:expr) => {
            #[test]
            fn $name() {
                let mut f = Fixture::new(1, true);
                f.report[$field] = $value;
                failed(&f, "live_status_invalid");
            }
        };
    }
    corruption!(schema_boolean, "schema_version", json!(true));
    corruption!(wrong_contract, "contract", json!("other"));
    corruption!(unobserved_projection, "assessment", json!("unavailable"));
    corruption!(revision_corruption, "revision", json!("changed"));
    corruption!(
        identity_corruption,
        "group",
        json!({"group_id":"wrong","key":"team","name":"Review Team"})
    );
    corruption!(authorization_corruption, "authorization", json!("execute"));
    corruption!(dispatchable_corruption, "dispatchable", json!(true));
    corruption!(lifecycle_corruption, "native_lifecycle", json!("running"));
    corruption!(completion_corruption, "job_completion", json!("done"));
    corruption!(missing_participant, "participants", json!([]));
    #[test]
    fn duplicate_participant() {
        let mut f = Fixture::new(2, true);
        f.report["participants"][1] = f.report["participants"][0].clone();
        failed(&f, "live_status_invalid");
    }
    #[test]
    fn changed_member_key() {
        let mut f = Fixture::new(1, true);
        f.report["participants"][0]["key"] = json!("new");
        failed(&f, "live_status_invalid");
    }
    #[test]
    fn changed_member_role() {
        let mut f = Fixture::new(1, true);
        f.report["participants"][0]["role"] = json!("new");
        failed(&f, "live_status_invalid");
    }
    #[test]
    fn changed_attachment_pin() {
        let mut f = Fixture::new(1, true);
        f.report["participants"][0]["live_attachment"]["instance_id"] = json!("replacement");
        failed(&f, "live_status_invalid");
    }
    #[test]
    fn changed_association() {
        let mut f = Fixture::new(1, true);
        f.report["participants"][0]["association_basis"] = json!("inferred");
        failed(&f, "live_status_invalid");
    }
    #[test]
    fn call_start_anchor() {
        let mut f = Fixture::new(1, true);
        f.report["participants"][0]["expires_in_ms"] = json!(900);
        f.delay = Some(("status", 200));
        f.run(&request(true));
        assert_eq!(f.writes.borrow()[0]["valid_until_unix_ms"], 100900);
    }
    #[test]
    fn reservation_latency_downgrade_same_revision() {
        let mut f = Fixture::new(1, true);
        f.report["participants"][0]["expires_in_ms"] = json!(200);
        f.delay = Some(("reserve", 300));
        f.run(&request(true));
        let p = &f.writes.borrow()[0];
        assert_eq!(p["ttl_ms"], 6000);
        assert_eq!(p["revision"], 1);
        assert!(p.get("valid_until_unix_ms").is_none());
    }
    #[test]
    fn connect_latency_downgrade() {
        let mut f = Fixture::new(1, true);
        f.report["participants"][0]["expires_in_ms"] = json!(200);
        f.delay = Some(("connect", 300));
        f.run(&request(true));
        assert_eq!(f.writes.borrow()[0]["ttl_ms"], 6000);
    }
    #[test]
    fn serialization_latency_downgrade() {
        let mut f = Fixture::new(1, true);
        f.report["participants"][0]["expires_in_ms"] = json!(200);
        f.delay = Some(("serialize", 300));
        f.run(&request(true));
        assert_eq!(f.writes.borrow()[0]["ttl_ms"], 6000);
    }
    #[test]
    fn earlier_member_latency() {
        let mut f = Fixture::new(2, true);
        for row in f.report["participants"].as_array_mut().unwrap() {
            row["expires_in_ms"] = json!(200);
        }
        f.delay = Some(("ack", 300));
        f.run(&request(true));
        assert_eq!(f.writes.borrow()[0]["ttl_ms"], 3000);
        assert_eq!(f.writes.borrow()[1]["ttl_ms"], 6000);
    }
    #[test]
    fn expired_cycle_no_write() {
        let mut f = Fixture::new(1, true);
        f.delay = Some(("reserve", 1000));
        let r = f.run(&request(true));
        assert_eq!(r["cycles"], 1);
        assert_eq!(r["unsent"], 1);
        assert!(f.writes.borrow().is_empty());
    }
    #[test]
    fn wall_regression() {
        let c = TestClock::new();
        let mut l = LiveClock::default();
        l.check(&c).unwrap();
        c.wall.set(99999.0);
        assert_eq!(l.check(&c).unwrap_err().code, "live_clock_regressed");
    }
    #[test]
    fn mono_regression() {
        let c = TestClock::new();
        c.advance(100);
        let mut l = LiveClock::default();
        l.check(&c).unwrap();
        c.elapsed.set(Duration::ZERO);
        assert_eq!(l.check(&c).unwrap_err().code, "live_clock_regressed");
    }
    #[test]
    fn invalid_clock_range() {
        for value in [f64::NAN, f64::INFINITY, -1.0, SAFE_JS_MAX + 1.0] {
            let c = TestClock::new();
            c.wall.set(value);
            assert_eq!(
                LiveClock::default().check(&c).unwrap_err().code,
                "live_clock_unavailable"
            );
        }
    }
    #[test]
    fn same_evidence_tightens_both_horizons() {
        let c = TestClock::new();
        let mut h = BTreeMap::new();
        let s = source(1, 100000);
        retain(
            &mut h,
            "p",
            s.clone(),
            "busy",
            103000.0,
            c.base + Duration::from_secs(3),
            100000.0,
        );
        assert!(retain(
            &mut h,
            "p",
            s,
            "busy",
            102000.0,
            c.base + Duration::from_secs(2),
            100000.0
        ));
        assert_eq!(h["p"].wall, 102000.0);
        assert_eq!(h["p"].mono, c.base + Duration::from_secs(2));
    }
    #[test]
    fn same_evidence_cannot_renew() {
        let c = TestClock::new();
        let mut h = BTreeMap::new();
        let s = source(1, 100000);
        retain(
            &mut h,
            "p",
            s.clone(),
            "busy",
            101000.0,
            c.base + Duration::from_secs(1),
            100000.0,
        );
        retain(
            &mut h,
            "p",
            s,
            "busy",
            103000.0,
            c.base + Duration::from_secs(3),
            100000.0,
        );
        assert_eq!(h["p"].wall, 101000.0);
        assert_eq!(h["p"].mono, c.base + Duration::from_secs(1));
    }
    #[test]
    fn contradictory_activity_is_unknown() {
        let c = TestClock::new();
        let mut h = BTreeMap::new();
        let s = source(1, 100000);
        retain(
            &mut h,
            "p",
            s.clone(),
            "busy",
            103000.0,
            c.base + Duration::from_secs(3),
            100000.0,
        );
        assert!(!retain(
            &mut h,
            "p",
            s,
            "unknown",
            102000.0,
            c.base + Duration::from_secs(2),
            100000.0
        ));
        assert_eq!(h["p"].wall, 102000.0);
    }
    #[test]
    fn newer_unknown_blocks_old_busy() {
        let c = TestClock::new();
        let mut h = BTreeMap::new();
        retain(
            &mut h,
            "p",
            source(1, 100000),
            "busy",
            103000.0,
            c.base,
            100000.0,
        );
        assert!(!retain(
            &mut h,
            "p",
            source(2, 100001),
            "unknown",
            103001.0,
            c.base,
            100001.0
        ));
        assert!(!retain(
            &mut h,
            "p",
            source(1, 100000),
            "busy",
            103000.0,
            c.base,
            100000.0
        ));
        assert_eq!(h["p"].source["sequence"], 2);
    }
    #[test]
    fn new_lease_requires_both_increasing() {
        let c = TestClock::new();
        for (seq, sample) in [(1, 100001), (2, 100000), (0, 100001), (2, 99999)] {
            let mut h = BTreeMap::new();
            retain(
                &mut h,
                "p",
                source(1, 100000),
                "busy",
                103000.0,
                c.base,
                100000.0,
            );
            assert!(!retain(
                &mut h,
                "p",
                source(seq, sample),
                "busy",
                103001.0,
                c.base,
                100001.0
            ));
        }
    }
    #[test]
    fn no_following_changed_pins() {
        let c = TestClock::new();
        for pin in ["contract", "runtime_id", "instance_id", "session_id"] {
            let mut h = BTreeMap::new();
            retain(
                &mut h,
                "p",
                source(1, 100000),
                "busy",
                103000.0,
                c.base,
                100000.0,
            );
            let mut new = source(2, 100001);
            new[pin] = json!("replacement");
            assert!(!retain(
                &mut h, "p", new, "busy", 103001.0, c.base, 100001.0
            ));
        }
    }
    #[test]
    fn reused_record_bounded_to_participants() {
        let c = TestClock::new();
        let mut h = BTreeMap::new();
        for seq in 1..=1800 {
            for i in 0..16 {
                retain(
                    &mut h,
                    &format!("p{i}"),
                    source(seq, 100000 + seq),
                    "busy",
                    103000.0,
                    c.base,
                    100000.0,
                );
            }
        }
        assert_eq!(h.len(), 16);
        assert!(h.values().all(|v| v.source["sequence"] == 1800));
    }
    #[test]
    fn temporary_loss_retains_record_not_positive_cache() {
        let mut f = Fixture::new(1, true);
        let mut horizons = BTreeMap::new();
        let mut clock = LiveClock::default();
        let mut summary = Summary::new(true);
        let r = request(true);
        let revision = json!("revision");
        let deadline = f.clock.base + Duration::from_secs(10);
        let first = evidence(
            &r,
            &f,
            &f.clock,
            &f.stop,
            &mut clock,
            &revision,
            &f.group,
            deadline,
            &mut summary,
            &mut horizons,
        )
        .unwrap();
        assert_eq!(first, vec![Some("participant-0".into())]);
        let previous_wall = horizons["participant-0"].wall;
        f.report["participants"][0]["assessment"] = json!("unavailable");
        let missing = evidence(
            &r,
            &f,
            &f.clock,
            &f.stop,
            &mut clock,
            &revision,
            &f.group,
            deadline,
            &mut summary,
            &mut horizons,
        )
        .unwrap();
        assert_eq!(missing, vec![None]);
        assert_eq!(horizons.len(), 1);
        assert_eq!(horizons["participant-0"].wall, previous_wall);
        f.clock.advance(1);
        f.report["participants"][0]["assessment"] = json!("observed");
        f.report["participants"][0]["activity"] = json!("unknown");
        f.report["participants"][0]["source"] = source(2, 100001);
        let unknown = evidence(
            &r,
            &f,
            &f.clock,
            &f.stop,
            &mut clock,
            &revision,
            &f.group,
            deadline,
            &mut summary,
            &mut horizons,
        )
        .unwrap();
        assert_eq!(unknown, vec![None]);
        f.report["participants"][0]["activity"] = json!("busy");
        f.report["participants"][0]["source"] = source(1, 100000);
        let replay = evidence(
            &r,
            &f,
            &f.clock,
            &f.stop,
            &mut clock,
            &revision,
            &f.group,
            deadline,
            &mut summary,
            &mut horizons,
        )
        .unwrap();
        assert_eq!(replay, vec![None]);
        assert_eq!(horizons["participant-0"].source["sequence"], 2);
        assert_eq!(
            summary.finish(None, &[])["diagnostics"],
            json!(["live_source_unknown"])
        );
    }
    #[test]
    fn role_only_never_reupgraded_in_frame() {
        let c = TestClock::new();
        let mut l = LiveClock::default();
        let mut s = Summary::new(true);
        let mut h = Horizon {
            source: source(1, 100000),
            activity: "busy".into(),
            wall: 103000.0,
            mono: c.base + Duration::from_secs(3),
            sample: 100000.0,
        };
        let mut p = json!({"text":"role","ttl_ms":6000});
        prepare(
            &mut p,
            "role",
            Some(&mut h),
            &c,
            &mut l,
            c.base + Duration::from_secs(5),
            &Stop::default(),
            &mut s,
        )
        .unwrap();
        assert_eq!(p, json!({"text":"role","ttl_ms":6000}));
    }
    #[test]
    fn python_byte_oracle_live() {
        crate::group_publisher::tests::oracle(true);
    }
    #[test]
    fn fake_actual_effects_whole_frame_expiry() {
        crate::group_publisher::tests::unix_effects();
    }
}
