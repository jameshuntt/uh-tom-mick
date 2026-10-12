#![cfg(not(feature = "loom"))]
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use uh_tom_mick::{atomic_admission, atomic_counter, atomic_flags, uh_tom_mick};
use uh_tom_mick::{CounterError, FlagSetError};

#[atomic_counter]
struct Requests;
#[atomic_counter(initial = 7)]
pub struct Completed;
#[atomic_flags]
pub enum WorkerFlags {
    Ready,
    Stopping,
    Faulted,
}
#[atomic_flags]
enum OneFlag {
    Active,
}
#[atomic_admission(max_active = 2)]
struct Gate;
struct Hybrid {
    gate: Arc<Gate>,
    requests: Arc<Requests>,
    completed: Completed,
    flags: Arc<AtomicWorkerFlags>,
    records: Arc<Mutex<Vec<u64>>>,
    manual: AtomicU64,
    manual_bool: AtomicBool,
}
#[uh_tom_mick]
impl Hybrid {
    #[uh_tom_mick_track(admission = gate)]
    async fn append(&self, v: u64) -> usize {
        let len = {
            let mut guard = self.records.lock().unwrap();
            guard.push(v);
            guard.len()
        };
        self.requests.increment().unwrap();
        self.completed.increment().unwrap();
        self.manual.fetch_add(v, Ordering::Relaxed);
        self.manual_bool.store(true, Ordering::Release);
        self.flags.enable(WorkerFlags::Ready);
        len
    }
}
fn ready<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut c = std::task::Context::from_waker(std::task::Waker::noop());
    match f.as_mut().poll(&mut c) {
        std::task::Poll::Ready(v) => v,
        std::task::Poll::Pending => panic!("unexpected pending"),
    }
}
fn hybrid() -> Hybrid {
    Hybrid {
        gate: Arc::new(Gate::new()),
        requests: Arc::new(Requests::new()),
        completed: Completed::new(),
        flags: Arc::new(AtomicWorkerFlags::new()),
        records: Arc::new(Mutex::new(Vec::new())),
        manual: AtomicU64::new(0),
        manual_bool: AtomicBool::new(false),
    }
}
#[test]
fn counter_marker_default() {
    let c = Requests::new();
    assert_eq!(c.load(), 0);
    assert_eq!(c.increment(), Ok(1));
}
#[test]
fn counter_marker_custom_initial() {
    let c = Completed::new();
    assert_eq!(c.load(), 7);
    assert_eq!(c.decrement(), Ok(6));
}
#[test]
fn counter_marker_with_initial_override() {
    let c = Completed::with_initial(u64::MAX);
    assert_eq!(c.load(), u64::MAX);
    assert_eq!(c.increment(), Err(CounterError::Overflow));
}
#[test]
fn counter_marker_checked_sub_and_reset() {
    let c = Requests::with_initial(3);
    assert_eq!(c.subtract(4), Err(CounterError::Underflow));
    assert_eq!(c.reset(), 3);
    assert_eq!(c.load(), 0);
}
#[test]
fn counter_marker_default_trait() {
    let c = Requests::default();
    assert_eq!(c.load(), 0);
}
#[test]
fn flag_mask_generated_from_enum() {
    assert_eq!(WorkerFlags::Ready.bit(), 1);
    assert_eq!(WorkerFlags::Stopping.bit(), 2);
    assert_eq!(WorkerFlags::Faulted.bit(), 4);
    assert_eq!(AtomicWorkerFlags::KNOWN_MASK, 7);
}
#[test]
fn flag_empty_start_and_default() {
    let f = AtomicWorkerFlags::default();
    assert_eq!(f.bits(), 0);
    assert!(!f.is_set(WorkerFlags::Ready));
}
#[test]
fn flag_enable_reports_prior_state() {
    let f = AtomicWorkerFlags::new();
    assert!(!f.enable(WorkerFlags::Ready));
    assert!(f.enable(WorkerFlags::Ready));
    assert_eq!(f.bits(), 1);
}
#[test]
fn flag_disable_reports_prior_state() {
    let f = AtomicWorkerFlags::with_bits(3).unwrap();
    assert!(f.disable(WorkerFlags::Ready));
    assert!(!f.disable(WorkerFlags::Ready));
    assert_eq!(f.bits(), 2);
}
#[test]
fn flag_toggle_reports_new_state() {
    let f = AtomicWorkerFlags::new();
    assert!(f.toggle(WorkerFlags::Ready));
    assert!(!f.toggle(WorkerFlags::Ready));
    assert_eq!(f.bits(), 0);
}
#[test]
fn flag_all_any_typed() {
    let f = AtomicWorkerFlags::with_bits(5).unwrap();
    assert!(f.all(&[WorkerFlags::Ready, WorkerFlags::Faulted]));
    assert!(f.any(&[WorkerFlags::Stopping, WorkerFlags::Faulted]));
    assert!(!f.all(&[WorkerFlags::Ready, WorkerFlags::Stopping]));
}
#[test]
fn flag_empty_slice_semantics() {
    let f = AtomicWorkerFlags::new();
    assert!(f.all(&[]));
    assert!(!f.any(&[]));
}
#[test]
fn flag_with_bits_rejects_unknown() {
    assert!(matches!(
        AtomicWorkerFlags::with_bits(8),
        Err(FlagSetError::UnknownBits { bits: 8 })
    ));
}
#[test]
fn flag_whole_state_cas() {
    let f = AtomicWorkerFlags::new();
    assert_eq!(f.compare_exchange(0, 5), Ok(Ok(0)));
    assert_eq!(f.compare_exchange(0, 2), Ok(Err(5)));
}
#[test]
fn flag_one_variant() {
    let f = AtomicOneFlag::new();
    assert_eq!(OneFlag::Active.bit(), 1);
    assert!(!f.enable(OneFlag::Active));
    assert_eq!(f.bits(), 1);
}
#[test]
fn coexist_with_manual_native_state_and_generated_gate() {
    let h = hybrid();
    assert_eq!(ready(h.__uh_tom_mick_dispatch_append(10)), Ok(1));
    assert_eq!(h.requests.load(), 1);
    assert_eq!(h.completed.load(), 8);
    assert_eq!(h.manual.load(Ordering::Relaxed), 10);
    assert!(h.manual_bool.load(Ordering::Acquire));
    assert!(h.flags.is_set(WorkerFlags::Ready));
    assert_eq!(*h.records.lock().unwrap(), vec![10]);
    assert_eq!(h.gate.active(), 0);
}
#[test]
fn shared_arc_generated_counter() {
    let h = hybrid();
    let clone = Arc::clone(&h.requests);
    clone.add(3).unwrap();
    assert_eq!(h.requests.load(), 3);
}
#[test]
fn native_mutex_remains_independent() {
    let h = hybrid();
    h.records.lock().unwrap().push(1);
    h.requests.increment().unwrap();
    assert_eq!(h.records.lock().unwrap().len(), 1);
    assert_eq!(h.requests.load(), 1);
}
#[test]
fn generated_gate_shutdown_does_not_mutate_metrics_or_flags() {
    let h = hybrid();
    h.gate.begin_shutdown();
    assert!(ready(h.__uh_tom_mick_dispatch_append(2)).is_err());
    assert_eq!(h.requests.load(), 0);
    assert_eq!(h.completed.load(), 7);
    assert_eq!(h.flags.bits(), 0);
}
#[test]
fn counter_marker_concurrency() {
    let c = Arc::new(Requests::new());
    std::thread::scope(|s| {
        for _ in 0..4 {
            let c = &c;
            s.spawn(move || {
                for _ in 0..1000 {
                    c.increment().unwrap();
                }
            });
        }
    });
    assert_eq!(c.load(), 4000);
}

#[test]
fn generated_gate_exposes_single_word_drain_status() {
    use uh_tom_mick::DrainStatus;
    let g = Gate::new();
    assert_eq!(g.drain_status(), DrainStatus::Open { active: 0 });
    let guard = g.try_enter().unwrap();
    g.begin_shutdown();
    assert_eq!(g.drain_status(), DrainStatus::Stopping { active: 1 });
    drop(guard);
    assert_eq!(g.drain_status(), DrainStatus::Drained);
}

#[test]
fn generated_counter_threaded_checked_boundary() {
    let c = Arc::new(Requests::with_initial(u64::MAX - 1));
    let b = Arc::new(std::sync::Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let c = Arc::clone(&c);
            let b = Arc::clone(&b);
            std::thread::spawn(move || {
                b.wait();
                c.increment()
            })
        })
        .collect();
    b.wait();
    let wins = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .filter(|result| result.is_ok())
        .count();
    assert_eq!(wins, 1);
    assert_eq!(c.load(), u64::MAX);
}
