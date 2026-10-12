//! Bounded exhaustive schedule exploration of the *production* core, with
//! `AtomicU64` replaced by Loom's instrumented atomic via `feature = "loom"`.
//! No artificial timing sleeps, no model-only admission implementation.
#![cfg(feature = "loom")]

use loom::{model::Builder, sync::Arc, thread};
use uh_tom_mick_core::{AdmissionError, AdmissionSnapshot, AtomicAdmission};

fn exhaustive(f: impl Fn() + Sync + Send + 'static) {
    let mut model = Builder::new();
    model.max_branches = 2_000;
    // Crucial: no max_permutations, duration, or preemption bounds.
    // Completion means Loom exhausted the schedules of this *finite* scenario.
    model.check(f);
}

#[test]
fn two_contenders_capacity_one_all_schedules() {
    exhaustive(|| {
        let gate = Arc::new(AtomicAdmission::new(1).unwrap());
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let gate = gate.clone();
            tasks.push(thread::spawn(move || {
                if let Ok(permit) = gate.try_enter() {
                    assert_eq!(gate.active(), 1);
                    thread::yield_now();
                    assert_eq!(gate.active(), 1);
                    drop(permit);
                }
            }));
        }
        for task in tasks {
            task.join().unwrap();
        }
        assert_eq!(
            gate.snapshot(),
            AdmissionSnapshot {
                active: 0,
                stopping: false
            }
        );
    });
}

#[test]
fn two_contenders_capacity_two_all_schedules() {
    exhaustive(|| {
        let gate = Arc::new(AtomicAdmission::new(2).unwrap());
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let gate = gate.clone();
            tasks.push(thread::spawn(move || {
                let permit = gate.try_enter().unwrap();
                assert!((1..=2).contains(&gate.active()));
                thread::yield_now();
                assert!((1..=2).contains(&gate.active()));
                drop(permit);
            }));
        }
        for task in tasks {
            task.join().unwrap();
        }
        assert_eq!(gate.active(), 0);
    });
}

#[test]
fn held_capacity_one_rejects_both_contenders_all_schedules() {
    exhaustive(|| {
        let gate = Arc::new(AtomicAdmission::new(1).unwrap());
        let held = gate.try_enter().unwrap();
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let gate = gate.clone();
            tasks.push(thread::spawn(move || {
                assert_eq!(
                    gate.try_enter().unwrap_err(),
                    AdmissionError::AtCapacity {
                        active: 1,
                        limit: 1
                    }
                );
            }));
        }
        for task in tasks {
            task.join().unwrap();
        }
        assert_eq!(gate.active(), 1);
        drop(held);
        assert_eq!(gate.active(), 0);
        drop(gate.try_enter().unwrap());
        assert_eq!(gate.active(), 0);
    });
}

#[test]
fn shutdown_races_with_new_admission_all_schedules() {
    exhaustive(|| {
        let gate = Arc::new(AtomicAdmission::new(2).unwrap());
        // One guaranteed in-flight operation throughout both thread joins.
        let held = gate.try_enter().unwrap();
        let entering = gate.clone();
        let stopper = gate.clone();
        let first = thread::spawn(move || {
            if let Ok(permit) = entering.try_enter() {
                assert!(entering.active() <= 2);
                thread::yield_now();
                drop(permit);
            }
        });
        let second = thread::spawn(move || {
            let observed = stopper.begin_shutdown();
            assert!((1..=2).contains(&observed));
        });
        first.join().unwrap();
        second.join().unwrap();
        assert_eq!(
            gate.snapshot(),
            AdmissionSnapshot {
                active: 1,
                stopping: true
            }
        );
        assert!(matches!(
            gate.try_enter(),
            Err(AdmissionError::Stopping { active: 1 })
        ));
        drop(held);
        assert!(gate.is_drained());
    });
}

#[test]
fn last_release_races_with_shutdown_all_schedules() {
    exhaustive(|| {
        let gate = Arc::new(AtomicAdmission::new(1).unwrap());
        let held = gate.try_enter().unwrap();
        let stopper = gate.clone();
        let task = thread::spawn(move || stopper.begin_shutdown());
        // Loom explores shutdown preceding or following the final release.
        drop(held);
        let stopped_at = task.join().unwrap();
        assert!(stopped_at <= 1);
        assert!(gate.is_drained());
        assert!(matches!(
            gate.try_enter(),
            Err(AdmissionError::Stopping { active: 0 })
        ));
    });
}

#[test]
fn two_concurrent_shutdown_calls_are_idempotent_all_schedules() {
    exhaustive(|| {
        let gate = Arc::new(AtomicAdmission::new(1).unwrap());
        let held = gate.try_enter().unwrap();
        let a = gate.clone();
        let b = gate.clone();
        let first = thread::spawn(move || a.begin_shutdown());
        let second = thread::spawn(move || b.begin_shutdown());
        assert_eq!(first.join().unwrap(), 1);
        assert_eq!(second.join().unwrap(), 1);
        assert_eq!(
            gate.snapshot(),
            AdmissionSnapshot {
                active: 1,
                stopping: true
            }
        );
        drop(held);
        assert!(gate.is_drained());
    });
}

#[test]
fn snapshot_never_tears_across_flag_and_counter_all_schedules() {
    exhaustive(|| {
        let gate = Arc::new(AtomicAdmission::new(1).unwrap());
        let held = gate.try_enter().unwrap();
        let stopper = gate.clone();
        let task = thread::spawn(move || {
            stopper.begin_shutdown();
        });
        // Count cannot change until held is released; only stop flag races.
        for _ in 0..2 {
            let view = gate.snapshot();
            assert_eq!(view.active, 1);
        }
        task.join().unwrap();
        assert_eq!(
            gate.snapshot(),
            AdmissionSnapshot {
                active: 1,
                stopping: true
            }
        );
        drop(held);
        assert_eq!(
            gate.snapshot(),
            AdmissionSnapshot {
                active: 0,
                stopping: true
            }
        );
    });
}

#[test]
fn stop_precedes_all_attempts_all_schedules() {
    exhaustive(|| {
        let gate = Arc::new(AtomicAdmission::new(2).unwrap());
        assert_eq!(gate.begin_shutdown(), 0);
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let gate = gate.clone();
            tasks.push(thread::spawn(move || {
                assert!(matches!(
                    gate.try_enter(),
                    Err(AdmissionError::Stopping { active: 0 })
                ));
            }));
        }
        for task in tasks {
            task.join().unwrap();
        }
        assert!(gate.is_drained());
    });
}

#[test]
fn observation_races_with_entry_and_shutdown_all_schedules() {
    exhaustive(|| {
        let gate = Arc::new(AtomicAdmission::new(1).unwrap());
        let entering = gate.clone();
        let stopper = gate.clone();
        let observer = gate.clone();
        let first = thread::spawn(move || {
            if let Ok(permit) = entering.try_enter() {
                thread::yield_now();
                drop(permit);
            }
        });
        let second = thread::spawn(move || {
            stopper.begin_shutdown();
        });
        let third = thread::spawn(move || {
            let snapshot = observer.snapshot();
            assert!(snapshot.active <= 1);
            // is_drained() internally observes one coherent atomic word.
            let _ = observer.is_drained();
        });
        first.join().unwrap();
        second.join().unwrap();
        third.join().unwrap();
        assert_eq!(
            gate.snapshot(),
            AdmissionSnapshot {
                active: 0,
                stopping: true
            }
        );
    });
}

#[test]
fn nested_guards_and_shutdown_all_schedules() {
    exhaustive(|| {
        let gate = Arc::new(AtomicAdmission::new(2).unwrap());
        let first = gate.try_enter().unwrap();
        let second = gate.try_enter().unwrap();
        let stopper = gate.clone();
        let task = thread::spawn(move || stopper.begin_shutdown());
        drop(first);
        thread::yield_now();
        drop(second);
        let count = task.join().unwrap();
        assert!(count <= 2);
        assert!(gate.is_drained());
        assert!(matches!(
            gate.try_enter(),
            Err(AdmissionError::Stopping { active: 0 })
        ));
    });
}

#[test]
fn bounded_deeper_two_workers_with_repeated_admissions_and_shutdown() {
    // This separate scenario deliberately bounds preemptions and permutations;
    // unlike the ten small cases above, it is NOT claimed to be exhaustive.
    let mut model = Builder::new();
    model.max_branches = 2_000;
    model.preemption_bound = Some(3);
    model.max_permutations = Some(10_000);
    model.check(|| {
        let gate = Arc::new(AtomicAdmission::new(2).unwrap());
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let gate = gate.clone();
            tasks.push(thread::spawn(move || {
                for _ in 0..2 {
                    if let Ok(permit) = gate.try_enter() {
                        assert!(gate.active() <= 2);
                        thread::yield_now();
                        drop(permit);
                    }
                }
            }));
        }
        thread::yield_now();
        gate.begin_shutdown();
        for task in tasks {
            task.join().unwrap();
        }
        assert!(gate.is_drained());
    });
}
