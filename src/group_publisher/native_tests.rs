use super::*;
use crate::observation_time::{Clock, SystemClock};
use serde_json::json;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    thread,
};

#[test]
fn historical_endpoint_integers_are_exact_operational_mismatches() {
    // SIGINT ownership is process-wide; keep this public-API regression isolated
    // from the other native test's real signal/handler restoration matrix.
    if std::env::var_os("COVEN_GROUP_PUBLISHER_BIG_ENDPOINT_CHILD").is_none() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "group_publisher::native::tests::historical_endpoint_integers_are_exact_operational_mismatches", "--nocapture"])
            .env("COVEN_GROUP_PUBLISHER_BIG_ENDPOINT_CHILD", "1")
            .output().unwrap();
        assert!(
            child.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        return;
    }
    for key in ["device", "inode"] {
        for different_endpoints in [false, true] {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join("state")).unwrap();
            std::fs::write(root.path().join("coven.json"), b"{}").unwrap();
            let path = root.path().join("s");
            let listener = UnixListener::bind(&path).unwrap();
            listener.set_nonblocking(true).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            let socket = path.to_str().unwrap().to_owned();
            let actual = observation_socket::select(&socket)
                .unwrap()
                .identity()
                .unwrap();
            groups::create_group(root.path(), "team", "Review Team", None).unwrap();
            for index in 0..=usize::from(different_endpoints) {
                let mut binding = to_authority(
                    &kitsune_observation::binding_from_snapshot(
                        &json!({"endpoint":actual, "observed_at":SystemClock.timestamp().unwrap(),
                        "agents":[{"pane_id":"p", "terminal_id":format!("terminal-{index}"),
                            "agent":"test-agent", "agent_session":null}]}),
                        "p",
                    )
                    .unwrap(),
                )
                .unwrap();
                let mut presentation = binding
                    .as_object()
                    .unwrap()
                    .get("presentation")
                    .unwrap()
                    .clone();
                let mut endpoint = presentation
                    .as_object()
                    .unwrap()
                    .get("endpoint")
                    .unwrap()
                    .clone();
                // Adjacent 100-digit integers would collapse to the same f64.
                let digits = format!("{}{}", "9".repeat(99), if index == 0 { '9' } else { '8' });
                endpoint
                    .set_field(key, crate::values::Value::integer(&digits).unwrap())
                    .unwrap();
                presentation.set_field("endpoint", endpoint).unwrap();
                binding.set_field("presentation", presentation).unwrap();
                groups::add_member(
                    root.path(),
                    groups::AddMember {
                        group: "team",
                        member: &format!("member-{index}"),
                        role: "Reviewer",
                        binding: &binding,
                        rebind: false,
                        expected_revision: None,
                    },
                )
                .unwrap();
            }
            // The existing reader accepts the historical declaration exactly.
            let view = groups::read_groups(root.path()).unwrap();
            let before = std::fs::read(root.path().join("state/groups.json")).unwrap();
            let request = Request {
                workspace: root.path().into(),
                group: "team".into(),
                socket,
                source: "coven".into(),
                duration: 1,
                live: false,
            };
            let report = crate::group_publisher::publish(&request).unwrap();
            assert_eq!(report["outcome"], "failed", "{report}");
            assert_eq!(
                report["diagnostics"],
                json!([if different_endpoints {
                    "group_endpoint_mismatch"
                } else {
                    "endpoint_replaced"
                }])
            );
            for field in [
                "reservation_attempts",
                "reserved_targets",
                "acknowledged",
                "uncertain",
            ] {
                assert_eq!(report[field], 0);
            }
            assert!(
                matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
            );
            assert_eq!(
                std::fs::read(root.path().join("state/groups.json")).unwrap(),
                before
            );
            assert_eq!(
                groups::read_groups(root.path()).unwrap().revision(),
                view.revision()
            );
        }
    }
}

#[test]
fn bounded_authority_counter_projection_preserves_u64_max() {
    let counter = crate::values::Value::integer(&u64::MAX.to_string()).unwrap();
    let projected = from_authority(&counter).unwrap();
    assert_eq!(projected.as_u64(), Some(u64::MAX));
    assert!(!projected.as_number().unwrap().is_f64());
    assert_eq!(
        to_authority(&projected).unwrap().canonical().unwrap(),
        counter.canonical().unwrap()
    );
}

#[test]
fn reservation_endpoint_producer_is_ordered_and_exact() {
    let endpoint = json!({"inode":u64::MAX,"device":u64::MAX,"path":"/private/socket"});
    let original = crate::values::parse(
        format!(
            "{{\"path\":\"/private/socket\",\"device\":{},\"inode\":{}}}",
            u64::MAX,
            u64::MAX
        )
        .as_bytes(),
        crate::values::Profile::Authority,
    )
    .unwrap();
    let produced = reservation_endpoint(&endpoint, &original).unwrap();
    assert_eq!(
        produced.compact().unwrap(),
        format!(
            "{{\"path\":\"/private/socket\",\"device\":{},\"inode\":{}}}",
            u64::MAX,
            u64::MAX
        )
        .as_bytes()
    );
    assert_eq!(from_authority(&produced).unwrap(), endpoint);
    let reordered = to_authority(&endpoint).unwrap();
    assert_eq!(
        reservation_endpoint(&endpoint, &reordered)
            .unwrap()
            .compact()
            .unwrap(),
        reordered.compact().unwrap()
    );
    assert!(reservation_endpoint(&json!({"path":"different"}), &original).is_err());
}

#[test]
fn counter_authority_bytes_match_python_producer() {
    use crate::values::Value as Native;
    // Installing the publisher signal owner must not race other tests in this
    // binary. The child uses the real adapter and mutation core, without sockets.
    if std::env::var_os("COVEN_GROUP_PUBLISHER_COUNTER_CHILD").is_none() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "group_publisher::native::tests::counter_authority_bytes_match_python_producer",
                "--nocapture",
            ])
            .env("COVEN_GROUP_PUBLISHER_COUNTER_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        return;
    }
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|path| path.join("runner/groups.py").is_file())
        .unwrap();
    let mut mismatches = Vec::new();
    let mut cases = 0;
    for policy in ["primitive", "publisher"] {
        for reordered_declaration in [false, true] {
            for maximum_stats in [false, true] {
                for prior_stream in [false, true] {
                    let root = tempfile::tempdir().unwrap();
                    let oracle = tempfile::tempdir().unwrap();
                    for workspace in [root.path(), oracle.path()] {
                        std::fs::create_dir(workspace.join("state")).unwrap();
                        std::fs::write(workspace.join("coven.json"), b"{}").unwrap();
                    }
                    let path = root.path().join("s");
                    let _listener = UnixListener::bind(&path).unwrap();
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                        .unwrap();
                    let socket = path.to_str().unwrap().to_owned();
                    let mut endpoint = observation_socket::select(&socket)
                        .unwrap()
                        .identity()
                        .unwrap();
                    if maximum_stats {
                        endpoint["device"] = json!(u64::MAX);
                        endpoint["inode"] = json!(u64::MAX);
                    }
                    // Declaration order and primitive request order are independent.
                    let unordered = Native::object(
                        ["inode", "path", "device"]
                            .map(|key| (key.into(), to_authority(&endpoint[key]).unwrap()))
                            .to_vec(),
                    )
                    .unwrap();
                    let mut binding = to_authority(&kitsune_observation::binding_from_snapshot(
                &json!({"endpoint":endpoint,"observed_at":SystemClock.timestamp().unwrap(),
                    "agents":[{"pane_id":"p","terminal_id":"terminal","agent":"test-agent","agent_session":null}]}), "p").unwrap()).unwrap();
                    let mut presentation = binding
                        .as_object()
                        .unwrap()
                        .get("presentation")
                        .unwrap()
                        .clone();
                    presentation
                        .set_field(
                            "endpoint",
                            if reordered_declaration {
                                unordered.clone()
                            } else {
                                Native::object(
                                    ["path", "device", "inode"]
                                        .map(|key| {
                                            (key.into(), to_authority(&endpoint[key]).unwrap())
                                        })
                                        .to_vec(),
                                )
                                .unwrap()
                            },
                        )
                        .unwrap();
                    binding.set_field("presentation", presentation).unwrap();
                    groups::create_group(root.path(), "team", "Review Team", None).unwrap();
                    groups::add_member(
                        root.path(),
                        groups::AddMember {
                            group: "team",
                            member: "member",
                            role: "Reviewer",
                            binding: &binding,
                            rebind: false,
                            expected_revision: None,
                        },
                    )
                    .unwrap();
                    let mut state = groups::read_groups(root.path())
                        .unwrap()
                        .state()
                        .unwrap()
                        .clone();
                    let counter = Native::object(vec![
                        ("endpoint".into(), unordered.clone()),
                        (
                            "source".into(),
                            Native::String(if prior_stream { "coven" } else { "other" }.into()),
                        ),
                        ("epoch".into(), Native::String("epoch".into())),
                        (
                            "watermarks".into(),
                            Native::object(vec![(
                                "terminal".into(),
                                Native::integer(
                                    &(if prior_stream { u64::MAX - 1 } else { u64::MAX })
                                        .to_string(),
                                )
                                .unwrap(),
                            )])
                            .unwrap(),
                        ),
                    ])
                    .unwrap();
                    state
                        .set_field("publication_counters", Native::Array(vec![counter]))
                        .unwrap();
                    let before = state.storage().unwrap();
                    std::fs::write(root.path().join("state/groups.json"), &before).unwrap();
                    std::fs::write(oracle.path().join("state/groups.json"), &before).unwrap();
                    let request = Request {
                        workspace: root.path().into(),
                        group: "team".into(),
                        socket,
                        source: "coven".into(),
                        duration: 1,
                        live: false,
                    };
                    let session = Session::open(&request).unwrap();
                    let (revision, _) = session.group(&request).unwrap();
                    if policy == "publisher" {
                        // Same canonical revision, different on-disk declaration order.
                        let mut changed = state.clone();
                        let mut group = changed
                            .as_object()
                            .unwrap()
                            .get("groups")
                            .unwrap()
                            .as_array()
                            .unwrap()[0]
                            .clone();
                        let mut member = group
                            .as_object()
                            .unwrap()
                            .get("members")
                            .unwrap()
                            .as_array()
                            .unwrap()[0]
                            .clone();
                        let mut binding =
                            member.as_object().unwrap().get("binding").unwrap().clone();
                        let mut presentation = binding
                            .as_object()
                            .unwrap()
                            .get("presentation")
                            .unwrap()
                            .clone();
                        let original = presentation
                            .as_object()
                            .unwrap()
                            .get("endpoint")
                            .unwrap()
                            .as_object()
                            .unwrap();
                        let reordered =
                            Native::object(original.entries().iter().rev().cloned().collect())
                                .unwrap();
                        presentation.set_field("endpoint", reordered).unwrap();
                        binding.set_field("presentation", presentation).unwrap();
                        member.set_field("binding", binding).unwrap();
                        group
                            .set_field("members", Native::Array(vec![member]))
                            .unwrap();
                        changed
                            .set_field("groups", Native::Array(vec![group]))
                            .unwrap();
                        std::fs::write(
                            root.path().join("state/groups.json"),
                            changed.storage().unwrap(),
                        )
                        .unwrap();
                        assert_eq!(session.group(&request).unwrap().0, revision);
                    }
                    let reserved = if policy == "publisher" {
                        session
                            .reserve(
                                &request,
                                &revision,
                                &endpoint,
                                "epoch",
                                &["terminal".into()],
                                true,
                            )
                            .unwrap()
                    } else {
                        let request_endpoint = Native::object(
                            ["path", "device", "inode"]
                                .map(|key| (key.into(), to_authority(&endpoint[key]).unwrap()))
                                .to_vec(),
                        )
                        .unwrap();
                        from_authority(
                            &groups::reserve_cycle(
                                root.path(),
                                groups::ReserveRequest {
                                    group: "team",
                                    expected_revision: revision.as_str().unwrap(),
                                    endpoint: &request_endpoint,
                                    source: "coven",
                                    epoch: "epoch",
                                    terminal_ids: &["terminal".into()],
                                    allow_new_epoch: true,
                                },
                            )
                            .unwrap(),
                        )
                        .unwrap()
                    };
                    assert_eq!(
                        reserved["reservations"]["terminal"].as_u64(),
                        Some(if prior_stream { u64::MAX } else { 1 })
                    );
                    let expected = std::process::Command::new("python3")
                        .arg("-B")
                        .arg(project.join("src/group_publisher/counter_oracle.py"))
                        .arg(oracle.path())
                        .arg("coven")
                        .arg(policy)
                        .env("PYTHONDONTWRITEBYTECODE", "1")
                        .output()
                        .unwrap();
                    assert!(
                        expected.status.success(),
                        "{}",
                        String::from_utf8_lossy(&expected.stderr)
                    );
                    let actual = std::fs::read(root.path().join("state/groups.json")).unwrap();
                    if actual != expected.stdout {
                        let at = actual
                            .iter()
                            .zip(&expected.stdout)
                            .position(|(a, b)| a != b)
                            .unwrap_or(actual.len().min(expected.stdout.len()));
                        mismatches.push(format!("policy={policy} reordered_declaration={reordered_declaration} maximum_stats={maximum_stats} prior_stream={prior_stream}: authority bytes differ at offset {at}"));
                    }
                    cases += 1;
                    let current = groups::read_groups(root.path()).unwrap();
                    let counters = current
                        .state()
                        .unwrap()
                        .as_object()
                        .unwrap()
                        .get("publication_counters")
                        .unwrap()
                        .as_array()
                        .unwrap();
                    // Existing endpoint objects are never normalized by publication.
                    assert_eq!(
                        counters[0]
                            .as_object()
                            .unwrap()
                            .get("endpoint")
                            .unwrap()
                            .compact()
                            .unwrap(),
                        unordered.compact().unwrap()
                    );
                }
            }
        }
    }
    assert_eq!(cases, 16);
    assert!(
        mismatches.is_empty(),
        "{}/{} counter oracle cases differ:\n{}",
        mismatches.len(),
        cases,
        mismatches.join("\n")
    );
}

#[test]
fn public_publish_real_authority_status_reservation_and_fake_unix_peer() {
    const RUNTIME: &str = "616fed3e-ae7f-41a0-99e5-80546d659d76";
    const INSTANCE: &str = "e3c0e2ce-544c-4f1e-a489-5d3e0cf1978e";
    let signal = std::env::var_os("COVEN_GROUP_PUBLISHER_SIGNAL_CHILD").is_some();
    for (live, process) in if signal {
        vec![(false, false)]
    } else {
        vec![(false, false), (true, false), (false, true), (true, true)]
    } {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir(root.path().join("state")).unwrap();
        std::fs::write(root.path().join("coven.json"), b"{}").unwrap();
        let path = root.path().join("s");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let socket = path.to_str().unwrap().to_owned();
        let endpoint = observation_socket::select(&socket)
            .unwrap()
            .identity()
            .unwrap();
        let pane = json!({"workspace_id":"w","tab_id":"t","pane_id":"p","terminal_id":"terminal",
            "focused":false,"agent_status":"working","revision":1,"agent":"test-agent","agent_session":null});
        let mut binding = kitsune_observation::binding_from_snapshot(
            &json!({"endpoint":endpoint,
            "observed_at":SystemClock.timestamp().unwrap(),"agents":[pane]}),
            "p",
        )
        .unwrap();
        if process {
            binding["evidence"]["provenance"] = json!("kitsune.pane.process_info");
            binding["evidence"]["agent_kind_hint"] = json!("djinn");
            binding["evidence"]["process_evidence"] = json!({"interpreter":"coven.kitsune.djinn-process.v1",
                "matches":[{"pid":7,"executable_family":"djinn","rule":"exact_posix_argv0_basename"}]});
        }
        let binding = to_authority(&binding).unwrap();
        groups::create_group(root.path(), "team", "Review Team", None).unwrap();
        groups::add_member(
            root.path(),
            groups::AddMember {
                group: "team",
                member: "member",
                role: "Reviewer",
                binding: &binding,
                rebind: false,
                expected_revision: None,
            },
        )
        .unwrap();
        let sampled = SystemClock.wall_ms().floor() as u64;
        if live {
            let artifact = root.path().join(format!("{RUNTIME}.json"));
            let source = json!({"schema_version":1,"contract":"djinn.live-observation.v1","runtime_id":RUNTIME,
                "sequence":1,"sampled_at_ms":sampled,"valid_for_ms":3000,"source":"legacy_session_run_state",
                "activity_scope":"legacy_runner_ownership","capabilities":{"activity":true,"idle":false,"catalog_complete":false,
                    "decision_state":false,"retry_state":false,"durable_lifecycle":false,"job_completion":false},
                "authorization":"none","dispatchable":false,"native_lifecycle":"unestablished","job_completion":"unestablished",
                "instances":[{"instance_id":INSTANCE,"sessions":[{"session_id":"selected","activity":"busy"}]}]});
            std::fs::write(&artifact, serde_json::to_vec(&source).unwrap()).unwrap();
            std::fs::set_permissions(&artifact, std::fs::Permissions::from_mode(0o600)).unwrap();
            let inspected =
                crate::djinn_live_observation::inspect(&crate::djinn_live_observation::Request {
                    file: artifact.to_str().unwrap().into(),
                    runtime: RUNTIME.into(),
                    instance: INSTANCE.into(),
                    session: "selected".into(),
                    timeout: 1.0,
                })
                .unwrap();
            assert_eq!(inspected["assessment"], "observed", "{inspected}");
            crate::group_live::attach(
                root.path(),
                crate::group_live::AttachRequest {
                    group: "team",
                    member: "member",
                    file: artifact.to_str().unwrap(),
                    runtime: RUNTIME,
                    instance: INSTANCE,
                    session: "selected",
                    reattach: false,
                    expected_revision: None,
                    timeout: 1.0,
                },
            )
            .unwrap();
        }
        let frozen = groups::read_groups(root.path())
            .unwrap()
            .revision()
            .to_owned();
        let server = thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(3);
            let mut writes = Vec::new();
            let mut reads = 0;
            let mut process_reads = 0;
            while Instant::now() < until && writes.is_empty() {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                let result = if request["method"] == "session.snapshot" {
                    reads += 1;
                    json!({"type":"session_snapshot","snapshot":{"version":"fake","protocol":18,
                        "workspaces":if process {json!("malformed")} else {json!([])},"tabs":[],"layouts":[],
                        "panes":[pane],"agents":if process {json!("malformed")} else {json!([pane])},
                        "annotations":{"epoch":"epoch","sources":["coven"],"valid_until_unix_ms":true}}})
                } else if request["method"] == "pane.process_info" {
                    process_reads += 1;
                    assert_eq!(request["params"]["pane_id"], "p");
                    json!({"type":"pane_process_info","process_info":{"pane_id":"p",
                        "foreground_processes":[{"pid":7,"name":"djinn","argv":["/fake/djinn"]}]}})
                } else {
                    assert_eq!(request["method"], "pane.annotate");
                    writes.push(request["params"].clone());
                    if signal {
                        // Isolated test subprocess: a real native signal after
                        // request bytes arrived but before any acknowledgement.
                        // Wait for the handler on this thread before closing the socket.
                        assert_eq!(unsafe { libc::raise(libc::SIGINT) }, 0);
                        break;
                    }
                    json!({"type":"ok"})
                };
                let reply = json!({"id":request["id"],"result":result});
                stream
                    .write_all(&serde_json::to_vec(&reply).unwrap())
                    .unwrap();
                stream.write_all(b"\n").unwrap();
            }
            (reads, process_reads, writes)
        });
        let request = Request {
            workspace: root.path().into(),
            group: "team".into(),
            socket,
            source: "coven".into(),
            duration: 1,
            live,
        };
        let report = crate::group_publisher::publish(&request).unwrap();
        let (reads, process_reads, writes) = server.join().unwrap();
        assert_eq!(
            report["outcome"],
            if signal { "interrupted" } else { "completed" },
            "{report}"
        );
        assert_eq!(report["acknowledged"], if signal { 0 } else { 1 });
        assert_eq!(report["uncertain"], if signal { 1 } else { 0 });
        assert_eq!(reads, if process { 3 } else { 1 });
        assert_eq!(process_reads, usize::from(process));
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0]["revision"], 1);
        assert_eq!(writes[0]["ttl_ms"], if live { 3000 } else { 6000 });
        if live {
            assert_eq!(
                writes[0]["text"],
                "Review Team \u{b7} Reviewer \u{b7} legacy runner busy"
            );
            assert!(writes[0]["valid_until_unix_ms"].as_u64().unwrap() <= sampled + 3000);
        } else {
            assert!(writes[0].get("valid_until_unix_ms").is_none());
        }
        let current = groups::read_groups(root.path()).unwrap();
        assert_eq!(current.revision(), frozen);
        let state = from_authority(current.state().unwrap()).unwrap();
        assert_eq!(
            state["publication_counters"][0]["watermarks"]["terminal"],
            1
        );
        // Persistent lease inode remains; neither stop nor expiry clears remote state.
        assert!(root.path().join("state/.group-publisher.lock").is_file());
    }
    if !signal {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "group_publisher::native::tests::public_publish_real_authority_status_reservation_and_fake_unix_peer", "--nocapture"])
            .env("COVEN_GROUP_PUBLISHER_SIGNAL_CHILD", "1").output().unwrap();
        assert!(
            child.status.success(),
            "{}\n{}\n{}",
            child.status,
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
    }
}
