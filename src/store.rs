//! Rows in temporary files: a pass reads the rows of a file one at a time and writes its result to a new file, so the
//! large lists of the cut do not stay in memory. The same code runs natively (files in a folder) and in the browser (a
//! [`Store`] of the page, for example OPFS).
//!
//! [`sort`] puts the rows of a file in the order of a key with a memory budget: it sorts blocks of rows that fit the
//! budget, writes each block to a file, and merges the blocks. The sort is stable.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// A temporary file: it is read from the start after it is written.
pub trait Blob: Read + Write + Seek + Send {}
impl<T: Read + Write + Seek + Send> Blob for T {}

/// Where the temporary files are made.
pub trait Store {
    fn create(&self) -> io::Result<Box<dyn Blob>>;
}

/// The files in memory (the tests, and the browser until it has a store in OPFS).
pub struct MemStore;

impl Store for MemStore {
    fn create(&self) -> io::Result<Box<dyn Blob>> {
        Ok(Box::new(io::Cursor::new(Vec::new())))
    }
}

/// The files in a folder. Each file is removed when it is dropped.
pub struct DirStore(pub PathBuf);

struct TempFile {
    f: std::fs::File,
    path: PathBuf,
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Read for TempFile {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.f.read(b)
    }
}

impl Write for TempFile {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.f.write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.f.flush()
    }
}

impl Seek for TempFile {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.f.seek(p)
    }
}

/// The memory budget of the cut in bytes (0: not set, then [`default_memory`]): each part of the cut takes its share of it.
static MEMORY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Set the memory budget of the cut, in bytes.
pub fn set_memory(bytes: usize) {
    MEMORY.store(bytes.max(64 << 10), Ordering::Relaxed);
}

/// The memory budget of the cut, in bytes.
pub fn memory() -> usize {
    match MEMORY.load(Ordering::Relaxed) {
        0 => default_memory(),
        m => m,
    }
}

/// The default memory budget: a quarter of the physical memory, at least 2 GiB. A larger budget sorts less on disk:
/// Sweden at zoom 12 takes 65 s with 2G and 56 s with 4G (docs/explanation/speed-and-memory.md). A quarter keeps the
/// peak RSS (about 1.5 to 2 times the heap) well below the physical memory.
pub fn default_memory() -> usize {
    static DEFAULT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *DEFAULT.get_or_init(|| (physical_memory() / 4).max(2 << 30))
}

/// The physical memory in bytes; 0 when it is not known.
// ponytail: Linux and macOS only; Windows and WebAssembly get 0 (then 2 GiB). Add GlobalMemoryStatusEx when Windows needs it.
fn physical_memory() -> usize {
    #[cfg(target_os = "linux")]
    {
        let kib = std::fs::read_to_string("/proc/meminfo").ok().and_then(|t| {
            t.lines().find(|l| l.starts_with("MemTotal:"))?.split_whitespace().nth(1)?.parse::<usize>().ok()
        });
        return kib.unwrap_or(0).saturating_mul(1024);
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/sbin/sysctl").args(["-n", "hw.memsize"]).output().ok();
        return out.and_then(|o| String::from_utf8(o.stdout).ok()?.trim().parse().ok()).unwrap_or(0);
    }
    #[allow(unreachable_code)]
    0
}

/// The folder of the temporary files (native); none: the temporary folder of the system.
static TEMP_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Set the folder of the temporary files of [`temp_store`] (native).
pub fn set_temp_dir(dir: Option<PathBuf>) {
    *TEMP_DIR.lock().expect("the temporary folder") = dir;
}

/// The folder of the temporary files: the folder of [`set_temp_dir`], else the temporary folder of the system.
pub fn temp_dir() -> PathBuf {
    TEMP_DIR.lock().expect("the temporary folder").clone().unwrap_or_else(std::env::temp_dir)
}

/// A maker of the store that [`temp_store`] gives on this thread.
pub type StoreMaker = std::rc::Rc<dyn Fn() -> Box<dyn Store>>;

thread_local! {
    static STORE: std::cell::RefCell<Option<StoreMaker>> = const { std::cell::RefCell::new(None) };
}

/// Set the store of the temporary files of the steps that run on this thread, or `None` for the default of
/// [`temp_store`]. The browser gives a store in OPFS, so that the temporary files are not in the memory of the tab.
pub fn set_temp_store(maker: Option<StoreMaker>) {
    STORE.with(|s| *s.borrow_mut() = maker);
}

/// Run `f` with the store of `maker` on this thread, then give back the store before.
pub fn with_temp_store<T>(maker: StoreMaker, f: impl FnOnce() -> T) -> T {
    let old = STORE.with(|s| s.borrow_mut().replace(maker));
    let r = f();
    STORE.with(|s| *s.borrow_mut() = old);
    r
}

/// The store of the network step: the store that [`set_temp_store`] gave on this thread, else a folder for temporary
/// files natively and memory in the browser.
pub fn temp_store() -> Box<dyn Store> {
    if let Some(maker) = STORE.with(|s| s.borrow().clone()) {
        return maker();
    }
    #[cfg(target_arch = "wasm32")]
    return Box::new(MemStore);
    #[cfg(not(target_arch = "wasm32"))]
    Box::new(DirStore(temp_dir()))
}

impl Store for DirStore {
    fn create(&self) -> io::Result<Box<dyn Blob>> {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = self.0.join(format!(
            "pbfcut-spill-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        // Unix: the name goes at once and the open file stays, so the system frees the file also when the process is
        // stopped (a stopped France run left 45 GB). Windows cannot remove an open file: there, the drop removes it.
        #[cfg(unix)]
        let _ = std::fs::remove_file(&path);
        Ok(Box::new(TempFile { f, path }))
    }
}

/// How a row is written as bytes and read back. `put` can add to the codec (for example a new shared value), `get`
/// only reads it. A pass reads one file and writes the next with the same codec, so `put` takes `&self`.
pub trait Codec<T> {
    fn put(&self, row: &T, out: &mut Vec<u8>);
    fn get(&self, b: &mut &[u8]) -> T;
}

/// The rows of a temporary file: each row is its length (4 bytes) and its bytes.
pub struct Rows {
    blob: Box<dyn Blob>,
    pub len: usize,
    pub bytes: u64,
}

/// Writes rows to a new temporary file.
pub struct Writer {
    w: BufWriter<Box<dyn Blob>>,
    buf: Vec<u8>,
    len: usize,
    bytes: u64,
}

impl Writer {
    pub fn new(store: &dyn Store) -> io::Result<Self> {
        Self::with_buffer(store, 1 << 20)
    }

    /// A writer with a write buffer of `bytes` (many writers open at a time: a small buffer each).
    pub fn with_buffer(store: &dyn Store, bytes: usize) -> io::Result<Self> {
        Ok(Writer {
            w: BufWriter::with_capacity(bytes, store.create()?),
            buf: Vec::new(),
            len: 0,
            bytes: 0,
        })
    }

    pub fn push<T>(&mut self, codec: &impl Codec<T>, row: &T) -> io::Result<()> {
        let mut buf = std::mem::take(&mut self.buf);
        buf.clear();
        codec.put(row, &mut buf);
        let r = self.push_bytes(&buf);
        self.buf = buf;
        r
    }

    fn push_bytes(&mut self, b: &[u8]) -> io::Result<()> {
        self.w.write_all(&(b.len() as u32).to_le_bytes())?;
        self.w.write_all(b)?;
        self.len += 1;
        self.bytes += 4 + b.len() as u64;
        Ok(())
    }

    /// Add `rows` rows that are already in the bytes of a file: each its length (4 bytes, little endian) and its bytes
    /// (made on other threads, for example by [`encode_rows`]).
    pub fn push_encoded(&mut self, encoded: &[u8], rows: usize) -> io::Result<()> {
        self.w.write_all(encoded)?;
        self.len += rows;
        self.bytes += encoded.len() as u64;
        Ok(())
    }

    pub fn finish(self) -> io::Result<Rows> {
        let blob = self.w.into_inner().map_err(|e| e.into_error())?;
        Ok(Rows {
            blob,
            len: self.len,
            bytes: self.bytes,
        })
    }
}

/// Reads the bytes of each row from the start of a file.
pub struct RawIter<R = Box<dyn Blob>> {
    r: BufReader<R>,
    left: usize,
}

impl<R: Read> RawIter<R> {
    fn next_into(&mut self, buf: &mut Vec<u8>) -> io::Result<bool> {
        if self.left == 0 {
            return Ok(false);
        }
        let mut n = [0u8; 4];
        self.r.read_exact(&mut n)?;
        buf.resize(u32::from_le_bytes(n) as usize, 0);
        self.r.read_exact(buf)?;
        self.left -= 1;
        Ok(true)
    }

    /// Add the bytes of the next row at the end of `data`.
    fn append_into(&mut self, data: &mut Vec<u8>) -> io::Result<bool> {
        if self.left == 0 {
            return Ok(false);
        }
        let mut n = [0u8; 4];
        self.r.read_exact(&mut n)?;
        let start = data.len();
        data.resize(start + u32::from_le_bytes(n) as usize, 0);
        self.r.read_exact(&mut data[start..])?;
        self.left -= 1;
        Ok(true)
    }
}

impl Rows {
    fn raw(self) -> io::Result<RawIter> {
        self.raw_with(1 << 20)
    }

    /// [`Rows::raw`] with a read buffer of `bytes` (many runs open at a time in a merge).
    fn raw_with(mut self, bytes: usize) -> io::Result<RawIter> {
        self.blob.seek(SeekFrom::Start(0))?;
        Ok(RawIter {
            r: BufReader::with_capacity(bytes, self.blob),
            left: self.len,
        })
    }

    /// Read the rows, one at a time, and keep the file for a next read.
    pub fn read_ref<'a, T, C: Codec<T>>(
        &'a mut self,
        codec: &'a C,
    ) -> io::Result<impl Iterator<Item = io::Result<T>> + 'a> {
        self.blob.seek(SeekFrom::Start(0))?;
        let mut raw = RawIter {
            r: BufReader::with_capacity(1 << 20, &mut self.blob),
            left: self.len,
        };
        let mut buf = Vec::new();
        Ok(std::iter::from_fn(move || match raw.next_into(&mut buf) {
            Ok(true) => Some(Ok(codec.get(&mut &buf[..]))),
            Ok(false) => None,
            Err(e) => Some(Err(e)),
        }))
    }

    /// Read the rows, one at a time. The file is gone after the last row.
    pub fn read<'c, T, C: Codec<T>>(
        self,
        codec: &'c C,
    ) -> io::Result<impl Iterator<Item = io::Result<T>> + 'c> {
        let mut raw = self.raw()?;
        let mut buf = Vec::new();
        Ok(std::iter::from_fn(move || match raw.next_into(&mut buf) {
            Ok(true) => Some(Ok(codec.get(&mut &buf[..]))),
            Ok(false) => None,
            Err(e) => Some(Err(e)),
        }))
    }
}

/// The rows of `rows` in the order of `key` (stable), with the key from the decoded row. See [`sort_bytes`].
pub fn sort<T, K: Ord + Send, C: Codec<T>>(
    rows: Rows,
    codec: &C,
    key: impl Fn(&T) -> K,
    budget: usize,
    store: &dyn Store,
) -> io::Result<Rows> {
    sort_bytes(rows, |b| key(&codec.get(&mut &b[..])), budget, store)
}

/// The rows of `rows` in the order of `key` (stable), with the key from the bytes of the row: a codec with its key
/// fields at fixed places at the start of the row gives a key with no decode of the rest. Each sorted block holds about
/// `budget` bytes of memory: the bytes of its rows in one buffer, and for each row its key and its place.
pub fn sort_bytes<K: Ord + Send>(
    rows: Rows,
    key: impl Fn(&[u8]) -> K,
    budget: usize,
    store: &dyn Store,
) -> io::Result<Rows> {
    sort_bytes_where(rows, |_| true, key, budget, store)
}

/// [`sort_bytes`] of the rows for which `keep` is true. `keep` sees each row one time, in the order of the file.
pub fn sort_bytes_where<K: Ord + Send>(
    rows: Rows,
    keep: impl FnMut(&[u8]) -> bool,
    key: impl Fn(&[u8]) -> K,
    budget: usize,
    store: &dyn Store,
) -> io::Result<Rows> {
    let mut runs = sorted_runs(rows, keep, &key, budget, store)?;
    if runs.len() == 1 {
        return Ok(runs.pop().expect("one run"));
    }
    merge(runs, &key, store, budget)
}

/// [`sort_bytes`] for rows that are read one time: the rows in the order of `key` (stable), merged from the sorted runs
/// while they are read. No merged copy is written: one write and one read of all rows less, and no time with the runs
/// and their copy on the disk together (the cut of France: step 5b had 1.5 GB of runs and wrote 1.5 GB more). The key
/// can borrow: the rows live as long as it does.
pub fn sorted<'a, K: Ord + Send + 'a, T: 'a, C: Codec<T>>(
    rows: Rows,
    key: impl Fn(&[u8]) -> K + 'a,
    budget: usize,
    store: &dyn Store,
    codec: &'a C,
) -> io::Result<impl Iterator<Item = io::Result<T>> + 'a> {
    let runs = sorted_runs(rows, |_| true, &key, budget, store)?;
    merged(runs, key, budget, codec)
}

/// The rows of sorted `runs`, merged while they are read: the smallest (key, run) first, as in `merge`.
fn merged<'a, K: Ord + Send + 'a, T: 'a, C: Codec<T>>(
    runs: Vec<Rows>,
    key: impl Fn(&[u8]) -> K + 'a,
    budget: usize,
    codec: &'a C,
) -> io::Result<impl Iterator<Item = io::Result<T>> + 'a> {
    let mut its: Vec<RawIter> = runs.into_iter().map(|r| r.raw_with(run_buffer(budget))).collect::<io::Result<_>>()?;
    let mut heads: Vec<Vec<u8>> = vec![Vec::new(); its.len()];
    let mut heap = BinaryHeap::new();
    for (i, it) in its.iter_mut().enumerate() {
        if it.next_into(&mut heads[i])? {
            heap.push(Reverse((key(&heads[i]), i)));
        }
    }
    // the smallest (key, run) first, as in `merge`: the same order as `sort_bytes`
    let mut failed = false;
    Ok(std::iter::from_fn(move || {
        if failed {
            return None;
        }
        let Reverse((_, i)) = heap.pop()?;
        let row = codec.get(&mut &heads[i][..]);
        match its[i].next_into(&mut heads[i]) {
            Ok(true) => heap.push(Reverse((key(&heads[i]), i))),
            Ok(false) => {}
            Err(e) => {
                failed = true;
                return Some(Err(e));
            }
        }
        Some(Ok(row))
    }))
}

/// The sorted runs of [`sort_bytes_where`] (at least one; one run needs no merge).
fn sorted_runs<K: Ord + Send>(
    rows: Rows,
    mut keep: impl FnMut(&[u8]) -> bool,
    key: &impl Fn(&[u8]) -> K,
    budget: usize,
    store: &dyn Store,
) -> io::Result<Vec<Rows>> {
    let mut raw = rows.raw_with(run_buffer(budget))?;
    let mut runs: Vec<Rows> = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    let mut index: Vec<(K, usize, usize)> = Vec::new(); // key, start and end of the row in `data`
    let flush = |data: &mut Vec<u8>, index: &mut Vec<(K, usize, usize)>, runs: &mut Vec<Rows>| -> io::Result<()> {
        runs.push(write_run(data, index, store, budget)?);
        Ok(())
    };
    let row = std::mem::size_of::<(K, usize, usize)>();
    loop {
        let start = data.len();
        if !raw.append_into(&mut data)? {
            break;
        }
        if !keep(&data[start..]) {
            data.truncate(start);
            continue;
        }
        index.push((key(&data[start..]), start, data.len()));
        if data.len() + index.len() * row >= budget {
            flush(&mut data, &mut index, &mut runs)?;
            // each run is an open file: at most MERGE_FAN of them (France: hundreds of runs, more files than a process can
            // open). The runs so far are the first rows of the input, so their merge keeps the sort stable.
            if runs.len() == MERGE_FAN {
                let all = std::mem::take(&mut runs);
                runs.push(merge(all, key, store, budget)?);
            }
        }
    }
    drop(raw);
    // the last block (also the only block, also when it is empty)
    if runs.is_empty() || !index.is_empty() {
        flush(&mut data, &mut index, &mut runs)?;
    }
    Ok(runs)
}

/// One sorted run of the rows in `data` (`index`: the key, the start and the end of each row); `data` and `index` are empty after.
fn write_run<K: Ord + Send>(data: &mut Vec<u8>, index: &mut Vec<(K, usize, usize)>, store: &dyn Store, budget: usize) -> io::Result<Rows> {
    // stable, on all threads
    {
        use rayon::slice::ParallelSliceMut;
        index.par_sort_by(|a, b| a.0.cmp(&b.0));
    }
    let mut w = Writer::with_buffer(store, run_buffer(budget))?;
    for (_, a, b) in index.drain(..) {
        w.push_bytes(&data[a..b])?;
    }
    data.clear();
    w.finish()
}

/// A sort with no unsorted file: [`Sorter::push`] keeps the rows in memory up to `budget` bytes and writes each full block as a
/// sorted run, and [`Sorter::sorted`] merges the runs while they are read. The disk holds one copy of the rows at most; with
/// [`sort_bytes`], the unsorted file and the runs (or the runs and their merged copy) are on the disk together. The order is
/// the order of [`sort_bytes`] (stable).
pub struct Sorter<'s, K, F: Fn(&[u8]) -> K> {
    key: F,
    budget: usize,
    store: &'s dyn Store,
    data: Vec<u8>,
    index: Vec<(K, usize, usize)>,
    runs: Vec<Rows>,
    buf: Vec<u8>,
}

impl<'s, K: Ord + Send, F: Fn(&[u8]) -> K> Sorter<'s, K, F> {
    pub fn new(key: F, budget: usize, store: &'s dyn Store) -> Self {
        Sorter { key, budget, store, data: Vec::new(), index: Vec::new(), runs: Vec::new(), buf: Vec::new() }
    }

    pub fn push<T>(&mut self, codec: &impl Codec<T>, row: &T) -> io::Result<()> {
        let mut buf = std::mem::take(&mut self.buf);
        buf.clear();
        codec.put(row, &mut buf);
        let r = self.push_bytes(&buf);
        self.buf = buf;
        r
    }

    fn push_bytes(&mut self, b: &[u8]) -> io::Result<()> {
        let start = self.data.len();
        self.data.extend_from_slice(b);
        self.index.push(((self.key)(&self.data[start..]), start, self.data.len()));
        if self.data.len() + self.index.len() * std::mem::size_of::<(K, usize, usize)>() >= self.budget {
            self.runs.push(write_run(&mut self.data, &mut self.index, self.store, self.budget)?);
            // at most MERGE_FAN open runs; the runs so far are the first rows, so their merge keeps the sort stable
            if self.runs.len() == MERGE_FAN {
                let all = std::mem::take(&mut self.runs);
                self.runs.push(merge(all, &self.key, self.store, self.budget)?);
            }
        }
        Ok(())
    }

    /// The rows in the order of the key, merged from the runs while they are read.
    pub fn sorted<'a, T: 'a, C: Codec<T>>(mut self, codec: &'a C) -> io::Result<impl Iterator<Item = io::Result<T>> + 'a>
    where
        K: 'a,
        F: 'a,
    {
        if self.runs.is_empty() || !self.index.is_empty() {
            self.runs.push(write_run(&mut self.data, &mut self.index, self.store, self.budget)?);
        }
        let (runs, key, budget) = (std::mem::take(&mut self.runs), self.key, self.budget);
        merged(runs, key, budget, codec)
    }
}

/// The runs that a merge of [`sort_bytes`] reads at once.
const MERGE_FAN: usize = 64;

/// The read or write buffer of each run of a sort with `budget` bytes: the buffers of a merge (at most [`MERGE_FAN`]
/// runs) fit the budget, from 16 KiB to 1 MiB each.
fn run_buffer(budget: usize) -> usize {
    (budget / (MERGE_FAN + 1)).clamp(16 << 10, 1 << 20)
}

/// The merge of sorted runs: the smallest (key, run) first; a run is in the order of the input, so the result is stable.
fn merge<K: Ord>(runs: Vec<Rows>, key: &impl Fn(&[u8]) -> K, store: &dyn Store, budget: usize) -> io::Result<Rows> {
    let buffer = run_buffer(budget);
    let mut its: Vec<RawIter> = runs.into_iter().map(|r| r.raw_with(buffer)).collect::<io::Result<_>>()?;
    let mut heap = BinaryHeap::new();
    let mut heads: Vec<Vec<u8>> = vec![Vec::new(); its.len()];
    for (i, it) in its.iter_mut().enumerate() {
        if it.next_into(&mut heads[i])? {
            heap.push(Reverse((key(&heads[i]), i)));
        }
    }
    let mut w = Writer::with_buffer(store, buffer)?;
    while let Some(Reverse((_, i))) = heap.pop() {
        w.push_bytes(&heads[i])?;
        if its[i].next_into(&mut heads[i])? {
            heap.push(Reverse((key(&heads[i]), i)));
        }
    }
    w.finish()
}

pub fn put_u32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}
pub fn put_i64(o: &mut Vec<u8>, v: i64) {
    o.extend_from_slice(&v.to_le_bytes());
}
pub fn put_f32(o: &mut Vec<u8>, v: f32) {
    o.extend_from_slice(&v.to_le_bytes());
}
pub fn take<const N: usize>(b: &mut &[u8]) -> [u8; N] {
    let (h, t) = b.split_at(N);
    *b = t;
    h.try_into().unwrap()
}
pub fn get_u32(b: &mut &[u8]) -> u32 {
    u32::from_le_bytes(take(b))
}
pub fn get_i64(b: &mut &[u8]) -> i64 {
    i64::from_le_bytes(take(b))
}
pub fn get_f32(b: &mut &[u8]) -> f32 {
    f32::from_le_bytes(take(b))
}
pub fn get_f64(b: &mut &[u8]) -> f64 {
    f64::from_le_bytes(take(b))
}

/// Rows partitioned by the range of an `i64` key (`key >> shift`) into temporary files: a large table is then sorted one
/// small part at a time in memory, on all threads, instead of by an external merge on one thread. Each row is written
/// once and read once. Write with [`Buckets::push`]; [`Buckets::finish`] gives the parts in the order of their ranges,
/// and [`SortedParts`] reads them back, each part sorted.

/// The write buffer of each bucket: a file can have about 100 buckets open at a time (a range of 2^27 node ids each).
const BUCKET_BUFFER: usize = 64 << 10;

pub struct Buckets {
    shift: u32,
    parts: std::collections::BTreeMap<i64, Writer>,
}

impl Buckets {
    pub fn new(shift: u32) -> Self {
        Buckets { shift, parts: Default::default() }
    }

    pub fn push<T>(&mut self, store: &dyn Store, key: i64, codec: &impl Codec<T>, row: &T) -> io::Result<()> {
        let part = match self.parts.entry(key >> self.shift) {
            std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::btree_map::Entry::Vacant(e) => e.insert(Writer::with_buffer(store, BUCKET_BUFFER)?),
        };
        part.push(codec, row)
    }

    /// The parts, in the order of their key ranges.
    /// Add rows that [`Buckets::encode`] made (on any thread): (bucket, the bytes of its rows, the number of rows).
    pub fn push_encoded(&mut self, store: &dyn Store, encoded: Vec<(i64, Vec<u8>, usize)>) -> io::Result<()> {
        for (bucket, bytes, rows) in encoded {
            let part = match self.parts.entry(bucket) {
                std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::btree_map::Entry::Vacant(e) => e.insert(Writer::with_buffer(store, BUCKET_BUFFER)?),
            };
            part.push_encoded(&bytes, rows)?;
        }
        Ok(())
    }

    /// The rows (key, row) in the bytes of their buckets, for [`Buckets::push_encoded`]: the work of `push` that can run on
    /// any thread (the thread that owns the files only appends the bytes).
    pub fn encode<T>(shift: u32, codec: &impl Codec<T>, rows: impl Iterator<Item = (i64, T)>) -> Vec<(i64, Vec<u8>, usize)> {
        let mut out: std::collections::BTreeMap<i64, (Vec<u8>, usize)> = Default::default();
        let mut row = Vec::new();
        for (key, r) in rows {
            row.clear();
            codec.put(&r, &mut row);
            let e = out.entry(key >> shift).or_default();
            e.0.extend_from_slice(&(row.len() as u32).to_le_bytes());
            e.0.extend_from_slice(&row);
            e.1 += 1;
        }
        out.into_iter().map(|(b, (bytes, n))| (b, bytes, n)).collect()
    }

    pub fn finish(self) -> io::Result<Vec<Rows>> {
        self.parts.into_values().map(Writer::finish).collect()
    }
}

/// The rows of the parts of [`Buckets::finish`], one part at a time, each sorted by `key` on all threads (not stable).
/// [`SortedParts::next_into`] fills a buffer of the caller: the buffer is used again for each part, so the memory is one
/// block of the largest part (a new vector for each part left the freed blocks of each size in the cache of the
/// allocator: France, a footprint of 11 GB for 1.4 GB of rows). The file of a part is gone when it is read.

pub struct SortedParts<'c, C, F> {
    parts: std::vec::IntoIter<Rows>,
    codec: &'c C,
    key: F,
    /// the bytes of memory for the rows of a part; a larger part is sorted on disk and read in pieces
    budget: usize,
    /// a large part, sorted on disk, while its rows are read
    sorted: Option<RawIter>,
}

impl<'c, C, F> SortedParts<'c, C, F> {
    /// The parts of `parts`, each sorted, with a quarter of [`memory`] for the rows in memory.
    pub fn new(parts: Vec<Rows>, codec: &'c C, key: F) -> Self {
        SortedParts { parts: parts.into_iter(), codec, key, budget: memory() / 4, sorted: None }
    }

    /// The next rows in the order of the key, in `buf` (emptied first): a whole part when it fits the budget, else a
    /// piece of a part sorted on disk. False when there are no more rows.
    pub fn next_into<T: Send, K: Ord + Send>(&mut self, buf: &mut Vec<T>) -> io::Result<bool>
    where
        C: Codec<T>,
        F: Fn(&T) -> K + Sync,
    {
        use rayon::slice::ParallelSliceMut;
        buf.clear();
        let most = (self.budget / std::mem::size_of::<T>().max(1)).max(1);
        if let Some(raw) = self.sorted.as_mut() {
            let mut bytes = Vec::new();
            while buf.len() < most && raw.next_into(&mut bytes)? {
                buf.push(self.codec.get(&mut &bytes[..]));
            }
            if !buf.is_empty() {
                return Ok(true);
            }
            self.sorted = None;
        }
        let Some(rows) = self.parts.next() else {
            return Ok(false);
        };
        let key = &self.key;
        if rows.len > most {
            // the external sort, with the same budget; then this part comes in pieces. The buffer of the part before
            // goes first, so that the sort and the buffer are not in memory at the same time
            *buf = Vec::new();
            let store = temp_store();
            let sorted = sort(rows, self.codec, key, self.budget, &*store)?;
            self.sorted = Some(sorted.raw()?);
            return self.next_into(buf);
        }
        buf.reserve(rows.len);
        for r in rows.read(self.codec)? {
            buf.push(r?);
        }
        buf.par_sort_unstable_by(|a, b| key(a).cmp(&key(b)));
        Ok(true)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A row of one i64.
    struct I64;
    impl Codec<i64> for I64 {
        fn put(&self, r: &i64, out: &mut Vec<u8>) {
            out.extend_from_slice(&r.to_le_bytes());
        }
        fn get(&self, b: &mut &[u8]) -> i64 {
            get_i64(b)
        }
    }

    fn keys() -> Vec<i64> {
        (0..5000).map(|i| (i * 7919) % 5000 - 1300).collect()
    }

    /// The default budget is at least 2 GiB, and a quarter of the physical memory when that is larger.
    #[test]
    fn default_budget() {
        let m = default_memory();
        assert!(m >= 2 << 30);
        assert!(physical_memory() == 0 || m >= physical_memory() / 4);
    }

    /// The sort with a small budget (many runs, merged) gives the rows in the order of the key.
    #[test]
    fn sort_with_a_small_budget() {
        let store = temp_store();
        let mut w = Writer::new(&*store).unwrap();
        for k in keys() {
            w.push(&I64, &k).unwrap();
        }
        let rows = sort(w.finish().unwrap(), &I64, |k: &i64| *k, 1000, &*store).unwrap();
        let got: Vec<i64> = rows.read(&I64).unwrap().map(|r| r.unwrap()).collect();
        let mut want = keys();
        want.sort();
        assert_eq!(got, want);
    }

    /// The sorter (no unsorted file; more runs than MERGE_FAN with this budget) gives the rows of `sort_bytes`, in the same stable order:
    /// the key is the row divided by 10, so rows with one key keep their order.
    #[test]
    fn the_sorter_equals_sort_bytes() {
        let store = temp_store();
        let key = |b: &[u8]| i64::from_le_bytes(b[..8].try_into().unwrap()).div_euclid(10);
        let mut w = Writer::new(&*store).unwrap();
        let mut s = Sorter::new(key, 200, &*store);
        for k in keys() {
            w.push(&I64, &k).unwrap();
            s.push(&I64, &k).unwrap();
        }
        let want: Vec<i64> = sort_bytes(w.finish().unwrap(), key, 200, &*store).unwrap().read(&I64).unwrap().map(|r| r.unwrap()).collect();
        let got: Vec<i64> = s.sorted(&I64).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!((got.len(), &got), (5000, &want));
        let empty = Sorter::new(key, 200, &*store);
        assert_eq!(empty.sorted(&I64).unwrap().count(), 0);
    }

    #[test]
    fn buckets_give_all_rows_in_key_order() {
        let store = temp_store();
        let mut b = Buckets::new(4);
        for k in keys() {
            b.push(&*store, k, &I64, &k).unwrap();
        }
        let mut parts = SortedParts::new(b.finish().unwrap(), &I64, |r: &i64| *r);
        let (mut buf, mut got) = (Vec::new(), Vec::new());
        while parts.next_into(&mut buf).unwrap() {
            got.extend(buf.iter().copied());
        }
        let mut want = keys();
        want.sort();
        assert_eq!(got, want);
    }

    /// A part larger than the budget is sorted on disk and comes in pieces, in the same order.
    #[test]
    fn a_large_part_comes_sorted_in_pieces() {
        let store = temp_store();
        let mut b = Buckets::new(40);
        for k in keys() {
            b.push(&*store, k, &I64, &k).unwrap();
        }
        let mut parts = SortedParts::new(b.finish().unwrap(), &I64, |r: &i64| *r);
        parts.budget = 1000 * 8;
        let (mut buf, mut got, mut pieces) = (Vec::new(), Vec::new(), 0);
        while parts.next_into(&mut buf).unwrap() {
            assert!(buf.len() <= 1000);
            got.extend(buf.iter().copied());
            pieces += 1;
        }
        let mut want = keys();
        want.sort();
        assert_eq!(got, want);
        assert!(pieces >= 5, "{pieces} pieces");
    }
}
