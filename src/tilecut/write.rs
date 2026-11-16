//! Read 2 of the tile cut: each element of the region file goes straight into the
//! files of its tiles, once, in the order of the file. Read 1 (parts 1 to 4a of `cut`) gave the plan: the books of the
//! nodes and the ways, the ways in more tiles, the extra nodes of each tile (sorted rows), the extra ways and the tiles
//! of the relations. No part files, no part 5.

use super::*;

/// Each element of a block to its tiles: its own tiles (`own`) and its extra tiles (the rows of the block), the tiles
/// with a file only.
struct Route<'a> {
    own: &'a (dyn Fn(i64, &mut Vec<u32>) + Sync),
    extra: &'a [(i64, u32)],
    with_file: &'a [bool],
    /// the tiles of this job (`lo..hi`): a block with many extra rows is routed in more jobs, one for each range of tiles
    range: (u32, u32),
    raw: bool,
    tiles: Vec<u32>,
    /// the number of tiles of the element in all jobs (the packing of an element does not depend on the jobs)
    all_tiles: usize,
    out: Vec<(u32, Piece)>,
    all: All,
}

/// An element for a tile: the element, or (an element of many tiles) its block, packed one time for all of them.
enum Piece {
    Elem(Elem),
    Packed(std::sync::Arc<(Vec<u8>, CutCounts)>),
}

/// An element for more tiles than this is packed one time, and its block goes to each tile: a copy for each tile held
/// a boundary of a country hundreds of times (France: 2 GB at the relations).
const SHARED: usize = 8;

impl Route<'_> {
    /// The tiles of `id`; true when it has one.
    fn tiles_of(&mut self, id: i64) -> bool {
        self.tiles.clear();
        (self.own)(id, &mut self.tiles);
        self.tiles.extend(tiles_in(self.extra, id));
        self.tiles.retain(|&t| self.with_file[t as usize]);
        self.tiles.sort_unstable();
        self.tiles.dedup();
        self.all_tiles = self.tiles.len();
        let (lo, hi) = self.range;
        self.tiles.retain(|&t| lo <= t && t < hi);
        !self.tiles.is_empty()
    }
    /// The element to each of its tiles: a copy for each tile but the last, which gets the element itself (most elements
    /// have one tile: no copy).
    fn take(&mut self) {
        for e in std::mem::take(&mut self.all.0) {
            if self.all_tiles > SHARED {
                let mut w = PbfWriter::data(Vec::new()).raw_blocks(self.raw);
                let (mut n, mut last) = (CutCounts::default(), None);
                // the writer of a block takes any element: no error is possible here, but keep the element when it fails
                let packed = put(&mut w, &mut n, &mut last, e.clone()).and_then(|_| w.finish());
                if let Ok(bytes) = packed {
                    let shared = std::sync::Arc::new((bytes, n));
                    for &t in &self.tiles {
                        self.out.push((t, Piece::Packed(shared.clone())));
                    }
                    continue;
                }
            }
            let (last, others) = self.tiles.split_last().expect("a tile");
            for &t in others {
                self.out.push((t, Piece::Elem(e.clone())));
            }
            self.out.push((*last, Piece::Elem(e)));
        }
    }
}

impl RawSink for Route<'_> {
    fn node<'a>(&mut self, id: i64, lat: f64, lon: f64, tags: &mut dyn Iterator<Item = (&'a str, &'a str)>) {
        if self.tiles_of(id) {
            self.all.node(id, lat, lon, tags);
            self.take();
        }
    }
    fn way<'a>(&mut self, id: i64, tags: &mut dyn Iterator<Item = (&'a str, &'a str)>, refs: &mut dyn Iterator<Item = i64>) {
        if self.tiles_of(id) {
            self.all.way(id, tags, refs);
            self.take();
        }
    }
    fn relation<'a>(
        &mut self,
        id: i64,
        tags: &mut dyn Iterator<Item = (&'a str, &'a str)>,
        members: &mut dyn Iterator<Item = (i64, MemberType, &'a str)>,
    ) {
        if self.tiles_of(id) {
            self.all.relation(id, tags, members);
            self.take();
        }
    }
}

/// The tile files so far: the header of each file at its first bytes, and the numbers of elements of each tile. The bytes
/// go to `out` on this thread (in the browser the storage belongs to it).
pub(super) struct Writers<'o> {
    started: Vec<bool>,
    counts: Vec<CutCounts>,
    head: Vec<u8>,
    raw: bool,
    /// the bytes of a tile file to its place; true for the first bytes of the file (its header)
    out: Box<dyn FnMut(u32, &[u8], bool) -> Result<(), String> + 'o>,
}

impl<'o> Writers<'o> {
    pub(super) fn new(n: usize, timestamp: Option<i64>, raw: bool, out: Box<dyn FnMut(u32, &[u8], bool) -> Result<(), String> + 'o>) -> Result<Self, String> {
        let head = PbfWriter::new(Vec::new(), timestamp)?.finish()?;
        Ok(Writers { started: vec![false; n], counts: vec![CutCounts::default(); n], head, raw, out })
    }

    /// The numbers of elements of each tile; the writers go.
    pub(super) fn into_counts(self) -> Vec<CutCounts> {
        self.counts
    }

    /// The packed elements of one block, by tile, to the tile files.
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
    let writers_raw = writers.raw;
    // one job for each thread in a batch (the pipeline holds three batches), and at most `most` extra rows in a batch
    let n = rayon::current_num_threads();
    let most = (crate::store::memory() / 8 / ROW_BYTES).max(1);
    let per_job = (most / n).max(1);
    let (mut i, mut pos) = (0, None);
    let mut waiting = std::collections::VecDeque::new();
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
            Ok((!batch.is_empty()).then_some(batch))
        },
        |batch: &Vec<Job>| {
            batch
                .par_iter()
                .map_init(Buffers::default, |buf, job| {
                    let mut s = Route { own, extra: &job.rows, with_file, range: job.range, raw: writers_raw, tiles: Vec::new(), all_tiles: 0, out: Vec::new(), all: All::default() };
                    decode_block(&job.block, kinds, buf, &mut s)?;
                    // the elements of each tile packed at once (a stable sort keeps the order of the file in each tile):
                    // no element waits across blocks (a buffer for each tile took 5.4 GB and most of the time on France)
                    s.out.sort_by_key(|x| x.0);
                    let mut made: Vec<(u32, Vec<u8>, CutCounts)> = Vec::new();
                    let mut rest = s.out.into_iter().peekable();
                    while let Some(&(t, _)) = rest.peek() {
                        let mut w = PbfWriter::data(Vec::new()).raw_blocks(writers_raw);
                        let (mut n, mut last) = (CutCounts::default(), None);
                        while let Some((_, p)) = rest.next_if(|x| x.0 == t) {
                            match p {
                                Piece::Elem(e) => put(&mut w, &mut n, &mut last, e)?,
                                // a packed block after the elements before it (`blocks` writes them first)
                                Piece::Packed(shared) => {
                                    w.blocks(&shared.0)?;
                                    n.nodes += shared.1.nodes;
                                    n.ways += shared.1.ways;
                                    n.relations += shared.1.relations;
                                }
                            }
                        }
                        made.push((t, w.finish()?, n));
                    }
                    Ok(made)
                })
                .collect::<Result<Vec<_>, String>>()
        },
        |made, _| {
            for block in made {
                writers.put_bytes(block)?;
            }
            Ok(true)
        },
    )
}

/// Where the bytes of each tile file go: the file `z_x_y.osm.pbf` in `dir` (the build by tiles), the part "tile" in the
/// parts (a folder, or the store), or the sink (the browser).
pub(super) fn outputs<'o>(tiles: &'o [Tile], dir: Option<&'o Path>, parts: &'o Parts, sink: Option<&'o mut Sink<'_>>) -> Result<Box<dyn FnMut(u32, &[u8], bool) -> Result<(), String> + 'o>, String> {
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
        (Some(dir), _) => Box::new(move |t, b, first| {
            let tile = &tiles[t as usize];
            append(dir.join(format!("{}_{}_{}.osm.pbf", tile.z, tile.x, tile.y)), b, first)
        }),
        (None, Files::Dir(d)) => Box::new(move |t, b, first| append(Parts::path(d, t, "tile"), b, first)),
        (None, Files::Store(c)) => Box::new(move |t, b, _| c.append((t, "tile"), b).map_err(|x| format!("a tile file of the tile cut: {x}"))),
    })
}
