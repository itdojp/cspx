use crate::state_codec::StateCodec;
use crate::store::StateStore;
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

const INDEX_MAGIC: &str = "cspx-disk-index-v1";

#[derive(Debug)]
struct StorePaths {
    log_path: PathBuf,
    idx_path: PathBuf,
    lock_path: PathBuf,
}

impl StorePaths {
    fn new(log_path: PathBuf) -> Self {
        Self {
            idx_path: log_path.with_extension("idx"),
            lock_path: log_path.with_extension("lock"),
            log_path,
        }
    }
}

#[derive(Debug)]
struct LockGuard {
    path: PathBuf,
    file: Option<fs::File>,
}

impl LockGuard {
    fn acquire(path: &Path) -> io::Result<Self> {
        let mut file = match OpenOptions::new().create_new(true).write(true).open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    format!("state store is already open: {}", path.display()),
                ));
            }
            Err(err) => return Err(err),
        };
        writeln!(file, "pid={}", std::process::id())?;
        file.flush()?;
        Ok(Self {
            path: path.to_path_buf(),
            file: Some(file),
        })
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        self.file.take();
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug)]
pub struct DiskStateStore<S, C>
where
    C: StateCodec<S>,
{
    paths: StorePaths,
    codec: C,
    index: HashSet<Vec<u8>>,
    _lock: LockGuard,
    _marker: PhantomData<S>,
}

impl<S, C> DiskStateStore<S, C>
where
    C: StateCodec<S>,
{
    pub fn open(path: impl AsRef<Path>, codec: C) -> std::io::Result<Self> {
        let paths = StorePaths::new(path.as_ref().to_path_buf());
        if let Some(parent) = paths
            .log_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let lock = LockGuard::acquire(&paths.lock_path)?;
        let log_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&paths.log_path)?;
        let log_len = log_file.metadata()?.len();
        let index = match load_index_from_file(&paths.idx_path, &codec, log_len)? {
            Some(index) => index,
            None => {
                let (rebuilt, normalized_log_len) =
                    rebuild_index_from_log(&paths.log_path, &codec)?;
                write_index_file(&paths.idx_path, &rebuilt, normalized_log_len)?;
                rebuilt
            }
        };

        Ok(Self {
            paths,
            codec,
            index,
            _lock: lock,
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

        let log_len = append_log_record(&self.paths.log_path, &bytes)?;
        self.index.insert(bytes.clone());
        if let Err(err) = write_index_file(&self.paths.idx_path, &self.index, log_len) {
            self.index.remove(&bytes);
            return Err(err);
        }
        Ok(true)
    }

    fn len(&self) -> usize {
        self.index.len()
    }
}

fn append_log_record(path: &Path, bytes: &[u8]) -> io::Result<u64> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", hex::encode(bytes))?;
    file.flush()?;
    Ok(file.metadata()?.len())
}

fn load_index_from_file<S, C>(
    idx_path: &Path,
    codec: &C,
    expected_log_len: u64,
) -> io::Result<Option<HashSet<Vec<u8>>>>
where
    C: StateCodec<S>,
{
    if !idx_path.exists() {
        return Ok(None);
    }

    let file = fs::File::open(idx_path)?;
    let mut lines = BufReader::new(file).lines();
    let Some(header) = lines.next() else {
        return Ok(None);
    };
    let Some(log_len) = parse_index_header(&header?) else {
        return Ok(None);
    };
    if log_len != expected_log_len {
        return Ok(None);
    }

    let mut index = HashSet::new();
    for line in lines {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let bytes = decode_validated_record(&line, codec).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid index record in {}", idx_path.display()),
            )
        })?;
        index.insert(bytes);
    }
    Ok(Some(index))
}

fn parse_index_header(line: &str) -> Option<u64> {
    let value = line
        .trim_end_matches('\r')
        .strip_prefix(INDEX_MAGIC)?
        .strip_prefix(" log_len=")?;
    value.parse().ok()
}

fn rebuild_index_from_log<S, C>(log_path: &Path, codec: &C) -> io::Result<(HashSet<Vec<u8>>, u64)>
where
    C: StateCodec<S>,
{
    let data = fs::read(log_path)?;
    let mut index = HashSet::new();
    let mut line_start = 0usize;
    let mut normalized_len = 0u64;

    for (cursor, byte) in data.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }

        let line = &data[line_start..cursor];
        if !line.is_empty() {
            let text = std::str::from_utf8(line).map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid utf8 record: {err}"),
                )
            })?;
            let text = text.trim_end_matches('\r');
            if !text.is_empty() {
                let bytes = decode_validated_record(text, codec).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid log record")
                })?;
                index.insert(bytes);
            }
        }
        line_start = cursor + 1;
        normalized_len = usize_to_u64(line_start)?;
    }

    if line_start < data.len() {
        let file = OpenOptions::new().write(true).open(log_path)?;
        file.set_len(normalized_len)?;
    } else {
        normalized_len = usize_to_u64(data.len())?;
    }

    Ok((index, normalized_len))
}

fn write_index_file(path: &Path, index: &HashSet<Vec<u8>>, log_len: u64) -> io::Result<()> {
    let tmp_path = path.with_extension("idx.tmp");
    let mut file = fs::File::create(&tmp_path)?;
    writeln!(file, "{INDEX_MAGIC} log_len={log_len}")?;

    let mut records = index.iter().map(hex::encode).collect::<Vec<_>>();
    records.sort();
    for record in records {
        writeln!(file, "{record}")?;
    }
    file.flush()?;

    match fs::rename(&tmp_path, path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(path)?;
            fs::rename(&tmp_path, path)?;
            Ok(())
        }
        Err(err) => {
            let _ = fs::remove_file(&tmp_path);
            Err(err)
        }
    }
}

fn decode_validated_record<S, C>(line: &str, codec: &C) -> Option<Vec<u8>>
where
    C: StateCodec<S>,
{
    let bytes = hex::decode(line).ok()?;
    if bytes.is_empty() {
        return None;
    }
    codec.decode(&bytes).ok()?;
    Some(bytes)
}

fn usize_to_u64(value: usize) -> io::Result<u64> {
    u64::try_from(value).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "size overflow"))
}
