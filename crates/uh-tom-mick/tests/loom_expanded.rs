//! Verify the generated dispatch API against the Loom-instrumented core.
//! This exercises actual proc-macro output, not a textual expansion sketch.
#![cfg(feature = "loom")]

use loom::{model::Builder, sync::Arc, thread};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use uh_tom_mick::{atomic_admission, uh_tom_mick};
use uh_tom_mick::{AdmissionError, AdmissionSnapshot};

#[atomic_admission(max_active = 1)]
struct Gate;
struct Service {
    gate: Gate,
}
#[uh_tom_mick]
impl Service {
    #[uh_tom_mick_track(admission = gate)]
    async fn do_work(&self) -> u32 {
        thread::yield_now();
        17
    }
    #[uh_tom_mick_track(admission = gate)]
    async fn never_finishes(&self) {
        std::future::pending::<()>().await;
    }
}
fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("this test handler must finish after its first poll"),
    }
}
fn exhaustive(f: impl Fn() + Sync + Send + 'static) {
    let mut model = Builder::new();
    model.max_branches = 2_000;
    // Do not cap max_permutations or preemptions: these are finite model checks.
    model.check(f);
}

#[test]
fn generated_adapter_competes_with_shutdown_in_all_small_schedules() {
    exhaustive(|| {
        let service = Arc::new(Service { gate: Gate::new() });
        let worker = service.clone();
        let stopping = service.clone();
        let a = thread::spawn(
            move || match ready(worker.__uh_tom_mick_dispatch_do_work()) {
                Ok(17) | Err(AdmissionError::Stopping { .. }) => (),
                other => panic!("unexpected dispatch result: {other:?}"),
            },
        );
        let b = thread::spawn(move || {
            stopping.gate.begin_shutdown();
        });
        a.join().unwrap();
        b.join().unwrap();
        assert!(service.gate.is_drained());
        assert!(matches!(
            ready(service.__uh_tom_mick_dispatch_do_work()),
            Err(AdmissionError::Stopping { active: 0 })
        ));
    });
}

/// First poll and shutdown genuinely race. Both linearization orders must pass:
/// - admission wins => Pending with one live guard, released on cancellation;
/// - shutdown wins => Ready(Err(Stopping)), no guard ever acquired.
#[test]
fn generated_guard_stays_held_across_pending_and_releases_on_cancel() {
    exhaustive(|| {
        let service = Arc::new(Service { gate: Gate::new() });
        let worker = service.clone();
        let stopper = service.clone();
        let a = thread::spawn(move || {
            let mut future = Box::pin(worker.__uh_tom_mick_dispatch_never_finishes());
            let mut context = Context::from_waker(Waker::noop());
            match future.as_mut().poll(&mut context) {
                Poll::Pending => {
                    // The generated wrapper must hold the permit while the handler
                    // is suspended, even if begin_shutdown() races with this check.
                    assert_eq!(worker.gate.active(), 1);
                    thread::yield_now();
                    assert_eq!(worker.gate.active(), 1);
                    drop(future); // Cancellation: drop the generated admission guard.
                    assert_eq!(worker.gate.active(), 0);
                }
                Poll::Ready(Err(AdmissionError::Stopping { active: 0 })) => {
                    // Shutdown linearized before the first poll. The handler was
                    // never entered, so Pending would be an incorrect requirement.
                    assert_eq!(worker.gate.active(), 0);
                    drop(future);
                }
                Poll::Ready(other) => panic!("unexpected never-finishes result: {other:?}"),
            }
        });
        let b = thread::spawn(move || {
            stopper.gate.begin_shutdown();
        });
        a.join().unwrap();
        b.join().unwrap();
        assert_eq!(
            service.gate.snapshot(),
            AdmissionSnapshot {
                active: 0,
                stopping: true,
            }
        );
        assert!(service.gate.is_drained());
    });
}

/// Explicitly establish successful admission before starting the shutdown race;
/// unlike the preceding test, a Pending poll is now a valid required outcome.
#[test]
fn generated_pending_guard_survives_shutdown_then_cancel_all_schedules() {
    exhaustive(|| {
        let service = Arc::new(Service { gate: Gate::new() });
        let mut future = Box::pin(service.__uh_tom_mick_dispatch_never_finishes());
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
        assert_eq!(service.gate.active(), 1);
        let stopper = service.clone();
        let b = thread::spawn(move || stopper.gate.begin_shutdown());
        thread::yield_now();
        // Shutdown must not steal or forcibly release an admitted operation.
        assert_eq!(service.gate.active(), 1);
        assert_eq!(b.join().unwrap(), 1);
        assert_eq!(
            service.gate.snapshot(),
            AdmissionSnapshot {
                active: 1,
                stopping: true,
            }
        );
        assert!(!service.gate.is_drained());
        drop(future);
        assert!(service.gate.is_drained());
        assert_eq!(service.gate.active(), 0);
    });
}

/// Constructing a future does not admit it. If shutdown happens before its
/// initial poll, the adapter returns Stopping without running the handler.
#[test]
fn generated_shutdown_before_first_poll_rejects_without_guard() {
    exhaustive(|| {
        let service = Arc::new(Service { gate: Gate::new() });
        let mut future = Box::pin(service.__uh_tom_mick_dispatch_never_finishes());
        assert_eq!(service.gate.active(), 0);
        assert_eq!(service.gate.begin_shutdown(), 0);
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(
            future.as_mut().poll(&mut context),
            Poll::Ready(Err(AdmissionError::Stopping { active: 0 }))
        ));
        assert_eq!(service.gate.active(), 0);
        drop(future);
        assert!(service.gate.is_drained());
    });
}
