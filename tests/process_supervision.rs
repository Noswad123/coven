//! The runner is deliberately crate-private. Run its library acceptance test
//! with Cargo's actual application binary, never a libtest supervisor executable.
#![cfg(unix)]
use std::{
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn private_runner_uses_actual_application_binary() {
    // --lib avoids recursively executing this integration bridge. Let Cargo
    // select/build its current harness rather than discovering stale artifacts.
    let mut child = Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "test",
            "--offline",
            "--locked",
            "--lib",
            "platform::unix::process::tests::real_binary_supervision",
            "--",
            "--exact",
            "--nocapture",
        ])
        .env("COVEN_TEST_SUPERVISOR", env!("CARGO_BIN_EXE_coven"))
        .env("CARGO_BUILD_JOBS", "2")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("process acceptance harness exceeded 90 seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
