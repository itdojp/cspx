use cspx_core::{DiskStateStore, SimpleState, SimpleStateCodec, StateStore};

#[test]
fn disk_state_store_persists() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("states.log");

    let mut store = DiskStateStore::open(&path, SimpleStateCodec).expect("open store");
    assert!(store.insert(SimpleState::Stop));
    assert!(!store.insert(SimpleState::Stop));
    assert_eq!(store.len(), 1);

    let store = DiskStateStore::open(&path, SimpleStateCodec).expect("reopen");
    assert_eq!(store.len(), 1);
}
