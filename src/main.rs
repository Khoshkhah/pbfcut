//! The command line of pbfcut (docs/reference/cli.md).
//!
//!   pbfcut tiles IN.osm.pbf (--zoom Z | --tiles LIST.txt) [OPTIONS] -o DIR
//!   pbfcut h3 IN.osm.pbf --res R [OPTIONS] -o DIR
//!   pbfcut areas IN.osm.pbf --areas AREAS.geojson|CONFIG.json [--name PROPERTY] [OPTIONS] [-o DIR]
//!   pbfcut compare DIR_A DIR_B

use pbfcut::areas::AreaList;
use pbfcut::boundary::Boundary;
use pbfcut::job::{size, tile_list, Grid};
use std::path::{Path, PathBuf};
use std::time::Instant;

const USAGE: &str = "usage:
  pbfcut tiles IN.osm.pbf (--zoom Z | --tiles LIST.txt) [OPTIONS] -o DIR
  pbfcut h3 IN.osm.pbf --res R [OPTIONS] -o DIR
  pbfcut areas IN.osm.pbf --areas AREAS.geojson|CONFIG.json [--name PROPERTY] [OPTIONS] [-o DIR]
  pbfcut compare DIR_A DIR_B
OPTIONS: [--boundary FILE | --bbox W,S,E,N] [--border METRES] [--complete TYPES] [--no-compression] [--memory SIZE]
         [--temp DIR] [--steps]
  FILE: GeoJSON, an osmosis .poly file or WKT; --border: tiles and h3 only
  --no-compression: raw blocks, not zlib (a faster cut, larger files with the same elements)";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some(kind @ ("tiles" | "h3" | "areas")) => cut(&args[1..], kind),
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

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

/// The boundary of `--boundary FILE` or `--bbox W,S,E,N`, if the arguments have one.
fn boundary(args: &[String]) -> Result<Option<Boundary>, String> {
    match (opt(args, "--boundary"), opt(args, "--bbox")) {
        (Some(p), None) => Boundary::parse(&read(p)?).map_err(|e| format!("{p}: {e}")).map(Some),
        (None, Some(b)) => Boundary::from_box(b).map(Some),
        (None, None) => Ok(None),
        _ => Err("give --boundary or --bbox, not the two".into()),
    }
}

/// The list of `--areas`: a GeoJSON FeatureCollection or an osmium config. A file that the config names is relative to
/// the config.
fn area_list(args: &[String]) -> Result<AreaList, String> {
    let path = opt(args, "--areas").ok_or(format!("give --areas\n{USAGE}"))?;
    let base = Path::new(path).parent().unwrap_or(Path::new(""));
    AreaList::parse(&read(path)?, opt(args, "--name").unwrap_or("name"), &|f| read(&base.join(f).to_string_lossy()))
        .map_err(|e| format!("{path}: {e}"))
}

/// `tiles`, `h3` or `areas` (`kind`).
fn cut(args: &[String], kind: &str) -> Result<(), String> {
    let input = args.first().filter(|a| !a.starts_with("--")).ok_or(USAGE)?;
    let areas = if kind == "areas" { Some(area_list(args)?) } else { None };
    // an osmium config can give the output folder
    let out = PathBuf::from(opt(args, "-o").or(areas.as_ref().and_then(|a| a.directory.as_deref())).ok_or(USAGE)?);
    let given = boundary(args)?;
    let complete: Vec<&str> = opt(args, "--complete").unwrap_or("multipolygon,boundary").split(',').filter(|t| !t.is_empty()).collect();
    let border: f64 = match opt(args, "--border") {
        Some(_) if kind == "areas" => return Err("--border: only for tiles and h3".into()),
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
    let grid = match (kind, areas) {
        (_, Some(list)) => Grid::Areas(list),
        ("h3", _) => {
            let res = opt(args, "--res").ok_or(format!("give --res\n{USAGE}"))?;
            Grid::Res(res.parse().map_err(|_| "--res: a number from 0 to 15")?)
        }
        _ => match (opt(args, "--zoom"), opt(args, "--tiles")) {
            (Some(z), None) => Grid::Zoom(z.parse().map_err(|_| "--zoom: a number from 0 to 30")?),
            (None, Some(p)) => Grid::Tiles(tile_list(&read(p)?)?),
            _ => return Err(format!("give --zoom or --tiles\n{USAGE}")),
        },
    };
    let start = Instant::now();
    let compress = !args.iter().any(|a| a == "--no-compression");
    let r = pbfcut::job::cut(Path::new(input), &out, &grid, given, &complete, border, compress)?;
    let key = r.key;
    let files: Vec<_> = r
        .files
        .iter()
        .map(|w| serde_json::json!({ key: w.name, "file": w.file, "nodes": w.counts.nodes, "ways": w.counts.ways, "relations": w.counts.relations }))
        .collect();
    let mut report = serde_json::json!({ format!("{key}s"): r.listed, "files": files, "seconds": start.elapsed().as_secs_f64() });
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

/// The two folders have the same `.osm.pbf` files with the same elements (the bytes can differ). Exit 1 if not.
fn compare(a: &Path, b: &Path) -> Result<(), String> {
    let (n, lines) = pbfcut::job::compare(a, b)?;
    for l in &lines {
        println!("{l}");
    }
    println!("{n} files, {} differ", lines.len());
    if !lines.is_empty() {
        return Err(format!("{} files differ", lines.len()));
    }
    Ok(())
}
