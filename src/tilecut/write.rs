//! Read 2 of the tile cut: each element of the region file goes straight into the
//! files of its tiles, once, in the order of the file. Read 1 (steps 1 to 4 of `cut`) gave the plan: the books of the
//! nodes and the ways, the ways in more tiles, the extra nodes of each tile (sorted rows), the extra ways and the tiles
//! of the relations. No part files.

use super::*;

/// The elements of a tile from one block, in the order of the file, each written in `bytes` (see [`Chunk::put`]) at its
/// place in `starts`. One buffer holds all of them: a thread of the pool makes it and another one packs it, and the
/// allocator frees one buffer, not the strings and lists of each element (France: the step of the ways 18 s -> 29 s and
/// 500 MB more with an element for each).
#[derive(Default)]
struct Chunk {
    bytes: Vec<u8>,
    starts: Vec<u32>,
}

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Reads the bytes of [`Chunk::put`].
struct Cursor<'b>(&'b [u8]);

impl<'b> Cursor<'b> {
    fn varint(&mut self) -> u64 {
        let (mut v, mut s) = (0u64, 0);
        loop {
            let b = self.0[0];
            self.0 = &self.0[1..];
            v |= u64::from(b & 0x7f) << s;
            if b < 0x80 {
                return v;
            }
            s += 7;
        }
    }
    fn i64(&mut self) -> i64 {
        let (v, rest) = self.0.split_at(8);
        self.0 = rest;
        i64::from_le_bytes(v.try_into().expect("8 bytes"))
    }
    fn str(&mut self) -> String {
        let n = self.varint() as usize;
        let (v, rest) = self.0.split_at(n);
        self.0 = rest;
        // the bytes came from a &str
        String::from_utf8_lossy(v).into_owned()
    }
    fn tags(&mut self) -> Vec<(String, String)> {
        (0..self.varint()).map(|_| (self.str(), self.str())).collect()
    }
}

impl Chunk {
    fn len(&self) -> usize {
        self.starts.len()
    }
    /// The bytes that the chunk holds in memory.
    fn size(&self) -> usize {
        48 + self.bytes.len() + 4 * self.starts.len()
    }
    /// One element, written as: the kind (0 node, 1 way, 2 relation), the id (8 bytes), then the coordinates (two times
    /// 8 bytes), the refs (8 bytes each) or the members (id, type, role), and the tags.
    fn put(&mut self, element: &[u8]) {
        self.starts.push(self.bytes.len() as u32);
        self.bytes.extend_from_slice(element);
    }
    /// The elements from number `at` on: they go into a new chunk.
    fn split_off(&mut self, at: usize) -> Chunk {
        let from = self.starts[at];
        let bytes = self.bytes.split_off(from as usize);
        let starts = self.starts.split_off(at).into_iter().map(|x| x - from).collect();
        Chunk { bytes, starts }
    }
    /// The elements, whole.
    fn elements(&self) -> impl Iterator<Item = Elem> + '_ {
        let mut c = Cursor(&self.bytes);
        (0..self.len()).map(move |_| {
            let kind = c.0[0];
            c.0 = &c.0[1..];
            let id = c.i64();
            match kind {
                0 => {
                    let (lat, lon) = (f64::from_bits(c.i64() as u64), f64::from_bits(c.i64() as u64));
                    Elem::Node(RawNode { id, lat, lon, tags: c.tags() })
                }
                1 => {
                    let refs = (0..c.varint()).map(|_| c.i64()).collect();
                    Elem::Way(RawWay { id, tags: c.tags(), refs })
                }
                _ => {
                    let members = (0..c.varint())
                        .map(|_| {
                            let m = c.i64();
                            let t = match c.varint() {
                                0 => MemberType::Node,
                                1 => MemberType::Way,
                                _ => MemberType::Relation,
                            };
                            (m, t, c.str())
                        })
                        .collect();
                    Elem::Relation(Relation { id, tags: c.tags(), members })
                }
            }
        })
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    varint(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

fn put_tags<'a>(out: &mut Vec<u8>, tags: &mut dyn Iterator<Item = (&'a str, &'a str)>) {
    let tags: Vec<(&str, &str)> = tags.collect();
    varint(out, tags.len() as u64);
    for (k, v) in tags {
        put_str(out, k);
        put_str(out, v);
    }
}

/// Each element of a block to its tiles: its own tiles (`own`) and its extra tiles (the rows of the block), the tiles
/// with a file only.
struct Route<'a> {
    own: &'a (dyn Fn(i64, &mut Vec<u32>) + Sync),
    extra: &'a [(i64, u32)],
    with_file: &'a [bool],
    /// the tiles of this job (`lo..hi`): a block with many extra rows is routed in more jobs, one for each range of tiles
    range: (u32, u32),
    tiles: Vec<u32>,
    /// the element of now, written for [`Chunk::put`]
    element: Vec<u8>,
    out: FxHashMap<u32, Chunk>,
}

impl Route<'_> {
    /// The tiles of `id`; true when it has one. The element starts with its kind and its id.
    fn tiles_of(&mut self, kind: u8, id: i64) -> bool {
        self.tiles.clear();
        (self.own)(id, &mut self.tiles);
        self.tiles.extend(tiles_in(self.extra, id));
        let (lo, hi) = self.range;
        self.tiles.retain(|&t| self.with_file[t as usize] && lo <= t && t < hi);
        self.tiles.sort_unstable();
        self.tiles.dedup();
        self.element.clear();
        self.element.push(kind);
        self.element.extend_from_slice(&id.to_le_bytes());
        !self.tiles.is_empty()
    }
    /// The element to each of its tiles.
    fn take(&mut self) {
        for &t in &self.tiles {
            self.out.entry(t).or_default().put(&self.element);
        }
    }
}

impl RawSink for Route<'_> {
    fn node<'a>(&mut self, id: i64, lat: f64, lon: f64, tags: &mut dyn Iterator<Item = (&'a str, &'a str)>) {
        if self.tiles_of(0, id) {
            self.element.extend_from_slice(&lat.to_bits().to_le_bytes());
            self.element.extend_from_slice(&lon.to_bits().to_le_bytes());
            put_tags(&mut self.element, tags);
            self.take();
        }
    }
    fn way<'a>(&mut self, id: i64, tags: &mut dyn Iterator<Item = (&'a str, &'a str)>, refs: &mut dyn Iterator<Item = i64>) {
        if self.tiles_of(1, id) {
            let refs: Vec<i64> = refs.collect();
            varint(&mut self.element, refs.len() as u64);
            for r in refs {
                self.element.extend_from_slice(&r.to_le_bytes());
            }
            put_tags(&mut self.element, tags);
            self.take();
        }
    }
    fn relation<'a>(
        &mut self,
        id: i64,
        tags: &mut dyn Iterator<Item = (&'a str, &'a str)>,
        members: &mut dyn Iterator<Item = (i64, MemberType, &'a str)>,
    ) {
        if self.tiles_of(2, id) {
            let members: Vec<(i64, MemberType, &str)> = members.collect();
            varint(&mut self.element, members.len() as u64);
            for (m, t, role) in members {
                self.element.extend_from_slice(&m.to_le_bytes());
                varint(&mut self.element, t as u64);
                put_str(&mut self.element, role);
            }
            put_tags(&mut self.element, tags);
            self.take();
        }
    }
}

/// The bytes that the waiting elements of all tiles can hold: an eighth of the memory budget
/// ([`crate::store::memory`]). Above it, the tiles that hold the most write their blocks before they are full, so the
/// memory does not grow with the number of tiles.
fn waiting_limit() -> usize {
    crate::store::memory() / 8
}

/// The elements of a block of tile `t`, packed.
fn pack(t: u32, chunks: Vec<Chunk>, raw: bool) -> Result<(u32, Vec<u8>, CutCounts), String> {
    let mut w = PbfWriter::data(Vec::new()).raw_blocks(raw);
    let (mut n, mut last) = (CutCounts::default(), None);
    for c in &chunks {
        for e in c.elements() {
            put(&mut w, &mut n, &mut last, e)?;
        }
    }
    Ok((t, w.finish()?, n))
}

/// The tile files so far: the header of each file at its first bytes, and the numbers of elements of each tile. The
/// elements of each tile wait until they fill a block of [`BLOCK`] elements: one input block gives a tile only some
/// elements (France at zoom 8: 77 nodes on average, blocks of 1.3 KB, the files 11% larger). The bytes go to `out` on
/// this thread (in the browser the storage belongs to it).
pub(super) struct Writers<'o> {
    started: Vec<bool>,
    counts: Vec<CutCounts>,
    head: Vec<u8>,
    raw: bool,
    /// the bytes of a tile file to its place; true for the first bytes of the file (its header)
    out: Box<dyn FnMut(u32, &[u8], bool) -> Result<(), String> + 'o>,
    /// the elements of each tile that wait for a full block (as the blocks of the file gave them), their number and
    /// their bytes ([`size`])
    waiting: Vec<Vec<Chunk>>,
    elems: Vec<usize>,
    bytes: Vec<usize>,
    total: usize,
    /// the blocks to pack and write, in the order of the file in each tile
    ready: Vec<(u32, Vec<Chunk>)>,
}

impl<'o> Writers<'o> {
    pub(super) fn new(n: usize, timestamp: Option<i64>, raw: bool, out: Box<dyn FnMut(u32, &[u8], bool) -> Result<(), String> + 'o>) -> Result<Self, String> {
        let head = PbfWriter::new(Vec::new(), timestamp)?.finish()?;
        Ok(Writers {
            started: vec![false; n],
            counts: vec![CutCounts::default(); n],
            head,
            raw,
            out,
            waiting: (0..n).map(|_| Vec::new()).collect(),
            elems: vec![0; n],
            bytes: vec![0; n],
            total: 0,
            ready: Vec::new(),
        })
    }

    /// The elements of a tile from one block, in the order of the file: a full block is ready to write. This thread
    /// moves the chunks only, not each element.
    fn add(&mut self, t: u32, mut chunk: Chunk) {
        let i = t as usize;
        while chunk.len() > 0 {
            let room = BLOCK - self.elems[i];
            let rest = if chunk.len() > room { chunk.split_off(room) } else { Chunk::default() };
            let b = chunk.size();
            self.elems[i] += chunk.len();
            self.bytes[i] += b;
            self.total += b;
            self.waiting[i].push(chunk);
            if self.elems[i] == BLOCK {
                self.take(i);
            }
            chunk = rest;
        }
    }

    /// The waiting elements of tile `i` to a block.
    fn take(&mut self, i: usize) {
        self.total -= self.bytes[i];
        (self.elems[i], self.bytes[i]) = (0, 0);
        self.ready.push((i as u32, std::mem::take(&mut self.waiting[i])));
    }

    /// Above [`waiting_limit`], the waiting elements of the tiles that hold the most to blocks, until half of it is left.
    fn over_limit(&mut self) {
        if self.total <= waiting_limit() {
            return;
        }
        let mut most: Vec<usize> = (0..self.bytes.len()).filter(|&i| self.bytes[i] > 0).collect();
        most.sort_unstable_by_key(|&i| std::cmp::Reverse(self.bytes[i]));
        for i in most {
            if self.total <= waiting_limit() / 2 {
                break;
            }
            self.take(i);
        }
    }

    /// The end of a kind: the ready blocks and the waiting elements of each tile, packed on the pool and written (the
    /// blocks of the next kind come after them in each file).
    fn write_all(&mut self) -> Result<(), String> {
        for i in 0..self.waiting.len() {
            if !self.waiting[i].is_empty() {
                self.take(i);
            }
        }
        let raw = self.raw;
        let made = std::mem::take(&mut self.ready).into_par_iter().map(|(t, pieces)| pack(t, pieces, raw)).collect::<Result<Vec<_>, String>>()?;
        self.put_bytes(made)
    }

    /// The numbers of elements of each tile; the writers go.
    pub(super) fn into_counts(self) -> Vec<CutCounts> {
        self.counts
    }

    /// The packed blocks, by tile, to the tile files.
    fn put_bytes(&mut self, made: Vec<(u32, Vec<u8>, CutCounts)>) -> Result<(), String> {
        for (t, bytes, n) in made {
            let i = t as usize;
            if !self.started[i] {
                self.started[i] = true;
                (self.out)(t, &self.head, true)?;
            }
            (self.out)(t, &bytes, false)?;
            let c = &mut self.counts[i];
            c.nodes += n.nodes;
            c.ways += n.ways;
            c.relations += n.relations;
        }
        Ok(())
    }
}

/// The memory of the output of one extra row while it waits for the writers (the row, its piece and its bytes): the
/// rows in a batch are limited so that the batches in flight fit their share of the memory budget.
const ROW_BYTES: usize = 512;

/// A block (or a part of a block) to route: the block, all its extra rows (shared by its jobs, so that each element
/// knows all its tiles), the tiles of this job and the number of its rows in them.
struct Job {
    block: std::sync::Arc<Vec<u8>>,
    rows: std::sync::Arc<Vec<(i64, u32)>>,
    range: (u32, u32),
    n: usize,
}

/// The jobs of a block: one job, or, when the block has more than `most` extra rows, one job for each range of tiles
/// with at most `most` rows (a tile is never in two jobs, so the elements of each tile keep the order of the file). A
/// block of nodes on a long boundary can have millions of rows: it goes to every tile with a member of the boundary.
fn jobs(block: Vec<u8>, rows: Vec<(i64, u32)>, most: usize, out: &mut std::collections::VecDeque<Job>) {
    let block = std::sync::Arc::new(block);
    if rows.len() <= most {
        let n = rows.len();
        out.push_back(Job { block, rows: std::sync::Arc::new(rows), range: (0, u32::MAX), n });
        return;
    }
    let mut count: std::collections::BTreeMap<u32, usize> = Default::default();
    for r in &rows {
        *count.entry(r.1).or_default() += 1;
    }
    let rows = std::sync::Arc::new(rows);
    let (mut lo, mut n) = (0u32, 0usize);
    for (&t, &c) in &count {
        if n > 0 && n + c > most {
            out.push_back(Job { block: block.clone(), rows: rows.clone(), range: (lo, t), n });
            (lo, n) = (t, 0);
        }
        n += c;
    }
    out.push_back(Job { block, rows, range: (lo, u32::MAX), n });
}

/// The blocks at `blocks` (ascending places) to the tiles: this thread reads each block and its extra rows (`extra`,
/// which gets the span of the block and gives its rows), the pool decodes and routes the blocks, and this thread gives
/// the elements to the writers, in the order of the file. A batch has at most one job for each thread, and its extra
/// rows fit an eighth of the memory budget ([`crate::store::memory`]).
#[allow(clippy::too_many_arguments)]
pub(super) fn route_blocks<R: Read + Seek>(
    r: &mut R,
    blocks: &[(u64, Span)],
    kind: usize,
    mut extra: impl FnMut(Span) -> Result<Vec<(i64, u32)>, String>,
    own: &(dyn Fn(i64, &mut Vec<u32>) + Sync),
    with_file: &[bool],
    writers: &mut Writers,
) -> Result<(), String> {
    let mut kinds = [false; 3];
    kinds[kind] = true;
    // one job for each thread in a batch (the pipeline holds three batches), and at most `most` extra rows in a batch
    let n = rayon::current_num_threads();
    let most = (crate::store::memory() / 8 / ROW_BYTES).max(1);
    let per_job = (most / n).max(1);
    let raw = writers.raw;
    let (mut i, mut pos) = (0, None);
    let mut waiting = std::collections::VecDeque::new();
    // the ready blocks of tiles go with the next batch that this thread reads, and the pool packs them while it decodes
    // the blocks of that batch: the blocks of a tile stay in the order of the file (a batch is given after the batches
    // before it, and a tile is in one job of a block only)
    type Pack = std::sync::Mutex<Option<(u32, Vec<Chunk>)>>;
    let writers = std::cell::RefCell::new(writers);
    crate::extract::pipeline(
        || {
            let (mut batch, mut rows_in): (Vec<Job>, usize) = (Vec::with_capacity(n), 0);
            while batch.len() < n && rows_in < most {
                if waiting.is_empty() {
                    let Some(&(at, span)) = blocks.get(i) else { break };
                    i += 1;
                    let rows = extra(span)?;
                    if pos != Some(at) {
                        r.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
                    }
                    let b = read_block(r)?.ok_or("a block after the end of the file")?;
                    pos = Some(at + b.len() as u64);
                    jobs(b, rows, per_job, &mut waiting);
                }
                let job = waiting.pop_front().expect("a job");
                rows_in += job.n;
                batch.push(job);
            }
            let packs: Vec<Pack> = writers.borrow_mut().ready.drain(..).map(|j| std::sync::Mutex::new(Some(j))).collect();
            Ok((!batch.is_empty() || !packs.is_empty()).then_some((batch, packs)))
        },
        |(batch, packs): &(Vec<Job>, Vec<Pack>)| {
            let (routed, packed) = rayon::join(
                || {
                    batch
                        .par_iter()
                        .map_init(Buffers::default, |buf, job| {
                            let mut s = Route { own, extra: &job.rows, with_file, range: job.range, tiles: Vec::new(), element: Vec::new(), out: FxHashMap::default() };
                            decode_block(&job.block, kinds, buf, &mut s)?;
                            // the elements by tile; the writers collect them into full blocks
                            let mut made: Vec<(u32, Chunk)> = s.out.into_iter().collect();
                            made.sort_unstable_by_key(|x| x.0);
                            Ok(made)
                        })
                        .collect::<Result<Vec<_>, String>>()
                },
                || {
                    packs.par_iter()
                        .map(|j| {
                            let (t, pieces) = j.lock().unwrap_or_else(|e| e.into_inner()).take().expect("a job is packed once");
                            pack(t, pieces, raw)
                        })
                        .collect::<Result<Vec<_>, String>>()
                },
            );
            Ok(vec![(routed?, packed?)])
        },
        |made, _| {
            let mut w = writers.borrow_mut();
            for (routed, packed) in made {
                // the blocks made before the elements of this batch
                w.put_bytes(packed)?;
                for block in routed {
                    for (t, chunk) in block {
                        w.add(t, chunk);
                    }
                }
            }
            w.over_limit();
            Ok(true)
        },
    )?;
    writers.into_inner().write_all()
}

/// Where the bytes of each tile file go: the file `z_x_y.osm.pbf` in `dir` (the build by tiles), the part "tile" in the
/// parts (a folder, or the store), or the sink (the browser).
pub(super) fn outputs<'o>(grid: &'o Grid<'o>, dir: Option<&'o Path>, parts: &'o Parts, sink: Option<&'o mut Sink<'_>>) -> Result<Box<dyn FnMut(u32, &[u8], bool) -> Result<(), String> + 'o>, String> {
    let e = |p: &Path, e: io::Error| format!("{}: {e}", p.display());
    if let Some(sink) = sink {
        return Ok(Box::new(move |t, b, _| sink(t, b)));
    }
    let append = move |path: PathBuf, b: &[u8], first: bool| -> Result<(), String> {
        let mut o = std::fs::OpenOptions::new();
        if first {
            o.write(true).create(true).truncate(true);
        } else {
            o.append(true);
        }
        o.open(&path).and_then(|mut f| f.write_all(b)).map_err(|x| e(&path, x))
    };
    Ok(match (dir, &parts.files) {
        (Some(dir), _) => Box::new(move |t, b, first| append(dir.join(format!("{}.osm.pbf", grid.file(t))), b, first)),
        (None, Files::Dir(d)) => Box::new(move |t, b, first| append(Parts::path(d, t, "tile"), b, first)),
        (None, Files::Store(c)) => Box::new(move |t, b, _| c.append((t, "tile"), b).map_err(|x| format!("a tile file of the tile cut: {x}"))),
    })
}
