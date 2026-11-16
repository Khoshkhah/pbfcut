//! The peak heap of the cut (docs/design.md, section 4). This test binary counts the bytes that it allocates, so the
//! numbers are the same on every computer. The limits are in tests/peak_heap.json: lower them after an improvement;
//! raise them only after a review of the cause.

use pbfcut::boundary::Boundary;
use pbfcut::tiles::tiles;
use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: each call goes to the system allocator with the same arguments; the counters only add and subtract
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            PEAK.fetch_max(LIVE.fetch_add(l.size(), Relaxed) + l.size(), Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            if new >= l.size() {
                PEAK.fetch_max(LIVE.fetch_add(new - l.size(), Relaxed) + new - l.size(), Relaxed);
            } else {
                LIVE.fetch_sub(l.size() - new, Relaxed);
            }
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

/// The hashes of the files of a cut into a folder, and the peak heap of the cut in MiB.
fn cut(z: u8, memory: usize) -> (Vec<(String, Vec<u8>)>, f64) {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let pbf = data.join("monaco.osm.pbf");
    let monaco = Boundary::parse(&std::fs::read_to_string(data.join("monaco.geojson")).unwrap()).unwrap();
    let list = tiles(&monaco, z);
    let dir = std::env::temp_dir().join(format!("pbfcut-memory-{z}-{memory}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let open = || std::fs::File::open(&pbf).map(std::io::BufReader::new).map_err(|e| e.to_string());
    pbfcut::store::set_memory(memory);
    PEAK.store(LIVE.load(Relaxed), Relaxed);
    let start = LIVE.load(Relaxed);
    pbfcut::tilecut::cut_to_tiles(open, &monaco, &list, &dir, &["multipolygon", "boundary"], 0.0, None).unwrap();
    let peak = (PEAK.load(Relaxed) - start) as f64 / 1048576.0;
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (e.file_name().into_string().unwrap(), std::fs::read(e.path()).unwrap())
        })
        .collect();
    files.sort();
    std::fs::remove_dir_all(&dir).unwrap();
    (files, peak)
}

/// One test: the cuts run one after the other, so that the peaks do not mix.
#[test]
fn the_peak_heap_stays_in_its_limits() {
    let limits: serde_json::Value = serde_json::from_str(include_str!("peak_heap.json")).unwrap();
    let limit = |k: &str| limits[k].as_f64().unwrap_or_else(|| panic!("no limit {k}"));
    let (_, peak_12) = cut(12, 2 << 30);
    let (many, peak_17) = cut(17, 2 << 30);
    // a very small budget: the large sorts go to disk, the blocks are routed in many small jobs, and the files stay
    // the same
    let (many_small, peak_17_small) = cut(17, 64 << 10);
    eprintln!("peak heap: zoom 12 {peak_12:.1} MiB, zoom 17 {peak_17:.1} MiB, zoom 17 with 64 KiB {peak_17_small:.1} MiB");
    assert!(peak_12 <= limit("monaco-z12"), "zoom 12: {peak_12:.1} MiB");
    assert!(peak_17 <= limit("monaco-z17"), "zoom 17: {peak_17:.1} MiB");
    // no test of zoom 17 against zoom 12 here: on Monaco the output of zoom 17 is 24 times the output of zoom 12 (the
    // boundary of Monaco goes complete to all 78 tiles), and the blocks in flight hold their part of it. The growth with
    // the number of tiles is measured on Sweden (tools/measure.sh).
    assert!(many == many_small, "the files change with the memory budget");
}
