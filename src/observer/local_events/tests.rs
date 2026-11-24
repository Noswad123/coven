use super::*;
#[allow(dead_code)]
#[path = "../../platform/unsupported/observation_socket.rs"]
mod unsupported;
use crate::observation_socket::NoCancel;
use serde_json::{Value as J, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{PermissionsExt, symlink},
        net::UnixListener,
    },
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};

#[test]
fn unsupported_persistent_reader_surface_compiles_and_rejects() {
    let mut buffer = [0; 1];
    let result: WireResult<Option<usize>> =
        unsupported::Socket.stream_read(&mut buffer, later(10), &NoCancel);
    assert_eq!(result.unwrap_err().code, "unsupported_platform");
}

enum Command {
    Bytes(Vec<u8>),
    Close,
}
struct Peer {
    dir: tempfile::TempDir,
    path: String,
    tx: mpsc::Sender<Command>,
    done: mpsc::Receiver<()>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Peer {
    fn new(ack: impl FnOnce(&str) -> Vec<Vec<u8>> + Send + 'static) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("e.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (tx, rx) = mpsc::channel();
        let (sent, done) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = thread::spawn(move || {
            let mut stream = loop {
                if stopped.load(Ordering::Relaxed) {
                    return;
                }
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let request: J = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "events.subscribe");
            let expected = [
                "pane.created",
                "pane.closed",
                "pane.updated",
                "pane.moved",
                "pane.exited",
                "tab.created",
                "tab.closed",
                "tab.moved",
                "workspace.created",
                "workspace.closed",
                "workspace.moved",
                "layout.updated",
            ];
            assert_eq!(
                request["params"]["subscriptions"],
                json!(expected.map(|name| json!({"type": name})))
            );
            for part in ack(request["id"].as_str().unwrap()) {
                if stream.write_all(&part).is_err() {
                    return;
                }
                thread::sleep(Duration::from_millis(1));
            }
            while !stopped.load(Ordering::Relaxed) {
                match rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(Command::Bytes(bytes)) => {
                        let _ = stream.write_all(&bytes);
                        let _ = sent.send(());
                    }
                    Ok(Command::Close) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        });
        Self {
            dir,
            path: path.to_str().unwrap().into(),
            tx,
            done,
            stop,
            worker: Some(worker),
        }
    }
    fn standard() -> Self {
        Self::new(|id| vec![ack(id)])
    }
    fn open(&self) -> Box<dyn LocalEventStream> {
        NativeEvents
            .open(&self.path, later(1000), &NoCancel)
            .unwrap()
    }
    fn send(&self, bytes: Vec<u8>) {
        self.tx.send(Command::Bytes(bytes)).unwrap();
        self.done.recv_timeout(Duration::from_secs(2)).unwrap();
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.tx.send(Command::Close).ok();
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
fn later(ms: u64) -> Instant {
    Instant::now() + Duration::from_millis(ms)
}
fn line(value: J) -> Vec<u8> {
    let mut b = serde_json::to_vec(&value).unwrap();
    b.push(b'\n');
    b
}
fn ack(id: &str) -> Vec<u8> {
    line(json!({"id":id,"result":{"type":"subscription_started"}}))
}
fn updated(title: &str) -> J {
    json!({"event":"pane_updated","data":{"type":"pane_updated","pane":{
        "pane_id":"w:p", "terminal_id":"term", "workspace_id":"w", "tab_id":"w:t", "title":title
    }}})
}
fn next_hint(stream: &mut dyn LocalEventStream) -> Hint {
    let end = later(1000);
    loop {
        if let Some(hint) = stream.next(end, &NoCancel).unwrap() {
            return hint;
        }
        assert!(Instant::now() < end);
    }
}

#[test]
fn fragmented_ack_and_coalesced_utf8_events_preserve_unread_bytes() {
    let peer = Peer::new(|id| {
        let mut bytes = ack(id);
        bytes.extend(line(updated("caf\u{e9}")));
        bytes.extend(line(updated("changed")));
        bytes.chunks(7).map(<[u8]>::to_vec).collect()
    });
    let mut stream = peer.open();
    assert_eq!(stream.endpoint().selected, peer.path);
    assert_eq!(next_hint(&mut *stream), Hint::Inventory);
    assert_eq!(next_hint(&mut *stream), Hint::Inventory);
    assert_eq!(stream.next(later(10), &NoCancel).unwrap(), None);
}

#[test]
fn ack_correlation_type_and_error_branches_are_strict() {
    for variant in 0..6 {
        let peer = Peer::new(move |id| {
            vec![line(match variant {
                0 => json!({"id":"wrong", "result":{"type":"subscription_started"}}),
                1 => json!({"id":id, "result":{"type":"ok"}}),
                2 => json!({"id":id, "error":{"code":"unsupported", "message":"no"}}),
                3 => json!({"id":id, "error":{"code":1, "message":"no"}}),
                4 => json!({"id":id, "result":{"type":"subscription_started"}, "error":{}}),
                _ => json!({"id":id}),
            })]
        });
        let error = NativeEvents
            .open(&peer.path, later(1000), &NoCancel)
            .err()
            .unwrap();
        assert_eq!(
            error.code,
            if variant == 2 {
                "subscription_rejected"
            } else {
                "schema_error"
            }
        );
    }
}

#[test]
fn short_deadlines_keep_partial_utf8_and_eof_is_not_idle() {
    let peer = Peer::standard();
    let mut stream = peer.open();
    assert_eq!(stream.next(later(5), &NoCancel).unwrap(), None);
    let bytes = line(updated("\u{e9}"));
    let split = bytes.iter().position(|&b| b == 0xc3).unwrap() + 1;
    peer.send(bytes[..split].to_vec());
    for _ in 0..4 {
        assert_eq!(stream.next(later(5), &NoCancel).unwrap(), None);
    }
    peer.send(bytes[split..].to_vec());
    assert_eq!(next_hint(&mut *stream), Hint::Inventory);
    peer.send(b"{\"event\":".to_vec());
    assert_eq!(stream.next(later(5), &NoCancel).unwrap(), None);
    peer.tx.send(Command::Close).unwrap();
    assert_eq!(
        stream.next(later(100), &NoCancel).unwrap_err().code,
        "incomplete_response"
    );
}

#[test]
fn partial_assembly_has_an_overall_deadline_but_idle_does_not() {
    let peer = Peer::standard();
    let mut stream = peer.open();
    let idle = Instant::now();
    while idle.elapsed() < Duration::from_millis(5100) {
        assert_eq!(stream.next(later(15), &NoCancel).unwrap(), None);
    }
    peer.send(b"{".to_vec());
    let start = Instant::now();
    loop {
        match stream.next(later(15), &NoCancel) {
            Ok(None) => {}
            Err(e) => {
                assert_eq!(e.code, "partial_frame_timeout");
                break;
            }
            _ => panic!("partial frame accepted"),
        }
        assert!(start.elapsed() < Duration::from_secs(7));
    }
    assert!(start.elapsed() >= Duration::from_secs(5));
}

#[test]
fn complete_wire_is_validated_before_extension_projection() {
    let cases = [
        (b"{\"event\":\"x\",\"event\":\"x\",\"data\":{\"type\":\"x\"}}\n".to_vec(), "duplicate_key"),
        ([b"{\"event\":\"x\",\"data\":{\"type\":\"x\"},\"ext\":".as_slice(), "[".repeat(33).as_bytes(), b"0", "]".repeat(33).as_bytes(), b"}\n"].concat(), "nesting_limit"),
        (line(json!({"event":"x","data":{"type":"x"},"ext_path":"a".repeat(4097)})), "schema_error"),
        (b"{\"event\":\"pane_closed\",\"data\":{\"type\":\"pane_exited\",\"pane_id\":\"p\",\"workspace_id\":\"w\"}}\n".to_vec(), "schema_error"),
        (vec![0xff, b'\n'], "invalid_utf8"),
    ];
    for (bytes, code) in cases {
        let peer = Peer::standard();
        let mut stream = peer.open();
        peer.send(bytes);
        assert_eq!(stream.next(later(100), &NoCancel).unwrap_err().code, code);
    }
    let peer = Peer::standard();
    let mut stream = peer.open();
    let mut bytes = b"{\"event\":\"future_kind\",\"data\":{\"type\":\"future_kind\"},\"ext\":999999999999999999999999999999999999999999999999}\n".to_vec();
    bytes.extend(line(updated("works")));
    peer.send(bytes);
    assert_eq!(next_hint(&mut *stream), Hint::Inventory);
}

#[test]
fn meaningful_dedup_replay_reorder_and_bounded_capacity() {
    let mut panes = HashMap::new();
    let event =
        |v: J, panes: &mut HashMap<_, _>| project(&decode(&line(v)).unwrap(), panes).unwrap();
    assert!(event(updated("a"), &mut panes));
    let mut volatile = updated("a");
    volatile["data"]["pane"]["revision"] = json!(100);
    volatile["data"]["pane"]["focused"] = json!(true);
    volatile["data"]["pane"]["ignored"] = json!({"any":"extension"});
    assert!(!event(volatile, &mut panes));
    assert!(event(updated("b"), &mut panes));
    assert!(event(updated("a"), &mut panes));
    assert!(!event(updated("a"), &mut panes));
    let mut named = updated("a");
    named["data"]["pane"]["name"] = json!("stable display name");
    assert!(event(named.clone(), &mut panes));
    named["data"]["pane"]["title"] = json!("shadowed title changed");
    named["data"]["pane"]["foreground_cwd"] = json!("ignored project extension");
    assert!(!event(named, &mut panes));
    assert!(event(updated("a"), &mut panes));
    for key in [
        "terminal_id",
        "workspace_id",
        "tab_id",
        "name",
        "label",
        "agent",
        "cwd",
    ] {
        let mut v = updated("a");
        v["data"]["pane"][key] = json!("different");
        assert!(event(v, &mut panes));
        assert!(event(updated("a"), &mut panes));
    }
    for n in 1..1024 {
        let mut v = updated("a");
        v["data"]["pane"]["pane_id"] = json!(format!("p{n}"));
        assert!(event(v, &mut panes));
    }
    let mut v = updated("a");
    v["data"]["pane"]["pane_id"] = json!("overflow");
    assert_eq!(
        project(&decode(&line(v)).unwrap(), &mut panes)
            .unwrap_err()
            .code,
        "event_dedup_limit"
    );
    assert_eq!(panes.len(), 1024);
    assert!(event(updated("subsequent"), &mut panes));
}

#[test]
fn idle_replacement_and_mode_fences_reject_a_still_readable_old_socket() {
    for mode in 0..3 {
        let peer = Peer::standard();
        let mut stream = peer.open();
        if mode == 0 {
            std::fs::remove_file(&peer.path).unwrap();
        } else if mode == 1 {
            std::fs::remove_file(&peer.path).unwrap();
            let _replacement = UnixListener::bind(&peer.path).unwrap();
            std::fs::set_permissions(&peer.path, std::fs::Permissions::from_mode(0o600)).unwrap();
        } else {
            std::fs::set_permissions(&peer.path, std::fs::Permissions::from_mode(0o666)).unwrap();
        }
        peer.send(line(updated("old socket still writable")));
        assert!(stream.next(later(100), &NoCancel).is_err());
    }
    let peer = Peer::standard();
    let mut stream = peer.open();
    std::fs::remove_file(&peer.path).unwrap();
    let start = Instant::now();
    loop {
        if stream.next(later(30), &NoCancel).is_err() {
            break;
        }
        assert!(start.elapsed() < Duration::from_millis(1200));
    }
}

#[test]
fn parent_alias_is_frozen_and_absent_or_unsafe_paths_never_open() {
    let peer = Peer::standard();
    let alias = peer.dir.path().join("alias");
    symlink(peer.dir.path(), &alias).unwrap();
    let path = alias.join("e.sock");
    let mut stream = NativeEvents
        .open(path.to_str().unwrap(), later(1000), &NoCancel)
        .unwrap();
    std::fs::remove_file(&alias).unwrap();
    symlink("/nonexistent", &alias).unwrap();
    peer.send(line(updated("old")));
    assert!(stream.next(later(100), &NoCancel).is_err());
    assert!(
        NativeEvents
            .open(
                peer.dir.path().join("absent").to_str().unwrap(),
                later(100),
                &NoCancel
            )
            .is_err()
    );
    std::fs::set_permissions(&peer.path, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert_eq!(
        NativeEvents
            .open(&peer.path, later(100), &NoCancel)
            .err()
            .unwrap()
            .code,
        "unsafe_endpoint"
    );
}

#[test]
fn storm_turn_is_bounded_and_cancel_is_checked_with_buffered_frames() {
    let peer = Peer::standard();
    let mut stream = peer.open();
    let unknown = line(json!({"event":"future","data":{"type":"future"}}));
    let mut bytes = unknown.repeat(96);
    bytes.extend(line(updated("after storm")));
    peer.send(bytes);
    let start = Instant::now();
    assert_eq!(stream.next(later(1000), &NoCancel).unwrap(), None);
    assert!(start.elapsed() < Duration::from_millis(250));
    struct Stop;
    impl Cancel for Stop {
        fn interrupted(&self) -> bool {
            true
        }
    }
    assert_eq!(
        stream.next(later(1000), &Stop).unwrap_err().code,
        "interrupted"
    );
    assert_eq!(next_hint(&mut *stream), Hint::Inventory);
}

#[test]
fn oversize_frame_is_rejected_without_unbounded_buffering() {
    let peer = Peer::standard();
    let mut stream = peer.open();
    // Smaller writes keep fixture progress independent of the socket buffer size.
    for _ in 0..17 {
        peer.send(vec![b' '; 16384]);
        match stream.next(later(5), &NoCancel) {
            Ok(None) => {}
            Err(e) => {
                assert_eq!(e.code, "response_limit");
                return;
            }
            _ => panic!("oversize accepted"),
        }
    }
    panic!("oversize not rejected");
}

#[test]
fn single_write_ack_and_event_accept_bigint_extensions_without_narrowing() {
    let peer = Peer::new(|id| {
        let mut bytes = ack(id);
        let mut event = line(updated("ready"));
        event.truncate(event.len() - 2);
        event.extend_from_slice(b",\"extension\":9999999999999999999999999999999999999999999}\n");
        bytes.extend(event);
        vec![bytes]
    });
    let mut stream = peer.open();
    assert_eq!(next_hint(&mut *stream), Hint::Inventory);
    peer.tx.send(Command::Close).unwrap();
    assert_eq!(
        stream.next(later(100), &NoCancel).unwrap_err().code,
        "stream_eof"
    );
}

#[test]
fn ack_deadline_and_cancellation_while_idle_are_bounded() {
    let peer = Peer::new(|_| vec![]);
    let start = Instant::now();
    assert_eq!(
        NativeEvents
            .open(&peer.path, later(30), &NoCancel)
            .err()
            .unwrap()
            .code,
        "timeout"
    );
    assert!(start.elapsed() < Duration::from_millis(250));

    struct TimedCancel(Instant);
    impl Cancel for TimedCancel {
        fn interrupted(&self) -> bool {
            Instant::now() >= self.0
        }
    }
    let peer = Peer::standard();
    let mut stream = peer.open();
    let cancel = TimedCancel(later(5));
    let end = later(250);
    loop {
        match stream.next(end, &cancel) {
            Err(e) => {
                assert_eq!(e.code, "interrupted");
                break;
            }
            Ok(None) => assert!(Instant::now() < end),
            _ => panic!("idle peer produced a hint"),
        }
    }
}

#[test]
fn subscribed_topologies_validate_only_consumed_identity_fields() {
    let events = [
        json!({"event":"pane_created","data":{"type":"pane_created","pane":updated("a")["data"]["pane"]}}),
        json!({"event":"pane_moved","data":{"type":"pane_moved","previous_pane_id":"old:p","previous_workspace_id":"old:w","previous_tab_id":"old:t","pane":updated("a")["data"]["pane"]}}),
        json!({"event":"pane_closed","data":{"type":"pane_closed","pane_id":"p","workspace_id":"w"}}),
        json!({"event":"pane_exited","data":{"type":"pane_exited","pane_id":"p","workspace_id":"w"}}),
        json!({"event":"tab_created","data":{"type":"tab_created","tab":{"tab_id":"t","workspace_id":"w"}}}),
        json!({"event":"tab_closed","data":{"type":"tab_closed","tab_id":"t","workspace_id":"w"}}),
        json!({"event":"tab_moved","data":{"type":"tab_moved","tab_id":"t","workspace_id":"w"}}),
        json!({"event":"workspace_created","data":{"type":"workspace_created","workspace":{"workspace_id":"w"}}}),
        json!({"event":"workspace_closed","data":{"type":"workspace_closed","workspace_id":"w"}}),
        json!({"event":"workspace_moved","data":{"type":"workspace_moved","workspace_id":"w"}}),
        json!({"event":"layout_updated","data":{"type":"layout_updated","layout":{"workspace_id":"w","tab_id":"t","focused_pane_id":"p"}}}),
    ];
    let peer = Peer::standard();
    let mut stream = peer.open();
    peer.send(events.into_iter().flat_map(line).collect());
    for _ in 0..11 {
        assert_eq!(next_hint(&mut *stream), Hint::Inventory);
    }
    for key in ["pane_id", "terminal_id", "workspace_id", "tab_id"] {
        for bad in [json!(""), json!("space id"), json!(null), json!(1)] {
            let mut event = updated("a");
            event["data"]["pane"][key] = bad;
            assert_eq!(
                project(&decode(&line(event)).unwrap(), &mut HashMap::new())
                    .unwrap_err()
                    .code,
                "schema_error"
            );
        }
    }
}

#[test]
fn eof_refences_frozen_path_before_reporting_closed_or_partial_stream() {
    for partial in [false, true] {
        for change in ["unchanged", "missing", "replaced", "unsafe"] {
            let mut peer = Peer::standard();
            let mut stream = peer.open();
            if partial {
                peer.send(b"{".to_vec());
                assert_eq!(stream.next(later(5), &NoCancel).unwrap(), None);
            }
            let replacement = match change {
                "missing" => {
                    std::fs::remove_file(&peer.path).unwrap();
                    None
                }
                "replaced" => {
                    std::fs::remove_file(&peer.path).unwrap();
                    let listener = UnixListener::bind(&peer.path).unwrap();
                    std::fs::set_permissions(&peer.path, std::fs::Permissions::from_mode(0o600))
                        .unwrap();
                    Some(listener)
                }
                "unsafe" => {
                    std::fs::set_permissions(&peer.path, std::fs::Permissions::from_mode(0o666))
                        .unwrap();
                    None
                }
                _ => None,
            };
            peer.tx.send(Command::Close).unwrap();
            peer.worker.take().unwrap().join().unwrap();
            let code = match change {
                "missing" => "io_error",
                "replaced" => "endpoint_replaced",
                "unsafe" => "unsafe_endpoint",
                _ if partial => "incomplete_response",
                _ => "stream_eof",
            };
            assert_eq!(
                stream.next(later(100), &NoCancel).unwrap_err().code,
                code,
                "{change}, partial={partial}"
            );
            if change == "replaced" {
                // Failed negotiation with a replacement cannot requalify the old stream.
                assert!(NativeEvents.open(&peer.path, later(5), &NoCancel).is_err());
                assert_eq!(
                    stream.next(later(100), &NoCancel).unwrap_err().code,
                    "endpoint_replaced"
                );
            }
            drop(replacement);
        }
    }
}

#[test]
fn eof_refences_parent_alias_with_unchanged_and_retargeted_controls() {
    for retarget in [false, true] {
        let mut peer = Peer::standard();
        let other = tempfile::tempdir().unwrap();
        let other_path = other.path().join("e.sock");
        let _other_listener = UnixListener::bind(&other_path).unwrap();
        std::fs::set_permissions(&other_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let alias = peer.dir.path().join("alias");
        symlink(peer.dir.path(), &alias).unwrap();
        let selected = alias.join("e.sock");
        let mut stream = NativeEvents
            .open(selected.to_str().unwrap(), later(1000), &NoCancel)
            .unwrap();
        if retarget {
            std::fs::remove_file(&alias).unwrap();
            symlink(other.path(), &alias).unwrap();
        }
        peer.tx.send(Command::Close).unwrap();
        peer.worker.take().unwrap().join().unwrap();
        assert_eq!(
            stream.next(later(100), &NoCancel).unwrap_err().code,
            if retarget {
                "endpoint_replaced"
            } else {
                "stream_eof"
            }
        );
    }
}

#[test]
fn large_frames_reserve_the_512k_turn_budget_without_losing_deferred_frames() {
    for large_frames in [2, 3] {
        let peer = Peer::standard();
        let mut stream = peer.open();
        let mut ignored = json!({"event":"future","data":{"type":"future"}});
        ignored["extension"] = json!(vec!["x".repeat(4000); 48]);
        let mut frame = line(ignored);
        frame.pop();
        frame.resize(192 * 1024 - 1, b' ');
        frame.push(b'\n');
        assert_eq!(frame.len(), 192 * 1024);
        assert!(frame.len() <= EventSocket::MAX_FRAME_BYTES);
        assert!(!project(&decode(&frame).unwrap(), &mut HashMap::new()).unwrap());
        let mut bytes = frame.repeat(large_frames);
        bytes.extend(line(updated("after deferred frames")));
        // Write concurrently: the fixture socket buffer need not hold the whole turn.
        peer.tx.send(Command::Bytes(bytes)).unwrap();
        // Two frames fit (384 KiB), but reserving a third would exceed 512 KiB.
        // A wall deadline may yield earlier, never later than that byte boundary.
        assert_eq!(stream.next(later(1000), &NoCancel).unwrap(), None);
        assert_eq!(next_hint(&mut *stream), Hint::Inventory);
        peer.done.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(stream.next(later(5), &NoCancel).unwrap(), None);
    }
}
