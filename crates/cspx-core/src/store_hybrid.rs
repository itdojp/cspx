use crate::disk_store::{DiskStateStore, DiskStateStoreMetrics, DiskStateStoreOpenOptions};
use crate::state_codec::StateCodec;
use crate::store::StateStore;
use std::collections::HashSet;
use std::hash::Hash;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct HybridStateStoreOptions {
    pub spill_threshold: usize,
    pub disk_options: DiskStateStoreOpenOptions,
}

impl Default for HybridStateStoreOptions {
    fn default() -> Self {
        Self {
            spill_threshold: 100_000,
            disk_options: DiskStateStoreOpenOptions::default(),
        }
    }
}

#[derive(Debug)]
pub struct HybridStateStore<S, C>
where
    S: Clone + Eq + Hash,
    C: StateCodec<S> + Clone,
{
    in_memory: HashSet<S>,
    spill_path: PathBuf,
    codec: C,
    options: HybridStateStoreOptions,
    spill_store: Option<DiskStateStore<S, C>>,
}

impl<S, C> HybridStateStore<S, C>
where
    S: Clone + Eq + Hash,
    C: StateCodec<S> + Clone,
{
    pub fn open(
        spill_path: impl AsRef<Path>,
        codec: C,
        options: HybridStateStoreOptions,
    ) -> io::Result<Self> {
        if options.spill_threshold == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "spill_threshold must be >= 1",
            ));
        }
        Ok(Self {
            in_memory: HashSet::new(),
            spill_path: spill_path.as_ref().to_path_buf(),
            codec,
            options,
            spill_store: None,
        })
    }

    pub fn is_spilling(&self) -> bool {
        self.spill_store.is_some()
    }

    pub fn spill_metrics(&self) -> Option<&DiskStateStoreMetrics> {
        self.spill_store.as_ref().map(DiskStateStore::metrics)
    }

    pub fn spill_path(&self) -> &Path {
        &self.spill_path
    }

    fn activate_spill_if_needed(&mut self) -> io::Result<()> {
        if self.spill_store.is_some() || self.in_memory.len() <= self.options.spill_threshold {
            return Ok(());
        }

        let mut spill = DiskStateStore::open_with_options(
            &self.spill_path,
            self.codec.clone(),
            self.options.disk_options,
        )?;
        for state in &self.in_memory {
            let _ = spill.insert(state.clone())?;
        }
        self.spill_store = Some(spill);
        Ok(())
    }
}

impl<S, C> StateStore<S> for HybridStateStore<S, C>
where
    S: Clone + Eq + Hash,
    C: StateCodec<S> + Clone,
{
    fn insert(&mut self, state: S) -> io::Result<bool> {
        if !self.in_memory.insert(state.clone()) {
            return Ok(false);
        }

        let should_spill = self.in_memory.len() > self.options.spill_threshold;
        let had_spill = self.spill_store.is_some();
        self.activate_spill_if_needed()?;
        if should_spill && had_spill {
            if let Some(spill_store) = self.spill_store.as_mut() {
                let _ = spill_store.insert(state)?;
            }
        }
        Ok(true)
    }

    fn len(&self) -> usize {
        self.in_memory.len()
    }
}
