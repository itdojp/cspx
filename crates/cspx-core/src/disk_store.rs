use crate::state_codec::StateCodec;
use crate::store::StateStore;
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
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
                let bytes = hex::decode(&line).map_err(|err| {
                    io::Error::new(io::ErrorKind::InvalidData, format!("invalid hex: {err}"))
                })?;
                if bytes.is_empty() {
                    continue;
                }
                codec.decode(&bytes).map_err(|err| {
                    io::Error::new(io::ErrorKind::InvalidData, err.to_string())
                })?;
                index.insert(bytes);
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
    fn insert(&mut self, state: S) -> std::io::Result<bool> {
        let bytes = self.codec.encode(&state);
        if self.index.contains(&bytes) {
            return Ok(false);
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{}", hex::encode(&bytes))?;
        self.index.insert(bytes);
        Ok(true)
    }

    fn len(&self) -> usize {
        self.index.len()
    }
}
