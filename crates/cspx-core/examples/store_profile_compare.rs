use cspx_core::state_codec::StateCodecError;
use cspx_core::{DiskStateStore, InMemoryStateStore, StateCodec, StateStore};
use std::error::Error;
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
struct U32Codec;

impl StateCodec<u32> for U32Codec {
    fn encode(&self, state: &u32) -> Vec<u8> {
        state.to_le_bytes().to_vec()
    }

    fn decode(&self, bytes: &[u8]) -> Result<u32, StateCodecError> {
        if bytes.len() != 4 {
            return Err(StateCodecError::new("invalid u32 bytes"));
        }
        let mut buf = [0u8; 4];
        buf.copy_from_slice(bytes);
        Ok(u32::from_le_bytes(buf))
    }
}

fn build_workload(unique_states: u32, repeats_per_state: u32) -> Vec<u32> {
    let mut workload =
        Vec::with_capacity((unique_states as usize).saturating_mul(repeats_per_state as usize));
    for repeat in 0..repeats_per_state {
        for state in 0..unique_states {
            workload.push((state + repeat) % unique_states);
        }
    }
    workload
}

fn main() -> Result<(), Box<dyn Error>> {
    let workload = build_workload(5_000, 3);

    let mut memory_store = InMemoryStateStore::new();
    let memory_start = Instant::now();
    let mut memory_collisions = 0u64;
    for state in &workload {
        if !memory_store.insert(*state)? {
            memory_collisions = memory_collisions.saturating_add(1);
        }
    }
    let memory_elapsed_ns = memory_start.elapsed().as_nanos();

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("states.log");
    let mut disk_store = DiskStateStore::open(&path, U32Codec)?;
    let disk_start = Instant::now();
    for state in &workload {
        let _ = disk_store.insert(*state)?;
    }
    let disk_elapsed_ns = disk_start.elapsed().as_nanos();
    let disk_metrics = disk_store.metrics().clone();

    println!("workload_calls={}", workload.len());
    println!("workload_unique_states={}", memory_store.len());
    println!("inmemory_elapsed_ns={memory_elapsed_ns}");
    println!("inmemory_collisions={memory_collisions}");
    println!("disk_elapsed_ns={disk_elapsed_ns}");
    println!("disk_insert_calls={}", disk_metrics.insert_calls);
    println!("disk_insert_collisions={}", disk_metrics.insert_collisions);
    println!("disk_log_write_bytes={}", disk_metrics.log_write_bytes);
    println!("disk_index_write_bytes={}", disk_metrics.index_write_bytes);
    println!(
        "disk_total_write_bytes={}",
        disk_metrics.total_written_bytes()
    );
    println!("disk_lock_wait_ns={}", disk_metrics.lock_wait_ns);

    Ok(())
}
