use super::*;
use crate::observation_socket::{
    Cancel, Delivery, ReadBudget, ReadMethod, annotate, check, read, select,
};
use serde_json::json;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::time::{Duration, Instant};

static CUSTOM_CALLED: AtomicBool = AtomicBool::new(false);
extern "C" fn custom(_signal: libc::c_int) {
    CUSTOM_CALLED.store(true, Ordering::Relaxed);
}

fn action(signal: libc::c_int) -> libc::sigaction {
    let mut action = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { libc::sigaction(signal, std::ptr::null(), &mut action) },
        0
    );
    action
}

fn subprocess(case: &str) -> std::process::ExitStatus {
    let test = format!(
        "{}::subprocess_case",
        module_path!().split_once("::").unwrap().1
    );
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test, "--nocapture"])
        .env("COVEN_SOCKET_INTERRUPT_CASE", case)
        .status()
        .unwrap()
}

#[test]
fn actual_sigint_and_restoration_are_process_isolated() {
    for case in [
        "default", "custom", "nested", "error", "begin", "read", "write", "send", "sleep",
    ] {
        assert!(subprocess(case).success(), "{case}");
    }
}

#[test]
fn default_sigint_disposition_is_effective_after_drop() {
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(subprocess("default_death").signal(), Some(libc::SIGINT));
}

#[test]
fn guard_is_neither_send_nor_sync() {
    trait AmbiguousSend<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousSend<u8> for T {}
    let _ = <InterruptGuard as AmbiguousSend<_>>::marker;
    trait AmbiguousSync<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousSync<()> for T {}
    impl<T: ?Sized + Sync> AmbiguousSync<u8> for T {}
    let _ = <InterruptGuard as AmbiguousSync<_>>::marker;
}

#[test]
fn subprocess_case() {
    let Ok(case) = std::env::var("COVEN_SOCKET_INTERRUPT_CASE") else {
        return;
    };
    let child_before = action(libc::SIGCHLD);
    // Deliberately establish a known prior action only in this disposable process.
    let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
    previous.sa_sigaction = if case == "custom" {
        custom as *const () as usize
    } else {
        libc::SIG_DFL
    };
    previous.sa_flags = libc::SA_RESTART;
    unsafe {
        libc::sigemptyset(&mut previous.sa_mask);
        libc::sigaddset(&mut previous.sa_mask, libc::SIGUSR1);
    }
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGINT, &previous, std::ptr::null_mut()) },
        0
    );
    let before = action(libc::SIGINT);
    match case.as_str() {
        "default" | "custom" | "default_death" => {
            let guard = crate::observation_socket::InterruptGuard::install().unwrap();
            assert!(!Cancel::interrupted(&guard));
            assert_eq!(unsafe { libc::raise(libc::SIGINT) }, 0);
            assert!(Cancel::interrupted(&guard));
            assert!(!CUSTOM_CALLED.load(Ordering::Relaxed));
            drop(guard);
            if case == "custom" {
                assert_eq!(unsafe { libc::raise(libc::SIGINT) }, 0);
                assert!(CUSTOM_CALLED.load(Ordering::Relaxed));
            } else if case == "default_death" {
                unsafe {
                    libc::raise(libc::SIGINT);
                }
                panic!("default SIGINT handler was not restored");
            }
        }
        "nested" => {
            let guard = InterruptGuard::install().unwrap();
            let installed = action(libc::SIGINT);
            assert!(InterruptGuard::install().is_err());
            assert!(
                std::thread::spawn(|| InterruptGuard::install().is_err())
                    .join()
                    .unwrap()
            );
            assert_eq!(action(libc::SIGINT).sa_sigaction, installed.sa_sigaction);
            unsafe {
                libc::raise(libc::SIGINT);
            }
            assert!(guard.interrupted());
            drop(guard);
            let guard = InterruptGuard::install().unwrap();
            assert!(!guard.interrupted());
        }
        "error" => {
            fn fail() -> crate::Result<()> {
                let _guard = InterruptGuard::install()?;
                Err(crate::Error("expected_test_error"))
            }
            assert!(fail().is_err());
            assert!(
                std::panic::catch_unwind(|| {
                    let _guard = InterruptGuard::install().unwrap();
                    panic!("test-only unwind");
                })
                .is_err()
            );
        }
        "begin" => {
            let guard = InterruptGuard::install().unwrap();
            unsafe {
                libc::raise(libc::SIGINT);
            }
            let failure = annotate(
                "/no-endpoint-is-opened",
                &json!(null),
                &mut json!({}),
                Instant::now() + Duration::from_secs(1),
                1024,
                &guard,
                &mut |_| panic!("already interrupted"),
            )
            .unwrap_err();
            assert_eq!(failure.code, "interrupted");
            assert!(failure.interrupted);
            assert_eq!(failure.delivery, Delivery::Unsent);
        }
        "read" | "write" => {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("api.sock");
            let listener = UnixListener::bind(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            let (done, wait) = std::sync::mpsc::channel();
            let owner = unsafe { libc::pthread_self() } as usize;
            let worker = std::thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(&stream).read_line(&mut request).unwrap();
                assert_eq!(
                    unsafe { libc::pthread_kill(owner as libc::pthread_t, libc::SIGINT) },
                    0
                );
                let _ = wait.recv_timeout(Duration::from_secs(2));
            });
            let guard = InterruptGuard::install().unwrap();
            let path = path.to_str().unwrap();
            let deadline = Instant::now() + Duration::from_secs(1);
            let failure = if case == "read" {
                read(
                    path,
                    ReadMethod::Snapshot,
                    &json!({}),
                    deadline,
                    &mut ReadBudget::new(1024),
                    &guard,
                )
                .unwrap_err()
            } else {
                annotate(
                    path,
                    &select(path).unwrap().identity().unwrap(),
                    &mut json!({}),
                    deadline,
                    1024,
                    &guard,
                    &mut |_| Ok(()),
                )
                .unwrap_err()
            };
            assert_eq!(failure.code, "interrupted");
            assert!(failure.interrupted);
            assert_eq!(
                failure.delivery,
                if case == "read" {
                    Delivery::Unsent
                } else {
                    Delivery::Uncertain
                }
            );
            done.send(()).unwrap();
            worker.join().unwrap();
        }
        "send" => {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("api.sock");
            let listener = UnixListener::bind(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            let guard = InterruptGuard::install().unwrap();
            let endpoint = select(path.to_str().unwrap()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(1);
            let socket = super::super::Socket::connect(&endpoint, deadline, &guard).unwrap();
            let (stream, _) = listener.accept().unwrap();
            // Force a pending nonblocking send without depending on host buffer sizes.
            use std::os::fd::AsRawFd;
            let size: libc::c_int = 1024;
            assert_eq!(
                unsafe {
                    libc::setsockopt(
                        socket.0.as_raw_fd(),
                        libc::SOL_SOCKET,
                        libc::SO_SNDBUF,
                        (&size as *const libc::c_int).cast(),
                        std::mem::size_of_val(&size) as libc::socklen_t,
                    )
                },
                0
            );
            let owner = unsafe { libc::pthread_self() } as usize;
            let worker = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(30));
                assert_eq!(
                    unsafe { libc::pthread_kill(owner as libc::pthread_t, libc::SIGINT) },
                    0
                );
            });
            let failure = socket
                .send(&vec![b' '; 512 * 1024], deadline, &guard)
                .unwrap_err();
            assert_eq!(failure.code, "interrupted");
            assert!(failure.interrupted);
            worker.join().unwrap();
            drop(stream);
        }
        "sleep" => {
            let guard = InterruptGuard::install().unwrap();
            let worker = std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(15));
                assert_eq!(unsafe { libc::kill(libc::getpid(), libc::SIGINT) }, 0);
            });
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                if let Err(failure) = check(deadline, &guard) {
                    assert_eq!(failure.code, "interrupted");
                    assert!(failure.interrupted);
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            worker.join().unwrap();
        }
        _ => panic!("unknown interrupt scenario"),
    }
    let after = action(libc::SIGINT);
    assert_eq!(after.sa_sigaction, before.sa_sigaction);
    assert_eq!(after.sa_flags, before.sa_flags);
    assert_eq!(
        unsafe { libc::sigismember(&after.sa_mask, libc::SIGUSR1) },
        1
    );
    let child_after = action(libc::SIGCHLD);
    assert_eq!(child_after.sa_sigaction, child_before.sa_sigaction);
    assert_eq!(child_after.sa_flags, child_before.sa_flags);
}
