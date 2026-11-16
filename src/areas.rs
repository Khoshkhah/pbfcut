//! A list of areas for one cut, as the extracts of `osmium extract`: each area gives one output file. The areas can
//! overlap: a node then goes to each area that holds it (docs/design.md, 3.1).
//!
//! [`AreaList::parse`] reads two formats:
//!
//! - a GeoJSON FeatureCollection: each feature is one area (a Polygon, a MultiPolygon, or a `bbox` member). The name of
//!   the output comes from a property of the feature (default `name`), else from the `id` of the feature, else from
//!   its number.
//! - the config file of `osmium extract`: `{"extracts": [{"output": "NAME.osm.pbf", "bbox" | "polygon" |
//!   "multipolygon": ...}]}`. A polygon can be in another file (`{"file_name": "a.poly"}`): any format of
//!   [`Boundary::parse`].

use crate::boundary::Boundary;
use serde_json::Value;

/// One area of the list: the name of its output file (with no `.osm.pbf`) and its boundary.
pub struct Area {
    pub name: String,
    pub boundary: Boundary,
}

/// The areas, and a grid of their boxes: the areas of a point are the areas of its cell that hold it.
pub struct AreaList {
    pub areas: Vec<Area>,
    /// The output folder of an osmium config (`directory`), if it has one.
    pub directory: Option<String>,
    /// west, south, east, north of all areas
    bbox: (f64, f64, f64, f64),
    side: usize,
    cells: Vec<Vec<u32>>,
}

impl AreaList {
    /// The areas of a GeoJSON FeatureCollection or of an osmium config file. `name_key`: the property that gives the
    /// names of the features. `read`: the text of a file that the config names.
    pub fn parse(text: &str, name_key: &str, read: &dyn Fn(&str) -> Result<String, String>) -> Result<AreaList, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("areas: {e}"))?;
        let mut areas = Vec::new();
        if let Some(list) = v.get("extracts").and_then(Value::as_array) {
            for (i, x) in list.iter().enumerate() {
                let name = x.get("output").and_then(Value::as_str).ok_or(format!("areas: extract {i} has no output"))?;
                let name = name.strip_suffix(".osm.pbf").or(name.strip_suffix(".pbf")).unwrap_or(name);
                let boundary = extract_boundary(x, read).map_err(|e| format!("areas: extract {name}: {e}"))?;
                areas.push(Area { name: name.to_string(), boundary });
            }
        } else if v.get("type").and_then(Value::as_str) == Some("FeatureCollection") {
            for (i, f) in v.get("features").and_then(Value::as_array).into_iter().flatten().enumerate() {
                let name = [f.pointer(&format!("/properties/{name_key}")), f.get("id")]
                    .into_iter()
                    .flatten()
                    .find_map(|n| match n {
                        Value::String(s) => Some(s.clone()),
                        Value::Number(n) => Some(n.to_string()),
                        _ => None,
                    })
                    .unwrap_or_else(|| format!("area-{i}"));
                let boundary = Boundary::from_geojson(f).map_err(|e| format!("areas: feature {name}: {e}"))?;
                areas.push(Area { name, boundary });
            }
        } else {
            return Err("areas: give a GeoJSON FeatureCollection or an osmium config with \"extracts\"".into());
        }
        let directory = v.get("directory").and_then(Value::as_str).map(str::to_string);
        AreaList::new(areas, directory)
    }

    /// The list of `areas`. The names must be different, and a file name only (no `/`, `\`, or `..`).
    pub fn new(areas: Vec<Area>, directory: Option<String>) -> Result<AreaList, String> {
        if areas.is_empty() {
            return Err("areas: the list has no area".into());
        }
        let mut names = std::collections::BTreeSet::new();
        for a in &areas {
            if a.name.is_empty() || a.name.contains(['/', '\\']) || a.name.starts_with('.') {
                return Err(format!("areas: \"{}\" is not a file name", a.name));
            }
            if !names.insert(&a.name) {
                return Err(format!("areas: two areas have the name \"{}\"", a.name));
            }
        }
        let bbox = areas.iter().map(|a| a.boundary.bbox()).fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, a| {
            (b.0.min(a.0), b.1.min(a.1), b.2.max(a.2), b.3.max(a.3))
        });
        // about 4 cells for each area on each side, at most 256 x 256
        let side = ((areas.len() as f64).sqrt() as usize * 4).clamp(1, 256);
        let mut l = AreaList { areas, directory, bbox, side, cells: vec![Vec::new(); side * side] };
        for (i, a) in l.areas.iter().enumerate() {
            let (w, s, e, n) = a.boundary.bbox();
            let (x0, y0) = l.cell(w, s);
            let (x1, y1) = l.cell(e, n);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    l.cells[y * side + x].push(i as u32);
                }
            }
        }
        Ok(l)
    }

    fn cell(&self, lon: f64, lat: f64) -> (usize, usize) {
        let (w, s, e, n) = self.bbox;
        let at = |v: f64, lo: f64, hi: f64| (((v - lo) / (hi - lo).max(1e-12) * self.side as f64) as usize).min(self.side - 1);
        (at(lon, w, e), at(lat, s, n))
    }

    /// The areas that hold the point, in the order of the list.
    pub fn of(&self, lon: f64, lat: f64) -> impl Iterator<Item = u32> + '_ {
        let (w, s, e, n) = self.bbox;
        let out = lon < w || lon > e || lat < s || lat > n;
        let cell: &[u32] = if out { &[] } else {
            let (x, y) = self.cell(lon, lat);
            &self.cells[y * self.side + x]
        };
        cell.iter().copied().filter(move |&i| self.areas[i as usize].boundary.contains(lon, lat))
    }
}

/// The boundary of one extract of an osmium config: `bbox` ([W, S, E, N] or {left, bottom, right, top}), `polygon` or
/// `multipolygon` (GeoJSON coordinates, or `{"file_name": ...}`).
fn extract_boundary(x: &Value, read: &dyn Fn(&str) -> Result<String, String>) -> Result<Boundary, String> {
    if let Some(b) = x.get("bbox") {
        let n = |k: &str| b.get(k).and_then(Value::as_f64);
        let v = match b {
            Value::Array(a) => a.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>(),
            _ => [n("left"), n("bottom"), n("right"), n("top")].into_iter().collect(),
        };
        let v = v.ok_or("bbox must be [left, bottom, right, top] or {left, bottom, right, top}")?;
        return Boundary::from_box(&v.iter().map(f64::to_string).collect::<Vec<_>>().join(","));
    }
    for (key, kind) in [("polygon", "Polygon"), ("multipolygon", "MultiPolygon")] {
        match x.get(key) {
            Some(Value::Object(o)) => {
                let f = o.get("file_name").and_then(Value::as_str).ok_or(format!("{key} needs file_name"))?;
                return Boundary::parse(&read(f)?);
            }
            Some(c @ Value::Array(_)) => return Boundary::from_geojson(&serde_json::json!({"type": kind, "coordinates": c})),
            _ => {}
        }
    }
    Err("no bbox, polygon or multipolygon".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feature_collection_and_a_config() {
        let fc = r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"name":"a"},"geometry":{"type":"Polygon","coordinates":[[[0,0],[2,0],[2,2],[0,2],[0,0]]]}},
            {"type":"Feature","id":7,"properties":{},"bbox":[1,1,3,3],"geometry":null},
            {"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[10,10],[11,10],[11,11],[10,10]]]}}]}"#;
        let l = AreaList::parse(fc, "name", &|_| Err("no files".into())).unwrap();
        let names: Vec<&str> = l.areas.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["a", "7", "area-2"]);
        assert_eq!(l.of(0.5, 0.5).collect::<Vec<_>>(), [0]);
        assert_eq!(l.of(1.5, 1.5).collect::<Vec<_>>(), [0, 1], "the overlap");
        assert_eq!(l.of(10.8, 10.2).collect::<Vec<_>>(), [2]);
        assert_eq!(l.of(5.0, 5.0).count(), 0);

        let config = r#"{"directory":"out","extracts":[
            {"output":"box.osm.pbf","bbox":{"left":0,"bottom":0,"right":2,"top":2}},
            {"output":"tri.osm.pbf","polygon":[[[0,0],[4,0],[0,4],[0,0]]]},
            {"output":"file","polygon":{"file_name":"x.poly","file_type":"poly"}}]}"#;
        let read = |f: &str| if f == "x.poly" { Ok("x\n1\n 5 5\n 6 5\n 6 6\nEND\nEND\n".to_string()) } else { Err(f.to_string()) };
        let l = AreaList::parse(config, "name", &read).unwrap();
        assert_eq!(l.directory.as_deref(), Some("out"));
        assert_eq!(l.areas.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["box", "tri", "file"]);
        assert_eq!(l.of(1.0, 1.0).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(l.of(5.8, 5.2).collect::<Vec<_>>(), [2]);

        let two = r#"{"extracts":[{"output":"a","bbox":[0,0,1,1]},{"output":"a.osm.pbf","bbox":[0,0,1,1]}]}"#;
        assert!(AreaList::parse(two, "name", &read).is_err(), "two names");
        assert!(AreaList::parse(r#"{"extracts":[{"output":"../a","bbox":[0,0,1,1]}]}"#, "name", &read).is_err());
    }
}
