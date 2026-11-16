//! H3 cells of one resolution: the areas of the cell cut, as [`crate::tiles`] gives the map tiles. The cell of a point is
//! `h3o`'s `LatLng::to_cell`. `h3o` builds without its `std` feature, so its math is `libm` (pure Rust): the cell of a
//! point does not change with the target.

use crate::boundary::Boundary;
use crate::util::FxHashSet;
pub use h3o::{CellIndex, Resolution};
use h3o::LatLng;

/// The cell of resolution `res` that holds the point. None for a point that is not a valid place (not finite).
pub fn cell_of(res: Resolution, lon: f64, lat: f64) -> Option<CellIndex> {
    Some(LatLng::new(lat, lon).ok()?.to_cell(res))
}

/// The corners of the cell, as (longitude, latitude).
fn corners(c: CellIndex) -> Vec<(f64, f64)> {
    c.boundary().iter().map(|p| (p.lng(), p.lat())).collect()
}

/// West, south, east, north of the corners of the cell, in degrees.
// ponytail: a cell across the antimeridian gets a box of all longitudes; correct, but it touches more of the boundary
pub fn bounds(c: CellIndex) -> (f64, f64, f64, f64) {
    let p = corners(c);
    let b = p.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, &(x, y)| (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y)));
    if b.2 - b.0 > 180.0 {
        (-180.0, b.1, 180.0, b.3)
    } else {
        b
    }
}

/// The cells of resolution `res` that the boundary touches (the box of the corners of the cell touches it), sorted by
/// their index. The cut gives no file to a cell with no node inside, so a cell too many costs only a little time.
pub fn cells(boundary: &Boundary, res: Resolution) -> Vec<CellIndex> {
    let (w, s, e, n) = boundary.bbox();
    let in_box = |c: CellIndex| {
        let (cw, cs, ce, cn) = bounds(c);
        ce >= w && cw <= e && cn >= s && cs <= n
    };
    // a flood from the cell at the middle of the box, through all cells that touch the box (they are connected)
    let mut seen: FxHashSet<CellIndex> = FxHashSet::default();
    let mut todo: Vec<CellIndex> = [((w + e) / 2.0, (s + n) / 2.0), (w, s), (e, n)]
        .into_iter()
        .filter_map(|(x, y)| cell_of(res, x.clamp(-180.0, 180.0), y.clamp(-90.0, 90.0)))
        .collect();
    let mut out = Vec::new();
    while let Some(c) = todo.pop() {
        if !seen.insert(c) || !in_box(c) {
            continue;
        }
        let (cw, cs, ce, cn) = bounds(c);
        if boundary.touches_box(cw, cs, ce, cn) {
            out.push(c);
        }
        todo.extend(c.grid_disk_safe(1).filter(|d| !seen.contains(d)));
    }
    out.sort_unstable();
    out
}

/// Is the point within `m` metres of the cell `c` (inside it, or at most `m` from its edges)? The distance is on a flat
/// map at the latitude of the point, which is correct for a border of some metres.
pub fn near_cell(c: CellIndex, lon: f64, lat: f64, m: f64) -> bool {
    let ky = 111_320.0;
    let kx = ky * lat.to_radians().cos().abs().max(1e-9);
    let p: Vec<(f64, f64)> = corners(c).iter().map(|&(x, y)| ((x - lon) * kx, (y - lat) * ky)).collect();
    let mut inside = false;
    for k in 0..p.len() {
        let (a, b) = (p[k], p[(k + 1) % p.len()]);
        // the distance from the point (0, 0) to the edge a-b
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let t = (-(a.0 * dx + a.1 * dy) / (dx * dx + dy * dy).max(1e-18)).clamp(0.0, 1.0);
        if (a.0 + t * dx).hypot(a.1 + t * dy) <= m {
            return true;
        }
        if (a.1 > 0.0) != (b.1 > 0.0) && 0.0 < (b.0 - a.0) * (0.0 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_is_in_its_cell() {
        // the example cell of the H3 manual: resolution 9 at (37.7752702151959, -122.418307270836)
        let c = cell_of(Resolution::Nine, -122.418307270836, 37.775_270_215_195_9).unwrap();
        assert_eq!(c.to_string(), "8928308280fffff");
        assert!(near_cell(c, -122.418307270836, 37.775_270_215_195_9, 0.0));
        let (w, s, e, n) = bounds(c);
        assert!(w < -122.4183 && -122.4183 < e && s < 37.7752 && 37.7752 < n);
    }

    #[test]
    fn the_cells_of_a_box() {
        let b = Boundary::parse(
            r#"{"type":"Polygon","coordinates":[[[7.40,43.72],[7.44,43.72],[7.44,43.76],[7.40,43.76],[7.40,43.72]]]}"#,
        )
        .unwrap();
        let list = cells(&b, Resolution::Eight);
        assert!(list.windows(2).all(|p| p[0] < p[1]));
        // every point of a grid in the box is in a listed cell
        for i in 0..=20 {
            for j in 0..=20 {
                let (x, y) = (7.40 + 0.002 * i as f64, 43.72 + 0.002 * j as f64);
                assert!(list.contains(&cell_of(Resolution::Eight, x, y).unwrap()), "{x} {y}");
            }
        }
    }

    #[test]
    fn a_point_near_a_neighbour() {
        let c = cell_of(Resolution::Ten, 7.42, 43.73).unwrap();
        for d in c.grid_disk_safe(1).filter(|&d| d != c) {
            // a corner of the neighbour, shared with `c`, is near both
            let (x, y) = corners(d).into_iter().find(|&(x, y)| near_cell(c, x, y, 0.01)).unwrap();
            assert!(near_cell(d, x, y, 0.01));
        }
        // the middle of the cell is not within 1 m of a neighbour (an edge of resolution 10 is about 76 m)
        let mid = LatLng::from(c);
        assert!(c.grid_disk_safe(1).filter(|&d| d != c).all(|d| !near_cell(d, mid.lng(), mid.lat(), 1.0)));
    }
}
