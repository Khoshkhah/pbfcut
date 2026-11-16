//! The cell cut: each cell file has the elements of the cut of one area (`extract::cut_where`, the reference) whose nodes
//! are inside the boundary and in the cell. `pbfcut compare` makes the check. The hashes of the cell files are in
//! tests/golden/monaco_cells.txt. A failing test means that a change altered a result: find the cause. Never edit the
//! golden file to make the test pass.

use pbfcut::boundary::Boundary;
use pbfcut::cells::{cell_of, cells, Resolution};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn data() -> (PathBuf, Boundary) {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let monaco = Boundary::parse(&std::fs::read_to_string(data.join("monaco.geojson")).unwrap()).unwrap();
    (data.join("monaco.osm.pbf"), monaco)
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pbfcut-cells-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// FNV-1a, 64 bits, of the blocks after the header block (as in tests/golden.rs).
fn hash(file: &[u8]) -> String {
    let mut r = Cursor::new(file);
    pbfcut::decode::read_block(&mut r).unwrap();
    let rest = &file[r.position() as usize..];
    let h = rest.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    format!("{h:016x} {}", rest.len())
}

/// With no boundary: inside a boundary, the cut of many areas also gives the ways at a node outside the boundary to one
/// "home" area (the first area of these ways), and the cut of one area does not have this rule.
#[test]
fn cell_files_equal_the_cut_of_each_cell() {
    let (pbf, _) = data();
    let open = || std::fs::File::open(&pbf).map(std::io::BufReader::new).map_err(|e| e.to_string());
    let res = Resolution::Eight;
    // the box of the nodes of the file, a little larger
    let area = r#"{"type":"Polygon","coordinates":[[[7.38,43.51],[7.51,43.51],[7.51,43.77],[7.38,43.77],[7.38,43.51]]]}"#;
    let list = cells(&Boundary::parse(area).unwrap(), res);
    let world = Boundary::parse(pbfcut::boundary::WORLD).unwrap();
    let (a, b) = (temp("cut"), temp("reference"));
    let counts = pbfcut::tilecut::cut_to_cells(open, &world, &list, &a, &["multipolygon"], 0.0).unwrap();
    let mut files = 0;
    for (c, n) in list.iter().zip(&counts).filter(|(_, n)| n.nodes > 0) {
        let inside = |lon: f64, lat: f64| cell_of(res, lon, lat) == Some(*c);
        let (bytes, want) = pbfcut::extract::cut_where(open, &inside, Vec::new()).unwrap();
        assert_eq!((n.nodes, n.ways, n.relations), (want.nodes, want.ways, want.relations), "{c}");
        std::fs::write(b.join(format!("{c}.osm.pbf")), bytes).unwrap();
        files += 1;
    }
    let nodes: u64 = counts.iter().map(|n| n.nodes as u64).sum();
    assert!(files > 20, "{files} cell files");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pbfcut")).arg("compare").arg(&a).arg(&b).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    eprintln!("{} cells, {files} files, {nodes} nodes: {}", list.len(), String::from_utf8_lossy(&out.stdout).trim());
    std::fs::remove_dir_all(&a).unwrap();
    std::fs::remove_dir_all(&b).unwrap();
}

#[test]
fn cell_files_equal_the_reference() {
    let (pbf, monaco) = data();
    let open = || std::fs::File::open(&pbf).map(std::io::BufReader::new).map_err(|e| e.to_string());
    let golden = include_str!("golden/monaco_cells.txt");
    for (variant, border) in [("cells-r9", 0.0), ("cells-r9-border25", 25.0)] {
        let dir = temp(variant);
        let list = cells(&monaco, Resolution::Nine);
        pbfcut::tilecut::cut_to_cells(open, &monaco, &list, &dir, &["multipolygon", "boundary"], border).unwrap();
        let got: BTreeMap<String, String> = list
            .iter()
            .filter_map(|c| {
                let name = format!("{c}.osm.pbf");
                Some((name.clone(), hash(&std::fs::read(dir.join(&name)).ok()?)))
            })
            .collect();
        std::fs::remove_dir_all(&dir).unwrap();
        let want: BTreeMap<String, String> = golden
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let mut p = l.splitn(3, ' ');
                (p.next()? == variant).then(|| (p.next().unwrap().to_string(), p.next().unwrap().to_string()))
            })
            .collect();
        assert!(!want.is_empty(), "no golden rows for {variant}");
        assert_eq!(got, want, "{variant}");
    }
}

#[test]
fn a_wide_border_or_two_resolutions_is_an_error() {
    let (pbf, monaco) = data();
    let open = || std::fs::File::open(&pbf).map(std::io::BufReader::new).map_err(|e| e.to_string());
    let list = cells(&monaco, Resolution::Nine);
    let dir = temp("wide");
    assert!(pbfcut::tilecut::cut_to_cells(open, &monaco, &list, &dir, &[], 150.0).is_err());
    // cells of two resolutions
    let mixed = [list[0], list[1].parent(Resolution::Eight).unwrap()];
    assert!(pbfcut::tilecut::cut_to_cells(open, &monaco, &mixed, &dir, &[], 0.0).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
