//! Exhaust every short valid *sequential* event history against a small
//! independent reference state machine. Loom separately explores concurrent
//! schedules of the same production implementation.
#![cfg(not(feature = "loom"))]

use uh_tom_mick_core::{AdmissionError, AdmissionSnapshot, AtomicAdmission};

#[derive(Clone, Copy)]
enum Action {
    Enter,
    ReleaseOldest,
    ReleaseNewest,
    Shutdown,
    Snapshot,
}
const ACTIONS: [Action; 5] = [
    Action::Enter,
    Action::ReleaseOldest,
    Action::ReleaseNewest,
    Action::Shutdown,
    Action::Snapshot,
];

// Returns false for impossible histories (release without a live permit).
fn check_trace(capacity: u32, history: &[Action]) -> bool {
    let gate = AtomicAdmission::new(capacity).unwrap();
    let mut permits = Vec::new();
    let mut active = 0;
    let mut stopped = false;
    for step in history {
        match step {
            Action::Enter => match (stopped, active == capacity) {
                (true, _) => assert_eq!(
                    gate.try_enter().unwrap_err(),
                    AdmissionError::Stopping { active }
                ),
                (false, true) => assert_eq!(
                    gate.try_enter().unwrap_err(),
                    AdmissionError::AtCapacity {
                        active,
                        limit: capacity
                    }
                ),
                (false, false) => {
                    permits.push(gate.try_enter().unwrap());
                    active += 1;
                }
            },
            Action::ReleaseOldest => {
                if permits.is_empty() {
                    return false;
                }
                drop(permits.remove(0));
                active -= 1;
            }
            Action::ReleaseNewest => {
                if permits.is_empty() {
                    return false;
                }
                drop(permits.pop());
                active -= 1;
            }
            Action::Shutdown => {
                assert_eq!(gate.begin_shutdown(), active);
                stopped = true;
            }
            Action::Snapshot => { /* the postcondition below checks this as well */ }
        }
        assert_eq!(
            gate.snapshot(),
            AdmissionSnapshot {
                active,
                stopping: stopped
            }
        );
        assert_eq!(gate.is_drained(), stopped && active == 0);
    }
    drop(permits);
    assert_eq!(gate.active(), 0);
    if stopped {
        assert!(gate.is_drained());
    }
    true
}

fn enumerate(capacity: u32, prefix: &mut Vec<Action>, valid: &mut usize) {
    if check_trace(capacity, prefix) {
        *valid += 1;
    }
    if prefix.len() == 6 {
        return;
    }
    for action in ACTIONS {
        prefix.push(action);
        enumerate(capacity, prefix, valid);
        prefix.pop();
    }
}

#[test]
fn all_valid_sequential_histories_through_six_operations() {
    for capacity in [1, 2, 3, 4] {
        let mut valid = 0;
        enumerate(capacity, &mut Vec::new(), &mut valid);
        assert!(
            valid > 100,
            "insufficient reference coverage for capacity {capacity}: {valid}"
        );
        println!("capacity={capacity} valid_histories={valid}");
    }
}
