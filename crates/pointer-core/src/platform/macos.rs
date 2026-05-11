//! Main-thread dispatch for host input simulation.

use dispatch::Queue;
use libc::pthread_main_np;

pub fn run_on_main_thread_sync<R: Send, F: FnOnce() -> R + Send>(f: F) -> R {
    if unsafe { pthread_main_np() != 0 } {
        return f();
    }
    Queue::main().exec_sync(f)
}
