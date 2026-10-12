#![cfg(feature = "loom")]
use loom::{sync::Arc, thread};
use uh_tom_mick_core::{AtomicAdmission, AtomicCounter, AtomicFlagSet, DrainStatus};
#[test]
fn counter_two_threads_all_small_schedules() {
    loom::model(|| {
        let c = Arc::new(AtomicCounter::new(0));
        let x = Arc::clone(&c);
        let a = thread::spawn(move || {
            x.increment().unwrap();
        });
        let y = Arc::clone(&c);
        let b = thread::spawn(move || {
            y.increment().unwrap();
        });
        a.join().unwrap();
        b.join().unwrap();
        assert_eq!(c.load(), 2);
    });
}
#[test]
fn counter_boundary_race_cannot_wrap_all_small_schedules() {
    loom::model(|| {
        let c = Arc::new(AtomicCounter::new(u64::MAX - 1));
        let x = Arc::clone(&c);
        let a = thread::spawn(move || x.increment());
        let y = Arc::clone(&c);
        let b = thread::spawn(move || y.increment());
        let one = a.join().unwrap();
        let two = b.join().unwrap();
        assert_ne!(one.is_ok(), two.is_ok());
        assert_eq!(c.load(), u64::MAX);
    });
}
#[test]
fn distinct_flags_survive_concurrent_rmw_all_small_schedules() {
    loom::model(|| {
        let f = Arc::new(AtomicFlagSet::new(7, 0).unwrap());
        let x = Arc::clone(&f);
        let a = thread::spawn(move || {
            x.set_bits(1).unwrap();
        });
        let y = Arc::clone(&f);
        let b = thread::spawn(move || {
            y.set_bits(2).unwrap();
        });
        a.join().unwrap();
        b.join().unwrap();
        assert_eq!(f.bits(), 3);
    });
}
#[test]
fn competing_flag_transitions_only_one_wins_all_small_schedules() {
    loom::model(|| {
        let f = Arc::new(AtomicFlagSet::new(7, 0).unwrap());
        let x = Arc::clone(&f);
        let a = thread::spawn(move || x.compare_exchange(0, 1).unwrap());
        let y = Arc::clone(&f);
        let b = thread::spawn(move || y.compare_exchange(0, 2).unwrap());
        let aa = a.join().unwrap();
        let bb = b.join().unwrap();
        assert_ne!(aa.is_ok(), bb.is_ok());
        assert!(f.bits() == 1 || f.bits() == 2);
    });
}
#[test]
fn drain_status_only_drained_after_stop_and_release_all_small_schedules() {
    loom::model(|| {
        let g = Arc::new(AtomicAdmission::new(1).unwrap());
        let guard = g.try_enter().unwrap();
        let c = Arc::clone(&g);
        let worker = thread::spawn(move || {
            c.begin_shutdown();
        });
        drop(guard);
        worker.join().unwrap();
        assert_eq!(g.drain_status(), DrainStatus::Drained);
    });
}

#[test]
fn concurrent_counter_reset_is_atomic_all_small_schedules() {
    loom::model(|| {
        let c = Arc::new(AtomicCounter::new(0));
        let x = Arc::clone(&c);
        let a = thread::spawn(move || x.increment().unwrap());
        let y = Arc::clone(&c);
        let b = thread::spawn(move || y.reset());
        let inc = a.join().unwrap();
        let previous = b.join().unwrap();
        let last = c.load();
        assert_eq!(inc, 1);
        assert!((previous == 0 && last == 1) || (previous == 1 && last == 0));
    });
}
#[test]
fn concurrent_toggle_twice_restores_initial_state_all_small_schedules() {
    loom::model(|| {
        let f = Arc::new(AtomicFlagSet::new(1, 0).unwrap());
        let x = Arc::clone(&f);
        let a = thread::spawn(move || x.toggle_bits(1).unwrap());
        let y = Arc::clone(&f);
        let b = thread::spawn(move || y.toggle_bits(1).unwrap());
        a.join().unwrap();
        b.join().unwrap();
        assert_eq!(f.bits(), 0);
    });
}
