//! Main-thread dispatch for host input simulation and AppKit-only OS surfaces.

use anyhow::Result;
use dispatch::Queue;
use libc::pthread_main_np;
use std::sync::mpsc;
use std::time::Duration;

/// How long to wait for the main thread before giving up.
///
/// Clipboard and input work takes microseconds; anything approaching this bound
/// means the main thread is wedged (modal sheet, debugger breakpoint, or a test
/// process with no run loop at all). Failing with an error beats blocking the
/// caller — and with it the whole agent — forever.
const MAIN_THREAD_TIMEOUT: Duration = Duration::from_secs(5);

pub fn run_on_main_thread_sync<R: Send, F: FnOnce() -> R + Send>(f: F) -> R {
    if unsafe { pthread_main_np() != 0 } {
        return f();
    }
    Queue::main().exec_sync(f)
}

/// Run `f` on the main thread and return its value.
///
/// Unlike [`run_on_main_thread_sync`], which blocks until the main queue drains,
/// this posts the work and waits with a timeout: a main thread that never runs
/// the block yields an error instead of hanging the caller forever.
pub fn run_on_main_thread<T, F>(f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    if unsafe { pthread_main_np() != 0 } {
        log::trace!("main_thread: already on the main thread, running inline");
        return Ok(f());
    }
    log::debug!("main_thread: dispatching work to the main queue");
    run_on_queue(&Queue::main(), MAIN_THREAD_TIMEOUT, f)
}

/// Post `f` to `queue` and wait for its value.
///
/// Split out from [`run_on_main_thread`] so the wait/timeout behaviour can be
/// tested against a private queue: `cargo test` runs every case on a spawned
/// thread and never drains the main queue, so a test that used the main queue
/// would block until the timeout.
fn run_on_queue<T, F>(queue: &Queue, timeout: Duration, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    queue.exec_async(move || {
        // A panic escaping this block would unwind through the C trampoline and
        // abort the process, so contain it here; dropping `tx` without sending
        // turns it into an error on the caller's side.
        if let Ok(value) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            // The receiver is gone after a timeout — not worth reporting.
            let _ = tx.send(value);
        }
    });
    rx.recv_timeout(timeout).map_err(|err| match err {
        mpsc::RecvTimeoutError::Timeout => anyhow::anyhow!(
            "main thread did not run the task within {timeout:?}; is its run loop blocked?"
        ),
        mpsc::RecvTimeoutError::Disconnected => {
            anyhow::anyhow!("main thread task was dropped before returning a value")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dispatch::QueueAttribute;

    fn serial_queue(name: &str) -> Queue {
        Queue::create(name, QueueAttribute::Serial)
    }

    #[test]
    fn delivers_the_value_back_to_the_caller() {
        let queue = serial_queue("pointer-core.main-thread.delivers");
        let got = run_on_queue(&queue, Duration::from_secs(5), || 7u32).unwrap();
        assert_eq!(got, 7);
    }

    #[test]
    fn times_out_when_the_queue_never_runs_the_block() {
        let queue = serial_queue("pointer-core.main-thread.blocked");
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        queue.exec_async(move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
        });
        started_rx.recv().unwrap();

        let err = run_on_queue(&queue, Duration::from_millis(50), || 1u32).unwrap_err();
        assert!(err.to_string().contains("did not run the task"), "{err}");

        release_tx.send(()).unwrap();
    }

    #[test]
    fn a_panicking_task_becomes_an_error_instead_of_an_abort() {
        let queue = serial_queue("pointer-core.main-thread.panic");
        let err = run_on_queue(&queue, Duration::from_secs(5), || -> u32 {
            panic!("boom")
        })
        .unwrap_err();
        assert!(err.to_string().contains("dropped"), "{err}");
    }
}
