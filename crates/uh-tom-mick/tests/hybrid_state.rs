//! Proves that native Rust synchronization remains first class: Arc, Mutex,
//! AtomicU64, AtomicBool and a hand-constructed AtomicAdmission coexist with
//! the generated opt-in admission wrappers, with no automatic field rewriting.
#![cfg(not(feature = "loom"))]
use std::{
    future::Future,
    pin::pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Barrier, Mutex,
    },
    task::{Context, Poll, Waker},
};
use uh_tom_mick::{atomic_admission, uh_tom_mick};
use uh_tom_mick::{AdmissionError, AtomicAdmission};

#[atomic_admission(max_active = 4)]
struct GeneratedGate;

#[derive(Default)]
struct ManualStats {
    completed: AtomicU64,
    poisoned: AtomicBool,
}

struct HybridService {
    gate: Arc<GeneratedGate>,
    custom_gate: Arc<AtomicAdmission>,
    records: Arc<Mutex<Vec<u64>>>,
    counters: Arc<ManualStats>,
    local_counter: AtomicU64,
}

#[uh_tom_mick]
impl HybridService {
    #[uh_tom_mick_track(admission = gate)]
    async fn append(&self, value: u64) -> Result<usize, &'static str> {
        if value == 0 {
            return Err("business rejection");
        }
        // Never hold a std::sync::Mutex guard across an await.
        let len = {
            let mut records = self.records.lock().unwrap();
            records.push(value);
            records.len()
        };
        self.local_counter.fetch_add(1, Ordering::Relaxed);
        self.counters.completed.fetch_add(1, Ordering::Relaxed);
        Ok(len)
    }

    #[uh_tom_mick_track(admission = custom_gate)]
    async fn custom_admission(&self, value: u64) -> u64 {
        self.local_counter.fetch_add(value, Ordering::Relaxed)
    }

    #[uh_tom_mick_track(admission = gate)]
    async fn held(&self, barrier: &Barrier) -> u32 {
        barrier.wait();
        barrier.wait();
        7
    }

    #[uh_tom_mick_track(admission = gate)]
    async fn panic_inside(&self) {
        panic!("deliberate failure within an admitted operation")
    }

    // Native, hand-written methods stay native and are not implicitly gated.
    fn manual_flag(&self, value: bool) {
        self.counters.poisoned.store(value, Ordering::Release);
    }
}

fn new_hybrid() -> HybridService {
    HybridService {
        gate: Arc::new(GeneratedGate::new()),
        custom_gate: Arc::new(AtomicAdmission::new(2).unwrap()),
        records: Arc::new(Mutex::new(Vec::new())),
        counters: Arc::new(ManualStats::default()),
        local_counter: AtomicU64::new(0),
    }
}

fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("the test handler should finish during its first poll"),
    }
}

#[test]
fn native_arc_mutex_and_atomic_fields_coexist_with_generated_gate() {
    let service = new_hybrid();
    assert_eq!(ready(service.__uh_tom_mick_dispatch_append(12)), Ok(Ok(1)));
    assert_eq!(ready(service.__uh_tom_mick_dispatch_append(34)), Ok(Ok(2)));
    assert_eq!(*service.records.lock().unwrap(), vec![12, 34]);
    assert_eq!(service.local_counter.load(Ordering::Relaxed), 2);
    assert_eq!(service.counters.completed.load(Ordering::Relaxed), 2);
    assert_eq!(service.gate.active(), 0);
    assert_eq!(service.custom_gate.active(), 0);
}

#[test]
fn native_arc_clones_share_the_original_allocations() {
    let service = new_hybrid();
    let cloned = Arc::clone(&service.records);
    cloned.lock().unwrap().push(6);
    assert_eq!(ready(service.__uh_tom_mick_dispatch_append(8)), Ok(Ok(2)));
    assert_eq!(*cloned.lock().unwrap(), vec![6, 8]);
    assert_eq!(Arc::strong_count(&service.records), 2);
}

#[test]
fn hand_constructed_atomic_gate_is_supported_without_generator() {
    let service = new_hybrid();
    assert_eq!(
        ready(service.__uh_tom_mick_dispatch_custom_admission(9)),
        Ok(0)
    );
    assert_eq!(service.local_counter.load(Ordering::Relaxed), 9);
    assert_eq!(service.custom_gate.active(), 0);
    assert_eq!(service.custom_gate.limit(), 2);
}

#[test]
fn hand_constructed_gate_has_independent_shutdown() {
    let service = new_hybrid();
    service.custom_gate.begin_shutdown();
    assert!(matches!(
        ready(service.__uh_tom_mick_dispatch_custom_admission(4)),
        Err(AdmissionError::Stopping { active: 0 })
    ));
    assert_eq!(ready(service.__uh_tom_mick_dispatch_append(3)), Ok(Ok(1)));
    assert!(!service.gate.is_stopping());
}

#[test]
fn manually_written_atomic_flag_is_not_rewritten_or_hidden() {
    let service = new_hybrid();
    service.manual_flag(true);
    assert!(service.counters.poisoned.load(Ordering::Acquire));
    service.manual_flag(false);
    assert!(!service.counters.poisoned.load(Ordering::Acquire));
}

#[test]
fn business_error_does_not_poison_mutex_or_leak_admission() {
    let service = new_hybrid();
    assert_eq!(
        ready(service.__uh_tom_mick_dispatch_append(0)),
        Ok(Err("business rejection"))
    );
    assert!(service.records.lock().unwrap().is_empty());
    assert_eq!(service.gate.active(), 0);
    assert_eq!(ready(service.__uh_tom_mick_dispatch_append(1)), Ok(Ok(1)));
}

#[test]
fn panic_unwinds_the_generated_guard_without_mutex_poisoning() {
    let service = new_hybrid();
    let thrown = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = ready(service.__uh_tom_mick_dispatch_panic_inside());
    }));
    assert!(thrown.is_err());
    assert_eq!(service.gate.active(), 0);
    assert!(!service.records.is_poisoned());
}

#[test]
fn capacity_is_shared_across_generated_methods_but_not_other_manual_gate() {
    let service = new_hybrid();
    let mut permits = Vec::new();
    for _ in 0..4 {
        permits.push(service.gate.try_enter().unwrap());
    }
    assert!(matches!(
        ready(service.__uh_tom_mick_dispatch_append(12)),
        Err(AdmissionError::AtCapacity {
            active: 4,
            limit: 4
        })
    ));
    assert_eq!(
        ready(service.__uh_tom_mick_dispatch_custom_admission(2)),
        Ok(0)
    );
    drop(permits);
    assert_eq!(service.gate.active(), 0);
}

#[test]
fn four_simultaneous_handlers_hold_generated_permits() {
    let service = new_hybrid();
    let barrier = Barrier::new(5);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let service = &service;
                let barrier = &barrier;
                scope.spawn(move || ready(service.__uh_tom_mick_dispatch_held(barrier)))
            })
            .collect();
        barrier.wait();
        assert_eq!(service.gate.active(), 4);
        assert!(matches!(
            ready(service.__uh_tom_mick_dispatch_append(10)),
            Err(AdmissionError::AtCapacity {
                active: 4,
                limit: 4
            })
        ));
        barrier.wait();
        for worker in workers {
            assert_eq!(worker.join().unwrap(), Ok(7));
        }
    });
    assert_eq!(service.gate.active(), 0);
}

#[test]
fn original_method_remains_manual_and_only_generated_adapter_enforces_admission() {
    let service = new_hybrid();
    service.gate.begin_shutdown();
    // This is WHY the UDS registry must bind the generated adapter, not append().
    assert_eq!(ready(service.append(3)), Ok(1));
    assert!(matches!(
        ready(service.__uh_tom_mick_dispatch_append(4)),
        Err(AdmissionError::Stopping { active: 0 })
    ));
    assert_eq!(service.gate.active(), 0);
}
