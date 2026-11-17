//! The tile cut: one read of the region file gives one area file for each tile ([`crate::tiles`]). Each tile
//! file has the elements that [`crate::extract::cut_where`] gives for the tile: the nodes inside the boundary and in the
//! tile, the ways with such a node, whole, the relations with a member there and their parent relations, and the members
//! of those multipolygons.
//!
//! A node has its place, so its tile is known when it is read. A way and a relation have only the ids of their members,
//! and they come after the nodes in the file: the cut keeps the tile of each node in memory, as runs of ids, compressed
//! ([`Book`]: nodes with near ids are usually made together, in one place). The ways get a book too. The cut reads the
//! file two times: read 1 only plans, read 2 writes each element straight into the
//! files of its tiles, once.
//!
//! Read 1:
//! 1. The nodes: the book of the nodes (the tile of each node inside), and the tiles with a node inside (only these have
//!    a file).
//! 2. The ways and relations: the book of the ways (the tiles of each way with a node inside); the nodes that a tile
//!    needs from outside it go to a temporary list; the relations go whole to a temporary file.
//! 3. The relations: their tiles (a member there, their parents); the members of the multipolygons go to the lists of
//!    the extra nodes and the extra ways.
//! 4. The extra nodes, sorted; the junctions files of the build by tiles.
//!
//! Read 2 (`write`): the nodes, the ways and the relations, in the order of the file, each to its own tiles and to the
//! tiles that have it as an extra element.

use crate::areas::AreaList;
use crate::boundary::Boundary;
use crate::write::{file_blob_as, nodes_block, relations_block, ways_block, PbfWriter, BLOCK};
use crate::extract::{each_block, each_block_at, header, widen, CutCounts, Span, EMPTY};
use crate::elements::{MemberType, Relation};
use crate::decode::{decode_block, read_block, Buffers};
use crate::elements::{RawNode, RawSink, RawWay};
use crate::store::{Codec, Writer};
use crate::tiles::Tile;
use crate::cells::{cell_of, near_cell, CellIndex, Resolution};
use crate::util::FxHashMap;
use rayon::prelude::*;
use std::collections::BTreeSet;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

mod write;

/// Ids with a tile, as runs (first id, last id, tile): ids that follow each other in the file, with no element of the
/// kind between them that is not in the tile. Ids in no run have no tile.
#[derive(Default)]
struct Runs {
    runs: Vec<(i64, i64, u32)>,
    /// the last element given to `push` was in the last run
    open: bool,
}

impl Runs {
    fn push(&mut self, id: i64, tile: Option<u32>) {
        match (tile, self.runs.last_mut()) {
            (Some(t), Some(r)) if self.open && r.2 == t => r.1 = id,
            (Some(t), _) => self.runs.push((id, id, t)),
            (None, _) => {}
        }
        self.open = tile.is_some();
    }

    #[cfg(test)]
    fn get(&self, id: i64) -> Option<u32> {
        let i = self.runs.partition_point(|r| r.0 <= id);
        let r = self.runs.get(i.checked_sub(1)?)?;
        (id <= r.1).then_some(r.2)
    }

    #[cfg(test)]
    /// Is an id of a run in `lo..=hi`?
    fn any_in(&self, lo: i64, hi: i64) -> bool {
        let i = self.runs.partition_point(|r| r.1 < lo);
        i < self.runs.len() && self.runs[i].0 <= hi
    }
}

/// The runs of a whole file, compressed (a phone book): pages of `PAGE` runs; in a page each run is three varints (the
/// gap from the end of the run before, the length, the tile). Only the first id and the place of each page are full
/// numbers. A look-up searches the pages, then reads at most `PAGE` runs. The last run stays open, so that the next
/// block can make it longer.
#[derive(Default)]
struct Book {
    pages: Vec<(i64, usize)>,
    bytes: Vec<u8>,
    in_page: usize,
    end: i64,
    last: Option<(i64, i64, u32)>,
    open: bool,
    runs: usize,
}


const PAGE: usize = 32;

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn get_varint(b: &[u8], at: &mut usize) -> u64 {
    let (mut v, mut shift) = (0u64, 0);
    loop {
        let x = b[*at];
        *at += 1;
        v |= ((x & 0x7f) as u64) << shift;
        if x < 0x80 {
            return v;
        }
        shift += 7;
    }
}

impl Book {
    fn pack(&mut self, (first, last, tile): (i64, i64, u32)) {
        if self.in_page == PAGE || self.pages.is_empty() {
            self.pages.push((first, self.bytes.len()));
            self.in_page = 0;
            self.end = first;
        }
        put_varint(&mut self.bytes, first.abs_diff(self.end));
        put_varint(&mut self.bytes, last.abs_diff(first));
        put_varint(&mut self.bytes, tile as u64);
        self.end = last;
        self.in_page += 1;
        self.runs += 1;
    }

    /// Add the runs of the next block (made by `push` from an empty `Runs`; `first_open`: its first element had a tile).
    fn append(&mut self, next: Runs, first_open: bool) {
        let mut it = next.runs.into_iter();
        if let (Some(r), true) = (self.last.as_mut(), self.open && first_open) {
            if let Some(n) = it.next() {
                if n.2 == r.2 {
                    r.1 = n.1;
                } else if let Some(old) = self.last.replace(n) {
                    self.pack(old);
                }
            }
        }
        for n in it {
            if let Some(old) = self.last.replace(n) {
                self.pack(old);
            }
        }
        self.open = next.open;
    }

    fn finish(&mut self) {
        if let Some(old) = self.last.take() {
            self.pack(old);
        }
        self.bytes.shrink_to_fit();
        self.pages.shrink_to_fit();
    }

    /// The bytes of page `i`.
    fn page_bytes<T>(&self, i: usize, f: impl FnOnce(&[u8]) -> T) -> T {
        let end = self.pages.get(i + 1).map_or(self.bytes.len(), |p| p.1);
        f(&self.bytes[self.pages[i].1..end])
    }

    /// Scan the runs from the page that can hold `id`: `f(first, last, tile)` gives Some(answer) to stop, or None to go on.
    fn scan<T>(&self, id: i64, mut f: impl FnMut(i64, i64, u32) -> Option<T>) -> Option<T> {
        let mut i = self.pages.partition_point(|p| p.0 <= id).saturating_sub(1);
        while i < self.pages.len() {
            let found = self.page_bytes(i, |b| {
                let (mut at, mut end) = (0, self.pages[i].0);
                while at < b.len() {
                    let first = end + get_varint(b, &mut at) as i64;
                    let last = first + get_varint(b, &mut at) as i64;
                    let tile = get_varint(b, &mut at) as u32;
                    end = last;
                    if let Some(x) = f(first, last, tile) {
                        return Some(Some(x));
                    }
                }
                None
            });
            match found {
                Some(x) => return x,
                None => i += 1,
            }
        }
        None
    }

    fn get(&self, id: i64) -> Option<u32> {
        self.scan(id, |first, last, tile| {
            if first > id {
                Some(None)
            } else if id <= last {
                Some(Some(tile))
            } else {
                None
            }
        })
        .flatten()
    }

    #[cfg(test)]
    /// Is an id of a run in `lo..=hi`?
    fn any_in(&self, lo: i64, hi: i64) -> bool {
        self.scan(lo, |first, last, _| {
            if first > hi {
                Some(false)
            } else if last >= lo {
                Some(true)
            } else {
                None
            }
        })
        .unwrap_or(false)
    }

    fn len(&self) -> usize {
        self.runs
    }

    fn size(&self) -> usize {
        self.bytes.len() + self.pages.len() * 16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cut into the store (the browser's way: the tile files made in memory, a few tiles at a time) gives the same
    /// tile files as the cut into a folder.
    #[test]
    fn the_cut_into_the_store_equals_the_cut_into_a_folder() {
        let data = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
        let pbf = data.join("monaco.osm.pbf");
        let b = Boundary::parse(&std::fs::read_to_string(data.join("monaco.geojson")).unwrap()).unwrap();
        let tiles = crate::tiles::tiles(&b, 15);
        let open = || std::fs::File::open(&pbf).map_err(|e| e.to_string());
        let dir = std::env::temp_dir().join(format!("pbfcut-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        cut_to_tiles(open, &b, &tiles, &dir, &["multipolygon"], 25.0, None, true).unwrap();
        let store = cut_to_store_build(open, &b, &tiles, true, 25.0, None, true).unwrap();
        let mut n = 0;
        for (i, (t, _)) in store.tiles.iter().enumerate() {
            let mut got = Vec::new();
            store.reader(i).read_to_end(&mut got).unwrap();
            let want = std::fs::read(dir.join(format!("{}_{}_{}.osm.pbf", t.z, t.x, t.y))).unwrap_or_default();
            assert!(got == want, "tile {t:?}: {} bytes in the store, {} in the folder", got.len(), want.len());
            n += usize::from(!got.is_empty());
        }
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(n > 4, "the cut gave {n} tile files");
    }

    /// The cut into a sink (the online page) gives the same tile files as the cut into a folder, also with a border.
    #[test]
    fn the_cut_into_a_sink_equals_the_cut_into_a_folder() {
        let data = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
        let pbf = data.join("monaco.osm.pbf");
        let b = Boundary::parse(&std::fs::read_to_string(data.join("monaco.geojson")).unwrap()).unwrap();
        let tiles = crate::tiles::tiles(&b, 15);
        let open = || std::fs::File::open(&pbf).map_err(|e| e.to_string());
        let complete = ["multipolygon", "boundary"];
        let dir = std::env::temp_dir().join(format!("pbfcut-sink-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        cut_to_tiles(open, &b, &tiles, &dir, &complete, 25.0, None, true).unwrap();
        let mut got = vec![Vec::new(); tiles.len()];
        cut_to_sink(open, &b, &tiles, &complete, 25.0, true, &mut |t, bytes| {
            got[t as usize].extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();
        for (t, got) in tiles.iter().zip(&got) {
            let want = std::fs::read(dir.join(format!("{}_{}_{}.osm.pbf", t.z, t.x, t.y))).unwrap_or_default();
            assert!(*got == want, "tile {t:?}: {} bytes in the sink, {} in the folder", got.len(), want.len());
        }
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(got.iter().filter(|g| !g.is_empty()).count() > 4);
    }

    /// A point on the edge between two areas goes to the first one, with no error; a point inside two areas is an overlap.
    #[test]
    fn a_shared_edge_is_not_an_overlap() {
        let list = |boxes: &[&str]| {
            let areas = boxes.iter().enumerate().map(|(i, b)| crate::areas::Area { name: format!("a{i}"), boundary: Boundary::parse(b).unwrap() });
            AreaList::new(areas.collect(), None).unwrap()
        };
        let edge = list(&["0,0,1,1", "1,0,2,1"]);
        let g = Grid::areas(&edge);
        assert_eq!((g.of(1.0, 0.5), g.of(0.5, 0.5), g.of(1.5, 0.5)), (Some(0), Some(0), Some(1)));
        assert!(g.overlap().is_ok());
        let over = list(&["0,0,1,1", "0.5,0,2,1"]);
        let g = Grid::areas(&over);
        assert_eq!(g.of(0.75, 0.5), Some(0));
        assert!(g.overlap().unwrap_err().contains("\"a0\" and \"a1\""));
    }

    /// The cut of an area list into a sink gives the bytes of the area files of [`cut_to_areas`].
    #[test]
    fn the_areas_into_a_sink_equal_the_area_files() {
        let data = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
        let pbf = data.join("monaco.osm.pbf");
        let text = std::fs::read_to_string(data.join("monaco-areas.json")).unwrap();
        let list = AreaList::parse(&text, "name", &|f| std::fs::read_to_string(data.join(f)).map_err(|e| e.to_string())).unwrap();
        let world = Boundary::parse(crate::boundary::WORLD).unwrap();
        let open = || std::fs::File::open(&pbf).map_err(|e| e.to_string());
        let complete = ["multipolygon", "boundary"];
        let dir = std::env::temp_dir().join(format!("pbfcut-areas-sink-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        cut_to_areas(open, &world, &list, &dir, &complete, true).unwrap();
        let mut got = vec![Vec::new(); list.areas.len()];
        cut_areas_to_sink(open, &world, &list, &complete, true, &mut |a, bytes| {
            got[a as usize].extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();
        for (a, got) in list.areas.iter().zip(&got) {
            let want = std::fs::read(dir.join(format!("{}.osm.pbf", a.name))).unwrap();
            assert!(*got == want, "area {}: {} bytes in the sink, {} in the folder", a.name, got.len(), want.len());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_cut_of_cells_into_a_sink_equals_the_cut_into_a_folder() {
        let data = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
        let pbf = data.join("monaco.osm.pbf");
        let b = Boundary::parse(&std::fs::read_to_string(data.join("monaco.geojson")).unwrap()).unwrap();
        let cells = crate::cells::cells(&b, Resolution::Nine);
        let open = || std::fs::File::open(&pbf).map_err(|e| e.to_string());
        let complete = ["multipolygon", "boundary"];
        let dir = std::env::temp_dir().join(format!("pbfcut-cell-sink-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        cut_to_cells(open, &b, &cells, &dir, &complete, 25.0, true).unwrap();
        let mut got = vec![Vec::new(); cells.len()];
        cut_cells_to_sink(open, &b, &cells, &complete, 25.0, true, &mut |t, bytes| {
            got[t as usize].extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();
        for (c, got) in cells.iter().zip(&got) {
            let want = std::fs::read(dir.join(format!("{c}.osm.pbf"))).unwrap_or_default();
            assert!(*got == want, "cell {c}: {} bytes in the sink, {} in the folder", got.len(), want.len());
        }
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(got.iter().filter(|g| !g.is_empty()).count() > 20);
    }

    /// With a [`NodeUses`], each tile file has a junctions file: rows sorted by node, with the flags of `of_node` and
    /// [`HOME`], for nodes outside the tile only.
    #[test]
    fn the_junctions_files_have_the_flags_of_node_uses() {
        /// A node used by two ways or more is a junction (flag 1).
        struct Shared;
        impl NodeUses for Shared {
            fn of_way(&self, _: &[(String, String)], refs: &[i64], out: &mut Vec<(i64, u8)>) {
                out.extend(refs.iter().map(|&n| (n, 1)));
            }
            fn of_node(&self, uses: &mut dyn Iterator<Item = u8>) -> u8 {
                u8::from(uses.count() >= 2)
            }
        }
        let data = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
        let pbf = data.join("monaco.osm.pbf");
        let b = Boundary::parse(&std::fs::read_to_string(data.join("monaco.geojson")).unwrap()).unwrap();
        let tiles = crate::tiles::tiles(&b, 15);
        let open = || std::fs::File::open(&pbf).map_err(|e| e.to_string());
        let dir = std::env::temp_dir().join(format!("pbfcut-junctions-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        cut_to_tiles(open, &b, &tiles, &dir, &[], 0.0, Some(&Shared), true).unwrap();
        let (mut files, mut rows, mut junctions) = (0, 0, 0);
        for t in &tiles {
            if !dir.join(format!("{}_{}_{}.osm.pbf", t.z, t.x, t.y)).exists() {
                continue;
            }
            let p = dir.join(format!("{}_{}_{}.junctions", t.z, t.x, t.y));
            let r = read_junctions(&p).unwrap();
            files += 1;
            rows += r.len();
            junctions += r.iter().filter(|x| x.1 & 1 != 0).count();
            assert!(r.windows(2).all(|w| w[0].0 < w[1].0), "{p:?}: not sorted");
            assert!(r.iter().all(|x| x.1 != 0 && x.1 & !(1 | HOME) == 0), "{p:?}: other flags");
        }
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(files > 4 && junctions > 0 && rows >= junctions, "{files} files, {rows} rows, {junctions} junctions");
    }

    /// The book gives the tile of each id as the list of runs does, also across pages and blocks.
    #[test]
    fn the_book_finds_the_runs() {
        let mut want = Runs::default();
        let mut book = Book::default();
        let mut x: u64 = 7;
        let mut id = 1_000i64;
        for block in 0..20 {
            let mut b = BlockRuns::new();
            for _ in 0..200 {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                id += 1 + (x >> 60) as i64;
                let tile = match (x >> 33) % 5 {
                    0 => None,
                    k => Some((k as u32 + block) % 3),
                };
                b.push(id, tile);
                want.push(id, tile);
            }
            book.append(b.runs, b.first.unwrap_or(false));
        }
        book.finish();
        for probe in 990..=id + 5 {
            assert_eq!(book.get(probe), want.get(probe), "id {probe}");
            assert_eq!(book.any_in(probe, probe + 3), want.any_in(probe, probe + 3), "span {probe}");
        }
        assert!(book.len() <= want.runs.len());
    }
}

/// Runs made one block at a time, on many threads: the runs of the block, and whether its first element had a tile.
struct BlockRuns {
    runs: Runs,
    first: Option<bool>,
}

impl BlockRuns {
    fn new() -> BlockRuns {
        BlockRuns { runs: Runs::default(), first: None }
    }
    fn push(&mut self, id: i64, tile: Option<u32>) {
        self.first.get_or_insert(tile.is_some());
        self.runs.push(id, tile);
    }
}

/// (id, tile) pairs, sorted, with no doubles.
fn tidy(v: &mut Vec<(i64, u32)>) {
    v.sort_unstable();
    v.dedup();
    v.shrink_to_fit();
}

/// The tiles of `id` in the sorted pairs.
fn tiles_in(pairs: &[(i64, u32)], id: i64) -> impl Iterator<Item = u32> + '_ {
    let i = pairs.partition_point(|p| p.0 < id);
    pairs[i..].iter().take_while(move |p| p.0 == id).map(|p| p.1)
}

/// The ways of a block with their nodes.
#[derive(Default)]
struct WayRefs<'w> {
    /// only these ways (sorted), when the list fits the memory budget; else all ways of the block
    wanted: Option<&'w [i64]>,
    found: Vec<(i64, Vec<i64>)>,
}

impl RawSink for WayRefs<'_> {
    fn node<'a>(&mut self, _: i64, _: f64, _: f64, _: &mut dyn Iterator<Item = (&'a str, &'a str)>) {}
    fn way<'a>(&mut self, id: i64, _: &mut dyn Iterator<Item = (&'a str, &'a str)>, refs: &mut dyn Iterator<Item = i64>) {
        if self.wanted.is_none_or(|w| w.binary_search(&id).is_ok()) {
            self.found.push((id, refs.collect()));
        }
    }
    fn relation<'a>(
        &mut self,
        _: i64,
        _: &mut dyn Iterator<Item = (&'a str, &'a str)>,
        _: &mut dyn Iterator<Item = (i64, MemberType, &'a str)>,
    ) {
    }
}

/// An element of the region file, whole.
#[derive(Clone)]
enum Elem {
    Node(RawNode),
    Way(RawWay),
    Relation(Relation),
}

impl Elem {
    fn id(&self) -> i64 {
        match self {
            Elem::Node(n) => n.id,
            Elem::Way(w) => w.id,
            Elem::Relation(r) => r.id,
        }
    }
}

/// The blobs of elements of one kind (sorted by id), BLOCK elements in each, as in a `.osm.pbf` file, each after a record
/// (first id, last id, number of elements: 20 bytes). The elements move into the blocks (no copy).
fn blobs(elems: Vec<Elem>, out: &mut Vec<u8>, raw: bool) -> Result<(), String> {
    let mut it = elems.into_iter().peekable();
    while it.peek().is_some() {
        let chunk: Vec<Elem> = it.by_ref().take(BLOCK).collect();
        // the record of the blob: its first and last id and its number of elements, so that a merge can copy it unread
        out.extend_from_slice(&chunk[0].id().to_le_bytes());
        out.extend_from_slice(&chunk[chunk.len() - 1].id().to_le_bytes());
        out.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
        let block = match &chunk[0] {
            Elem::Node(_) => nodes_block(&chunk.into_iter().filter_map(|e| if let Elem::Node(n) = e { Some(n) } else { None }).collect::<Vec<_>>()),
            Elem::Way(_) => ways_block(&chunk.into_iter().filter_map(|e| if let Elem::Way(w) = e { Some(w) } else { None }).collect::<Vec<_>>()),
            Elem::Relation(_) => relations_block(
                &chunk.into_iter().filter_map(|e| if let Elem::Relation(r) = e { Some(r) } else { None }).collect::<Vec<_>>(),
            ),
        };
        file_blob_as(out, "OSMData", &block, raw)?;
    }
    Ok(())
}

/// The part files of the tiles. The blobs of a part wait in memory until a limit, then go to the end of their files.
struct Parts {
    files: Files,
    /// the blobs that wait, and their bytes (a lock: a pass reads a part while it adds to others)
    waiting: std::sync::Mutex<(FxHashMap<(u32, &'static str), Vec<u8>>, usize)>,
}

/// Where the part files are: files in a temporary folder (the build by tiles: a file is open only while it is written,
/// because the tiles are more than a process can keep open), or chunks of one temporary file of the store
/// ([`cut_to_store`]: the browser has no folders, only the store in OPFS).
enum Files {
    Dir(PathBuf),
    // the browser only
    Store(Chunks),
}

/// The bytes of blobs that wait in memory before they go to the part files.
const WAITING: usize = 64 << 20;

/// The tile of the part with all relations (pass 2).
const ALL: u32 = u32::MAX;

impl Parts {
    fn path(dir: &Path, tile: u32, part: &str) -> PathBuf {
        dir.join(format!("{tile}.{part}"))
    }
    fn add(&self, tile: u32, part: &'static str, mut blobs: Vec<u8>) -> Result<(), String> {
        let full = {
            let mut w = self.waiting.lock().expect("the parts");
            w.1 += blobs.len();
            w.0.entry((tile, part)).or_default().append(&mut blobs);
            w.1 > WAITING
        };
        if full {
            self.flush()?;
        }
        Ok(())
    }
    fn flush(&self) -> Result<(), String> {
        let e = |e: io::Error| format!("a part file of the tile cut: {e}");
        let waiting = std::mem::take(&mut *self.waiting.lock().expect("the parts")).0;
        for ((tile, part), bytes) in waiting {
            match &self.files {
                Files::Dir(dir) => std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(Self::path(dir, tile, part))
                    .and_then(|mut f| f.write_all(&bytes))
                    .map_err(e)?,
                Files::Store(c) => c.append((tile, part), &bytes).map_err(e)?,
            }
        }
        Ok(())
    }
    /// The bytes of a part (after [`Parts::flush`]), or None when it has none.
    fn reader(&self, tile: u32, part: &'static str) -> Option<Box<dyn Read + Send + '_>> {
        match &self.files {
            Files::Dir(dir) => std::fs::File::open(Self::path(dir, tile, part)).ok().map(|f| Box::new(f) as Box<dyn Read + Send>),
            Files::Store(c) => c.reader((tile, part)).map(|r| Box::new(r) as Box<dyn Read + Send>),
        }
    }
    fn remove(&self, tile: u32, part: &'static str) {
        match &self.files {
            Files::Dir(dir) => {
                let _ = std::fs::remove_file(Self::path(dir, tile, part));
            }
            Files::Store(c) => c.remove((tile, part)),
        }
    }
}

/// Files as chunks of one temporary file of the store: each file is the list of its chunks (place, length). A removed
/// file keeps its bytes in the temporary file until the end.
pub struct Chunks {
    /// the temporary file and its end
    file: std::sync::Mutex<(Box<dyn crate::store::Blob>, u64)>,
    /// each file: its chunks (place in the file, length)
    index: std::sync::Mutex<FxHashMap<(u32, &'static str), Vec<(u64, u64)>>>,
    /// the places in the file of removed chunks (place, length), used again before the file grows: the parts of a tile
    /// go when its tile file is made, and the tile files take their place (France: the file had all parts and all tile
    /// files, more than 10 GB, and the 5 GiB of a fresh browser profile were full in part 1 of a second cut)
    free: std::sync::Mutex<Vec<(u64, u64)>>,
}

/// A removed chunk shorter than this is not used again: a file in many small pieces needs many reads.
const SMALLEST_FREE: u64 = 64 << 10;

impl Chunks {
    fn new(store: &dyn crate::store::Store) -> io::Result<Chunks> {
        Ok(Chunks { file: std::sync::Mutex::new((store.create()?, 0)), index: Default::default(), free: Default::default() })
    }
    fn append(&self, key: (u32, &'static str), bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let mut f = self.file.lock().expect("the file");
        // first the places of removed chunks, then the end of the file
        let mut rest = bytes;
        let mut placed = Vec::new();
        while !rest.is_empty() {
            let Some((pos, len)) = self.free.lock().expect("the free places").pop() else { break };
            let n = len.min(rest.len() as u64);
            f.0.seek(SeekFrom::Start(pos))?;
            f.0.write_all(&rest[..n as usize])?;
            placed.push((pos, n));
            if len - n >= SMALLEST_FREE {
                self.free.lock().expect("the free places").push((pos + n, len - n));
            }
            rest = &rest[n as usize..];
        }
        if !rest.is_empty() {
            let at = f.1;
            f.0.seek(SeekFrom::Start(at))?;
            f.0.write_all(rest)?;
            f.1 += rest.len() as u64;
            placed.push((at, rest.len() as u64));
        }
        self.index.lock().expect("the index").entry(key).or_default().extend(placed);
        Ok(())
    }
    fn reader(&self, key: (u32, &'static str)) -> Option<ChunkReader<'_>> {
        let chunks = self.index.lock().expect("the index").get(&key)?.clone();
        Some(ChunkReader { c: self, chunks, i: 0, off: 0 })
    }
    /// A file goes: its places in the file are used again (`free`).
    fn remove(&self, key: (u32, &'static str)) {
        if let Some(chunks) = self.index.lock().expect("the index").remove(&key) {
            self.free.lock().expect("the free places").extend(chunks.into_iter().filter(|c| c.1 >= SMALLEST_FREE));
        }
    }
}

/// The bytes of a file of [`Chunks`], in order.
pub struct ChunkReader<'a> {
    c: &'a Chunks,
    chunks: Vec<(u64, u64)>,
    i: usize,
    off: u64,
}

impl Read for ChunkReader<'_> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        while self.chunks.get(self.i).is_some_and(|c| self.off == c.1) {
            self.i += 1;
            self.off = 0;
        }
        let Some(&(at, len)) = self.chunks.get(self.i) else { return Ok(0) };
        let n = (len - self.off).min(b.len() as u64) as usize;
        let mut f = self.c.file.lock().expect("the file");
        f.0.seek(SeekFrom::Start(at + self.off))?;
        f.0.read_exact(&mut b[..n])?;
        self.off += n as u64;
        Ok(n)
    }
}

/// The blobs of a part file, with their records: (first id, last id, number of elements, the blob as in a `.osm.pbf`).
struct Blobs<'a>(Option<io::BufReader<Box<dyn Read + Send + 'a>>>);

type Blob = (i64, i64, u32, Vec<u8>);

/// The bytes of a part, or none.
type Src<'a> = Option<Box<dyn Read + Send + 'a>>;

/// Where [`cut_to_sink`] gives the bytes of the tile files: (tile, the next bytes of its file).
pub type Sink<'a> = &'a mut dyn FnMut(u32, &[u8]) -> Result<(), String>;

impl<'a> Blobs<'a> {
    fn from(src: Src<'a>) -> Blobs<'a> {
        Blobs(src.map(|f| io::BufReader::with_capacity(1 << 20, f)))
    }
    fn next(&mut self) -> Result<Option<Blob>, String> {
        let Some(r) = self.0.as_mut() else { return Ok(None) };
        let mut head = [0u8; 20];
        match r.read_exact(&mut head) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.to_string()),
        }
        let i = |at: usize| i64::from_le_bytes(head[at..at + 8].try_into().expect("8 bytes"));
        let n = u32::from_le_bytes(head[16..20].try_into().expect("4 bytes"));
        let blob = read_block(r)?.ok_or("a part file ends in a record")?;
        Ok(Some((i(0), i(8), n, blob)))
    }
}

/// The relations of a block with only what part 3 needs: the id, whether its type is in `complete`, and the members (id
/// and type). No strings: the roles and the tags as strings (France: 12 million, in three passes on all threads) made
/// part 3 in the browser 9 times slower on Windows (one allocator for all threads), with 500 MiB more memory each pass.
struct RelMembers<'c> {
    complete: &'c [&'c str],
    /// id, type in `complete`, the end of its members in `members`
    rels: Vec<(i64, bool, usize)>,
    members: Vec<(i64, MemberType)>,
}

impl RelMembers<'_> {
    fn iter(&self) -> impl Iterator<Item = (i64, bool, &[(i64, MemberType)])> + '_ {
        let mut start = 0;
        self.rels.iter().map(move |&(id, complete, end)| {
            let m = &self.members[start..end];
            start = end;
            (id, complete, m)
        })
    }
}

impl RawSink for RelMembers<'_> {
    fn node<'a>(&mut self, _: i64, _: f64, _: f64, _: &mut dyn Iterator<Item = (&'a str, &'a str)>) {}
    fn way<'a>(&mut self, _: i64, _: &mut dyn Iterator<Item = (&'a str, &'a str)>, _: &mut dyn Iterator<Item = i64>) {}
    fn relation<'a>(
        &mut self,
        id: i64,
        tags: &mut dyn Iterator<Item = (&'a str, &'a str)>,
        members: &mut dyn Iterator<Item = (i64, MemberType, &'a str)>,
    ) {
        let mut complete = false;
        for (k, v) in tags {
            complete |= k == "type" && self.complete.contains(&v);
        }
        self.members.extend(members.map(|(m, t, _)| (m, t)));
        self.rels.push((id, complete, self.members.len()));
    }
}

/// The relations of a part file in batches, in the order of the file: one block for each thread, and at most 32 MB of
/// input (a block of relations holds thousands of member lists). The pool decodes a batch; `f` gets its blocks.
fn rel_batches(src: Src<'_>, complete: &[&str], f: &mut dyn FnMut(&[RelMembers]) -> Result<(), String>) -> Result<(), String> {
    let mut blobs = Blobs::from(src);
    loop {
        let (mut batch, mut bytes) = (Vec::new(), 0usize);
        while batch.len() < rayon::current_num_threads() && bytes < 32 << 20 {
            match blobs.next()? {
                None => break,
                Some((_, _, _, blob)) => {
                    bytes += blob.len();
                    batch.push(blob);
                }
            }
        }
        if batch.is_empty() {
            return Ok(());
        }
        let decoded: Vec<RelMembers> = batch
            .par_iter()
            .map_init(Buffers::default, |b, blob| {
                let mut s = RelMembers { complete, rels: Vec::new(), members: Vec::new() };
                decode_block(blob, [false, false, true], b, &mut s)?;
                Ok(s)
            })
            .collect::<Result<_, String>>()?;
        f(&decoded)?;
    }
}

/// An element to the file of a tile, but one with the id of the element before (a node of the tile that a part of the
/// extra elements also has).
fn put<W: Write>(w: &mut PbfWriter<W>, n: &mut CutCounts, last: &mut Option<i64>, e: Elem) -> Result<(), String> {
    if *last == Some(e.id()) {
        return Ok(());
    }
    *last = Some(e.id());
    match e {
        Elem::Node(x) => {
            n.nodes += 1;
            w.node(x)
        }
        Elem::Way(x) => {
            n.ways += 1;
            w.way(x)
        }
        Elem::Relation(x) => {
            n.relations += 1;
            w.relation(x)
        }
    }
}

/// The ids of the elements of a block, in their order, as zigzag varints of the steps between them (the first one whole):
/// about 1.5 bytes for each id. A row of the id file of a kind (`present_ids`).
#[derive(Default)]
struct IdPack {
    last: i64,
    n: usize,
    bytes: Vec<u8>,
}

impl IdPack {
    fn push(&mut self, id: i64) {
        let d = if self.n == 0 { id } else { id.wrapping_sub(self.last) };
        let mut z = ((d << 1) ^ (d >> 63)) as u64;
        while z >= 0x80 {
            self.bytes.push(z as u8 | 0x80);
            z >>= 7;
        }
        self.bytes.push(z as u8);
        (self.last, self.n) = (id, self.n + 1);
    }
}

/// The ids of one row of an id file.
fn unpack_ids(mut b: &[u8], out: &mut Vec<i64>) {
    let mut last = 0i64;
    let mut first = true;
    while !b.is_empty() {
        let (mut z, mut shift) = (0u64, 0);
        loop {
            let c = b[0];
            b = &b[1..];
            z |= ((c & 0x7f) as u64) << shift;
            shift += 7;
            if c < 0x80 {
                break;
            }
        }
        let d = (z >> 1) as i64 ^ -((z & 1) as i64);
        last = if first { d } else { last.wrapping_add(d) };
        first = false;
        out.push(last);
    }
}

/// The bytes of a row as they are (the rows of an id file).
struct RawBytes;
impl Codec<Vec<u8>> for RawBytes {
    fn put(&self, r: &Vec<u8>, out: &mut Vec<u8>) {
        out.extend_from_slice(r);
    }
    fn get(&self, b: &mut &[u8]) -> Vec<u8> {
        let v = b.to_vec();
        *b = &b[b.len()..];
        v
    }
}

/// The ids of `wanted` (sorted) that the id file of a kind has (its rows in the order of the file, so all its ids are
/// sorted): the rows of a batch on all threads. One read of the id file, with no block of the region file read again.
fn present_ids(ids: crate::store::Rows, wanted: &[i64]) -> io::Result<Vec<i64>> {
    let mut rows = ids.read(&RawBytes)?;
    let mut found = Vec::new();
    loop {
        let batch: Vec<Vec<u8>> = rows.by_ref().take(256).collect::<io::Result<_>>()?;
        if batch.is_empty() {
            return Ok(found);
        }
        let parts: Vec<Vec<i64>> = batch
            .par_iter()
            .map(|b| {
                let mut all = Vec::new();
                unpack_ids(b, &mut all);
                all.retain(|id| wanted.binary_search(id).is_ok());
                all
            })
            .collect();
        for p in parts {
            found.extend(p);
        }
    }
}

/// Pass 1: the runs of the nodes inside, the nodes inside with their tiles, and whether the block has ways or relations.
struct Nodes<'a> {
    boundary: &'a Boundary,
    grid: &'a Grid<'a>,
    /// the border of a tile, in metres: a node inside the area this near another tile goes to `near` with that tile
    border_m: f64,
    runs: BlockRuns,
    /// the tiles of the nodes inside (with doubles): read 1 writes no element
    own: Vec<u32>,
    near: Vec<(i64, u32)>,
    other: bool,
    span: Span,
    /// the ids of all nodes of the block (a row of the id file of the nodes)
    ids: IdPack,
}

/// The areas of a cut: map tiles (of one zoom, or of mixed zooms with no tile in another), H3 cells of one resolution,
/// or a list of areas that do not overlap ([`AreaList`]). A point is in at most one area. The number of an area is its
/// place in the list. Only this type knows the kind of area: the rest of the cut uses the numbers.
enum Grid<'a> {
    /// the zooms of the tiles, the finest first
    Tiles { list: &'a [Tile], zooms: Vec<u8>, map: FxHashMap<(u8, u32, u32), u32> },
    Cells { list: &'a [CellIndex], res: Resolution, map: FxHashMap<CellIndex, u32> },
    /// `overlap`: two areas that read 1 found with a node inside the two (the cut then stops)
    Areas { list: &'a AreaList, overlap: std::sync::OnceLock<(u32, u32)> },
}

impl<'a> Grid<'a> {
    fn tiles(list: &'a [Tile]) -> Grid<'a> {
        let mut zooms: Vec<u8> = list.iter().map(|t| t.z).collect();
        zooms.sort_unstable_by(|a, b| b.cmp(a));
        zooms.dedup();
        Grid::Tiles { list, zooms, map: list.iter().enumerate().map(|(i, t)| ((t.z, t.x, t.y), i as u32)).collect() }
    }

    fn cells(list: &'a [CellIndex]) -> Result<Grid<'a>, String> {
        let res = list.first().map_or(Resolution::Zero, |c| c.resolution());
        if list.iter().any(|c| c.resolution() != res) {
            return Err("the cell cut needs cells of one resolution".into());
        }
        Ok(Grid::Cells { list, res, map: list.iter().enumerate().map(|(i, &c)| (c, i as u32)).collect() })
    }

    fn areas(list: &'a AreaList) -> Grid<'a> {
        Grid::Areas { list, overlap: Default::default() }
    }

    /// The error of an area list with two areas that overlap.
    fn overlap(&self) -> Result<(), String> {
        match self {
            Grid::Areas { list, overlap } => match overlap.get() {
                Some(&(a, b)) => Err(format!(
                    "the areas \"{}\" and \"{}\" overlap: the areas of a list must not overlap",
                    list.areas[a as usize].name, list.areas[b as usize].name
                )),
                None => Ok(()),
            },
            _ => Ok(()),
        }
    }

    fn len(&self) -> usize {
        match self {
            Grid::Tiles { list, .. } => list.len(),
            Grid::Cells { list, .. } => list.len(),
            Grid::Areas { list, .. } => list.areas.len(),
        }
    }

    /// Are all areas of one zoom or of one resolution (the cut with a border needs it)?
    fn one_level(&self) -> bool {
        match self {
            Grid::Tiles { zooms, .. } => zooms.len() <= 1,
            Grid::Cells { .. } => true,
            Grid::Areas { .. } => false,
        }
    }

    /// The area that holds the point. A tile: `Tile::of` of the finest zoom, shifted to the zoom of the tile (the same
    /// tile as `Tile::of` of that zoom). An area list: the first area of the list that holds the point. A point on the
    /// border of two areas is in the two, and goes to the first one; a point inside the two is an overlap.
    fn of(&self, lon: f64, lat: f64) -> Option<u32> {
        match self {
            Grid::Tiles { zooms, map, .. } => {
                let fine = Tile::of(*zooms.first()?, lon, lat);
                zooms.iter().find_map(|&z| map.get(&(z, fine.x >> (fine.z - z), fine.y >> (fine.z - z))).copied())
            }
            Grid::Cells { res, map, .. } => map.get(&cell_of(*res, lon, lat)?).copied(),
            Grid::Areas { list, overlap } => {
                let mut it = list.of(lon, lat);
                let first = it.next()?;
                // the test of the other areas of the cell is the check of the overlap (no extra time on Sweden, 10 areas)
                if let Some(second) = it.next() {
                    let border = |a: u32| list.areas[a as usize].boundary.on_border(lon, lat);
                    if !border(first) && !border(second) {
                        let _ = overlap.set((first, second));
                    }
                }
                Some(first)
            }
        }
    }

    /// Each other area (of the same zoom or resolution) within `m` metres of the point, to `f`.
    fn near(&self, lon: f64, lat: f64, m: f64, mut f: impl FnMut(u32)) {
        match self {
            Grid::Tiles { zooms, map, .. } => {
                let me = Tile::of(zooms[0], lon, lat);
                for (dx, dy) in [(-1i64, -1i64), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                    let (x, y) = (me.x as i64 + dx, me.y as i64 + dy);
                    if x < 0 || y < 0 {
                        continue;
                    }
                    let other = Tile { z: me.z, x: x as u32, y: y as u32 };
                    if let Some(&o) = map.get(&(other.z, other.x, other.y)) {
                        if near_tile(other, lon, lat, m) {
                            f(o);
                        }
                    }
                }
            }
            Grid::Cells { res, map, .. } => {
                let Some(me) = cell_of(*res, lon, lat) else { return };
                // the neighbours of ring 1 only: `cut_to_cells` refuses a border wider than half the edge of a cell
                for other in me.grid_disk_safe(1).filter(|&c| c != me) {
                    if let Some(&o) = map.get(&other) {
                        if near_cell(other, lon, lat, m) {
                            f(o);
                        }
                    }
                }
            }
            // the cut of an area list has no border
            Grid::Areas { .. } => {}
        }
    }

    /// The name of the file of area `i`, with no extension: `z_x_y` for a tile, the index for a cell.
    fn file(&self, i: u32) -> String {
        match self {
            Grid::Tiles { list, .. } => {
                let t = list[i as usize];
                format!("{}_{}_{}", t.z, t.x, t.y)
            }
            Grid::Cells { list, .. } => list[i as usize].to_string(),
            Grid::Areas { list, .. } => list.areas[i as usize].name.clone(),
        }
    }
}

pub use crate::tiles::near_tile;

/// The border of the tiles of a build by tiles, in metres: the connector of the dangling paths (`snap_m`) joins a dangling
/// end to the nearest node within `snap_m`, so a tile with all the ways within twice `snap_m` of it (and 5 m more) makes the
/// same connectors at its nodes as the whole build.
pub fn border_m(snap_m: f64) -> f64 {
    2.0 * snap_m + 5.0
}

impl RawSink for Nodes<'_> {
    fn node<'a>(&mut self, id: i64, lat: f64, lon: f64, _: &mut dyn Iterator<Item = (&'a str, &'a str)>) {
        widen(&mut self.span, id);
        self.ids.push(id);
        let tile = if self.boundary.contains(lon, lat) {
            self.grid.of(lon, lat)
        } else {
            None
        };
        if let Some(t) = tile {
            if self.own.last() != Some(&t) {
                self.own.push(t);
            }
            // the other tiles whose border holds the node
            if self.border_m > 0.0 {
                let near = &mut self.near;
                self.grid.near(lon, lat, self.border_m, |o| near.push((id, o)));
            }
        }
        self.runs.push(id, tile);
    }
    fn way<'a>(&mut self, _: i64, _: &mut dyn Iterator<Item = (&'a str, &'a str)>, _: &mut dyn Iterator<Item = i64>) {
        self.other = true;
    }
    fn relation<'a>(
        &mut self,
        _: i64,
        _: &mut dyn Iterator<Item = (&'a str, &'a str)>,
        _: &mut dyn Iterator<Item = (i64, MemberType, &'a str)>,
    ) {
        self.other = true;
    }
}

/// Pass 2: the ways with their tiles (the tiles of their nodes inside), the nodes that each tile of a way needs from
/// outside the tile, the runs of the ways, and the relations, whole.
struct Ways<'a> {
    nodes: &'a Book,
    /// the nodes in the border of other tiles (sorted (node, tile)): a way with such a node goes to those tiles too
    near: &'a [(i64, u32)],
    runs: BlockRuns,
    multi: Vec<(i64, u32)>,
    outside: Vec<(i64, u32)>,
    /// the uses of the nodes by the kept ways (node, flags of [`NodeUses::of_way`]); none: no junctions files
    node_uses: Option<&'a dyn NodeUses>,
    uses: Vec<(i64, u8)>,
    /// the nodes outside the area of the kept ways: (node, way, the first tile of the way), for their home tile
    homes: Vec<(i64, i64, u32)>,
    rels: Vec<Elem>,
    tiles: Vec<u32>,
    spans: [Span; 2],
    /// the ids of all ways of the block (a row of the id file of the ways)
    ids: IdPack,
}

impl RawSink for Ways<'_> {
    fn node<'a>(&mut self, _: i64, _: f64, _: f64, _: &mut dyn Iterator<Item = (&'a str, &'a str)>) {}
    fn way<'a>(&mut self, id: i64, tags: &mut dyn Iterator<Item = (&'a str, &'a str)>, refs: &mut dyn Iterator<Item = i64>) {
        widen(&mut self.spans[0], id);
        self.ids.push(id);
        let refs: Vec<(i64, Option<u32>)> = refs.map(|n| (n, self.nodes.get(n))).collect();
        self.tiles.clear();
        self.tiles.extend(refs.iter().filter_map(|r| r.1));
        self.tiles.sort_unstable();
        self.tiles.dedup();
        // the home tile of a node outside the area comes from the tiles of the nodes of the ways, not from the borders
        let first = self.tiles.first().copied();
        if !self.near.is_empty() && first.is_some() {
            self.tiles.extend(refs.iter().flat_map(|r| tiles_in(self.near, r.0)));
            self.tiles.sort_unstable();
            self.tiles.dedup();
        }
        match self.tiles.len() {
            0 => {
                self.runs.push(id, None);
                return;
            }
            1 => self.runs.push(id, Some(self.tiles[0])),
            _ => {
                self.runs.push(id, None);
                self.multi.extend(self.tiles.iter().map(|&t| (id, t)));
            }
        }
        let way = RawWay {
            id,
            tags: tags.map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            refs: refs.iter().map(|r| r.0).collect(),
        };
        for &t in &self.tiles {
            self.outside.extend(refs.iter().filter(|r| r.1 != Some(t)).map(|r| (r.0, t)));
        }
        if let Some(u) = self.node_uses {
            u.of_way(&way.tags, &way.refs, &mut self.uses);
        }
        let first = first.expect("a tile");
        self.homes.extend(refs.iter().filter(|r| r.1.is_none()).map(|r| (r.0, id, first)));
    }
    fn relation<'a>(
        &mut self,
        id: i64,
        tags: &mut dyn Iterator<Item = (&'a str, &'a str)>,
        members: &mut dyn Iterator<Item = (i64, MemberType, &'a str)>,
    ) {
        widen(&mut self.spans[1], id);
        let tags = tags.map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let members = members.map(|(m, t, r)| (m, t, r.to_string())).collect();
        self.rels.push(Elem::Relation(Relation { id, tags, members }));
    }
}

/// A use of a node by a way, 8 bytes: the node id with the 2 bits of its flags ([`NodeUses::of_way`]) below it, so the
/// rows sort by the node.
struct Use;
impl Codec<(i64, u8)> for Use {
    fn put(&self, r: &(i64, u8), out: &mut Vec<u8>) {
        out.extend_from_slice(&((r.0 << 2) | (r.1 & 3) as i64).to_le_bytes());
    }
    fn get(&self, b: &mut &[u8]) -> (i64, u8) {
        let v = i64::from_le_bytes(b[..8].try_into().expect("8 bytes"));
        *b = &b[8..];
        (v >> 2, (v & 3) as u8)
    }
}

/// Two numbers, 16 bytes: (id, tile).
/// The buckets of the nodes from outside: 2^27 ids each.
const BUCKET_SHIFT: u32 = 27;

struct Pair;
impl Codec<(i64, i64)> for Pair {
    fn put(&self, r: &(i64, i64), out: &mut Vec<u8>) {
        out.extend_from_slice(&r.0.to_le_bytes());
        out.extend_from_slice(&r.1.to_le_bytes());
    }
    fn get(&self, b: &mut &[u8]) -> (i64, i64) {
        (i64::from_le_bytes(b[..8].try_into().expect("8 bytes")), i64::from_le_bytes(b[8..16].try_into().expect("8 bytes")))
    }
}

/// The uses of the nodes by the ways, for the junctions files of a build by tiles. The cut does not know about roads:
/// the program that calls it gives these two functions (for example, which nodes are junctions of its road network).
pub trait NodeUses: Sync {
    /// The uses of the nodes of a way: (node, flags), with flags in bits 0 and 1 only.
    fn of_way(&self, tags: &[(String, String)], refs: &[i64], out: &mut Vec<(i64, u8)>);
    /// The flags of a node from the flags of all its uses, in bits 0 and 1 only ([`HOME`] is bit 2). 0: the node has no
    /// row in the junctions file, unless the tile is its home tile.
    fn of_node(&self, uses: &mut dyn Iterator<Item = u8>) -> u8;
}

/// The start of a junctions file: the name and the version of the format. Then one row for each node outside the tile
/// with flags ([`NodeUses::of_node`] in bits 0 and 1, [`HOME`]): the node id (i64, little endian) and its flags, in the
/// order of the node ids.
pub const JUNCTIONS_MAGIC: &[u8; 8] = b"DOSMJN02";
/// The node is outside the area, and this tile is its home tile: the tile has all ways of the node, and holds it.
pub const HOME: u8 = 4;

/// The junctions file of each tile: for each (node, tile) of `outside` (sorted), the junction of the node in the whole area
/// (from `uses`, the uses of the nodes by all kept ways, sorted by node), and whether the tile is the home tile of the node
/// (`home`, sorted (node, tile)), to the part "junctions" of the tile. A tile has only some of the ways of a node outside
/// it, so it can miss a junction there.
fn outside_junctions(
    outside: impl Iterator<Item = io::Result<(i64, i64)>>,
    uses: impl Iterator<Item = io::Result<(i64, i64)>>,
    home: &[(i64, u32)],
    parts: &Parts,
    node_uses: &dyn NodeUses,
) -> Result<(), String> {
    let io = |e: io::Error| format!("a temporary file of the tile cut: {e}");
    let mut uses = uses.peekable();
    let (mut last, mut junction, mut prev) = (None, 0u8, None);
    for p in outside {
        let (n, t) = p.map_err(io)?;
        if prev == Some((n, t)) {
            continue;
        }
        prev = Some((n, t));
        if last != Some(n) {
            last = Some(n);
            while uses.next_if(|x| x.as_ref().is_ok_and(|x| x.0 < n)).is_some() {}
            let mut flags = Vec::new();
            while let Some(x) = uses.next_if(|x| x.as_ref().is_ok_and(|x| x.0 == n)) {
                flags.push(x.map_err(io)?.1 as u8);
            }
            if let Some(Err(_)) = uses.peek() {
                return Err(io(uses.next().expect("peeked").expect_err("an error")));
            }
            junction = node_uses.of_node(&mut flags.into_iter()) & 3;
        }
        let mut flags = junction;
        if home.binary_search(&(n, t as u32)).is_ok() {
            flags |= HOME;
        }
        if flags != 0 {
            let mut row = n.to_le_bytes().to_vec();
            row.push(flags);
            parts.add(t as u32, "junctions", row)?;
        }
    }
    parts.flush()
}

/// The junctions file of a tile from its part (no part: a file with no rows).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn write_junctions(parts: &Parts, tile: u32, out: &Path) -> Result<(), String> {
    let e = |e: io::Error| format!("{}: {e}", out.display());
    let mut f = io::BufWriter::new(std::fs::File::create(out).map_err(e)?);
    f.write_all(JUNCTIONS_MAGIC).map_err(e)?;
    if let Some(mut p) = parts.reader(tile, "junctions") {
        io::copy(&mut p, &mut f).map_err(e)?;
    }
    parts.remove(tile, "junctions");
    f.flush().map_err(e)
}

/// Read a junctions file of the tile cut: (node id, flags).
pub fn read_junctions(path: &Path) -> Result<Vec<(i64, u8)>, String> {
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if b.len() < 8 || &b[..8] != JUNCTIONS_MAGIC || (b.len() - 8) % 9 != 0 {
        return Err(format!("{}: not a junctions file of this version of the tile cut", path.display()));
    }
    Ok(b[8..].chunks(9).map(|r| (i64::from_le_bytes(r[..8].try_into().unwrap()), r[8])).collect())
}

/// The positions of the nodes of `ids` in a PBF file (the home nodes of a tile: the build of the tile holds them).
pub fn node_positions<R: Read + Send>(r: R, ids: &crate::util::FxHashSet<i64>) -> Result<Vec<(i64, (f64, f64))>, String> {
    struct Find<'a>(&'a crate::util::FxHashSet<i64>, Vec<(i64, (f64, f64))>);
    impl RawSink for Find<'_> {
        fn node<'a>(&mut self, id: i64, lat: f64, lon: f64, _: &mut dyn Iterator<Item = (&'a str, &'a str)>) {
            if self.0.contains(&id) {
                self.1.push((id, (lon, lat)));
            }
        }
        fn way<'a>(&mut self, _: i64, _: &mut dyn Iterator<Item = (&'a str, &'a str)>, _: &mut dyn Iterator<Item = i64>) {}
        fn relation<'a>(
            &mut self,
            _: i64,
            _: &mut dyn Iterator<Item = (&'a str, &'a str)>,
            _: &mut dyn Iterator<Item = (i64, MemberType, &'a str)>,
        ) {
        }
    }
    let mut out = Vec::new();
    let mut r = r;
    let mut buf = Buffers::default();
    while let Some(b) = read_block(&mut r)? {
        let mut s = Find(ids, Vec::new());
        decode_block(&b, [true, false, false], &mut buf, &mut s)?;
        out.extend(s.1);
    }
    Ok(out)
}

/// Cut the region file that `open` reads (a new reader for each pass) to the tiles of `tiles` inside `boundary` (one zoom
/// with a border; else also mixed zooms, no tile in another):
/// the file of a tile goes into `dir` as `z_x_y.osm.pbf`; a tile with no node inside has no file. Gives the numbers of
/// elements of each tile. `complete`: the types of the relations to complete in each tile with all their members
/// (osmium's smart cut: `multipolygon`). A program that reads no multipolygons gives none: then the cut of France reads 60
/// of 80 million extra nodes less. `node_uses`: write the junctions file of each tile next to it
/// (`z_x_y.junctions`). `compress`: zlib blocks, as most `.osm.pbf` files (about 2 times smaller); false: raw blocks (a
/// faster cut, larger files that each OSM program reads too). The elements are the same.
pub fn cut_to_tiles<R: Read + Seek + Send>(
    open: impl Fn() -> Result<R, String>,
    boundary: &Boundary,
    tiles: &[Tile],
    dir: &Path,
    complete: &[&str],
    border_m: f64,
    node_uses: Option<&dyn NodeUses>,
    compress: bool,
) -> Result<Vec<CutCounts>, String> {
    cut_to_dir(open, boundary, &Grid::tiles(tiles), dir, complete, border_m, node_uses, compress)
}

/// [`cut_to_tiles`] for H3 cells of one resolution ([`crate::cells::cells`]): the file of a cell goes into `dir` as
/// `<cell index>.osm.pbf`, for example `8939604a4ffffff.osm.pbf`. The rules are the rules of the tiles. `border_m` must be
/// less than half the mean edge of a cell (the cut looks for the border in the six neighbours of a cell only).
pub fn cut_to_cells<R: Read + Seek + Send>(
    open: impl Fn() -> Result<R, String>,
    boundary: &Boundary,
    cells: &[CellIndex],
    dir: &Path,
    complete: &[&str],
    border_m: f64,
    compress: bool,
) -> Result<Vec<CutCounts>, String> {
    cut_to_dir(open, boundary, &cell_grid(cells, border_m)?, dir, complete, border_m, None, compress)
}

/// Cut the region file to each area of `areas` inside `boundary`, as [`cut_to_tiles`] does to tiles: the file of an area
/// goes into `dir` as `NAME.osm.pbf`; an area with no node inside has no file. The areas must not overlap: a node inside
/// two areas stops the cut with an error that names them (a node on the border of two areas goes to the first one).
/// Gives the numbers of elements of each area.
pub fn cut_to_areas<R: Read + Seek + Send>(
    open: impl Fn() -> Result<R, String>,
    boundary: &Boundary,
    areas: &AreaList,
    dir: &Path,
    complete: &[&str],
    compress: bool,
) -> Result<Vec<CutCounts>, String> {
    cut_to_dir(open, boundary, &Grid::areas(areas), dir, complete, 0.0, None, compress)
}

/// [`cut_to_areas`], with each area file given to `sink` (in pieces, area by area; the number of an area is its place
/// in `areas.areas`). The bytes equal the files of [`cut_to_areas`].
pub fn cut_areas_to_sink<R: Read + Seek + Send>(
    open: impl Fn() -> Result<R, String>,
    boundary: &Boundary,
    areas: &AreaList,
    complete: &[&str],
    compress: bool,
    sink: Sink,
) -> Result<Vec<CutCounts>, String> {
    cut_into_sink(open, boundary, &Grid::areas(areas), complete, 0.0, compress, sink)
}

/// The grid of the cells, with an error for cells of two resolutions or a border of half the edge of a cell or more.
fn cell_grid(cells: &[CellIndex], border_m: f64) -> Result<Grid<'_>, String> {
    if let Some(c) = cells.first() {
        let half = c.resolution().edge_length_m() / 2.0;
        if border_m >= half {
            return Err(format!("the border ({border_m} m) must be less than half the edge of a cell ({half:.0} m)"));
        }
    }
    Grid::cells(cells)
}

/// The cut into the files of a folder (the tiles or the cells), with the temporary files in a folder of their own.
fn cut_to_dir<R: Read + Seek + Send>(
    open: impl Fn() -> Result<R, String>,
    boundary: &Boundary,
    grid: &Grid,
    dir: &Path,
    complete: &[&str],
    border_m: f64,
    node_uses: Option<&dyn NodeUses>,
    compress: bool,
) -> Result<Vec<CutCounts>, String> {
    // a number for each cut: two cuts at a time in one process (the tests) must not share the folder
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = crate::store::temp_dir().join(format!("pbfcut-tiles-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let cut = cut(open, boundary, grid, complete, border_m, Files::Dir(tmp.clone()), Some(dir), None, !compress, node_uses);
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(cut?.0)
}

/// The tile files of [`cut_to_store`], in one temporary file of the store.
pub struct TileFiles {
    parts: Parts,
    /// The tiles of the cut (the order of the list of the cut) and the numbers of elements of each; a tile with no node
    /// inside has no file.
    pub tiles: Vec<(Tile, CutCounts)>,
}

impl TileFiles {
    /// The `.osm.pbf` file of tile `i` of [`TileFiles::tiles`] (empty for a tile with no file).
    pub fn reader(&self, i: usize) -> impl Read + Send + '_ {
        self.parts.reader(i as u32, "tile").unwrap_or_else(|| Box::new(io::empty()))
    }

    /// The size of the file of tile `i` in bytes.
    pub fn size(&self, i: usize) -> u64 {
        let key = (i as u32, "tile");
        match &self.parts.files {
            Files::Dir(d) => std::fs::metadata(Parts::path(d, key.0, key.1)).map_or(0, |m| m.len()),
            Files::Store(c) => c.index.lock().expect("the index").get(&key).map_or(0, |v| v.iter().map(|c| c.1).sum()),
        }
    }
}

impl Drop for TileFiles {
    /// The folder of the tile files goes (natively; the chunks of the store go with their temporary file).
    fn drop(&mut self) {
        if let Files::Dir(d) = &self.parts.files {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

/// [`cut_to_tiles`] into the temporary store (`spill::temp_store`: a folder natively, OPFS in the browser), with no
/// junctions files. With `compress` false (no zlib), for a program that reads each tile file one time, soon after the
/// cut. `complete`: the types of the relations to complete with all their members (for example `multipolygon` and
/// `boundary`), each one in each tile with a member, so each tile file is complete (as the cut of the build by tiles and
/// osmium's smart cut).
pub fn cut_to_store<R: Read + Seek + Send>(
    open: impl Fn() -> Result<R, String>,
    boundary: &Boundary,
    tiles: &[Tile],
    complete: &[&str],
    compress: bool,
) -> Result<TileFiles, String> {
    // natively, a file for each part and each tile in a temporary folder, so the tiles are written at the same time with
    // no lock (France: one file for all took 31 s for step 5); in the browser, chunks of one file of the store (OPFS)
    #[cfg(not(target_arch = "wasm32"))]
    let files = {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = crate::store::temp_dir().join(format!("pbfcut-tilefiles-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        Files::Dir(tmp)
    };
    #[cfg(target_arch = "wasm32")]
    let files = Files::Store(Chunks::new(&*crate::store::temp_store()).map_err(|e| format!("a temporary file of the tile cut: {e}"))?);
    let (counts, parts) = cut(open, boundary, &Grid::tiles(tiles), complete, 0.0, files, None, None, !compress, None)?;
    Ok(TileFiles { parts, tiles: tiles.iter().copied().zip(counts).collect() })
}

/// The cut of the build by tiles ([`cut_to_tiles`]: each multipolygon complete in every tile with a member, the junctions)
/// into the temporary store: the cut where no folder is (the browser). The tile files are the part "tile" of each tile,
/// the junctions the part "junctions".
pub fn cut_to_store_build<R: Read + Seek + Send>(
    open: impl Fn() -> Result<R, String>,
    boundary: &Boundary,
    tiles: &[Tile],
    multipolygons: bool,
    border_m: f64,
    node_uses: Option<&dyn NodeUses>,
    compress: bool,
) -> Result<TileFiles, String> {
    let chunks = Chunks::new(&*crate::store::temp_store()).map_err(|e| format!("a temporary file of the tile cut: {e}"))?;
    let complete: &[&str] = if multipolygons { &["multipolygon"] } else { &[] };
    let (counts, parts) = cut(open, boundary, &Grid::tiles(tiles), complete, border_m, Files::Store(chunks), None, None, !compress, node_uses)?;
    Ok(TileFiles { parts, tiles: tiles.iter().copied().zip(counts).collect() })
}

/// [`cut_to_tiles`], with each tile file given to `sink` (in pieces, tile by tile) and not to a folder: the browser
/// writes the tile files into storage of its own. The tile files are equal to the files of [`cut_to_tiles`]. The
/// temporary files go into a temporary folder natively and into the store of the page in the browser
/// ([`crate::store::set_temp_store`], for example OPFS).
pub fn cut_to_sink<R: Read + Seek + Send>(
    open: impl Fn() -> Result<R, String>,
    boundary: &Boundary,
    tiles: &[Tile],
    complete: &[&str],
    border_m: f64,
    compress: bool,
    sink: Sink,
) -> Result<Vec<CutCounts>, String> {
    cut_into_sink(open, boundary, &Grid::tiles(tiles), complete, border_m, compress, sink)
}

/// [`cut_to_cells`], with each cell file given to `sink` (in pieces, cell by cell), as [`cut_to_sink`] does for the
/// tiles. The cell files are equal to the files of [`cut_to_cells`].
pub fn cut_cells_to_sink<R: Read + Seek + Send>(
    open: impl Fn() -> Result<R, String>,
    boundary: &Boundary,
    cells: &[CellIndex],
    complete: &[&str],
    border_m: f64,
    compress: bool,
    sink: Sink,
) -> Result<Vec<CutCounts>, String> {
    cut_into_sink(open, boundary, &cell_grid(cells, border_m)?, complete, border_m, compress, sink)
}

/// The cut into a sink (the tiles or the cells), with the temporary files in a folder natively, in the store of the
/// page in the browser.
fn cut_into_sink<R: Read + Seek + Send>(
    open: impl Fn() -> Result<R, String>,
    boundary: &Boundary,
    grid: &Grid,
    complete: &[&str],
    border_m: f64,
    compress: bool,
    mut sink: Sink,
) -> Result<Vec<CutCounts>, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let files = Files::Store(Chunks::new(&*crate::store::temp_store()).map_err(|e| format!("a temporary file of the tile cut: {e}"))?);
        Ok(cut(open, boundary, grid, complete, border_m, files, None, Some(&mut sink), !compress, None)?.0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = crate::store::temp_dir().join(format!("pbfcut-sink-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let cut = cut(open, boundary, grid, complete, border_m, Files::Dir(tmp.clone()), None, Some(&mut sink), !compress, None);
        let _ = std::fs::remove_dir_all(&tmp);
        Ok(cut?.0)
    }
}

/// The cut of [`cut_to_tiles`] and [`cut_to_store`]: the temporary files in `files`; the tile files into `dir` with their
/// junctions files, with no `dir` into `files` as the part "tile" of each tile, or into `sink`. Each relation of a type in
/// `complete` is complete in every tile with a member (every tile file is complete). `node_uses`: the cut of the build
/// by tiles, with the junctions of each tile; none: no junctions.
fn cut<R: Read + Seek + Send>(
    open: impl Fn() -> Result<R, String>,
    boundary: &Boundary,
    grid: &Grid,
    complete: &[&str],
    border_m: f64,
    files: Files,
    dir: Option<&Path>,
    // the tile files to this sink, not to the store (`cut_to_sink`)
    sink: Option<&mut Sink>,
    // the parts and the tile files with no zlib (a temporary file read one time soon after); the build by tiles keeps zlib
    raw: bool,
    // the cut of the build by tiles: the junctions of each tile (in `dir`, or in `files` as the part "junctions")
    node_uses: Option<&dyn NodeUses>,
) -> Result<(Vec<CutCounts>, Parts), String> {
    let io = |e: io::Error| format!("a temporary file of the tile cut: {e}");
    let (timestamp, sorted) = header(open()?)?;
    if !sorted {
        return Err("the region file is not sorted by type and id (Sort.Type_then_ID)".into());
    }
    let parts = Parts { files, waiting: Default::default() };
    if grid.len() == 0 {
        return Ok((Vec::new(), parts));
    }
    if border_m > 0.0 && !grid.one_level() {
        return Err("the tile cut with a border needs tiles of one zoom".into());
    }
    let store = crate::store::temp_store();
    let store = &*store;
    // the tiles with a node inside: only these have a file
    let mut with_file = vec![false; grid.len()];
    // 1. the book of the nodes, and the tiles with a node inside
    let (mut nodes, mut node_blocks, mut ways_at) = (Book::default(), Vec::new(), None);
    let mut near: Vec<(i64, u32)> = Vec::new();
    // the ids of all nodes and all ways of the file: part 3b reads them, not the blocks again
    let (mut node_ids, mut way_ids) = (Writer::new(store).map_err(io)?, Writer::new(store).map_err(io)?);
    each_block(
        &mut open()?,
        |b, buf| {
            let mut s = Nodes {
                boundary,
                grid,
                border_m,
                runs: BlockRuns::new(),
                own: Vec::new(),
                near: Vec::new(),
                other: false,
                span: EMPTY,
                ids: IdPack::default(),
            };
            decode_block(b, [true, true, true], buf, &mut s)?;
            s.own.sort_unstable();
            s.own.dedup();
            Ok((s.runs, s.other, s.span, (s.near, s.own, s.ids.bytes)))
        },
        |(runs, other, span, (mut n, own, ids)), at| {
            for t in own {
                with_file[t as usize] = true;
            }
            if !ids.is_empty() {
                node_ids.push(&RawBytes, &ids).map_err(io)?;
            }
            near.append(&mut n);
            if span != EMPTY {
                node_blocks.push((at, span));
            }
            nodes.append(runs.runs, runs.first.unwrap_or(false));
            if other {
                ways_at = Some(at);
            }
            Ok(!other)
        },
    )?;
    nodes.finish();
    tidy(&mut near);
    grid.overlap()?;
    crate::util::step(&format!("tile cut 1: the nodes inside ({} runs, {} MB; {} nodes in the border of another tile)", nodes.len(), nodes.size() >> 20, near.len()));
    // 2. the book of the ways (each way with a node inside); the nodes that a tile needs from outside it (to a
    // temporary list); the relations, whole (to a temporary file)
    let ways_at = ways_at.unwrap_or(0);
    let (mut ways, mut multi) = (Book::default(), Vec::new());
    // the nodes that a tile needs from outside it, in buckets by node id (2^27 ids each): each bucket is sorted in memory
    // on all threads in step 4 (one merge on one thread took 3.3 s of the 14.8 s of the cut of Sweden)
    let mut outside = crate::store::Buckets::new(BUCKET_SHIFT);
    // the uses of the nodes: only the junctions files of the build by tiles need them
    let build = node_uses.is_some();
    let mut uses = Writer::new(store).map_err(io)?;
    // the nodes outside the area are few (the border of the area): in memory
    let mut homes: Vec<(i64, i64, u32)> = Vec::new();
    let (mut way_blocks, mut rel_blocks) = (Vec::new(), Vec::new());
    let mut r = open()?;
    r.seek(SeekFrom::Start(ways_at)).map_err(|e| e.to_string())?;
    each_block(
        &mut r,
        |b, buf| {
            let mut s = Ways {
                nodes: &nodes,
                near: &near,
                runs: BlockRuns::new(),
                multi: Vec::new(),
                outside: Vec::new(),
                node_uses,
                uses: Vec::new(),
                homes: Vec::new(),
                rels: Vec::new(),
                tiles: Vec::new(),
                spans: [EMPTY; 2],
                ids: IdPack::default(),
            };
            decode_block(b, [false, true, true], buf, &mut s)?;
            let mut rels = Vec::new();
            blobs(s.rels, &mut rels, raw)?;
            // the nodes from outside, in the bytes of their buckets (here, on all threads)
            let outside = crate::store::Buckets::encode(BUCKET_SHIFT, &Pair, s.outside.iter().map(|&(n, t)| (n, (n, t as i64))));
            let uses = s.uses;
            Ok((s.runs, s.multi, (outside, uses, s.homes, s.ids.bytes), rels, s.spans))
        },
        |(runs, mut m, (o, u, h, ids), rels, spans), at| {
            if !ids.is_empty() {
                way_ids.push(&RawBytes, &ids).map_err(io)?;
            }
            if spans[0] != EMPTY {
                way_blocks.push((ways_at + at, spans[0]));
                ways.append(runs.runs, runs.first.unwrap_or(false));
            }
            if spans[1] != EMPTY {
                rel_blocks.push((ways_at + at, spans[1]));
            }
            multi.append(&mut m);
            outside.push_encoded(store, o).map_err(io)?;
            for (n, f) in u {
                uses.push(&Use, &(n, f)).map_err(io)?;
            }
            homes.extend(h);
            parts.add(ALL, "relations.all", rels)?;
            Ok(true)
        },
    )?;
    parts.flush()?;
    ways.finish();
    tidy(&mut multi);
    // the home tile of each node outside the area: no tile holds it, so the first tile (in the order of the tiles) of the ways
    // at the node gets all these ways, and holds the node (its turns, its rows). Not in a cut to an area list: an area file
    // gets only the ways with a node inside it, as in osmium
    if let Grid::Areas { .. } = grid {
        homes.clear();
    }
    homes.sort_unstable();
    let (mut home_ways, mut home_nodes): (Vec<(i64, u32)>, Vec<(i64, u32)>) = (Vec::new(), Vec::new());
    for run in homes.chunk_by(|a, b| a.0 == b.0) {
        let home = run.iter().map(|h| h.2).min().expect("a run");
        home_nodes.push((run[0].0, home));
        for &(_, w, first) in run {
            let multi_tiles: Vec<u32> = tiles_in(&multi, w).collect();
            let in_home = if multi_tiles.is_empty() { first == home } else { multi_tiles.contains(&home) };
            if !in_home {
                home_ways.push((w, home));
            }
        }
    }
    drop(homes);
    crate::util::step(&format!(
        "tile cut 2: the ways ({} runs, {} MB; {} pairs of ways in more tiles)",
        ways.len(),
        ways.size() >> 20,
        multi.len()
    ));
    // 3. the relations with a member node inside a tile or a member way kept in a tile (only the members that the file
    // has: a run can hold the id of an element that the file does not have), their parents, and the members of the
    // multipolygons
    let (mut member_nodes, mut member_ways) = (Vec::new(), Vec::new());
    rel_batches(parts.reader(ALL, "relations.all"), complete, &mut |batch| {
        for (m, t) in batch.iter().flat_map(|b| b.members.iter()) {
            match t {
                MemberType::Node => member_nodes.push(*m),
                MemberType::Way => member_ways.push(*m),
                MemberType::Relation => {}
            }
        }
        Ok(())
    })?;
    for v in [&mut member_nodes, &mut member_ways] {
        v.par_sort_unstable();
        v.dedup();
    }
    crate::util::step(&format!("tile cut 3a: the members of the relations ({} nodes, {} ways)", member_nodes.len(), member_ways.len()));
    // which members the file has: the id files of parts 1 and 2 (one read of the ids; before, two reads of the blocks)
    let node_members = present_ids(node_ids.finish().map_err(io)?, &member_nodes).map_err(io)?;
    drop(member_nodes);
    let way_members = present_ids(way_ids.finish().map_err(io)?, &member_ways).map_err(io)?;
    drop(member_ways);
    let way_tiles = |id: i64, out: &mut Vec<u32>| match ways.get(id) {
        Some(t) => out.push(t),
        None => out.extend(tiles_in(&multi, id)),
    };
    let (mut direct, mut parents, mut multipolygons) = (Vec::new(), Vec::new(), Vec::new());
    // the relations of each batch on all threads (France: 3.5 s on one thread before), the results in the order of the file
    rel_batches(parts.reader(ALL, "relations.all"), complete, &mut |batch| {
        let found: Vec<Vec<(Vec<u32>, Vec<i64>)>> = batch
            .par_iter()
            .map(|b| {
                b.iter()
                    .map(|(_, _, members)| {
                        // the tiles of this relation, with no doubles (a route has thousands of members in a few tiles)
                        let (mut tiles, mut children) = (Vec::new(), Vec::new());
                        for &(m, t) in members {
                            match t {
                                MemberType::Node if node_members.binary_search(&m).is_ok() => tiles.extend(nodes.get(m)),
                                MemberType::Way if way_members.binary_search(&m).is_ok() => way_tiles(m, &mut tiles),
                                MemberType::Relation => children.push(m),
                                _ => {}
                            }
                        }
                        tiles.sort_unstable();
                        tiles.dedup();
                        (tiles, children)
                    })
                    .collect()
            })
            .collect();
        for (b, found) in batch.iter().zip(found) {
            for ((id, multipolygon, _), (tiles, children)) in b.iter().zip(found) {
                if multipolygon {
                    multipolygons.push(id);
                }
                parents.extend(children.into_iter().map(|m| (m, id)));
                direct.extend(tiles.into_iter().map(|t| (id, t)));
            }
        }
        Ok(())
    })?;
    crate::util::step(&format!("tile cut 3b: the members in the file ({} nodes, {} ways)", node_members.len(), way_members.len()));
    drop((node_members, way_members));
    tidy(&mut direct);
    parents.sort_unstable();
    let mut chosen: BTreeSet<(i64, u32)> = direct.iter().copied().collect();
    let mut queue = direct.clone();
    while let Some((rel, t)) = queue.pop() {
        let at = parents.partition_point(|p| p.0 < rel);
        for &(_, parent) in parents[at..].iter().take_while(|p| p.0 == rel) {
            if chosen.insert((parent, t)) {
                queue.push((parent, t));
            }
        }
    }
    drop(parents);
    let chosen: Vec<(i64, u32)> = chosen.into_iter().collect();
    crate::util::step(&format!("tile cut 3c: the tiles of the relations ({} pairs)", chosen.len()));
    // each chosen relation to its tiles; the members of the multipolygons with a member in a tile. The member ways in
    // more tiles (`more`: way, tile) go to a temporary file: a boundary goes to every tile with a member, so these pairs
    // grow with the number of tiles (Sweden at zoom 12: millions)
    let mut more = Writer::new(store).map_err(io)?;
    rel_batches(parts.reader(ALL, "relations.all"), complete, &mut |batch| {
        for (id, multipolygon, members) in batch.iter().flat_map(|b| b.iter()) {
            if multipolygon {
                for t in tiles_in(&direct, id) {
                    for &(m, ty) in members {
                        match ty {
                            MemberType::Node => outside.push(store, m, &Pair, &(m, t as i64)).map_err(io)?,
                            MemberType::Way => more.push(&Pair, &(m, t as i64)).map_err(io)?,
                            MemberType::Relation => {}
                        }
                    }
                }
            }
        }
        Ok(())
    })?;
    parts.remove(ALL, "relations.all");
    drop((direct, multipolygons));
    for &(w, t) in &home_ways {
        more.push(&Pair, &(w, t as i64)).map_err(io)?;
    }
    drop(home_ways);
    // sorted as pairs in memory (16 bytes each) when they fit a quarter of the budget, else on disk; then one sorted file
    // for the three reads below
    let mut more = {
        let mut parts = crate::store::SortedParts::new(vec![more.finish().map_err(io)?], &Pair, |p: &(i64, i64)| *p);
        let (mut w, mut buf) = (Writer::new(store).map_err(io)?, Vec::new());
        while parts.next_into(&mut buf).map_err(io)? {
            for p in &buf {
                w.push(&Pair, p).map_err(io)?;
            }
        }
        w.finish().map_err(io)?
    };
    if more.len > 0 {
        // the nodes of these ways: only the blocks with one of them
        let mut at = Vec::new();
        {
            let mut rows = more.read_ref(&Pair).map_err(io)?.peekable();
            for &(place, (lo, hi)) in &way_blocks {
                while rows.next_if(|x| x.as_ref().is_ok_and(|x| x.0 < lo)).is_some() {}
                match rows.peek() {
                    Some(Ok(x)) if x.0 <= hi => at.push(place),
                    Some(Err(_)) => return Err(io(rows.next().expect("peeked").expect_err("an error"))),
                    _ => {}
                }
            }
        }
        // the ways of the pairs, so that a block gives only them: a list of all ways of each block (France: 73 million) made
        // this step 2 s slower. The list is 8 bytes for each way; when it does not fit an eighth of the memory budget,
        // each block gives all its ways, as before.
        let wanted: Option<Vec<i64>> = if more.len.saturating_mul(8) <= crate::store::memory() / 8 {
            let mut w = Vec::new();
            for x in more.read_ref(&Pair).map_err(io)? {
                let id = x.map_err(io)?.0;
                if w.last() != Some(&id) {
                    w.push(id);
                }
            }
            Some(w)
        } else {
            None
        };
        let mut rows = more.read_ref(&Pair).map_err(io)?.peekable();
        let mut err = None;
        each_block_at(
            &mut open()?,
            &at,
            |b, buf| {
                let mut s = WayRefs { wanted: wanted.as_deref(), found: Vec::new() };
                // the node lists only: no tags
                crate::decode::decode_block_tags(b, [false, true, false], [false; 3], buf, &mut s)?;
                Ok(s.found)
            },
            |found| {
                // the nodes of these ways for each of their tiles, one row at a time, in the order of the ways (the blocks
                // come in the order of the file, and the pairs are sorted)
                for (w, refs) in &found {
                    while rows.next_if(|x| x.as_ref().is_ok_and(|x| x.0 < *w)).is_some() {}
                    let mut last = None;
                    while let Some(x) = rows.next_if(|x| x.as_ref().is_ok_and(|x| x.0 == *w)) {
                        let t = x.map_err(io)?.1;
                        if last == Some(t) {
                            continue;
                        }
                        last = Some(t);
                        for &n in refs {
                            if let Err(e) = outside.push(store, n, &Pair, &(n, t)) {
                                err.get_or_insert(e);
                            }
                        }
                    }
                }
                Ok(())
            },
        )?;
        err.map_or(Ok(()), Err).map_err(io)?;
    }
    crate::util::step("tile cut 3d: the members of the multipolygons");
    // 4. the nodes from outside the tiles, sorted; then read 2
    let buckets = outside.finish().map_err(io)?;
    let rows: usize = buckets.iter().map(|b| b.len).sum();
    crate::util::step(&format!("tile cut 4: the nodes from outside ({rows} rows, {} buckets)", buckets.len()));
    let mut sorted_parts = crate::store::SortedParts::new(buckets, &Pair, |p: &(i64, i64)| *p);
    let (mut buf, mut at): (Vec<(i64, i64)>, usize) = (Vec::new(), 0);
    let mut sorted = std::iter::from_fn(move || loop {
        if at < buf.len() {
            at += 1;
            return Some(Ok(buf[at - 1]));
        }
        match sorted_parts.next_into(&mut buf) {
            Ok(true) => at = 0,
            Ok(false) => return None,
            Err(e) => return Some(Err(e)),
        }
    });
    // the build by tiles reads the sorted rows again for its junctions files: a copy on disk while they go by
    let mut copy = if build { Some(Writer::new(store).map_err(io)?) } else { None };
    let mut copy_err = None;
    let pairs = std::iter::from_fn(|| {
        let p = sorted.next()?;
        if let (Ok(p), Some(c)) = (&p, copy.as_mut()) {
            if let Err(e) = c.push(&Pair, p) {
                copy_err.get_or_insert(e);
            }
        }
        Some(p)
    });
    let mut r = open()?;
    {
        // read 2: each element straight into its tile files, once: the nodes, the
        // junctions (the build by tiles), the ways, the relations
        let with_file = &with_file;
        let mut writers = write::Writers::new(grid.len(), timestamp, raw, write::outputs(grid, dir, &parts, sink)?)?;
        let mut rows = pairs.peekable();
        write::route_blocks(
            &mut r,
            &node_blocks,
            0,
            |(lo, hi)| {
                // the rows of this block (the rows before it are of nodes that the file does not have)
                while rows.next_if(|x| x.as_ref().is_ok_and(|x| x.0 < lo)).is_some() {}
                let mut want = Vec::new();
                while let Some(x) = rows.next_if(|x| x.as_ref().is_ok_and(|x| x.0 <= hi)) {
                    let (id, t) = x.map_err(io)?;
                    want.push((id, t as u32));
                }
                if let Some(Err(_)) = rows.peek() {
                    return Err(io(rows.next().expect("peeked").expect_err("an error")));
                }
                want.dedup();
                Ok(want)
            },
            &|id, v: &mut Vec<u32>| v.extend(nodes.get(id)),
            with_file,
            &mut writers,
        )?;
        drop(rows);
        // the sorted rows and their buffer (the closures of `pairs` and `sorted` borrow them until here)
        drop(sorted);
        // the lists of read 1 go as soon as read 2 is past them
        drop(nodes);
        crate::util::trim_memory();
        copy_err.map_or(Ok(()), Err).map_err(io)?;
        crate::util::step("tile cut 5a: the nodes to the tiles");
        if build {
            let uses = uses.finish().map_err(io)?;
            let n_uses = uses.len;
            let uses = crate::store::sorted(uses, |b| i64::from_le_bytes(b[..8].try_into().expect("8 bytes")), 64 << 20, store, &Use).map_err(io)?;
            crate::util::step(&format!("tile cut 5b: the uses of the nodes, sorted ({n_uses} rows)"));
            home_nodes.sort_unstable();
            let uses = uses.map(|u| u.map(|(n, f)| (n, f as i64)));
            let outside = copy.take().expect("a copy for the junctions").finish().map_err(io)?;
            outside_junctions(outside.read(&Pair).map_err(io)?, uses, &home_nodes, &parts, node_uses.expect("the uses of the nodes"))?;
            crate::util::step("tile cut 5c: the junctions files");
        }
        crate::util::trim_memory();
        let mut more_rows = more.read_ref(&Pair).map_err(io)?.peekable();
        write::route_blocks(
            &mut r,
            &way_blocks,
            1,
            |(lo, hi)| {
                while more_rows.next_if(|x| x.as_ref().is_ok_and(|x| x.0 < lo)).is_some() {}
                let mut want = Vec::new();
                while let Some(x) = more_rows.next_if(|x| x.as_ref().is_ok_and(|x| x.0 <= hi)) {
                    let (w, t) = x.map_err(io)?;
                    want.push((w, t as u32));
                }
                if let Some(Err(_)) = more_rows.peek() {
                    return Err(io(more_rows.next().expect("peeked").expect_err("an error")));
                }
                want.dedup();
                Ok(want)
            },
            &|id, v: &mut Vec<u32>| way_tiles(id, v),
            with_file,
            &mut writers,
        )?;
        drop(more_rows);
        drop((ways, multi, more));
        crate::util::trim_memory();
        crate::util::step("tile cut 5d: the ways to the tiles");
        let chosen = &chosen;
        write::route_blocks(&mut r, &rel_blocks, 2, |_| Ok(Vec::new()), &|id, v: &mut Vec<u32>| v.extend(tiles_in(chosen, id)), with_file, &mut writers)?;
        let counts = writers.into_counts();
        // the junctions files next to the tile files (the build by tiles into a folder)
        if let (Some(dir), true) = (dir, build) {
            for t in 0..grid.len() {
                if with_file[t] {
                    write_junctions(&parts, t as u32, &dir.join(format!("{}.junctions", grid.file(t as u32))))?;
                }
            }
        }
        crate::util::step("tile cut 5e: the relations, and the tile files");
        return Ok((counts, parts));
    }
}
