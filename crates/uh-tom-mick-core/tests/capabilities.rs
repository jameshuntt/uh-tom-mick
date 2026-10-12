#![cfg(not(feature = "loom"))]
use std::sync::{Arc, Barrier};
use uh_tom_mick_core::{
    AtomicAdmission, AtomicCounter, AtomicFlagSet, CounterError, DrainStatus, FlagSetError,
};

#[test]
fn counter_initial_and_default() {
    assert_eq!(AtomicCounter::new(7).load(), 7);
    assert_eq!(AtomicCounter::default().load(), 0);
}
#[test]
fn counter_add_returns_new() {
    let c = AtomicCounter::new(9);
    assert_eq!(c.add(4), Ok(13));
    assert_eq!(c.load(), 13);
}
#[test]
fn counter_zero_add_is_valid() {
    let c = AtomicCounter::new(u64::MAX);
    assert_eq!(c.add(0), Ok(u64::MAX));
}
#[test]
fn counter_increment() {
    let c = AtomicCounter::default();
    assert_eq!(c.increment(), Ok(1));
    assert_eq!(c.increment(), Ok(2));
}
#[test]
fn counter_overflow_rejected_without_mutation() {
    let c = AtomicCounter::new(u64::MAX);
    assert_eq!(c.increment(), Err(CounterError::Overflow));
    assert_eq!(c.load(), u64::MAX);
}
#[test]
fn counter_near_overflow() {
    let c = AtomicCounter::new(u64::MAX - 1);
    assert_eq!(c.increment(), Ok(u64::MAX));
    assert_eq!(c.increment(), Err(CounterError::Overflow));
}
#[test]
fn counter_subtract_returns_new() {
    let c = AtomicCounter::new(100);
    assert_eq!(c.subtract(31), Ok(69));
}
#[test]
fn counter_decrement() {
    let c = AtomicCounter::new(2);
    assert_eq!(c.decrement(), Ok(1));
    assert_eq!(c.decrement(), Ok(0));
}
#[test]
fn counter_underflow_rejected_without_mutation() {
    let c = AtomicCounter::default();
    assert_eq!(c.decrement(), Err(CounterError::Underflow));
    assert_eq!(c.load(), 0);
}
#[test]
fn counter_subtract_more_than_current_rejected() {
    let c = AtomicCounter::new(4);
    assert_eq!(c.subtract(5), Err(CounterError::Underflow));
    assert_eq!(c.load(), 4);
}
#[test]
fn counter_subtract_zero_is_valid() {
    let c = AtomicCounter::default();
    assert_eq!(c.subtract(0), Ok(0));
}
#[test]
fn counter_reset_returns_previous_value() {
    let c = AtomicCounter::new(8);
    assert_eq!(c.reset(), 8);
    assert_eq!(c.reset(), 0);
}
#[test]
fn counter_threaded_exact_total() {
    let c = Arc::new(AtomicCounter::default());
    let mut handles = Vec::new();
    for _ in 0..8 {
        let c = Arc::clone(&c);
        handles.push(std::thread::spawn(move || {
            for _ in 0..5000 {
                c.increment().unwrap();
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.load(), 40_000);
}
#[test]
fn counter_boundary_contention_never_wraps() {
    let c = Arc::new(AtomicCounter::new(u64::MAX - 2));
    let b = Arc::new(Barrier::new(9));
    let handles: Vec<_> = (0..8)
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
    let accepted = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .filter(|result| result.is_ok())
        .count();
    assert_eq!(accepted, 2);
    assert_eq!(c.load(), u64::MAX);
}
#[test]
fn flags_reject_empty_schema() {
    assert!(matches!(
        AtomicFlagSet::new(0, 0),
        Err(FlagSetError::EmptySchema)
    ));
}
#[test]
fn flags_reject_unknown_initial() {
    assert!(matches!(
        AtomicFlagSet::new(0b111, 0b1000),
        Err(FlagSetError::UnknownBits { bits: 8 })
    ));
}
#[test]
fn flags_initial_valid() {
    let f = AtomicFlagSet::new(0b111, 0b101).unwrap();
    assert_eq!(f.bits(), 0b101);
    assert_eq!(f.known_mask(), 0b111);
}
#[test]
fn flags_set_returns_old() {
    let f = AtomicFlagSet::new(7, 1).unwrap();
    assert_eq!(f.set_bits(2), Ok(1));
    assert_eq!(f.bits(), 3);
}
#[test]
fn flags_clear_returns_old() {
    let f = AtomicFlagSet::new(7, 3).unwrap();
    assert_eq!(f.clear_bits(2), Ok(3));
    assert_eq!(f.bits(), 1);
}
#[test]
fn flags_toggle_returns_old() {
    let f = AtomicFlagSet::new(7, 1).unwrap();
    assert_eq!(f.toggle_bits(3), Ok(1));
    assert_eq!(f.bits(), 2);
}
#[test]
fn flags_reject_zero_selection() {
    let f = AtomicFlagSet::new(7, 0).unwrap();
    assert_eq!(f.set_bits(0), Err(FlagSetError::EmptySelection));
    assert_eq!(f.clear_bits(0), Err(FlagSetError::EmptySelection));
    assert_eq!(f.toggle_bits(0), Err(FlagSetError::EmptySelection));
}
#[test]
fn flags_reject_unknown_selection() {
    let f = AtomicFlagSet::new(7, 0).unwrap();
    assert_eq!(f.set_bits(8), Err(FlagSetError::UnknownBits { bits: 8 }));
    assert_eq!(f.clear_bits(8), Err(FlagSetError::UnknownBits { bits: 8 }));
    assert_eq!(f.bits(), 0);
}
#[test]
fn flags_contains_all_and_any() {
    let f = AtomicFlagSet::new(7, 5).unwrap();
    assert_eq!(f.contains_all(5), Ok(true));
    assert_eq!(f.contains_all(3), Ok(false));
    assert_eq!(f.contains_any(3), Ok(true));
    assert_eq!(f.contains_any(2), Ok(false));
}
#[test]
fn flags_cas_success() {
    let f = AtomicFlagSet::new(7, 1).unwrap();
    assert_eq!(f.compare_exchange(1, 6), Ok(Ok(1)));
    assert_eq!(f.bits(), 6);
}
#[test]
fn flags_cas_failure_reports_observed_word() {
    let f = AtomicFlagSet::new(7, 1).unwrap();
    assert_eq!(f.compare_exchange(2, 3), Ok(Err(1)));
    assert_eq!(f.bits(), 1);
}
#[test]
fn flags_cas_rejects_unknown_expected() {
    let f = AtomicFlagSet::new(7, 0).unwrap();
    assert_eq!(
        f.compare_exchange(8, 1),
        Err(FlagSetError::UnknownBits { bits: 8 })
    );
}
#[test]
fn flags_cas_rejects_unknown_desired() {
    let f = AtomicFlagSet::new(7, 0).unwrap();
    assert_eq!(
        f.compare_exchange(0, 8),
        Err(FlagSetError::UnknownBits { bits: 8 })
    );
}
#[test]
fn flags_support_highest_bit() {
    let f = AtomicFlagSet::new(u64::MAX, 0).unwrap();
    assert_eq!(f.set_bits(1u64 << 63), Ok(0));
    assert_eq!(f.bits(), 1u64 << 63);
}
#[test]
fn flags_threaded_nonoverlapping_sets_are_preserved() {
    let f = Arc::new(AtomicFlagSet::new(u64::MAX, 0).unwrap());
    let handles: Vec<_> = (0..64)
        .map(|i| {
            let f = Arc::clone(&f);
            std::thread::spawn(move || f.set_bits(1u64 << i).unwrap())
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(f.bits(), u64::MAX);
}
#[test]
fn flags_threaded_cas_single_winner() {
    let f = Arc::new(AtomicFlagSet::new(0b111, 0).unwrap());
    let b = Arc::new(Barrier::new(9));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let f = Arc::clone(&f);
            let b = Arc::clone(&b);
            std::thread::spawn(move || {
                b.wait();
                f.compare_exchange(0, 1u64 << (i % 3)).unwrap()
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
    assert!(matches!(f.bits(), 1 | 2 | 4));
}
#[test]
fn flags_unknown_bits_never_published_after_rejection() {
    let f = AtomicFlagSet::new(3, 0).unwrap();
    for _ in 0..1000 {
        let _ = f.toggle_bits(4);
    }
    assert_eq!(f.bits(), 0);
}
#[test]
fn drain_status_from_open_to_stop_to_drained() {
    let gate = AtomicAdmission::new(2).unwrap();
    assert_eq!(gate.drain_status(), DrainStatus::Open { active: 0 });
    let permit = gate.try_enter().unwrap();
    assert_eq!(gate.drain_status(), DrainStatus::Open { active: 1 });
    gate.begin_shutdown();
    assert_eq!(gate.drain_status(), DrainStatus::Stopping { active: 1 });
    drop(permit);
    assert_eq!(gate.drain_status(), DrainStatus::Drained);
}
#[test]
fn drain_status_does_not_start_shutdown() {
    let gate = AtomicAdmission::new(1).unwrap();
    assert_eq!(gate.drain_status(), DrainStatus::Open { active: 0 });
    assert!(!gate.is_stopping());
}
