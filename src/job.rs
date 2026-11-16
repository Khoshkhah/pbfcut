//! The jobs of the command line: one cut to a folder, and the comparison of two folders. The command (`src/main.rs`)
//! and the Python binding (`python/`) call these functions, so that they give the same files.

use crate::areas::AreaList;
use crate::boundary::{Boundary, WORLD};
use crate::cells::{cells, Resolution};
use crate::decode::{decode_block, read_block, Buffers};
use crate::elements::{MemberType, RawSink};
use crate::extract::CutCounts;
use crate::tilecut::{cut_to_areas, cut_to_cells, cut_to_tiles};
use crate::tiles::{tiles, Tile};
use std::collections::BTreeSet;
use std::fs::File;
use std::path::Path;

/// The outputs of one cut.
pub enum Grid {
    /// The tiles of one zoom (0 to 30) in the boundary.
    Zoom(u8),
    /// The tiles of a list (see [`tile_list`]).
    Tiles(Vec<Tile>),
    /// The H3 cells of one resolution (0 to 15) in the boundary.
    Res(u8),
    /// The areas of a list.
    Areas(AreaList),
}

/// One file that the cut wrote.
pub struct Written {
    /// The tile `z/x/y`, the cell index or the area name.
    pub name: String,
    /// The file name in the output folder.
    pub file: String,
    pub counts: CutCounts,
    /// The size of the file.
    pub bytes: u64,
}

/// The result of [`cut`].
pub struct Report {
    /// `tile`, `cell` or `area`.
    pub key: &'static str,
    /// The number of tiles, cells or areas of the cut (also the ones with no file).
    pub listed: usize,
    /// The files that the cut wrote: the tiles, cells or areas with one node or more, in the order of the list.
    pub files: Vec<Written>,
}

/// Cut `input` to `grid` in the folder `out` (the cut makes the folder). `boundary`: the area of the cut; with no
/// boundary, the zoom and the resolution take the bounding box in the header of the file. `complete`: the relation
/// types that the cut keeps whole. `border_m`: the border in metres (tiles and cells only).
pub fn cut(input: &Path, out: &Path, grid: &Grid, boundary: Option<Boundary>, complete: &[&str], border_m: f64) -> Result<Report, String> {
    let name = input.display();
    if border_m.is_nan() || border_m < 0.0 {
        return Err("border: a distance in metres".into());
    }
    if border_m > 0.0 && matches!(grid, Grid::Areas(_)) {
        return Err("border: only for tiles and h3".into());
    }
    let has_boundary = boundary.is_some();
    let boundary = boundary.map_or_else(|| Boundary::parse(WORLD), Ok)?;
    // the area of the list of a zoom or a resolution: the boundary, else the bounding box in the header of the file
    let header_box = || -> Result<Option<Boundary>, String> {
        if has_boundary {
            return Ok(None);
        }
        let h = crate::extract::full_header(File::open(input).map_err(|e| format!("{name}: {e}"))?)?;
        let (w, s, e, n) = h.bbox.ok_or("the file has no bounding box in its header: give a boundary")?;
        Boundary::from_box(&format!("{w},{s},{e},{n}")).map(Some)
    };
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let open = || File::open(input).map(|f| std::io::BufReader::with_capacity(1 << 20, f)).map_err(|e| format!("{name}: {e}"));
    // (the name of each tile, cell or area, its file), and the numbers of elements of each
    let (key, names, counts): (_, Vec<(String, String)>, _) = match grid {
        Grid::Areas(list) => {
            let counts = cut_to_areas(open, &boundary, list, out, complete)?;
            ("area", list.areas.iter().map(|a| (a.name.clone(), format!("{}.osm.pbf", a.name))).collect(), counts)
        }
        Grid::Res(r) => {
            let res = Resolution::try_from(*r).map_err(|_| "res: a number from 0 to 15")?;
            let list = cells(header_box()?.as_ref().unwrap_or(&boundary), res);
            let counts = cut_to_cells(open, &boundary, &list, out, complete, border_m)?;
            ("cell", list.iter().map(|c| (c.to_string(), format!("{c}.osm.pbf"))).collect(), counts)
        }
        Grid::Zoom(_) | Grid::Tiles(_) => {
            let list = match grid {
                Grid::Zoom(z) if *z <= 30 => tiles(header_box()?.as_ref().unwrap_or(&boundary), *z),
                Grid::Zoom(_) => return Err("zoom: a number from 0 to 30".into()),
                Grid::Tiles(l) => l.clone(),
                _ => unreachable!(),
            };
            let counts = cut_to_tiles(open, &boundary, &list, out, complete, border_m, None)?;
            ("tile", list.iter().map(|t| (t.name(), format!("{}_{}_{}.osm.pbf", t.z, t.x, t.y))).collect(), counts)
        }
    };
    let listed = names.len();
    let mut files = Vec::new();
    for ((name, file), counts) in names.into_iter().zip(counts).filter(|(_, c)| c.nodes > 0) {
        let bytes = std::fs::metadata(out.join(&file)).map_err(|e| format!("{file}: {e}"))?.len();
        files.push(Written { name, file, counts, bytes });
    }
    Ok(Report { key, listed, files })
}

/// The tiles of a list file: one `z/x/y` on each line. A tile must not be in another tile of the list.
pub fn tile_list(text: &str) -> Result<Vec<Tile>, String> {
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

/// A size such as `4G`, `512M`, `64K` or a number of bytes.
pub fn size(text: &str) -> Result<usize, String> {
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

/// Compare the `.osm.pbf` files of two folders by their elements (the bytes can differ). Gives the number of files and
/// one line for each file that differs or is in one folder only. No lines: the folders are equal.
pub fn compare(a: &Path, b: &Path) -> Result<(usize, Vec<String>), String> {
    let names = |d: &Path| -> Result<BTreeSet<String>, String> {
        let rd = std::fs::read_dir(d).map_err(|e| format!("{}: {e}", d.display()))?;
        Ok(rd.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| n.ends_with(".osm.pbf")).collect())
    };
    let (x, y) = (names(a)?, names(b)?);
    let mut lines = Vec::new();
    for n in x.symmetric_difference(&y) {
        lines.push(format!("{n}: only in {}", if x.contains(n) { a.display() } else { b.display() }));
    }
    for n in x.intersection(&y) {
        let (p, q) = (elements(&a.join(n))?, elements(&b.join(n))?);
        if p != q {
            lines.push(format!(
                "{n}: nodes {} / {}, ways {} / {}, relations {} / {} (or other tags, coordinates or members)",
                p.nodes.len(),
                q.nodes.len(),
                p.ways.len(),
                q.ways.len(),
                p.rels.len(),
                q.rels.len()
            ));
        }
    }
    Ok((x.union(&y).count(), lines))
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
