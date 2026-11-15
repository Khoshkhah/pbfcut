//! The command line of pbfcut (docs/reference/cli.md).
//!
//!   pbfcut tiles IN.osm.pbf (--zoom Z | --tiles LIST.txt) [--boundary AREA.geojson] [--complete TYPES] [--steps] -o DIR
//!   pbfcut compare DIR_A DIR_B

use pbfcut::boundary::{Boundary, WORLD};
use pbfcut::decode::{decode_block, read_block, Buffers};
use pbfcut::elements::{MemberType, RawSink};
use pbfcut::tilecut::cut_to_tiles;
use pbfcut::tiles::{tiles, Tile};
use std::collections::BTreeSet;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Instant;

const USAGE: &str = "usage:
  pbfcut tiles IN.osm.pbf (--zoom Z | --tiles LIST.txt) [--boundary AREA.geojson] [--complete TYPES] [--steps] -o DIR
  pbfcut compare DIR_A DIR_B";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("tiles") => cut_tiles(&args[1..]),
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

fn cut_tiles(args: &[String]) -> Result<(), String> {
    let input = args.first().filter(|a| !a.starts_with("--")).ok_or(USAGE)?;
    let out = PathBuf::from(opt(args, "-o").ok_or(USAGE)?);
    let boundary = Boundary::parse(&match opt(args, "--boundary") {
        Some(p) => read(p)?,
        None => WORLD.to_string(),
    })?;
    let list = match (opt(args, "--zoom"), opt(args, "--tiles")) {
        (Some(z), None) => {
            let z = z.parse().ok().filter(|z| *z <= 30).ok_or("--zoom: a number from 0 to 30")?;
            // no boundary: the tiles of the bounding box in the header of the file
            let header_box;
            let area = match opt(args, "--boundary") {
                Some(_) => &boundary,
                None => {
                    let h = pbfcut::extract::full_header(File::open(input).map_err(|e| format!("{input}: {e}"))?)?;
                    let (w, s, e, n) = h.bbox.ok_or("the file has no bounding box in its header: give --boundary")?;
                    let ring = format!("[[{w},{s}],[{e},{s}],[{e},{n}],[{w},{n}],[{w},{s}]]");
                    header_box = Boundary::parse(&format!(r#"{{"type":"Polygon","coordinates":[{ring}]}}"#))?;
                    &header_box
                }
            };
            tiles(area, z)
        }
        (None, Some(p)) => tile_list(&read(p)?)?,
        _ => return Err(format!("give --zoom or --tiles\n{USAGE}")),
    };
    let complete: Vec<&str> = opt(args, "--complete").unwrap_or("multipolygon,boundary").split(',').filter(|t| !t.is_empty()).collect();
    if args.iter().any(|a| a == "--steps") {
        let _ = pbfcut::util::STEP_HOOK.set(step);
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let open = || File::open(input).map(|f| std::io::BufReader::with_capacity(1 << 20, f)).map_err(|e| format!("{input}: {e}"));
    let start = Instant::now();
    let counts = cut_to_tiles(open, &boundary, &list, &out, &complete, 0.0, None)?;
    let files: Vec<_> = list
        .iter()
        .zip(&counts)
        .filter(|(_, c)| c.nodes > 0)
        .map(|(t, c)| {
            serde_json::json!({
                "tile": t.name(),
                "file": format!("{}_{}_{}.osm.pbf", t.z, t.x, t.y),
                "nodes": c.nodes, "ways": c.ways, "relations": c.relations,
            })
        })
        .collect();
    let report = serde_json::json!({ "tiles": list.len(), "files": files, "seconds": start.elapsed().as_secs_f64() });
    println!("{}", serde_json::to_string_pretty(&report).expect("JSON"));
    Ok(())
}

/// `--steps`: the name of each step and its time, to standard error.
fn step(name: &str) {
    static LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().expect("the time");
    let now = Instant::now();
    let secs = last.map_or(0.0, |t| (now - t).as_secs_f64());
    *last = Some(now);
    eprintln!("{secs:>8.3} s  {name}");
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
