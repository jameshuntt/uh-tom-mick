//! Real AF_UNIX socket test of the generated dispatch methods using a test-only
//! mini-router. This deliberately does NOT claim integration with the current
//! Uh Tom Mick production UDS registry (not present in the recovered workspace).
#![cfg(all(unix, not(feature = "loom")))]
use std::{
    future::Future,
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    pin::pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Barrier, Mutex,
    },
    task::{Context, Poll, Waker},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uh_tom_mick::AdmissionError;
use uh_tom_mick::{atomic_admission, uh_tom_mick};

#[atomic_admission(max_active = 2)]
struct SocketGate;
struct SocketService {
    gate: Arc<SocketGate>,
    records: Arc<Mutex<Vec<u8>>>,
    manual_count: Arc<AtomicU64>,
}
#[uh_tom_mick]
impl SocketService {
    #[uh_tom_mick_track(admission = gate)]
    async fn hold(&self, barrier: &Barrier) {
        {
            self.records.lock().unwrap().push(1);
        }
        self.manual_count.fetch_add(1, Ordering::Relaxed);
        barrier.wait();
        barrier.wait();
    }
    #[uh_tom_mick_track(admission = gate)]
    async fn ping(&self) -> u8 {
        self.manual_count.fetch_add(1, Ordering::Relaxed);
        42
    }
}
fn ready<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    match f.as_mut().poll(&mut cx) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("test-only handler unexpectedly pending"),
    }
}
fn connect(path: &Path, op: u8) -> u8 {
    let mut socket = UnixStream::connect(path).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    socket.write_all(&[op]).unwrap();
    let mut result = [0u8; 1];
    socket.read_exact(&mut result).unwrap();
    result[0]
}
fn socket_path() -> std::path::PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::path::PathBuf::from(format!("/tmp/npad-{}-{n}.sock", std::process::id()))
}
fn serve(mut conn: UnixStream, service: Arc<SocketService>, barrier: Arc<Barrier>) {
    let mut op = [0u8; 1];
    conn.read_exact(&mut op).unwrap();
    // The route dispatch binds ONLY the generated adapters. The original
    // business methods are intentionally never referenced here.
    let reply = match op[0] {
        1 => match ready(service.__uh_tom_mick_dispatch_hold(&barrier)) {
            Ok(()) => b'O',
            Err(AdmissionError::AtCapacity { .. }) => b'C',
            Err(AdmissionError::Stopping { .. }) => b'S',
            Err(other) => panic!("unexpected admission error: {other:?}"),
        },
        2 => match ready(service.__uh_tom_mick_dispatch_ping()) {
            Ok(42) => b'P',
            Ok(other) => panic!("wrong result: {other}"),
            Err(AdmissionError::AtCapacity { .. }) => b'C',
            Err(AdmissionError::Stopping { .. }) => b'S',
            Err(other) => panic!("unexpected admission error: {other:?}"),
        },
        other => panic!("unexpected request code: {other}"),
    };
    conn.write_all(&[reply]).unwrap();
}

#[test]
fn real_unix_socket_capacity_shutdown_and_drain_use_generated_adapters() {
    let path = socket_path();
    let listener = UnixListener::bind(&path).unwrap();
    let service = Arc::new(SocketService {
        gate: Arc::new(SocketGate::new()),
        records: Arc::new(Mutex::new(Vec::new())),
        manual_count: Arc::new(AtomicU64::new(0)),
    });
    let barrier = Arc::new(Barrier::new(3));
    let for_server = Arc::clone(&service);
    let for_barrier = Arc::clone(&barrier);
    let server = thread::spawn(move || {
        let mut workers = Vec::new();
        for _ in 0..4 {
            let (conn, _) = listener.accept().unwrap();
            let service = Arc::clone(&for_server);
            let barrier = Arc::clone(&for_barrier);
            workers.push(thread::spawn(move || serve(conn, service, barrier)));
        }
        for worker in workers {
            worker.join().unwrap();
        }
    });
    let first_path = path.clone();
    let first = thread::spawn(move || connect(&first_path, 1));
    let second_path = path.clone();
    let second = thread::spawn(move || connect(&second_path, 1));
    // Both admitted calls have entered their business handlers; admission has
    // linearized before any capacity/shutdown assertions.
    barrier.wait();
    assert_eq!(service.gate.active(), 2);
    assert_eq!(connect(&path, 2), b'C');
    assert_eq!(service.manual_count.load(Ordering::Relaxed), 2);
    assert_eq!(service.gate.begin_shutdown(), 2);
    assert_eq!(connect(&path, 2), b'S');
    assert!(!service.gate.is_drained());
    barrier.wait();
    assert_eq!(first.join().unwrap(), b'O');
    assert_eq!(second.join().unwrap(), b'O');
    server.join().unwrap();
    assert!(service.gate.is_drained());
    assert_eq!(service.gate.active(), 0);
    assert_eq!(service.manual_count.load(Ordering::Relaxed), 2);
    assert_eq!(service.records.lock().unwrap().as_slice(), &[1, 1]);
    std::fs::remove_file(&path).unwrap();
}
