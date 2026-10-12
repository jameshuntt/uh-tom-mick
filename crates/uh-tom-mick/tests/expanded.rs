#![cfg(not(feature = "loom"))]
// Standard runtime integration; Loom exercises generated wrappers separately.
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Barrier,
    },
    task::{Context, Poll, Wake, Waker},
};
use uh_tom_mick::{atomic_admission, uh_tom_mick};
use uh_tom_mick::{AdmissionError, AdmissionSnapshot};

#[atomic_admission(max_active = 8)]
pub struct Gate;
#[atomic_admission(max_active = 1)]
struct OneGate;
struct Example {
    gate: Gate,
    calls: AtomicU64,
}
#[uh_tom_mick]
impl Example {
    #[uh_tom_mick_track(admission = gate)]
    pub async fn read(&self, value: u32) -> u32 {
        self.calls.fetch_add(1, Ordering::Relaxed);
        value + 1
    }
    #[uh_tom_mick_track(admission = gate)]
    async fn business_error(&self) -> Result<u32, &'static str> {
        Err("business")
    }
    #[uh_tom_mick_track(admission = gate)]
    async fn never(&self) {
        std::future::pending::<()>().await;
    }
    #[uh_tom_mick_track(admission = gate)]
    async fn blocked(&self, barrier: &Barrier) -> u32 {
        barrier.wait();
        barrier.wait();
        17
    }
    pub fn direct(&self) -> u32 {
        99
    }
}
impl Example {
    fn new() -> Self {
        Self {
            gate: Gate::new(),
            calls: AtomicU64::new(0),
        }
    }
}
struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}
fn waker() -> Waker {
    Waker::from(Arc::new(Noop))
}
fn block_on<F: Future>(future: F) -> F::Output {
    let w = waker();
    let mut c = Context::from_waker(&w);
    let mut f = Box::pin(future);
    loop {
        match f.as_mut().poll(&mut c) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
#[test]
fn generated_gate_capacity() {
    assert_eq!(Gate::new().limit(), 8);
    assert_eq!(OneGate::new().limit(), 1);
}
#[test]
fn generated_gate_default() {
    assert_eq!(Gate::default().active(), 0);
}
#[test]
fn generated_gate_initial_state() {
    let g = Gate::new();
    assert_eq!(
        g.snapshot(),
        AdmissionSnapshot {
            active: 0,
            stopping: false
        }
    );
}
#[test]
fn generated_gate_enter_and_release() {
    let g = Gate::new();
    let x = g.try_enter().unwrap();
    assert_eq!(g.active(), 1);
    drop(x);
    assert_eq!(g.active(), 0);
}
#[test]
fn generated_gate_shutdown() {
    let g = Gate::new();
    g.begin_shutdown();
    assert!(g.is_stopping());
    assert!(g.is_drained());
}
#[test]
fn original_method_remains_available() {
    let s = Example::new();
    assert_eq!(block_on(s.read(7)), 8);
    assert_eq!(s.calls.load(Ordering::Relaxed), 1);
    assert_eq!(s.gate.active(), 0);
}
#[test]
fn generated_dispatch_returns_nested_business_value() {
    let s = Example::new();
    assert_eq!(block_on(s.__uh_tom_mick_dispatch_read(8)), Ok(9));
    assert_eq!(s.calls.load(Ordering::Relaxed), 1);
    assert_eq!(s.gate.active(), 0);
}
#[test]
fn generated_dispatch_business_error_is_not_admission_error() {
    let s = Example::new();
    assert_eq!(
        block_on(s.__uh_tom_mick_dispatch_business_error()),
        Ok(Err("business"))
    );
    assert_eq!(s.gate.active(), 0);
}
#[test]
fn generated_dispatch_rejects_after_shutdown() {
    let s = Example::new();
    s.gate.begin_shutdown();
    assert_eq!(
        block_on(s.__uh_tom_mick_dispatch_read(7)),
        Err(AdmissionError::Stopping { active: 0 })
    );
    assert_eq!(s.calls.load(Ordering::Relaxed), 0);
}
#[test]
fn generated_dispatch_rejects_at_capacity() {
    let s = Example::new();
    let mut permits = Vec::new();
    for _ in 0..8 {
        permits.push(s.gate.try_enter().unwrap());
    }
    assert_eq!(
        block_on(s.__uh_tom_mick_dispatch_read(0)),
        Err(AdmissionError::AtCapacity {
            active: 8,
            limit: 8
        })
    );
    assert_eq!(s.calls.load(Ordering::Relaxed), 0);
    drop(permits);
    assert_eq!(s.gate.active(), 0);
}
#[test]
fn generated_dispatch_cancellation_decrements() {
    let s = Example::new();
    let w = waker();
    let mut c = Context::from_waker(&w);
    let mut fut = Box::pin(s.__uh_tom_mick_dispatch_never());
    assert!(matches!(Pin::as_mut(&mut fut).poll(&mut c), Poll::Pending));
    assert_eq!(s.gate.active(), 1);
    drop(fut);
    assert_eq!(s.gate.active(), 0);
}
#[test]
fn unpolled_future_never_increments() {
    let s = Example::new();
    let fut = s.__uh_tom_mick_dispatch_never();
    assert_eq!(s.gate.active(), 0);
    drop(fut);
    assert_eq!(s.gate.active(), 0);
}
#[test]
fn shutdown_blocks_future_poll_of_already_created_future() {
    let s = Example::new();
    let fut = s.__uh_tom_mick_dispatch_read(4);
    s.gate.begin_shutdown();
    assert_eq!(block_on(fut), Err(AdmissionError::Stopping { active: 0 }));
}
#[test]
fn generated_dispatch_tracks_across_await() {
    let service = Example::new();
    let barrier = Barrier::new(9);
    std::thread::scope(|scope| {
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let s = &service;
            let b = &barrier;
            tasks.push(scope.spawn(move || block_on(s.__uh_tom_mick_dispatch_blocked(b))));
        }
        barrier.wait();
        assert_eq!(service.gate.active(), 8);
        assert!(matches!(
            service.gate.try_enter(),
            Err(AdmissionError::AtCapacity { .. })
        ));
        barrier.wait();
        for t in tasks {
            assert_eq!(t.join().unwrap(), Ok(17));
        }
    });
    assert_eq!(service.gate.active(), 0);
}
#[test]
fn generated_dispatch_shutdown_during_flight() {
    let service = Example::new();
    let barrier = Barrier::new(2);
    std::thread::scope(|scope| {
        let s = &service;
        let b = &barrier;
        let task = scope.spawn(move || block_on(s.__uh_tom_mick_dispatch_blocked(b)));
        barrier.wait();
        assert_eq!(service.gate.begin_shutdown(), 1);
        assert!(service.gate.is_stopping());
        assert!(!service.gate.is_drained());
        barrier.wait();
        assert_eq!(task.join().unwrap(), Ok(17));
    });
    assert!(service.gate.is_drained());
}
#[test]
fn untracked_method_is_unmodified() {
    let s = Example::new();
    assert_eq!(s.direct(), 99);
}
#[test]
fn original_calls_do_not_implicitly_track() {
    let s = Example::new();
    assert_eq!(block_on(s.read(1)), 2);
    assert_eq!(s.gate.active(), 0);
}
#[test]
fn generated_multiple_methods_share_gate() {
    let s = Example::new();
    assert_eq!(block_on(s.__uh_tom_mick_dispatch_read(1)), Ok(2));
    assert_eq!(
        block_on(s.__uh_tom_mick_dispatch_business_error()),
        Ok(Err("business"))
    );
    assert_eq!(s.gate.active(), 0);
}
#[test]
fn generated_gate_send_sync() {
    fn require<T: Send + Sync>() {}
    require::<Gate>();
    require::<OneGate>();
}
#[test]
fn generated_dispatch_10000_cycles() {
    let s = Example::new();
    for n in 0..10000 {
        assert_eq!(block_on(s.__uh_tom_mick_dispatch_read(n)), Ok(n + 1));
    }
    assert_eq!(s.gate.active(), 0);
    assert_eq!(s.calls.load(Ordering::Relaxed), 10000);
}
