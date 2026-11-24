//! Bounded, content-free foreground invalidations. A wake token is not an event queue.
use http_body_util::Full;
use hyper::body::{Body, Bytes, Frame, SizeHint};
use std::{
    collections::HashMap,
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU8, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    sync::mpsc,
    time::{Instant, Sleep},
};

const FLEET: u8 = 1;
const METADATA: u8 = 2;
const READY: &[u8] = b"event: ready\ndata: {\"contract\":\"coven.events.v1\"}\n\n";
const FLEET_FRAME: &[u8] = b"event: fleet\ndata: {\"contract\":\"coven.events.v1\"}\n\n";
const METADATA_FRAME: &[u8] = b"event: metadata\ndata: {\"contract\":\"coven.events.v1\"}\n\n";

struct Wake {
    dirty: AtomicU8,
    tx: mpsc::Sender<()>,
}
#[derive(Default)]
struct State {
    serial: u64,
    streams: HashMap<u64, (String, Arc<Wake>)>,
}
#[derive(Default)]
pub(super) struct Events(Mutex<State>, Mutex<Weak<super::metadata::MetadataWake>>);
impl Events {
    // Internal inventory invalidation is deliberately separate from browser delivery.
    pub(super) fn wake_metadata(&self) {
        let wake = self.1.lock().unwrap_or_else(|p| p.into_inner()).upgrade();
        if let Some(wake) = wake {
            wake.wake();
        }
    }
    pub(super) fn register_metadata(&self, wake: &Arc<super::metadata::MetadataWake>) -> bool {
        let mut registered = self.1.lock().unwrap_or_else(|p| p.into_inner());
        if registered.upgrade().is_some() {
            return false;
        }
        *registered = Arc::downgrade(wake);
        true
    }
    pub(super) fn unregister_metadata(&self, wake: &Arc<super::metadata::MetadataWake>) {
        let mut registered = self.1.lock().unwrap_or_else(|p| p.into_inner());
        if registered.ptr_eq(&Arc::downgrade(wake)) {
            *registered = Weak::new();
        }
    }
    pub(super) fn notify(&self, login: Option<&str>) {
        let state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        for (owner, wake) in state.streams.values() {
            if login.is_none_or(|login| login == owner) {
                wake.dirty.fetch_or(
                    if login.is_some() { METADATA } else { FLEET },
                    Ordering::Release,
                );
                let _ = wake.tx.try_send(());
            }
        }
    }
    pub(super) fn admit(self: &Arc<Self>, login: &str) -> Option<Sse> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.streams.len() >= 16
            || state
                .streams
                .values()
                .filter(|(owner, _)| owner == login)
                .count()
                >= 2
        {
            return None;
        }
        let (tx, rx) = mpsc::channel(1);
        let wake = Arc::new(Wake {
            dirty: AtomicU8::new(0),
            tx,
        });
        state.serial += 1;
        let id = state.serial;
        state.streams.insert(id, (login.into(), wake.clone()));
        Some(Sse {
            events: self.clone(),
            id,
            wake,
            rx,
            ready: false,
            pending: 0,
            heartbeat: Box::pin(tokio::time::sleep(Duration::from_secs(10))),
            expires: Box::pin(tokio::time::sleep(Duration::from_secs(60))),
        })
    }
}

pub(super) struct Sse {
    events: Arc<Events>,
    id: u64,
    wake: Arc<Wake>,
    rx: mpsc::Receiver<()>,
    ready: bool,
    pending: u8,
    heartbeat: Pin<Box<Sleep>>,
    expires: Pin<Box<Sleep>>,
}
impl Drop for Sse {
    fn drop(&mut self) {
        self.events
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .streams
            .remove(&self.id);
    }
}
pub(super) enum StreamingBody {
    Full(Full<Bytes>),
    Sse(Sse),
}
impl Body for StreamingBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        let this = self.get_mut();
        let s = match this {
            Self::Full(body) => return Pin::new(body).poll_frame(cx),
            Self::Sse(s) => s,
        };
        if s.expires.deadline() <= Instant::now() || s.expires.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        let bytes = if !s.ready {
            s.ready = true;
            READY
        } else if s.heartbeat.deadline() <= Instant::now()
            || s.heartbeat.as_mut().poll(cx).is_ready()
        {
            s.heartbeat
                .as_mut()
                .reset(Instant::now() + Duration::from_secs(10));
            b": heartbeat\n\n"
        } else {
            let _ = s.rx.poll_recv(cx);
            s.pending |= s.wake.dirty.swap(0, Ordering::AcqRel);
            if s.pending & FLEET != 0 {
                s.pending &= !FLEET;
                FLEET_FRAME
            } else if s.pending & METADATA != 0 {
                s.pending &= !METADATA;
                METADATA_FRAME
            } else {
                return Poll::Pending;
            }
        };
        Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(bytes)))))
    }
    fn is_end_stream(&self) -> bool {
        match self {
            Self::Full(b) => b.is_end_stream(),
            Self::Sse(_) => false,
        }
    }
    fn size_hint(&self) -> SizeHint {
        match self {
            Self::Full(b) => b.size_hint(),
            Self::Sse(_) => SizeHint::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }
    async fn frame(body: &mut StreamingBody) -> Bytes {
        body.frame().await.unwrap().unwrap().into_data().unwrap()
    }
    #[test]
    fn ready_first_scoped_coalesced_static_and_drop_releases() {
        runtime().block_on(async {
            let events = Arc::new(Events::default());
            let mut a = StreamingBody::Sse(events.admit("alice").unwrap());
            let mut b = StreamingBody::Sse(events.admit("bob").unwrap());
            for _ in 0..10000 {
                events.notify(None);
                events.notify(Some("alice"));
            }
            assert_eq!(frame(&mut a).await, READY);
            assert_eq!(frame(&mut b).await, READY);
            assert_eq!(frame(&mut a).await, FLEET_FRAME);
            assert_eq!(frame(&mut a).await, METADATA_FRAME);
            assert_eq!(frame(&mut b).await, FLEET_FRAME);
            assert!(
                tokio::time::timeout(Duration::from_millis(5), b.frame())
                    .await
                    .is_err()
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(5), a.frame())
                    .await
                    .is_err()
            );
            for bytes in [READY, FLEET_FRAME, METADATA_FRAME] {
                assert!(bytes.len() < 80);
                let text = std::str::from_utf8(bytes).unwrap();
                let data = text.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(data).unwrap(),
                    serde_json::json!({"contract":"coven.events.v1"})
                );
                assert!(!text.contains("id:"));
            }
            drop(a);
            drop(b);
            assert!(events.0.lock().unwrap().streams.is_empty());
        });
    }
    #[test]
    fn admission_is_atomic_global_and_per_principal() {
        let runtime = runtime();
        let _entered = runtime.enter();
        let events = Arc::new(Events::default());
        let a = events.admit("alice").unwrap();
        let b = events.admit("alice").unwrap();
        assert!(events.admit("alice").is_none());
        drop(a);
        let a = events.admit("alice").unwrap();
        let streams = (0..14)
            .map(|i| events.admit(&format!("owner{i}")).unwrap())
            .collect::<Vec<_>>();
        assert!(events.admit("extra").is_none());
        drop(streams);
        drop(a);
        drop(b);
        assert!(events.0.lock().unwrap().streams.is_empty());
        let streams = std::thread::scope(|scope| {
            let handles = (0..32)
                .map(|_| {
                    let events = &events;
                    let runtime = &runtime;
                    scope.spawn(move || {
                        let _entered = runtime.enter();
                        events.admit("alice")
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .filter_map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(streams.len(), 2);
    }
    #[test]
    fn heartbeat_and_lifetime_are_independent_of_notifications() {
        runtime().block_on(async {
            let events = Arc::new(Events::default());
            let mut body = StreamingBody::Sse(events.admit("alice").unwrap());
            assert_eq!(frame(&mut body).await, READY);
            if let StreamingBody::Sse(s) = &mut body {
                s.heartbeat.as_mut().reset(Instant::now());
            }
            // A dirty client cannot starve the heartbeat indefinitely.
            events.notify(None);
            assert_eq!(frame(&mut body).await, b": heartbeat\n\n".as_slice());
            assert_eq!(frame(&mut body).await, FLEET_FRAME);
            if let StreamingBody::Sse(s) = &mut body {
                assert!(s.heartbeat.deadline() > Instant::now() + Duration::from_secs(9));
                s.expires.as_mut().reset(Instant::now());
            }
            events.notify(None);
            assert!(body.frame().await.is_none());
            drop(body);
            assert!(events.0.lock().unwrap().streams.is_empty());
        });
    }
}
