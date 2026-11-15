//! The hashes of the tile files of duckOSM's tile cut on Monaco (the variants of tests/golden.rs).

use duckosm_core::boundary::Boundary;
use duckosm_core::tiles::{tiles, Tile};
use std::io::{Cursor, Read};
use std::path::Path;

/// FNV-1a, 64 bits, of the blocks after the header block (the header names the program that wrote the file).
fn hash(file: &[u8]) -> String {
    let mut r = Cursor::new(file);
    let _header = duckosm_core::pbfraw::read_block(&mut r).unwrap();
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
        let children = fine.iter().filter(|c| c.x >> 1 == up.x && c.y >> 1 == up.y).count();
        if children == 4 {
            if !out.contains(&up) {
                out.push(up);
            }
        } else {
            out.push(*t);
        }
    }
    out
}

fn main() {
    let data = std::env::args().nth(1).expect("the folder of monaco.osm.pbf and monaco.geojson");
    let data = Path::new(&data);
    let pbf = data.join("monaco.osm.pbf");
    let monaco = Boundary::parse(&std::fs::read_to_string(data.join("monaco.geojson")).unwrap()).unwrap();
    let world = Boundary::parse(duckosm_core::quack::WORLD).unwrap();
    let open = || std::fs::File::open(&pbf).map(std::io::BufReader::new).map_err(|e| e.to_string());
    let tmp = std::env::temp_dir().join(format!("pbfcut-golden-{}", std::process::id()));
    println!("# the tile cut of duckOSM e3ce06a on tests/data/monaco.osm.pbf: variant, file, FNV-1a 64 and size of the blocks after the header");
    // the build by tiles: the Monaco boundary, multipolygons complete, with and without a border
    for (variant, z, border) in [("tiles-z14", 14u8, 0.0), ("tiles-z15-border25", 15, 25.0)] {
        let dir = tmp.join(variant);
        std::fs::create_dir_all(&dir).unwrap();
        let list = tiles(&monaco, z);
        duckosm_core::tilecut::cut_to_tiles(open, &monaco, &list, &dir, true, border).unwrap();
        for t in &list {
            if let Ok(b) = std::fs::read(dir.join(name(t))) {
                println!("{variant} {} {}", name(t), hash(&b));
            }
        }
    }
    // quack: the world boundary, multipolygons and boundaries complete, no zlib; one zoom and mixed zooms
    for (variant, list) in [("quack-z14", tiles(&monaco, 14)), ("quack-mixed-z14-z15", mixed(&monaco))] {
        let files = duckosm_core::tilecut::cut_to_store(open, &world, &list, &["multipolygon", "boundary"]).unwrap();
        for (i, (t, c)) in files.tiles.iter().enumerate() {
            if c.nodes > 0 {
                let mut b = Vec::new();
                files.reader(i).read_to_end(&mut b).unwrap();
                println!("{variant} {} {}", name(t), hash(&b));
            }
        }
    }
    // one area: the cut of the Monaco boundary
    let (b, _) = duckosm_core::cutstream::cut_to_pbf(open, &monaco, Vec::new()).unwrap();
    println!("area monaco.osm.pbf {}", hash(&b));
    let _ = std::fs::remove_dir_all(&tmp);
}
