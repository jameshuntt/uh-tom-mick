#![cfg(feature = "loom")]
use loom::{sync::Arc, thread};
use uh_tom_mick::{atomic_counter, atomic_flags};
#[atomic_counter]
struct Count;
#[atomic_flags]
enum Flags {
    Ready,
    Stop,
    Busy,
}
#[test]
fn generated_counter_two_threads_all_small_schedules() {
    loom::model(|| {
        let c = Arc::new(Count::new());
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
fn generated_flags_two_threads_all_small_schedules() {
    loom::model(|| {
        let f = Arc::new(AtomicFlags::new());
        let x = Arc::clone(&f);
        let a = thread::spawn(move || {
            x.enable(Flags::Ready);
        });
        let y = Arc::clone(&f);
        let b = thread::spawn(move || {
            y.enable(Flags::Stop);
        });
        a.join().unwrap();
        b.join().unwrap();
        assert!(f.all(&[Flags::Ready, Flags::Stop]));
        assert!(!f.is_set(Flags::Busy));
    });
}
