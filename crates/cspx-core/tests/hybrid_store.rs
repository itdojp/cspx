use cspx_core::state_codec::StateCodecError;
use cspx_core::{
    DiskStateStore, HybridStateStore, HybridStateStoreOptions, StateCodec, StateStore,
};

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
fn hybrid_store_stays_in_memory_below_threshold() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("spill.log");
    let mut store = HybridStateStore::open(
        &path,
        ByteCodec,
        HybridStateStoreOptions {
            spill_threshold: 10,
            ..HybridStateStoreOptions::default()
        },
    )
    .expect("open");

    assert!(store.insert(1).expect("insert"));
    assert!(store.insert(2).expect("insert"));
    assert!(store.insert(3).expect("insert"));
    assert!(!store.insert(3).expect("dedup"));
    assert_eq!(store.len(), 3);
    assert!(!store.is_spilling());
    assert!(!path.exists());
}

#[test]
fn hybrid_store_spills_and_persists_states() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("spill.log");
    let mut store = HybridStateStore::open(
        &path,
        ByteCodec,
        HybridStateStoreOptions {
            spill_threshold: 2,
            ..HybridStateStoreOptions::default()
        },
    )
    .expect("open");

    assert!(store.insert(1).expect("insert"));
    assert!(store.insert(2).expect("insert"));
    assert!(store.insert(3).expect("insert"));
    assert_eq!(store.len(), 3);
    assert!(store.is_spilling());

    let spill_metrics = store.spill_metrics().expect("spill metrics");
    assert!(spill_metrics.insert_calls >= 3);
    drop(store);

    let persisted = DiskStateStore::open(&path, ByteCodec).expect("open spill");
    assert_eq!(persisted.len(), 3);
}

#[test]
fn hybrid_store_rejects_zero_threshold() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("spill.log");
    let err = HybridStateStore::open(
        &path,
        ByteCodec,
        HybridStateStoreOptions {
            spill_threshold: 0,
            ..HybridStateStoreOptions::default()
        },
    )
    .expect_err("threshold=0 must fail");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}
