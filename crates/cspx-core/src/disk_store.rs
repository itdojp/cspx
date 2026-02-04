use crate::state_codec::StateCodec;
use crate::store::StateStore;
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct DiskStateStore<S, C>
where
    C: StateCodec<S>,
{
    path: PathBuf,
    codec: C,
    index: HashSet<Vec<u8>>,
    _marker: PhantomData<S>,
}

impl<S, C> DiskStateStore<S, C>
where
    C: StateCodec<S>,
{
    pub fn open(path: impl AsRef<Path>, codec: C) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut index = HashSet::new();
        if path.exists() {
            let file = fs::File::open(&path)?;
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let line = line?;
                if line.is_empty() {
                    continue;
                }
                let bytes = hex::decode(line).unwrap_or_default();
                if !bytes.is_empty() {
                    index.insert(bytes);
                }
            }
        }
        Ok(Self {
            path,
            codec,
            index,
            _marker: PhantomData,
        })
    }
}

impl<S, C> StateStore<S> for DiskStateStore<S, C>
where
    C: StateCodec<S>,
{
    fn insert(&mut self, state: S) -> bool {
        let bytes = self.codec.encode(&state);
        if self.index.contains(&bytes) {
            return false;
        }
        self.index.insert(bytes.clone());
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = writeln!(file, "{}", hex::encode(bytes));
        }
        true
    }

    fn len(&self) -> usize {
        self.index.len()
    }
}
