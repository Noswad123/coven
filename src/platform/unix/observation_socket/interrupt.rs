use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::ThreadId;

static INSTALLED: AtomicBool = AtomicBool::new(false);
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn interrupt(_signal: libc::c_int) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

/// Scoped process-wide SIGINT ownership. No fork is supported while installed;
/// inherited copies fail closed and must never restore a parent's handler state.
pub(crate) struct InterruptGuard {
    previous: libc::sigaction,
    pid: libc::pid_t,
    thread: ThreadId,
    _local: PhantomData<Rc<()>>,
}

impl InterruptGuard {
    pub(crate) fn install() -> crate::Result<Self> {
        INSTALLED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| crate::Error("publisher_interrupt_error"))?;
        INTERRUPTED.store(false, Ordering::Relaxed);
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = interrupt as *const () as usize;
        // No SA_RESTART: EINTR must return control to deadline/cancellation checks.
        if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0
            || unsafe { libc::sigaction(libc::SIGINT, &action, &mut previous) } != 0
        {
            INSTALLED.store(false, Ordering::Release);
            return Err(crate::Error("publisher_interrupt_error"));
        }
        Ok(Self {
            previous,
            pid: unsafe { libc::getpid() },
            thread: std::thread::current().id(),
            _local: PhantomData,
        })
    }

    pub(crate) fn interrupted(&self) -> bool {
        self.pid != unsafe { libc::getpid() }
            || self.thread != std::thread::current().id()
            || INTERRUPTED.load(Ordering::Relaxed)
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        if self.pid != unsafe { libc::getpid() } {
            return;
        }
        if self.thread != std::thread::current().id() {
            return;
        }
        // Keep installation reserved on an impossible-to-recover restoration error
        // rather than allowing another guard to overwrite the saved disposition.
        if unsafe { libc::sigaction(libc::SIGINT, &self.previous, std::ptr::null_mut()) } == 0 {
            INSTALLED.store(false, Ordering::Release);
        }
    }
}

#[cfg(test)]
#[path = "interrupt_tests.rs"]
mod tests;
