use crate::state_codec::StateCodec;
use crate::store::StateStore;
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskStateStoreOpenOptions {
    pub lock_retry_count: u32,
    pub lock_retry_backoff: Duration,
    pub index_flush_every: u32,
}

impl Default for DiskStateStoreOpenOptions {
    fn default() -> Self {
        Self {
            lock_retry_count: 0,
            lock_retry_backoff: Duration::ZERO,
            index_flush_every: 1,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskStateStoreMetrics {
    pub open_ns: u64,
    pub lock_wait_ns: u64,
    pub lock_contention_events: u64,
    pub lock_retries: u64,
    pub index_load_ns: u64,
    pub index_rebuild_ns: u64,
    pub index_entries_loaded: u64,
    pub index_entries_rebuilt: u64,
    pub log_read_bytes: u64,
    pub index_read_bytes: u64,
    pub insert_calls: u64,
    pub insert_collisions: u64,
    pub log_write_ops: u64,
    pub log_write_ns: u64,
    pub log_write_bytes: u64,
    pub index_write_ops: u64,
    pub index_write_ns: u64,
    pub index_write_bytes: u64,
    pub pending_index_updates: u64,
}

impl DiskStateStoreMetrics {
    pub fn total_written_bytes(&self) -> u64 {
        self.log_write_bytes.saturating_add(self.index_write_bytes)
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
    metrics: DiskStateStoreMetrics,
    index_flush_every: u32,
    pending_index_updates: u32,
    current_log_len: u64,
    _lock: LockGuard,
    _marker: PhantomData<S>,
}

impl<S, C> DiskStateStore<S, C>
where
    C: StateCodec<S>,
{
    pub fn open(path: impl AsRef<Path>, codec: C) -> std::io::Result<Self> {
        Self::open_with_options(path, codec, DiskStateStoreOpenOptions::default())
    }

    pub fn open_with_options(
        path: impl AsRef<Path>,
        codec: C,
        options: DiskStateStoreOpenOptions,
    ) -> io::Result<Self> {
        if options.index_flush_every == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "index_flush_every must be >= 1",
            ));
        }
        let open_start = Instant::now();
        let paths = StorePaths::new(path.as_ref().to_path_buf());
        if let Some(parent) = paths
            .log_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }

        let lock_wait_start = Instant::now();
        let mut contention_events = 0u64;
        let mut retries = 0u64;
        let lock = loop {
            match LockGuard::acquire(&paths.lock_path) {
                Ok(lock) => break lock,
                Err(err)
                    if err.kind() == io::ErrorKind::WouldBlock
                        && retries < u64::from(options.lock_retry_count) =>
                {
                    contention_events = contention_events.saturating_add(1);
                    retries = retries.saturating_add(1);
                    if options.lock_retry_backoff.is_zero() {
                        thread::yield_now();
                    } else {
                        thread::sleep(options.lock_retry_backoff);
                    }
                }
                Err(err) => return Err(err),
            }
        };

        let log_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&paths.log_path)?;
        let log_len = log_file.metadata()?.len();

        let mut metrics = DiskStateStoreMetrics {
            lock_wait_ns: duration_ns(lock_wait_start.elapsed()),
            lock_contention_events: contention_events,
            lock_retries: retries,
            ..DiskStateStoreMetrics::default()
        };

        let index_load_start = Instant::now();
        let (index, current_log_len) = match load_index_from_file(&paths.idx_path, &codec, log_len)?
        {
            Some(index) => {
                metrics.index_entries_loaded = usize_to_u64(index.len())?;
                metrics.index_read_bytes = fs::metadata(&paths.idx_path)
                    .map(|metadata| metadata.len())
                    .unwrap_or_default();
                (index, log_len)
            }
            None => {
                metrics.log_read_bytes = log_len;
                let rebuild_start = Instant::now();
                let (rebuilt, normalized_log_len) =
                    rebuild_index_from_log(&paths.log_path, &codec)?;
                metrics.index_rebuild_ns = duration_ns(rebuild_start.elapsed());
                metrics.index_entries_rebuilt = usize_to_u64(rebuilt.len())?;

                let index_write_start = Instant::now();
                let bytes_written =
                    write_index_file(&paths.idx_path, &rebuilt, normalized_log_len)?;
                metrics.index_write_ns = metrics
                    .index_write_ns
                    .saturating_add(duration_ns(index_write_start.elapsed()));
                metrics.index_write_ops = metrics.index_write_ops.saturating_add(1);
                metrics.index_write_bytes = metrics.index_write_bytes.saturating_add(bytes_written);
                (rebuilt, normalized_log_len)
            }
        };
        metrics.index_load_ns = duration_ns(index_load_start.elapsed());
        metrics.open_ns = duration_ns(open_start.elapsed());
        metrics.pending_index_updates = 0;

        Ok(Self {
            paths,
            codec,
            index,
            metrics,
            index_flush_every: options.index_flush_every,
            pending_index_updates: 0,
            current_log_len,
            _lock: lock,
            _marker: PhantomData,
        })
    }

    fn flush_index_snapshot(&mut self) -> io::Result<()> {
        if self.pending_index_updates == 0 {
            return Ok(());
        }
        let index_write_start = Instant::now();
        let bytes_written =
            write_index_file(&self.paths.idx_path, &self.index, self.current_log_len)?;
        self.metrics.index_write_ns = self
            .metrics
            .index_write_ns
            .saturating_add(duration_ns(index_write_start.elapsed()));
        self.metrics.index_write_ops = self.metrics.index_write_ops.saturating_add(1);
        self.metrics.index_write_bytes =
            self.metrics.index_write_bytes.saturating_add(bytes_written);
        self.pending_index_updates = 0;
        self.metrics.pending_index_updates = 0;
        Ok(())
    }

    pub fn metrics(&self) -> &DiskStateStoreMetrics {
        &self.metrics
    }
}

impl<S, C> Drop for DiskStateStore<S, C>
where
    C: StateCodec<S>,
{
    fn drop(&mut self) {
        let _ = self.flush_index_snapshot();
    }
}

impl<S, C> StateStore<S> for DiskStateStore<S, C>
where
    C: StateCodec<S>,
{
    fn insert(&mut self, state: S) -> std::io::Result<bool> {
        self.metrics.insert_calls = self.metrics.insert_calls.saturating_add(1);
        let bytes = self.codec.encode(&state);
        if self.index.contains(&bytes) {
            self.metrics.insert_collisions = self.metrics.insert_collisions.saturating_add(1);
            return Ok(false);
        }

        let log_write_start = Instant::now();
        let append = append_log_record(&self.paths.log_path, &bytes)?;
        self.metrics.log_write_ns = self
            .metrics
            .log_write_ns
            .saturating_add(duration_ns(log_write_start.elapsed()));
        self.metrics.log_write_ops = self.metrics.log_write_ops.saturating_add(1);
        self.metrics.log_write_bytes = self
            .metrics
            .log_write_bytes
            .saturating_add(append.written_bytes);
        self.current_log_len = append.log_len;

        self.index.insert(bytes);
        self.pending_index_updates = self.pending_index_updates.saturating_add(1);
        self.metrics.pending_index_updates = u64::from(self.pending_index_updates);

        if self.pending_index_updates >= self.index_flush_every {
            self.flush_index_snapshot()?;
        }
        Ok(true)
    }

    fn len(&self) -> usize {
        self.index.len()
    }
}

struct AppendLogResult {
    log_len: u64,
    written_bytes: u64,
}

fn append_log_record(path: &Path, bytes: &[u8]) -> io::Result<AppendLogResult> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let encoded = hex::encode(bytes);
    writeln!(file, "{encoded}")?;
    file.flush()?;
    Ok(AppendLogResult {
        log_len: file.metadata()?.len(),
        written_bytes: (encoded.len() as u64).saturating_add(1),
    })
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

fn write_index_file(path: &Path, index: &HashSet<Vec<u8>>, log_len: u64) -> io::Result<u64> {
    let tmp_path = path.with_extension("idx.tmp");
    let mut file = fs::File::create(&tmp_path)?;
    let mut written_bytes = 0u64;
    let header = format!("{INDEX_MAGIC} log_len={log_len}");
    writeln!(file, "{header}")?;
    written_bytes = written_bytes.saturating_add((header.len() as u64).saturating_add(1));

    let mut records = index.iter().map(hex::encode).collect::<Vec<_>>();
    records.sort();
    for record in records {
        writeln!(file, "{record}")?;
        written_bytes = written_bytes.saturating_add((record.len() as u64).saturating_add(1));
    }
    file.flush()?;

    match fs::rename(&tmp_path, path) {
        Ok(()) => Ok(written_bytes),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(path)?;
            fs::rename(&tmp_path, path)?;
            Ok(written_bytes)
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

fn duration_ns(duration: Duration) -> u64 {
    duration
        .as_nanos()
        .min(u128::from(u64::MAX))
        .try_into()
        .unwrap_or(u64::MAX)
}
