//! Running numbered tasks on threads (docs/adr/0009, docs/adr/0012).
//!
//! Indices are handed out in increasing order, each to one thread. After a
//! task fails, no new index is handed out, and the tasks already running
//! finish. The error returned is the one of the smallest failed index: that
//! index was handed out before any larger failed one, and every task handed
//! out runs to its end, so it does not depend on the number of threads or on
//! the order in which they finish.

use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Runs `task(0)` … `task(count - 1)` on at most `jobs` threads, and returns
/// the error of the smallest failed index.
pub fn run<E, F>(count: usize, jobs: NonZeroUsize, task: F) -> Result<(), (usize, E)>
where
    E: Send,
    F: Fn(usize) -> Result<(), E> + Sync,
{
    run_with(count, jobs, || (), |(), i| task(i))
}

/// [`run`] with a state for each thread: `init` makes it when the thread
/// starts, and `task(state, i)` gets the state of its thread.
pub fn run_with<S, E, I, F>(
    count: usize,
    jobs: NonZeroUsize,
    init: I,
    task: F,
) -> Result<(), (usize, E)>
where
    E: Send,
    I: Fn() -> S + Sync,
    F: Fn(&mut S, usize) -> Result<(), E> + Sync,
{
    let threads = jobs.get().min(count);
    if threads <= 1 {
        let mut state = init();
        return (0..count).try_for_each(|i| task(&mut state, i).map_err(|e| (i, e)));
    }
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let failures: Mutex<Vec<(usize, E)>> = Mutex::new(Vec::new());
    let panic = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut state = init();
                    while !stop.load(Ordering::SeqCst) {
                        let i = next.fetch_add(1, Ordering::SeqCst);
                        if i >= count {
                            break;
                        }
                        if let Err(e) = task(&mut state, i) {
                            stop.store(true, Ordering::SeqCst);
                            failures
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .push((i, e));
                        }
                    }
                })
            })
            .collect();
        // Joining here keeps the payload of a panic, which scope would replace.
        let panics: Vec<_> = handles.into_iter().filter_map(|h| h.join().err()).collect();
        panics.into_iter().next()
    });
    if let Some(payload) = panic {
        std::panic::resume_unwind(payload);
    }
    let failures = failures.into_inner().unwrap_or_else(|p| p.into_inner());
    match failures.into_iter().min_by_key(|(i, _)| *i) {
        Some(f) => Err(f),
        None => Ok(()),
    }
}

/// The number of threads the machine can run at once, or 1 if it is unknown.
pub fn available_jobs() -> NonZeroUsize {
    std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN)
}

#[cfg(test)]
mod tests;
