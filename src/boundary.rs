//! A boundary: the area of a cut. The cut keeps the nodes inside it. [`Boundary::parse`] reads these formats and finds the
//! format from the text:
//!
//! - GeoJSON: a Polygon or a MultiPolygon, also inside a Feature or a FeatureCollection, or a Feature with a `bbox`
//!   member and no geometry;
//! - an osmosis polygon file (`.poly`, as on Geofabrik);
//! - WKT: `POLYGON` or `MULTIPOLYGON`;
//! - a box: `W,S,E,N` (west, south, east, north, in degrees).

use serde_json::Value;

type Ring = Vec<(f64, f64)>;

/// A boundary that holds every point: the cut of a whole file.
pub const WORLD: &str = r#"{"type":"Polygon","coordinates":[[[-181,-91],[181,-91],[181,91],[-181,91],[-181,-91]]]}"#;

pub struct Boundary {
    /// Each polygon: the outer ring and its holes, as (longitude, latitude).
    polygons: Vec<Vec<Ring>>,
    /// west, south, east, north
    bbox: (f64, f64, f64, f64),
    /// The box of the whole area (`bbox` is the box of the part in the tile, with [`Boundary::within_tile`]).
    area_bbox: (f64, f64, f64, f64),
    /// The edges by latitude band, for [`Boundary::contains`].
    bands: Bands,
    /// `properties.name` of the first Feature, if any.
    pub name: Option<String>,
    /// The build of one tile of a large build: only the part of the area in this tile ([`Boundary::within_tile`]).
    tile: Option<crate::tiles::Tile>,
    /// The build of one tile: the positions of its home nodes, outside the area ([`Boundary::with_points`]).
    points: crate::util::FxHashSet<(u64, u64)>,
}

/// An edge of a ring: the number of its polygon, and its two ends.
type Edge = (usize, (f64, f64), (f64, f64));

/// The edges of the polygons by latitude band. A point needs only the edges whose latitude range holds its own latitude: an
/// edge that does not reach it can neither cross the ray of the point nor have the point on it. Step 0 tests each node of the
/// region file in the box of the area (US South for Kentucky: tens of millions), and testing all edges of the border was
/// its largest cost.
struct Bands {
    south: f64,
    per_degree: f64,
    edges: Vec<Vec<Edge>>,
}

impl Bands {
    fn new(polygons: &[Vec<Ring>], south: f64, north: f64) -> Bands {
        let count = polygons.iter().flatten().map(Vec::len).sum::<usize>().clamp(1, 1 << 16);
        let per_degree = if north > south { count as f64 / (north - south) } else { 0.0 };
        let mut b = Bands { south, per_degree, edges: vec![Vec::new(); count] };
        for (p, rings) in polygons.iter().enumerate() {
            for ring in rings {
                for k in 0..ring.len() {
                    let (a, c) = (ring[k], ring[(k + 1) % ring.len()]);
                    for i in b.band(a.1.min(c.1))..=b.band(a.1.max(c.1)) {
                        b.edges[i].push((p, a, c));
                    }
                }
            }
        }
        b
    }

    /// The band of a latitude: the same function for the edges and the points, and it never gets smaller when the latitude
    /// gets larger, so an edge is in the band of each latitude of its range.
    fn band(&self, lat: f64) -> usize {
        (((lat - self.south) * self.per_degree) as usize).min(self.edges.len() - 1)
    }
}

impl Boundary {
    /// A boundary from GeoJSON, a polygon file, WKT or a box `W,S,E,N`: the first character of the text gives the format.
    pub fn parse(text: &str) -> Result<Boundary, String> {
        let t = text.trim_start();
        if t.starts_with('{') {
            let v: Value = serde_json::from_str(t).map_err(|e| format!("boundary: {e}"))?;
            return Boundary::from_geojson(&v);
        }
        let head = t.get(..12).unwrap_or(t).to_ascii_uppercase();
        if head.starts_with("POLYGON") || head.starts_with("MULTIPOLYGON") {
            return Boundary::from_polygons(wkt(t)?, None);
        }
        if t.starts_with(|c: char| c == '-' || c.is_ascii_digit()) && !t.trim().contains('\n') {
            return Boundary::from_box(t);
        }
        let (polygons, name) = poly(t)?;
        Boundary::from_polygons(polygons, name)
    }

    /// A boundary from a GeoJSON value.
    pub fn from_geojson(v: &Value) -> Result<Boundary, String> {
        let mut polygons = Vec::new();
        let mut name = None;
        collect(v, &mut polygons, &mut name)?;
        Boundary::from_polygons(polygons, name)
    }

    /// A box from the text `W,S,E,N` (west, south, east, north, in degrees).
    pub fn from_box(text: &str) -> Result<Boundary, String> {
        let v: Vec<f64> = text.split(',').map(|n| n.trim().parse::<f64>()).collect::<Result<_, _>>().map_err(|_| format!("boundary: not a box W,S,E,N: {text}"))?;
        let [w, s, e, n] = v[..] else { return Err(format!("boundary: not a box W,S,E,N: {text}")) };
        if w >= e || s >= n {
            return Err(format!("boundary: the box {text} is empty (W must be less than E and S less than N)"));
        }
        Boundary::from_polygons(vec![vec![rect(w, s, e, n)]], None)
    }

    fn from_polygons(polygons: Vec<Vec<Ring>>, name: Option<String>) -> Result<Boundary, String> {
        if polygons.is_empty() {
            return Err("boundary: no polygon".into());
        }
        let pts = polygons.iter().flatten().flatten();
        let bbox = pts.fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, &(x, y)| {
            (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y))
        });
        let bands = Bands::new(&polygons, bbox.1, bbox.3);
        Ok(Boundary {
            polygons,
            bbox,
            area_bbox: bbox,
            bands,
            name,
            tile: None,
            points: Default::default(),
        })
    }

    /// The part of the area in tile `t`: a point is inside when it is inside the area and its tile is `t` (`Tile::of`, the
    /// rule of the tile cut). The build of a tile takes the tile file with this boundary.
    pub fn within_tile(mut self, t: crate::tiles::Tile) -> Boundary {
        let (w, s, e, n) = t.bounds();
        let b = self.bbox;
        self.bbox = (b.0.max(w), b.1.max(s), b.2.min(e), b.3.min(n));
        self.tile = Some(t);
        self
    }

    /// Points that are inside too: the home nodes of a tile (outside the area; the tile holds them, see `tilecut`).
    pub fn with_points(mut self, pts: impl IntoIterator<Item = (f64, f64)>) -> Boundary {
        self.points.extend(pts.into_iter().map(|(x, y)| (x.to_bits(), y.to_bits())));
        self
    }

    /// Is the point inside? A hole of a polygon is outside (the even-odd rule over all rings of a polygon). A
    /// point on a ring counts as inside, as in osmium's cut: a border often runs through the nodes of a road.
    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        if !self.points.is_empty() && self.points.contains(&(lon.to_bits(), lat.to_bits())) {
            return true;
        }
        let (w, s, e, n) = self.bbox;
        if lon < w || lon > e || lat < s || lat > n {
            return false;
        }
        if self.tile.is_some_and(|t| crate::tiles::Tile::of(t.z, lon, lat) != t) {
            return false;
        }
        self.in_polygons(lon, lat)
    }

    /// Is the point inside the whole area (not only the part in the tile, and not the home points)?
    pub fn in_area(&self, lon: f64, lat: f64) -> bool {
        let (w, s, e, n) = self.area_bbox;
        if lon < w || lon > e || lat < s || lat > n {
            return false;
        }
        self.in_polygons(lon, lat)
    }

    fn in_polygons(&self, lon: f64, lat: f64) -> bool {
        if self.polygons.len() > 64 {
            return self.contains_all(lon, lat);
        }
        // the edges of the point's band only; the even-odd rule of each polygon is one bit
        let mut inside = 0u64;
        for &(p, a, b) in &self.bands.edges[self.bands.band(lat)] {
            if on_segment((lon, lat), a, b) {
                return true;
            }
            if (a.1 > lat) != (b.1 > lat) && lon < (b.0 - a.0) * (lat - a.1) / (b.1 - a.1) + a.0 {
                inside ^= 1 << p;
            }
        }
        inside != 0
    }

    /// [`Boundary::contains`] with all edges of all polygons (more than 64 polygons, and the test of the bands).
    fn contains_all(&self, lon: f64, lat: f64) -> bool {
        let (w, s, e, n) = self.bbox;
        if lon < w || lon > e || lat < s || lat > n {
            return false;
        }
        self.polygons.iter().any(|rings| {
            let mut inside = false;
            for ring in rings {
                for k in 0..ring.len() {
                    let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                    if on_segment((lon, lat), a, b) {
                        return true;
                    }
                    if (a.1 > lat) != (b.1 > lat)
                        && lon < (b.0 - a.0) * (lat - a.1) / (b.1 - a.1) + a.0
                    {
                        inside = !inside;
                    }
                }
            }
            inside
        })
    }

    /// Does the boundary touch the box from (`w`, `s`) to (`e`, `n`): a corner of the box inside, or an edge of a ring in the
    /// box or across it?
    pub fn touches_box(&self, w: f64, s: f64, e: f64, n: f64) -> bool {
        let (bw, bs, be, bn) = self.bbox;
        if be < w || bw > e || bn < s || bs > n {
            return false;
        }
        if [(w, s), (e, s), (e, n), (w, n)].iter().any(|&(x, y)| self.contains(x, y)) {
            return true;
        }
        // the segment a-b clipped to the box (Liang-Barsky): something is left
        let crosses = |a: (f64, f64), b: (f64, f64)| {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let (mut t0, mut t1) = (0.0f64, 1.0f64);
            for (p, q) in [(-dx, a.0 - w), (dx, e - a.0), (-dy, a.1 - s), (dy, n - a.1)] {
                if p == 0.0 {
                    if q < 0.0 {
                        return false;
                    }
                } else {
                    let r = q / p;
                    if p < 0.0 {
                        t0 = t0.max(r);
                    } else {
                        t1 = t1.min(r);
                    }
                }
            }
            t0 <= t1
        };
        self.polygons.iter().flatten().any(|ring| (0..ring.len()).any(|k| crosses(ring[k], ring[(k + 1) % ring.len()])))
    }

    /// west, south, east, north
    pub fn bbox(&self) -> (f64, f64, f64, f64) {
        self.bbox
    }

    /// The outer rings, as (longitude, latitude): for the polygon filter of an Overpass query.
    pub fn outer_rings(&self) -> impl Iterator<Item = &Ring> {
        self.polygons.iter().filter_map(|p| p.first())
    }
}

/// Is p on the segment a-b? Exact: a vertex that is an OSM node has the same bits as the node (`degrees`).
fn on_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> bool {
    if p == a || p == b {
        return true;
    }
    let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    cross == 0.0
        && p.0 >= a.0.min(b.0)
        && p.0 <= a.0.max(b.0)
        && p.1 >= a.1.min(b.1)
        && p.1 <= a.1.max(b.1)
}

/// A coordinate as the PBF reader computes it (`1e-9 * nanodegrees`), so a vertex that is an OSM node has
/// exactly the bits of the node.
fn degrees(v: f64) -> f64 {
    1e-9 * ((v * 1e9).round() as i64) as f64
}

/// The ring of a box.
fn rect(w: f64, s: f64, e: f64, n: f64) -> Ring {
    [(w, s), (e, s), (e, n), (w, n), (w, s)].into_iter().map(|(x, y)| (degrees(x), degrees(y))).collect()
}

/// An osmosis polygon file: a name line, then sections, each a name line, one `longitude latitude` on each line, and
/// `END`; a last `END`. A section with a name that starts with `!` is a hole: it goes to the polygon that holds its
/// first point.
fn poly(text: &str) -> Result<(Vec<Vec<Ring>>, Option<String>), String> {
    let bad = |l: &str| format!("boundary: not a polygon file (the line \"{l}\")");
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let name = lines.next().map(str::to_string);
    let mut polygons: Vec<Vec<Ring>> = Vec::new();
    let mut holes: Vec<Ring> = Vec::new();
    loop {
        let section = lines.next().ok_or("boundary: a polygon file without the last END")?;
        if section == "END" {
            break;
        }
        let mut r = Ring::new();
        loop {
            let l = lines.next().ok_or("boundary: a polygon file section without END")?;
            if l == "END" {
                break;
            }
            let mut p = l.split_whitespace().map(str::parse::<f64>);
            match (p.next(), p.next(), p.next()) {
                (Some(Ok(x)), Some(Ok(y)), None) => r.push((degrees(x), degrees(y))),
                _ => return Err(bad(l)),
            }
        }
        if r.len() < 3 {
            return Err(format!("boundary: the polygon file section {section} has less than 3 points"));
        }
        if section.starts_with('!') {
            holes.push(r);
        } else {
            polygons.push(vec![r]);
        }
    }
    for h in holes {
        let p = polygons.iter().position(|p| inside_ring(h[0], &p[0])).ok_or("boundary: a hole of the polygon file is in no polygon")?;
        polygons[p].push(h);
    }
    Ok((polygons, name))
}

/// Is the point inside the ring (even-odd)?
fn inside_ring(p: (f64, f64), ring: &Ring) -> bool {
    let mut inside = false;
    for k in 0..ring.len() {
        let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    inside
}

/// WKT `POLYGON ((x y, ...), (...))` or `MULTIPOLYGON (((x y, ...)), ...)`.
fn wkt(text: &str) -> Result<Vec<Vec<Ring>>, String> {
    let bad = || format!("boundary: not a WKT POLYGON or MULTIPOLYGON: {}", text.get(..40).unwrap_or(text));
    let t = text.trim();
    let open = t.find('(').ok_or_else(bad)?;
    let kind = t[..open].trim().to_ascii_uppercase();
    // the list of lists of rings: one level more for MULTIPOLYGON
    let depth = match kind.as_str() {
        "POLYGON" => 2,
        "MULTIPOLYGON" => 3,
        _ => return Err(bad()),
    };
    let mut b = &t.as_bytes()[open..];
    let tree = wkt_list(&mut b, depth).ok_or_else(bad)?;
    if !std::str::from_utf8(b).map_err(|_| bad())?.trim().is_empty() {
        return Err(bad());
    }
    Ok(match tree {
        Wkt::List(v) if depth == 2 => vec![v.into_iter().map(Wkt::ring).collect::<Option<_>>().ok_or_else(bad)?],
        Wkt::List(v) => v
            .into_iter()
            .map(|p| match p {
                Wkt::List(rings) => rings.into_iter().map(Wkt::ring).collect::<Option<Vec<Ring>>>(),
                Wkt::Ring(_) => None,
            })
            .collect::<Option<_>>()
            .ok_or_else(bad)?,
        Wkt::Ring(_) => return Err(bad()),
    })
}

enum Wkt {
    List(Vec<Wkt>),
    Ring(Ring),
}

impl Wkt {
    fn ring(self) -> Option<Ring> {
        match self {
            Wkt::Ring(r) => Some(r),
            Wkt::List(_) => None,
        }
    }
}

/// One `( ... )` of WKT at `depth` (1: a list of points).
fn wkt_list(b: &mut &[u8], depth: u8) -> Option<Wkt> {
    let skip = |b: &mut &[u8]| {
        while b.first().is_some_and(u8::is_ascii_whitespace) {
            *b = &b[1..];
        }
    };
    skip(b);
    if b.first() != Some(&b'(') {
        return None;
    }
    *b = &b[1..];
    let close = b.iter().position(|&c| c == b')')?;
    if depth == 1 {
        let inner = std::str::from_utf8(&b[..close]).ok()?;
        *b = &b[close + 1..];
        let ring = inner
            .split(',')
            .map(|p| {
                let mut n = p.split_whitespace().map(str::parse::<f64>);
                match (n.next(), n.next()) {
                    (Some(Ok(x)), Some(Ok(y))) => Some((degrees(x), degrees(y))),
                    _ => None,
                }
            })
            .collect::<Option<Ring>>()?;
        return (ring.len() >= 3).then_some(Wkt::Ring(ring));
    }
    let mut items = Vec::new();
    loop {
        items.push(wkt_list(b, depth - 1)?);
        skip(b);
        match b.first()? {
            b',' => *b = &b[1..],
            b')' => {
                *b = &b[1..];
                return Some(Wkt::List(items));
            }
            _ => return None,
        }
    }
}

fn ring(v: &Value) -> Result<Ring, String> {
    v.as_array()
        .ok_or("boundary: a ring must be a list")?
        .iter()
        .map(|p| match p.as_array().map(|a| a.as_slice()) {
            Some([x, y, ..]) => match (x.as_f64(), y.as_f64()) {
                (Some(x), Some(y)) => Ok((degrees(x), degrees(y))),
                _ => Err("boundary: a position must be numbers".to_string()),
            },
            _ => Err("boundary: a position must be [longitude, latitude]".to_string()),
        })
        .collect()
}

fn polygon(v: &Value) -> Result<Vec<Ring>, String> {
    v.as_array()
        .ok_or("boundary: a Polygon must be a list of rings")?
        .iter()
        .map(ring)
        .collect()
}

fn collect(v: &Value, out: &mut Vec<Vec<Ring>>, name: &mut Option<String>) -> Result<(), String> {
    match v.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => {
            for f in v
                .get("features")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                collect(f, out, name)?;
            }
        }
        Some("Feature") => {
            if name.is_none() {
                *name = v
                    .pointer("/properties/name")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            match (v.get("geometry").filter(|g| !g.is_null()), v.get("bbox").and_then(Value::as_array)) {
                (Some(g), _) => collect(g, out, name)?,
                // no geometry: the box of the `bbox` member
                (None, Some(b)) => match b.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>().as_deref() {
                    Some(&[w, s, e, n]) => out.push(vec![rect(w, s, e, n)]),
                    _ => return Err("boundary: a bbox must be [west, south, east, north]".into()),
                },
                (None, None) => {}
            }
        }
        Some("Polygon") => out.push(polygon(&v["coordinates"])?),
        Some("MultiPolygon") => {
            for p in v["coordinates"]
                .as_array()
                .ok_or("boundary: bad MultiPolygon")?
            {
                out.push(polygon(p)?);
            }
        }
        Some(t) => return Err(format!("boundary: {t} is not a Polygon or a MultiPolygon")),
        None => return Err("boundary: GeoJSON without a type".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_with_a_hole() {
        let b = Boundary::parse(
            r#"{"type":"Feature","properties":{"name":"A"},"geometry":{"type":"Polygon","coordinates":[
            [[0,0],[10,0],[10,10],[0,10],[0,0]],[[4,4],[6,4],[6,6],[4,6],[4,4]]]}}"#,
        )
        .unwrap();
        assert_eq!(b.name.as_deref(), Some("A"));
        assert!(b.contains(1.0, 1.0) && b.contains(9.0, 5.0));
        assert!(!b.contains(5.0, 5.0), "in the hole");
        assert!(!b.contains(11.0, 5.0) && !b.contains(5.0, -1.0));
        assert!(Boundary::parse(r#"{"type":"Point","coordinates":[1,2]}"#).is_err());
    }

    /// The test with the bands gives the answer of the test with all edges: on Monaco's boundary and on a multipolygon with
    /// a hole, for random points in the box, the vertices, the middle of each edge and points just beside them.
    #[test]
    fn bands_give_the_answer_of_all_edges() {
        let monaco = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/monaco.geojson")).unwrap();
        let multi = r#"{"type":"MultiPolygon","coordinates":[[[[0,0],[10,0],[10,10],[0,10],[0,0]],[[4,4],[6,4],[6,6],[4,6],[4,4]]],
            [[[12,1],[15,1],[13.5,9],[12,1]]]]}"#;
        let mut seed = 12345u64;
        let mut rnd = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for text in [monaco.as_str(), multi] {
            let b = Boundary::parse(text).unwrap();
            let (w, s, e, n) = b.bbox;
            let mut pts: Vec<(f64, f64)> = (0..20_000).map(|_| (w + (e - w) * rnd(), s + (n - s) * rnd())).collect();
            for ring in b.polygons.iter().flatten() {
                for k in 0..ring.len() {
                    let (a, c) = (ring[k], ring[(k + 1) % ring.len()]);
                    pts.extend([a, ((a.0 + c.0) / 2.0, (a.1 + c.1) / 2.0), (a.0 + 1e-9, a.1), (a.0, a.1 - 1e-9)]);
                }
            }
            let inside = pts.iter().filter(|p| b.contains(p.0, p.1)).count();
            assert!(inside > 100 && inside < pts.len(), "the points are on both sides");
            for &(x, y) in &pts {
                assert_eq!(b.contains(x, y), b.contains_all(x, y), "({x}, {y})");
            }
        }
    }

    /// The same square with a hole in each format gives the same answers.
    #[test]
    fn the_formats_give_the_same_area() {
        let poly = "square\n1\n  0 0\n  10 0\n  10 10\n  0 10\nEND\n!hole\n  4 4\n  6 4\n  6 6\n  4 6\nEND\n2\n  20 0\n  30 0\n  25 5\nEND\nEND\n";
        let wkt = "MULTIPOLYGON (((0 0, 10 0, 10 10, 0 10, 0 0), (4 4, 6 4, 6 6, 4 6, 4 4)), ((20 0, 30 0, 25 5, 20 0)))";
        let json = r#"{"type":"MultiPolygon","coordinates":[[[[0,0],[10,0],[10,10],[0,10],[0,0]],[[4,4],[6,4],[6,6],[4,6],[4,4]]],
            [[[20,0],[30,0],[25,5],[20,0]]]]}"#;
        let p = Boundary::parse(poly).unwrap();
        assert_eq!(p.name.as_deref(), Some("square"));
        for b in [p, Boundary::parse(wkt).unwrap(), Boundary::parse(json).unwrap()] {
            assert!(b.contains(1.0, 1.0) && b.contains(25.0, 2.0));
            assert!(!b.contains(5.0, 5.0) && !b.contains(15.0, 1.0) && !b.contains(25.0, 6.0));
        }
        let w = Boundary::parse("POLYGON((0 0, 10 0, 10 10, 0 0))").unwrap();
        assert!(w.contains(9.0, 1.0) && !w.contains(1.0, 9.0));
        let b = Boundary::parse("7.40,43.72, 7.44,43.76").unwrap();
        assert!(b.contains(7.42, 43.73) && !b.contains(7.45, 43.73));
        let f = Boundary::parse(r#"{"type":"Feature","bbox":[0,0,2,2],"geometry":null,"properties":{}}"#).unwrap();
        assert!(f.contains(1.0, 1.0) && !f.contains(3.0, 1.0));
        for bad in ["1,2,3", "5,0,1,1", "POLYGON ((0 0, 1 1))", "POLYGON ((0 0, 1 0, 1 1)", "name\n1\n0 0\n1 0\nEND\nEND", "x\n1\n  0 0\n"] {
            assert!(Boundary::parse(bad).is_err(), "{bad}");
        }
    }
}
