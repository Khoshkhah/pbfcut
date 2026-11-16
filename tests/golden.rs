//! The tile files equal the reference files (tests/golden/monaco.txt: the hashes of the files of pbfcut 11276bc). A
//! failing test means that a change altered a result: find the cause. Never edit the golden file to
//! make the test pass.

use pbfcut::boundary::{Boundary, WORLD};
use pbfcut::tiles::{tiles, Tile};
use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::Path;

/// FNV-1a, 64 bits, of the blocks after the header block (the header names the program that wrote the file).
fn hash(file: &[u8]) -> String {
    let mut r = Cursor::new(file);
    pbfcut::decode::read_block(&mut r).unwrap();
    let rest = &file[r.position() as usize..];
    let h = rest.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    format!("{h:016x} {}", rest.len())
}

fn name(t: &Tile) -> String {
    format!("{}_{}_{}.osm.pbf", t.z, t.x, t.y)
}

/// The tiles of zoom 15 of Monaco, with the four children of a zoom-14 tile joined when all four are in the list.
fn mixed(b: &Boundary) -> Vec<Tile> {
    let fine = tiles(b, 15);
    let mut out = Vec::new();
    for t in &fine {
        let up = Tile { z: 14, x: t.x >> 1, y: t.y >> 1 };
        if fine.iter().filter(|c| c.x >> 1 == up.x && c.y >> 1 == up.y).count() == 4 {
            if !out.contains(&up) {
                out.push(up);
            }
        } else {
            out.push(*t);
        }
    }
    out
}

/// The golden lines of one variant: file name -> hash.
fn golden(variant: &str) -> BTreeMap<String, String> {
    let text = include_str!("golden/monaco.txt");
    let rows: BTreeMap<String, String> = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut p = l.splitn(3, ' ');
            (p.next()? == variant).then(|| (p.next().unwrap().to_string(), p.next().unwrap().to_string()))
        })
        .collect();
    assert!(!rows.is_empty(), "no golden rows for {variant}");
    rows
}

fn data() -> (std::path::PathBuf, Boundary) {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let monaco = Boundary::parse(&std::fs::read_to_string(data.join("monaco.geojson")).unwrap()).unwrap();
    (data.join("monaco.osm.pbf"), monaco)
}

#[test]
fn tile_files_equal_the_reference() {
    let (pbf, monaco) = data();
    let open = || std::fs::File::open(&pbf).map(std::io::BufReader::new).map_err(|e| e.to_string());
    for (variant, z, border) in [("tiles-z14", 14u8, 0.0), ("tiles-z15-border25", 15, 25.0)] {
        let dir = std::env::temp_dir().join(format!("pbfcut-golden-{variant}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let list = tiles(&monaco, z);
        pbfcut::tilecut::cut_to_tiles(open, &monaco, &list, &dir, &["multipolygon"], border, None).unwrap();
        let got: BTreeMap<String, String> =
            list.iter().filter_map(|t| Some((name(t), hash(&std::fs::read(dir.join(name(t))).ok()?)))).collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(got, golden(variant), "{variant}");
    }
}

#[test]
fn store_tile_files_equal_the_reference() {
    let (pbf, monaco) = data();
    let open = || std::fs::File::open(&pbf).map(std::io::BufReader::new).map_err(|e| e.to_string());
    let world = Boundary::parse(WORLD).unwrap();
    for (variant, list) in [("store-z14", tiles(&monaco, 14)), ("store-mixed-z14-z15", mixed(&monaco))] {
        let files = pbfcut::tilecut::cut_to_store(open, &world, &list, &["multipolygon", "boundary"]).unwrap();
        let mut got = BTreeMap::new();
        for (i, (t, c)) in files.tiles.iter().enumerate() {
            if c.nodes > 0 {
                let mut b = Vec::new();
                files.reader(i).read_to_end(&mut b).unwrap();
                got.insert(name(t), hash(&b));
            }
        }
        assert_eq!(got, golden(variant), "{variant}");
    }
}

#[test]
fn area_file_equals_the_reference() {
    let (pbf, monaco) = data();
    let open = || std::fs::File::open(&pbf).map(std::io::BufReader::new).map_err(|e| e.to_string());
    let (b, _) = pbfcut::extract::cut_to_pbf(open, &monaco, Vec::new()).unwrap();
    assert_eq!(golden("area")["monaco.osm.pbf"], hash(&b));
}

/// A list of three areas that do not overlap (tests/data/monaco-areas.json: a box, a polygon file with a hole, and a
/// polygon in the config, inside the hole). The reference files have the elements of `osmium extract -s smart -S
/// types=multipolygon,boundary` 1.14 with the same config.
#[test]
fn area_list_files_equal_the_reference() {
    let (pbf, _) = data();
    let open = || std::fs::File::open(&pbf).map(std::io::BufReader::new).map_err(|e| e.to_string());
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let text = std::fs::read_to_string(dir.join("monaco-areas.json")).unwrap();
    let list = pbfcut::areas::AreaList::parse(&text, "name", &|f| std::fs::read_to_string(dir.join(f)).map_err(|e| e.to_string())).unwrap();
    let out = std::env::temp_dir().join(format!("pbfcut-golden-areas-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let world = Boundary::parse(WORLD).unwrap();
    pbfcut::tilecut::cut_to_areas(open, &world, &list, &out, &["multipolygon", "boundary"]).unwrap();
    let got: BTreeMap<String, String> = list
        .areas
        .iter()
        .map(|a| format!("{}.osm.pbf", a.name))
        .filter_map(|n| Some((n.clone(), hash(&std::fs::read(out.join(&n)).ok()?))))
        .collect();
    std::fs::remove_dir_all(&out).unwrap();
    assert_eq!(got, golden("areas"));
}

/// Two areas that overlap stop the cut with an error that names them, and no area file is written.
#[test]
fn areas_that_overlap_give_an_error() {
    let (pbf, _) = data();
    let open = || std::fs::File::open(&pbf).map(std::io::BufReader::new).map_err(|e| e.to_string());
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let text = std::fs::read_to_string(dir.join("monaco-areas-overlap.json")).unwrap();
    let list = pbfcut::areas::AreaList::parse(&text, "name", &|_| Err("no files".into())).unwrap();
    let out = std::env::temp_dir().join(format!("pbfcut-golden-overlap-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let world = Boundary::parse(WORLD).unwrap();
    let e = pbfcut::tilecut::cut_to_areas(open, &world, &list, &out, &["multipolygon", "boundary"]).unwrap_err();
    let files = std::fs::read_dir(&out).unwrap().count();
    std::fs::remove_dir_all(&out).unwrap();
    assert!(e.contains("\"west\"") && e.contains("\"harbour\"") && e.contains("overlap"), "{e}");
    assert_eq!(files, 0);
}
