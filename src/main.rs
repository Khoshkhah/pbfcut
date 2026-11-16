//! The command line of pbfcut (docs/reference/cli.md).
//!
//!   pbfcut tiles IN.osm.pbf (--zoom Z | --tiles LIST.txt) [OPTIONS] -o DIR
//!   pbfcut h3 IN.osm.pbf --res R [OPTIONS] -o DIR
//!   pbfcut compare DIR_A DIR_B

use pbfcut::boundary::{Boundary, WORLD};
use pbfcut::decode::{decode_block, read_block, Buffers};
use pbfcut::elements::{MemberType, RawSink};
use pbfcut::cells::{cells, Resolution};
use pbfcut::tilecut::{cut_to_cells, cut_to_tiles};
use pbfcut::tiles::{tiles, Tile};
use std::collections::BTreeSet;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Instant;

const USAGE: &str = "usage:
  pbfcut tiles IN.osm.pbf (--zoom Z | --tiles LIST.txt) [OPTIONS] -o DIR
  pbfcut h3 IN.osm.pbf --res R [OPTIONS] -o DIR
  pbfcut compare DIR_A DIR_B
OPTIONS: [--boundary AREA.geojson] [--border METRES] [--complete TYPES] [--memory SIZE] [--temp DIR] [--steps]";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("tiles") => cut(&args[1..], false),
        Some("h3") => cut(&args[1..], true),
        Some("compare") if args.len() == 3 => compare(Path::new(&args[1]), Path::new(&args[2])),
        _ => Err(USAGE.to_string()),
    };
    if let Err(e) = r {
        eprintln!("pbfcut: {e}");
        std::process::exit(1);
    }
}

/// The value of `--name`, if the arguments have it.
fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// A size such as `4G`, `512M`, `64K` or a number of bytes.
fn size(text: &str) -> Result<usize, String> {
    let t = text.trim();
    let (num, unit) = match t.char_indices().find(|c| !c.1.is_ascii_digit() && c.1 != '.') {
        Some((i, _)) => (&t[..i], t[i..].trim_end_matches(['B', 'b', 'i'])),
        None => (t, ""),
    };
    let n: f64 = num.parse().map_err(|_| format!("not a size: {text} (for example 4G or 512M)"))?;
    let k = match unit.to_ascii_uppercase().as_str() {
        "" => 1.0,
        "K" => 1024.0,
        "M" => 1048576.0,
        "G" => 1073741824.0,
        _ => return Err(format!("not a size: {text} (for example 4G or 512M)")),
    };
    Ok((n * k) as usize)
}

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

/// The tiles of a list file: one `z/x/y` on each line. A tile must not be in another tile of the list.
fn tile_list(text: &str) -> Result<Vec<Tile>, String> {
    let mut list = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let v: Vec<u32> = line.split('/').map(|n| n.parse().map_err(|_| format!("not a tile z/x/y: {line}"))).collect::<Result<_, _>>()?;
        let [z, x, y] = v[..] else { return Err(format!("not a tile z/x/y: {line}")) };
        if z > 30 || x >= 1 << z || y >= 1 << z {
            return Err(format!("not a tile z/x/y: {line}"));
        }
        list.push(Tile { z: z as u8, x, y });
    }
    let all: BTreeSet<Tile> = list.iter().copied().collect();
    if all.len() != list.len() {
        return Err("the list has a tile two times".into());
    }
    for t in &list {
        for z in 0..t.z {
            let up = Tile { z, x: t.x >> (t.z - z), y: t.y >> (t.z - z) };
            if all.contains(&up) {
                return Err(format!("the tile {} is in the tile {} of the list", t.name(), up.name()));
            }
        }
    }
    Ok(list)
}

/// `tiles` (`cells_cut` false) or `h3`.
fn cut(args: &[String], cells_cut: bool) -> Result<(), String> {
    let input = args.first().filter(|a| !a.starts_with("--")).ok_or(USAGE)?;
    let out = PathBuf::from(opt(args, "-o").ok_or(USAGE)?);
    let boundary = Boundary::parse(&match opt(args, "--boundary") {
        Some(p) => read(p)?,
        None => WORLD.to_string(),
    })?;
    // the area of the list of a zoom or a resolution: the boundary, else the bounding box in the header of the file
    let header_box = || -> Result<Option<Boundary>, String> {
        if opt(args, "--boundary").is_some() {
            return Ok(None);
        }
        let h = pbfcut::extract::full_header(File::open(input).map_err(|e| format!("{input}: {e}"))?)?;
        let (w, s, e, n) = h.bbox.ok_or("the file has no bounding box in its header: give --boundary")?;
        let ring = format!("[[{w},{s}],[{e},{s}],[{e},{n}],[{w},{n}],[{w},{s}]]");
        Boundary::parse(&format!(r#"{{"type":"Polygon","coordinates":[{ring}]}}"#)).map(Some)
    };
    let complete: Vec<&str> = opt(args, "--complete").unwrap_or("multipolygon,boundary").split(',').filter(|t| !t.is_empty()).collect();
    let border: f64 = match opt(args, "--border") {
        Some(m) => m.parse().ok().filter(|m: &f64| *m >= 0.0).ok_or("--border: a distance in metres")?,
        None => 0.0,
    };
    if let Some(m) = opt(args, "--memory") {
        pbfcut::store::set_memory(size(m)?);
    }
    if let Some(d) = opt(args, "--temp") {
        std::fs::create_dir_all(d).map_err(|e| format!("{d}: {e}"))?;
        pbfcut::store::set_temp_dir(Some(PathBuf::from(d)));
    }
    let steps = args.iter().any(|a| a == "--steps");
    if steps {
        heap::ON.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = pbfcut::util::STEP_HOOK.set(step);
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let open = || File::open(input).map(|f| std::io::BufReader::with_capacity(1 << 20, f)).map_err(|e| format!("{input}: {e}"));
    let start = Instant::now();
    // (the name of each area, its file), and the numbers of elements of each
    let (names, counts): (Vec<(String, String)>, _) = if cells_cut {
        let res = opt(args, "--res").ok_or(format!("give --res\n{USAGE}"))?;
        let res = res.parse::<u8>().ok().and_then(|r| Resolution::try_from(r).ok()).ok_or("--res: a number from 0 to 15")?;
        let list = cells(header_box()?.as_ref().unwrap_or(&boundary), res);
        let counts = cut_to_cells(open, &boundary, &list, &out, &complete, border)?;
        (list.iter().map(|c| (c.to_string(), format!("{c}.osm.pbf"))).collect(), counts)
    } else {
        let list = match (opt(args, "--zoom"), opt(args, "--tiles")) {
            (Some(z), None) => tiles(header_box()?.as_ref().unwrap_or(&boundary), z.parse().ok().filter(|z| *z <= 30).ok_or("--zoom: a number from 0 to 30")?),
            (None, Some(p)) => tile_list(&read(p)?)?,
            _ => return Err(format!("give --zoom or --tiles\n{USAGE}")),
        };
        let counts = cut_to_tiles(open, &boundary, &list, &out, &complete, border, None)?;
        (list.iter().map(|t| (t.name(), format!("{}_{}_{}.osm.pbf", t.z, t.x, t.y))).collect(), counts)
    };
    let key = if cells_cut { "cell" } else { "tile" };
    let files: Vec<_> = names
        .iter()
        .zip(&counts)
        .filter(|(_, c)| c.nodes > 0)
        .map(|((name, file), c)| serde_json::json!({ key: name, "file": file, "nodes": c.nodes, "ways": c.ways, "relations": c.relations }))
        .collect();
    let mut report = serde_json::json!({ if cells_cut { "cells" } else { "tiles" }: names.len(), "files": files, "seconds": start.elapsed().as_secs_f64() });
    if steps {
        report["peak_heap_mib"] = serde_json::json!(heap::max() as f64 / 1048576.0);
    }
    println!("{}", serde_json::to_string_pretty(&report).expect("JSON"));
    Ok(())
}

/// `--steps`: the name of each step, its time, and the heap now and at its peak in the step, to standard error.
fn step(name: &str) {
    static LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().expect("the time");
    let now = Instant::now();
    let secs = last.map_or(0.0, |t| (now - t).as_secs_f64());
    *last = Some(now);
    let (live, peak) = heap::take_peak();
    let mib = |b: usize| b as f64 / 1048576.0;
    eprintln!("{secs:>8.3} s  heap {:>8.1} MiB, peak {:>8.1} MiB  {name}", mib(live), mib(peak));
}

/// The heap of the process: the bytes in use now and the most since the last step. It counts only after `--steps`.
mod heap {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};

    struct Counting;
    static LIVE: AtomicUsize = AtomicUsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);
    pub static ON: AtomicBool = AtomicBool::new(false);

    // SAFETY: each call goes to the system allocator with the same arguments; the counters only add and subtract
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            let p = unsafe { System.alloc(l) };
            if !p.is_null() && ON.load(Relaxed) {
                PEAK.fetch_max(LIVE.fetch_add(l.size(), Relaxed) + l.size(), Relaxed);
            }
            p
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            unsafe { System.dealloc(p, l) };
            if ON.load(Relaxed) {
                LIVE.fetch_sub(l.size(), Relaxed);
            }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
            let q = unsafe { System.realloc(p, l, new) };
            if !q.is_null() && ON.load(Relaxed) {
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

    /// (live, peak since the last call); the peak starts again at the live bytes.
    pub fn take_peak() -> (usize, usize) {
        let live = LIVE.load(Relaxed);
        let peak = PEAK.swap(live, Relaxed);
        MAX.fetch_max(peak, Relaxed);
        (live, peak)
    }

    /// The most bytes in use since the start of the count.
    pub fn max() -> usize {
        MAX.load(Relaxed).max(PEAK.load(Relaxed))
    }
    static MAX: AtomicUsize = AtomicUsize::new(0);
}

/// All elements of a file, in its order.
#[derive(Default, PartialEq)]
struct All {
    nodes: Vec<(i64, u64, u64, Vec<(String, String)>)>,
    ways: Vec<(i64, Vec<(String, String)>, Vec<i64>)>,
    rels: Vec<(i64, Vec<(String, String)>, Vec<(i64, u8, String)>)>,
}

impl RawSink for All {
    fn node<'a>(&mut self, id: i64, lat: f64, lon: f64, t: &mut dyn Iterator<Item = (&'a str, &'a str)>) {
        self.nodes.push((id, lat.to_bits(), lon.to_bits(), t.map(|(k, v)| (k.into(), v.into())).collect()));
    }
    fn way<'a>(&mut self, id: i64, t: &mut dyn Iterator<Item = (&'a str, &'a str)>, r: &mut dyn Iterator<Item = i64>) {
        self.ways.push((id, t.map(|(k, v)| (k.into(), v.into())).collect(), r.collect()));
    }
    fn relation<'a>(
        &mut self,
        id: i64,
        t: &mut dyn Iterator<Item = (&'a str, &'a str)>,
        m: &mut dyn Iterator<Item = (i64, MemberType, &'a str)>,
    ) {
        let m = m.map(|(i, ty, r)| (i, ty as u8, r.to_string())).collect();
        self.rels.push((id, t.map(|(k, v)| (k.into(), v.into())).collect(), m));
    }
}

fn elements(path: &Path) -> Result<All, String> {
    let e = |e: std::io::Error| format!("{}: {e}", path.display());
    let (mut all, mut buf) = (All::default(), Buffers::default());
    let mut r = std::io::BufReader::new(File::open(path).map_err(e)?);
    while let Some(b) = read_block(&mut r)? {
        decode_block(&b, [true; 3], &mut buf, &mut all)?;
    }
    Ok(all)
}

/// The two folders have the same `.osm.pbf` files with the same elements (the bytes can differ). Exit 1 if not.
fn compare(a: &Path, b: &Path) -> Result<(), String> {
    let names = |d: &Path| -> Result<BTreeSet<String>, String> {
        let rd = std::fs::read_dir(d).map_err(|e| format!("{}: {e}", d.display()))?;
        Ok(rd.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| n.ends_with(".osm.pbf")).collect())
    };
    let (x, y) = (names(a)?, names(b)?);
    let mut bad = 0;
    for n in x.symmetric_difference(&y) {
        println!("{n}: only in {}", if x.contains(n) { a.display() } else { b.display() });
        bad += 1;
    }
    for n in x.intersection(&y) {
        let (p, q) = (elements(&a.join(n))?, elements(&b.join(n))?);
        if p != q {
            bad += 1;
            println!(
                "{n}: nodes {} / {}, ways {} / {}, relations {} / {} (or other tags, coordinates or members)",
                p.nodes.len(),
                q.nodes.len(),
                p.ways.len(),
                q.ways.len(),
                p.rels.len(),
                q.rels.len()
            );
        }
    }
    println!("{} files, {bad} differ", x.union(&y).count());
    if bad > 0 {
        return Err(format!("{bad} files differ"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes() {
        assert_eq!(super::size("4G"), Ok(4 << 30));
        assert_eq!(super::size("512M"), Ok(512 << 20));
        assert_eq!(super::size("1.5GiB"), Ok(3 << 29));
        assert_eq!(super::size("1000"), Ok(1000));
        assert!(super::size("4T").is_err() && super::size("lots").is_err());
    }

    #[test]
    fn tile_lists() {
        assert_eq!(super::tile_list("14/8529/5974\n15/17060/11949\n").unwrap().len(), 2);
        assert!(super::tile_list("14/8529/5974\n15/17058/11948\n").is_err(), "a tile in another tile");
        assert!(super::tile_list("1/2/0").is_err(), "x out of the zoom");
    }
}
