#![cfg(not(feature = "loom"))]
//! Independent bounded reference histories for the new atomic state capabilities.
use uh_tom_mick_core::{AtomicCounter, AtomicFlagSet, CounterError};

#[test]
fn all_short_counter_histories_match_checked_arithmetic() {
    // Exhaustively enumerate 5^6 operation sequences at each starting boundary.
    // Operations: increment, add(2), decrement, subtract(2), reset.
    for initial in [0, 1, 2, u64::MAX - 1, u64::MAX] {
        for mut encoded in 0..15_625usize {
            let count = AtomicCounter::new(initial);
            let mut model = initial;
            for _ in 0..6 {
                let op = encoded % 5;
                encoded /= 5;
                match op {
                    0 | 1 => {
                        let amount = if op == 0 { 1 } else { 2 };
                        let expected = model.checked_add(amount).ok_or(CounterError::Overflow);
                        assert_eq!(count.add(amount), expected);
                        if let Ok(next) = expected {
                            model = next;
                        }
                    }
                    2 | 3 => {
                        let amount = if op == 2 { 1 } else { 2 };
                        let expected = model.checked_sub(amount).ok_or(CounterError::Underflow);
                        assert_eq!(count.subtract(amount), expected);
                        if let Ok(next) = expected {
                            model = next;
                        }
                    }
                    _ => {
                        assert_eq!(count.reset(), model);
                        model = 0;
                    }
                }
                assert_eq!(count.load(), model);
            }
        }
    }
}

#[test]
fn all_short_flag_histories_match_independent_bitset_model() {
    // Enumerate 6^6 histories of independent bit transformations/CAS.
    for mut encoded in 0..46_656usize {
        let flags = AtomicFlagSet::new(0b111, 0).unwrap();
        let mut model = 0u64;
        for _ in 0..6 {
            let op = encoded % 6;
            encoded /= 6;
            match op {
                0 => {
                    assert_eq!(flags.set_bits(1), Ok(model));
                    model |= 1;
                }
                1 => {
                    assert_eq!(flags.set_bits(2), Ok(model));
                    model |= 2;
                }
                2 => {
                    assert_eq!(flags.clear_bits(1), Ok(model));
                    model &= !1;
                }
                3 => {
                    assert_eq!(flags.toggle_bits(3), Ok(model));
                    model ^= 3;
                }
                4 => {
                    let result = if model == 0 { Ok(model) } else { Err(model) };
                    assert_eq!(flags.compare_exchange(0, 4), Ok(result));
                    if model == 0 {
                        model = 4;
                    }
                }
                _ => {
                    let result = if model == 4 { Ok(model) } else { Err(model) };
                    assert_eq!(flags.compare_exchange(4, 0), Ok(result));
                    if model == 4 {
                        model = 0;
                    }
                }
            }
            assert_eq!(flags.bits(), model);
            assert!(model & !7 == 0);
        }
    }
}
