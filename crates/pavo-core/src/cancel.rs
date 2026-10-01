use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use anyhow::{bail, Result};

static CANCELLED: AtomicBool = AtomicBool::new(false);
static CHILD: AtomicI32 = AtomicI32::new(0);

/// Stop whatever is running. Safe to call from any thread.
pub fn cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
    let pid = CHILD.load(Ordering::SeqCst);
    if pid > 0 {
        // SAFETY: plain syscall on a pid we spawned
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
}

pub(crate) fn reset() {
    CANCELLED.store(false, Ordering::SeqCst);
}

pub(crate) fn check() -> Result<()> {
    if CANCELLED.load(Ordering::SeqCst) {
        bail!("cancelled");
    }
    Ok(())
}

pub(crate) fn set_child(pid: u32) {
    CHILD.store(pid as i32, Ordering::SeqCst);
}

pub(crate) fn clear_child() {
    CHILD.store(0, Ordering::SeqCst);
}
