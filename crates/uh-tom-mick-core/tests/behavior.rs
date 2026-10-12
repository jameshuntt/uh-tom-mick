#![cfg(not(feature = "loom"))]
// Production-atomics tests. Run separately from the instrumented Loom backend.
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc, Barrier,
};
use uh_tom_mick_core::{AdmissionError, AdmissionWord, AtomicAdmission};

#[test]
fn bitpack_uses_existing_big_bad_bool_schema() {
    assert_eq!(AdmissionWord::STORAGE_BITS, 64);
    assert_eq!(AdmissionWord::USED_BITS, 33);
    assert_eq!(AdmissionWord::FIELDS.len(), 2);
    assert_eq!(AdmissionWord::FIELDS[0].name, "active");
    assert_eq!(AdmissionWord::FIELDS[0].shift, 0);
    assert_eq!(AdmissionWord::FIELDS[0].width, 32);
    assert_eq!(AdmissionWord::FIELDS[1].name, "stopping");
    assert_eq!(AdmissionWord::FIELDS[1].shift, 32);
    assert_eq!(AdmissionWord::FIELDS[1].width, 1);
}
#[test]
fn bitpack_preserves_both_fields() {
    for active in [0, 1, 2, 7, 127, 128, 65535, u32::MAX] {
        for stopping in [0, 1] {
            let w = AdmissionWord::new(active, stopping);
            assert_eq!(w.active(), active);
            assert_eq!(w.stopping(), stopping);
            assert_eq!(AdmissionWord::try_from_raw_checked(w.raw()).unwrap(), w);
        }
    }
}
#[test]
fn bitpack_rejects_reserved_bit() {
    assert!(AdmissionWord::try_from_raw_checked(1u64 << 33).is_err());
}
#[test]
fn bitpack_masked_constructor() {
    assert_eq!(
        AdmissionWord::from_raw(u64::MAX).raw(),
        AdmissionWord::RAW_MASK
    );
}
#[test]
fn bitpack_checked_overflow() {
    assert!(AdmissionWord::new_checked(1, 2).is_err());
}
#[test]
fn bitpack_checked_construction() {
    assert!(AdmissionWord::new_checked(1, 1).is_ok());
}
#[test]
fn zero_capacity_is_error() {
    assert!(matches!(
        AtomicAdmission::new(0),
        Err(AdmissionError::ZeroCapacity)
    ));
}
#[test]
fn new_is_idle() {
    let a = AtomicAdmission::new(1).unwrap();
    assert_eq!(a.active(), 0);
    assert!(!a.is_stopping());
    assert!(!a.is_drained());
}
#[test]
fn one_entry() {
    let a = AtomicAdmission::new(1).unwrap();
    let g = a.try_enter().unwrap();
    assert_eq!(a.active(), 1);
    drop(g);
    assert_eq!(a.active(), 0);
}
#[test]
fn capacity_is_enforced() {
    let a = AtomicAdmission::new(1).unwrap();
    let _g = a.try_enter().unwrap();
    assert_eq!(
        a.try_enter().unwrap_err(),
        AdmissionError::AtCapacity {
            active: 1,
            limit: 1
        }
    );
}
#[test]
fn capacity_two() {
    let a = AtomicAdmission::new(2).unwrap();
    let x = a.try_enter().unwrap();
    let y = a.try_enter().unwrap();
    assert_eq!(a.active(), 2);
    assert!(a.try_enter().is_err());
    drop(x);
    assert_eq!(a.active(), 1);
    drop(y);
    assert_eq!(a.active(), 0);
}
#[test]
fn guard_scope_releases_on_early_return() {
    fn run(a: &AtomicAdmission) -> Result<(), &'static str> {
        let _guard = a.try_enter().unwrap();
        Err("business-error")
    }
    let a = AtomicAdmission::new(3).unwrap();
    assert_eq!(run(&a), Err("business-error"));
    assert_eq!(a.active(), 0);
}
#[test]
fn stop_when_empty() {
    let a = AtomicAdmission::new(2).unwrap();
    assert_eq!(a.begin_shutdown(), 0);
    assert!(a.is_drained());
    assert_eq!(
        a.try_enter().unwrap_err(),
        AdmissionError::Stopping { active: 0 }
    );
}
#[test]
fn stop_while_active() {
    let a = AtomicAdmission::new(2).unwrap();
    let g = a.try_enter().unwrap();
    assert_eq!(a.begin_shutdown(), 1);
    assert!(a.is_stopping());
    assert!(!a.is_drained());
    assert_eq!(
        a.try_enter().unwrap_err(),
        AdmissionError::Stopping { active: 1 }
    );
    drop(g);
    assert!(a.is_drained());
}
#[test]
fn shutdown_idempotent() {
    let a = AtomicAdmission::new(3).unwrap();
    assert_eq!(a.begin_shutdown(), 0);
    assert_eq!(a.begin_shutdown(), 0);
    assert!(a.is_drained());
}
#[test]
fn stop_does_not_force_drop_guards() {
    let a = AtomicAdmission::new(2).unwrap();
    let x = a.try_enter().unwrap();
    let y = a.try_enter().unwrap();
    assert_eq!(a.begin_shutdown(), 2);
    assert_eq!(a.active(), 2);
    drop(x);
    assert_eq!(a.active(), 1);
    drop(y);
    assert!(a.is_drained());
}
#[test]
fn no_reopen_after_shutdown() {
    let a = AtomicAdmission::new(1).unwrap();
    a.begin_shutdown();
    for _ in 0..5000 {
        assert!(matches!(
            a.try_enter(),
            Err(AdmissionError::Stopping { .. })
        ));
    }
}
#[test]
fn large_capacity_initializes() {
    let a = AtomicAdmission::new(u32::MAX).unwrap();
    let _g = a.try_enter().unwrap();
    assert_eq!(a.limit(), u32::MAX);
    assert_eq!(a.active(), 1);
}
#[test]
fn sequential_100k_cycles() {
    let a = AtomicAdmission::new(5).unwrap();
    for _ in 0..100_000 {
        let _g = a.try_enter().unwrap();
        assert_eq!(a.active(), 1);
    }
    assert_eq!(a.active(), 0);
}
#[test]
fn nesting_releases_in_any_order() {
    let a = AtomicAdmission::new(4).unwrap();
    let a1 = a.try_enter().unwrap();
    let a2 = a.try_enter().unwrap();
    let a3 = a.try_enter().unwrap();
    drop(a2);
    assert_eq!(a.active(), 2);
    drop(a1);
    assert_eq!(a.active(), 1);
    drop(a3);
    assert_eq!(a.active(), 0);
}
#[test]
fn snapshot_is_coherent_across_bits() {
    let a = AtomicAdmission::new(10).unwrap();
    let g = a.try_enter().unwrap();
    let s = a.snapshot();
    assert_eq!((s.active, s.stopping), (1, false));
    a.begin_shutdown();
    let s = a.snapshot();
    assert_eq!((s.active, s.stopping), (1, true));
    drop(g);
    let s = a.snapshot();
    assert_eq!((s.active, s.stopping), (0, true));
}
#[test]
fn concurrent_entry_exact_capacity() {
    let n = 16;
    let gate = Arc::new(AtomicAdmission::new(n).unwrap());
    let entered = Arc::new(Barrier::new(n as usize + 1));
    let release = Arc::new(Barrier::new(n as usize + 1));
    let mut handles = Vec::new();
    for _ in 0..n {
        let (g, e, r) = (gate.clone(), entered.clone(), release.clone());
        handles.push(std::thread::spawn(move || {
            let _guard = g.try_enter().unwrap();
            e.wait();
            r.wait();
        }));
    }
    entered.wait();
    assert_eq!(gate.active(), n);
    release.wait();
    for t in handles {
        t.join().unwrap();
    }
    assert_eq!(gate.active(), 0);
}
#[test]
fn concurrent_shutdown_blocks_new_admissions() {
    for _round in 0..32 {
        let gate = Arc::new(AtomicAdmission::new(8).unwrap());
        let barrier = Arc::new(Barrier::new(17));
        let mut threads = Vec::new();
        for _ in 0..16 {
            let a = gate.clone();
            let b = barrier.clone();
            threads.push(std::thread::spawn(move || {
                b.wait();
                for _ in 0..100 {
                    let _ = a.try_enter();
                    std::thread::yield_now();
                }
            }));
        }
        barrier.wait();
        gate.begin_shutdown();
        for t in threads {
            t.join().unwrap();
        }
        assert!(gate.is_drained());
        assert_eq!(gate.active(), 0);
        assert!(matches!(
            gate.try_enter(),
            Err(AdmissionError::Stopping { .. })
        ));
    }
}
#[test]
fn capacity_never_exceeded_under_heavy_contention() {
    let gate = Arc::new(AtomicAdmission::new(4).unwrap());
    let observed = Arc::new(AtomicU32::new(0));
    let mut threads = Vec::new();
    for _ in 0..24 {
        let g = gate.clone();
        let max = observed.clone();
        threads.push(std::thread::spawn(move || {
            for _ in 0..2000 {
                if let Ok(_token) = g.try_enter() {
                    let count = g.active();
                    max.fetch_max(count, Ordering::Relaxed);
                    assert!(count <= 4);
                    std::thread::yield_now();
                }
            }
        }));
    }
    for t in threads {
        t.join().unwrap();
    }
    assert!(observed.load(Ordering::Relaxed) <= 4);
    assert_eq!(gate.active(), 0);
}
#[test]
fn shutdown_under_heavy_contention_drains() {
    let gate = Arc::new(AtomicAdmission::new(32).unwrap());
    let mut threads = Vec::new();
    for _ in 0..12 {
        let g = gate.clone();
        threads.push(std::thread::spawn(move || {
            for _ in 0..5000 {
                if let Ok(_guard) = g.try_enter() {
                    std::thread::yield_now();
                } else if g.is_stopping() {
                    break;
                }
            }
        }));
    }
    std::thread::yield_now();
    gate.begin_shutdown();
    for t in threads {
        t.join().unwrap();
    }
    assert!(gate.is_drained());
}
#[test]
fn admission_error_display_is_structured() {
    assert!(AdmissionError::ZeroCapacity.to_string().contains("nonzero"));
    assert!(AdmissionError::AtCapacity {
        active: 2,
        limit: 2
    }
    .to_string()
    .contains("2/2"));
    assert!(AdmissionError::Stopping { active: 2 }
        .to_string()
        .contains("stopping"));
}
macro_rules! capacity_matrix { ($($name:ident:$cap:expr),* $(,)?) => { $(#[test] fn $name() {
    let a=AtomicAdmission::new($cap).unwrap(); let mut guards=Vec::new();
    for i in 0..$cap { guards.push(a.try_enter().unwrap()); assert_eq!(a.active(),i+1); }
    assert!(matches!(a.try_enter(),Err(AdmissionError::AtCapacity{..})));
    while guards.pop().is_some() { assert_eq!(a.active(),guards.len() as u32); }
    assert_eq!(a.begin_shutdown(),0); assert!(a.is_drained());
})* }; }
capacity_matrix!(capacity_1:1,capacity_2:2,capacity_3:3,capacity_4:4,capacity_5:5,capacity_6:6,capacity_7:7,capacity_8:8,capacity_9:9,capacity_10:10,capacity_11:11,capacity_12:12,capacity_13:13,capacity_14:14,capacity_15:15,capacity_16:16,capacity_17:17,capacity_18:18,capacity_19:19,capacity_20:20,capacity_21:21,capacity_22:22,capacity_23:23,capacity_24:24,capacity_25:25,capacity_26:26,capacity_27:27,capacity_28:28,capacity_29:29,capacity_30:30,capacity_31:31,capacity_32:32,capacity_33:33,capacity_34:34,capacity_35:35,capacity_36:36,capacity_37:37,capacity_38:38,capacity_39:39,capacity_40:40,capacity_41:41,capacity_42:42,capacity_43:43,capacity_44:44,capacity_45:45,capacity_46:46,capacity_47:47,capacity_48:48,capacity_49:49,capacity_50:50,capacity_51:51,capacity_52:52,capacity_53:53,capacity_54:54,capacity_55:55,capacity_56:56,capacity_57:57,capacity_58:58,capacity_59:59,capacity_60:60,capacity_61:61,capacity_62:62,capacity_63:63,capacity_64:64,capacity_127:127,capacity_128:128,capacity_255:255,capacity_256:256,capacity_511:511,capacity_512:512,capacity_1023:1023,capacity_1024:1024);
