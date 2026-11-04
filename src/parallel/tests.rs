use super::*;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn jobs(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

#[test]
fn every_index_runs_once() {
    for count in [0, 1, 100] {
        for j in [1, 3, 16] {
            let runs: Vec<AtomicUsize> = (0..count).map(|_| AtomicUsize::new(0)).collect();
            let result: Result<(), (usize, ())> = run(count, jobs(j), |i| {
                runs[i].fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
            assert!(result.is_ok());
            assert!(
                runs.iter().all(|r| r.load(Ordering::SeqCst) == 1),
                "{count} {j}"
            );
        }
    }
}

#[test]
fn one_job_runs_in_order_on_the_calling_thread() {
    let me = std::thread::current().id();
    let order = Mutex::new(Vec::new());
    let result: Result<(), (usize, ())> = run(10, jobs(1), |i| {
        assert_eq!(std::thread::current().id(), me);
        order.lock().unwrap().push(i);
        Ok(())
    });
    assert!(result.is_ok());
    assert_eq!(order.into_inner().unwrap(), (0..10).collect::<Vec<_>>());
}

#[test]
fn the_smallest_failure_is_returned_whatever_finishes_first() {
    for j in [1, 2, 8] {
        for _ in 0..50 {
            let result = run(20, jobs(j), |i| match i {
                3 => {
                    // index 3 finishes after index 7 has failed
                    std::thread::sleep(Duration::from_millis(2));
                    Err(format!("failed {i}"))
                }
                7 => Err(format!("failed {i}")),
                _ => Ok(()),
            });
            assert_eq!(result, Err((3, "failed 3".to_string())), "jobs {j}");
        }
    }
}

#[test]
fn no_new_index_is_handed_out_after_a_failure() {
    for j in [2, 8] {
        let ran = AtomicUsize::new(0);
        let result = run(1000, jobs(j), |i| {
            ran.fetch_add(1, Ordering::SeqCst);
            if i == 0 {
                Err(())
            } else {
                std::thread::sleep(Duration::from_millis(1));
                Ok(())
            }
        });
        assert_eq!(result, Err((0, ())));
        assert!(
            ran.load(Ordering::SeqCst) <= j + 1,
            "jobs {j}: {} ran",
            ran.load(Ordering::SeqCst)
        );
    }
}

#[test]
#[should_panic(expected = "boom")]
fn a_panic_reaches_the_caller() {
    let _: Result<(), (usize, ())> =
        run(4, jobs(2), |i| if i == 2 { panic!("boom") } else { Ok(()) });
}

#[test]
fn available_jobs_is_at_least_one() {
    assert!(available_jobs().get() >= 1);
}

/// A document is shared by the threads that render its pages (docs/adr/0009).
#[test]
fn a_document_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<crate::pdf::Document>();
}

#[test]
fn each_thread_keeps_one_state() {
    // docs/adr/0014: `init` runs once per thread, and the tasks of a thread
    // get its state.
    for (count, j) in [(0, 4), (1, 4), (100, 1), (100, 3), (100, 16)] {
        let inits = AtomicUsize::new(0);
        let seen: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
        let result: Result<(), (usize, ())> = run_with(
            count,
            jobs(j),
            || {
                (
                    inits.fetch_add(1, Ordering::SeqCst),
                    std::thread::current().id(),
                )
            },
            |state, i| {
                assert_eq!(state.1, std::thread::current().id());
                seen.lock().unwrap().push((state.0, i));
                Ok(())
            },
        );
        assert!(result.is_ok());
        let threads = j.min(count);
        assert!(
            inits.load(Ordering::SeqCst) <= threads.max(1),
            "{count} {j}"
        );
        let mut indices: Vec<usize> = seen.into_inner().unwrap().iter().map(|&(_, i)| i).collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..count).collect::<Vec<_>>());
    }
}
