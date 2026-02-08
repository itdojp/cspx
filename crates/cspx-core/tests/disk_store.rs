use cspx_core::state_codec::StateCodecError;
use cspx_core::{DiskStateStore, DiskStateStoreOpenOptions, StateCodec, StateStore};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
struct ByteCodec;

impl StateCodec<u8> for ByteCodec {
    fn encode(&self, state: &u8) -> Vec<u8> {
        vec![*state]
    }

    fn decode(&self, bytes: &[u8]) -> Result<u8, StateCodecError> {
        if bytes.len() == 1 {
            return Ok(bytes[0]);
        }
        Err(StateCodecError::new("invalid byte state"))
    }
}

#[test]
fn disk_state_store_persists() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("states.log");

    let mut store = DiskStateStore::open(&path, ByteCodec).expect("open store");
    assert!(store.insert(1).expect("insert"));
    assert!(!store.insert(1).expect("insert"));
    assert!(store.insert(2).expect("insert"));
    assert_eq!(store.len(), 2);
    drop(store);

    assert!(path.with_extension("idx").exists());
    assert!(!path.with_extension("lock").exists());

    let store = DiskStateStore::open(&path, ByteCodec).expect("reopen");
    assert_eq!(store.len(), 2);
}

#[test]
fn disk_state_store_rejects_concurrent_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("states.log");

    let _store = DiskStateStore::open(&path, ByteCodec).expect("first open");
    let err = DiskStateStore::open(&path, ByteCodec).expect_err("second open should fail");
    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn disk_state_store_rebuilds_index_when_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("states.log");

    {
        let mut store = DiskStateStore::open(&path, ByteCodec).expect("open");
        assert!(store.insert(1).expect("insert"));
        assert!(store.insert(2).expect("insert"));
    }

    fs::remove_file(path.with_extension("idx")).expect("remove idx");
    let store = DiskStateStore::open(&path, ByteCodec).expect("reopen");
    assert_eq!(store.len(), 2);
}

#[test]
fn disk_state_store_rebuilds_index_when_corrupted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("states.log");

    {
        let mut store = DiskStateStore::open(&path, ByteCodec).expect("open");
        assert!(store.insert(1).expect("insert"));
        assert!(store.insert(2).expect("insert"));
    }

    fs::write(path.with_extension("idx"), b"broken-index\nzz\n").expect("corrupt idx");
    let store = DiskStateStore::open(&path, ByteCodec).expect("reopen");
    assert_eq!(store.len(), 2);
}

#[test]
fn disk_state_store_ignores_partial_log_tail() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("states.log");

    {
        let mut store = DiskStateStore::open(&path, ByteCodec).expect("open");
        assert!(store.insert(1).expect("insert"));
        assert!(store.insert(2).expect("insert"));
    }

    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open for append");
    file.write_all(b"ff").expect("write partial tail");
    file.flush().expect("flush");

    let mut store = DiskStateStore::open(&path, ByteCodec).expect("reopen");
    assert_eq!(store.len(), 2);
    assert!(!store.insert(1).expect("dedup"));
    assert!(!store.insert(2).expect("dedup"));

    let log = fs::read(&path).expect("read normalized log");
    assert!(!log.ends_with(b"ff"));
}

#[test]
fn disk_state_store_errors_on_invalid_complete_log_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("states.log");

    fs::write(&path, b"01\nzz\n").expect("write invalid log");
    let err = DiskStateStore::open(&path, ByteCodec).expect_err("open should fail");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn disk_state_store_metrics_track_io_and_collisions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("states.log");

    let mut store = DiskStateStore::open(&path, ByteCodec).expect("open");
    assert!(store.insert(1).expect("insert"));
    assert!(!store.insert(1).expect("dedup"));
    assert!(store.insert(2).expect("insert"));

    let metrics = store.metrics().clone();
    assert_eq!(metrics.insert_calls, 3);
    assert_eq!(metrics.insert_collisions, 1);
    assert!(metrics.log_write_bytes > 0);
    assert!(metrics.index_write_bytes > 0);
    assert_eq!(metrics.lock_contention_events, 0);
    assert_eq!(metrics.lock_retries, 0);
}

#[test]
fn disk_state_store_metrics_capture_lock_retry_wait() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("states.log");

    let first = DiskStateStore::open(&path, ByteCodec).expect("first open");
    let retry_path = path.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        started_tx.send(()).expect("signal start");
        let store = DiskStateStore::open_with_options(
            &retry_path,
            ByteCodec,
            DiskStateStoreOpenOptions {
                lock_retry_count: 50,
                lock_retry_backoff: Duration::from_millis(1),
            },
        )
        .expect("retry open");
        store.metrics().clone()
    });

    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("wait start signal");
    thread::sleep(Duration::from_millis(20));
    drop(first);

    let metrics = handle.join().expect("join");
    assert!(metrics.lock_contention_events > 0);
    assert!(metrics.lock_retries > 0);
    assert!(metrics.lock_wait_ns > 0);
}

#[test]
fn disk_state_store_metrics_track_index_load_and_rebuild() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("states.log");

    {
        let mut store = DiskStateStore::open(&path, ByteCodec).expect("open");
        assert!(store.insert(1).expect("insert"));
        assert!(store.insert(2).expect("insert"));
    }

    let loaded_store = DiskStateStore::open(&path, ByteCodec).expect("reopen");
    let loaded_metrics = loaded_store.metrics().clone();
    assert_eq!(loaded_metrics.index_entries_loaded, 2);
    assert!(loaded_metrics.index_read_bytes > 0);
    drop(loaded_store);

    fs::remove_file(path.with_extension("idx")).expect("remove idx");
    let rebuilt_store = DiskStateStore::open(&path, ByteCodec).expect("rebuild open");
    let rebuilt_metrics = rebuilt_store.metrics().clone();
    assert_eq!(rebuilt_metrics.index_entries_rebuilt, 2);
    assert!(rebuilt_metrics.log_read_bytes > 0);
}
