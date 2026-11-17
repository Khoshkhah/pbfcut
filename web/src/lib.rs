//! The online page of pbfcut: the tile cut in WebAssembly, in a Web Worker (`www/worker.js`). The worker owns the
//! browser objects: the input file, a pool of OPFS files for the temporary files, and the OPFS file of the tile files.
//! This module keeps only numbers (a file number, a position), and calls the worker through the functions of the
//! object `pbfcut` of the worker. The calls come only from the thread of the cut: the other threads of rayon decode and
//! pack blocks in memory, and do not read or write files.

use std::io::{self, Read, Seek, SeekFrom, Write};

use pbfcut::boundary::{Boundary, WORLD};
use pbfcut::store::{Blob, Store};
use wasm_bindgen::prelude::*;

#[cfg(feature = "threads")]
mod alloc;
#[cfg(feature = "threads")]
#[global_allocator]
static ALLOC: alloc::PerThread = alloc::PerThread;

/// `initThreadPool(n)` in JavaScript: n Web Workers for rayon (the build with the feature "threads").
#[cfg(feature = "threads")]
pub use wasm_bindgen_rayon::init_thread_pool;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = pbfcut, catch)]
    fn input_read(at: f64, buf: &mut [u8]) -> Result<u32, JsValue>;
    #[wasm_bindgen(js_namespace = pbfcut, catch)]
    fn temp_create() -> Result<u32, JsValue>;
    #[wasm_bindgen(js_namespace = pbfcut, catch)]
    fn temp_read(file: u32, at: f64, buf: &mut [u8]) -> Result<u32, JsValue>;
    #[wasm_bindgen(js_namespace = pbfcut, catch)]
    fn temp_write(file: u32, at: f64, buf: &[u8]) -> Result<u32, JsValue>;
    #[wasm_bindgen(js_namespace = pbfcut, catch)]
    fn temp_drop(file: u32) -> Result<(), JsValue>;
    #[wasm_bindgen(js_namespace = pbfcut, catch)]
    fn tile_write(tile: u32, buf: &[u8]) -> Result<(), JsValue>;
    #[wasm_bindgen(js_namespace = pbfcut, catch)]
    fn step(name: &str) -> Result<(), JsValue>;
}

fn io_err(e: JsValue) -> io::Error {
    io::Error::other(format!("{e:?}"))
}

/// The input file (a file of the disk, or a download in OPFS): a position in it.
struct Input {
    pos: u64,
    len: u64,
}

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = (self.len.saturating_sub(self.pos)).min(buf.len() as u64) as usize;
        if n == 0 {
            return Ok(0);
        }
        let got = input_read(self.pos as f64, &mut buf[..n]).map_err(io_err)? as usize;
        self.pos += got as u64;
        Ok(got)
    }
}

impl Seek for Input {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.pos = seek(self.pos, self.len, p)?;
        Ok(self.pos)
    }
}

fn seek(pos: u64, len: u64, p: SeekFrom) -> io::Result<u64> {
    let to = match p {
        SeekFrom::Start(n) => n as i128,
        SeekFrom::End(d) => len as i128 + d as i128,
        SeekFrom::Current(d) => pos as i128 + d as i128,
    };
    u64::try_from(to).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a seek before the start"))
}

/// A temporary file: an OPFS file of the pool of the worker. It goes back to the pool when it is dropped.
struct Temp {
    file: u32,
    pos: u64,
    len: u64,
}

impl Read for Temp {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = (self.len.saturating_sub(self.pos)).min(buf.len() as u64) as usize;
        if n == 0 {
            return Ok(0);
        }
        let got = temp_read(self.file, self.pos as f64, &mut buf[..n]).map_err(io_err)? as usize;
        self.pos += got as u64;
        Ok(got)
    }
}

impl Write for Temp {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // the worker writes all bytes, or gives an error
        let n = temp_write(self.file, self.pos as f64, buf).map_err(io_err)? as usize;
        if n != buf.len() {
            return Err(io::Error::other(format!("a write to OPFS gave {n} of {} bytes", buf.len())));
        }
        self.pos += n as u64;
        self.len = self.len.max(self.pos);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for Temp {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.pos = seek(self.pos, self.len, p)?;
        Ok(self.pos)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = temp_drop(self.file);
    }
}

/// The temporary files in OPFS, not in the WebAssembly memory.
struct Opfs;

impl Store for Opfs {
    fn create(&self) -> io::Result<Box<dyn Blob>> {
        let file = temp_create().map_err(io_err)?;
        Ok(Box::new(Temp { file, pos: 0, len: 0 }))
    }
}

fn on_step(name: &str) {
    let _ = step(name);
}

/// The hooks of a cut (the steps, a panic, the temporary files in OPFS), and the opener of the input file.
fn setup(size: f64, memory_mib: u32) -> impl Fn() -> Result<io::BufReader<Input>, String> {
    std::panic::set_hook(Box::new(|p| on_step(&format!("panic: {p}"))));
    let _ = pbfcut::util::STEP_HOOK.set(on_step);
    pbfcut::store::set_temp_store(Some(std::rc::Rc::new(|| Box::new(Opfs) as Box<dyn Store>)));
    if memory_mib > 0 {
        pbfcut::store::set_memory((memory_mib as usize) << 20);
    }
    let len = size as u64;
    move || Ok(io::BufReader::with_capacity(4 << 20, Input { pos: 0, len }))
}

/// Cut the input file (`size` bytes) into the map tiles of zoom `level` (`grid` "tiles"), into the H3 cells of
/// resolution `level` (`grid` "h3"), or into the areas of the GeoJSON `areas` (`grid` "areas", one file for each
/// feature, named by its property "name", as `pbfcut areas`), inside `boundary` (GeoJSON; none: the bounding box in the header of the file).
/// `border_m`: the border of each file in metres. `complete`: the types of the relations to complete, separated by
/// commas. `memory_mib`: the memory budget of the cut (0: the default of the library). `compress`: zlib blocks; false: raw
/// blocks (a faster cut, larger files with the same elements). The files go to
/// `pbfcut.tile_write`. Gives the JSON report of the command `pbfcut tiles` or `pbfcut h3`, with the number of each file.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn cut(
    size: f64,
    grid: &str,
    level: u8,
    boundary: Option<String>,
    border_m: f64,
    complete: &str,
    memory_mib: u32,
    areas: Option<String>,
    compress: bool,
) -> Result<String, JsValue> {
    let open = setup(size, memory_mib);
    // the area of the list of tiles or cells; the areas need none (with no boundary, a file with no box in its header
    // can be cut to areas, as with the command line)
    let world = Boundary::parse(WORLD)?;
    let area = if grid == "areas" && boundary.is_none() { Boundary::parse(WORLD)? } else { area_of(&open, &boundary)? };
    let inside = if boundary.is_some() { &area } else { &world };
    let complete: Vec<&str> = complete.split(',').map(str::trim).filter(|t| !t.is_empty()).collect();
    let mut sink = |t: u32, b: &[u8]| tile_write(t, b).map_err(|e| format!("a file in OPFS: {e:?}"));
    let started = js_sys_now();
    // (the name of each tile or cell, its file name), and the numbers of elements
    let (names, counts): (Vec<(String, String)>, _) = if grid == "areas" {
        let text = areas.ok_or("give the GeoJSON file of the areas")?;
        let list = pbfcut::areas::AreaList::parse(&text, "name", &|f| Err(format!("{f}: the page reads one GeoJSON file only")))?;
        let counts = pbfcut::tilecut::cut_areas_to_sink(open, inside, &list, &complete, compress, &mut sink)?;
        (list.areas.iter().map(|a| (a.name.clone(), format!("{}.osm.pbf", a.name))).collect(), counts)
    } else if grid == "h3" {
        let res = pbfcut::cells::Resolution::try_from(level).map_err(|e| format!("the H3 resolution {level}: {e}"))?;
        let list = pbfcut::cells::cells(&area, res);
        let counts = pbfcut::tilecut::cut_cells_to_sink(open, inside, &list, &complete, border_m, compress, &mut sink)?;
        (list.iter().map(|c| (c.to_string(), format!("{c}.osm.pbf"))).collect(), counts)
    } else {
        let list = pbfcut::tiles::tiles(&area, level);
        let counts = pbfcut::tilecut::cut_to_sink(open, inside, &list, &complete, border_m, compress, &mut sink)?;
        (list.iter().map(|t| (t.name(), format!("{}_{}_{}.osm.pbf", t.z, t.x, t.y))).collect(), counts)
    };
    let files: Vec<String> = names
        .iter()
        .zip(&counts)
        .enumerate()
        .filter(|(_, (_, c))| c.nodes > 0)
        .map(|(i, ((name, file), c))| {
            format!(r#"{{"index":{i},"tile":"{name}","file":"{file}","nodes":{},"ways":{},"relations":{}}}"#, c.nodes, c.ways, c.relations)
        })
        .collect();
    Ok(format!(r#"{{"tiles":{},"files":[{}],"seconds":{}}}"#, names.len(), files.join(","), (js_sys_now() - started) / 1000.0))
}

/// The area of the list of tiles or cells: the boundary, else the bounding box in the header of the input file.
fn area_of(open: &impl Fn() -> Result<io::BufReader<Input>, String>, boundary: &Option<String>) -> Result<Boundary, String> {
    match boundary {
        Some(b) => Boundary::parse(b),
        None => {
            let h = pbfcut::extract::full_header(open()?)?;
            let (w, s, e, n) = h.bbox.ok_or("the file has no bounding box in its header: give a boundary")?;
            Boundary::from_box(&format!("{w},{s},{e},{n}"))
        }
    }
}

/// The plan of a cut, before the cut, for the map of the page: the tiles, cells or areas of [`cut`] with the same
/// arguments (the same lists), and the boundary. `size` 0: no input file yet (then the tiles and cells need a
/// boundary). Gives JSON:
///
/// - `count`: the number of tiles, cells or areas;
/// - `max_level`: the highest zoom or resolution that gives at most `limit` tiles or cells (none for areas): the page
///   cuts only up to it;
/// - `outlines`: a GeoJSON FeatureCollection of at most `max` of them, each with its name;
/// - `area`: the boundary, or the box of the file, as a GeoJSON MultiPolygon of its outer rings (none: areas only).
///
/// An error in the boundary or in the areas comes here, before the cut.
#[wasm_bindgen]
pub fn plan(size: f64, grid: &str, level: u8, boundary: Option<String>, areas: Option<String>, max: u32, limit: u32) -> Result<String, JsValue> {
    let open = setup(size, 0);
    let area = match (&boundary, size > 0.0, grid) {
        (Some(_), _, _) | (None, true, _) => Some(area_of(&open, &boundary)?),
        (None, false, "areas") => None,
        (None, false, _) => return Err("choose a file or a boundary".into()),
    };
    let ring = |pts: &mut dyn Iterator<Item = (f64, f64)>| {
        let mut v: Vec<String> = pts.map(|(x, y)| format!("[{x:.7},{y:.7}]")).collect();
        if let Some(first) = v.first().cloned() {
            v.push(first);
        }
        format!("[{}]", v.join(","))
    };
    let feature = |name: &str, rings: Vec<String>| {
        let name = serde_json::to_string(name).expect("a text");
        format!(r#"{{"type":"Feature","properties":{{"name":{name}}},"geometry":{{"type":"MultiPolygon","coordinates":[{}]}}}}"#,
            rings.iter().map(|r| format!("[{r}]")).collect::<Vec<_>>().join(","))
    };
    // the highest level whose list has at most `limit` items: each list stops after `limit` items (the lists grow with
    // the level, so the loop stops at the first level above the limit)
    let limit = limit as usize;
    let highest = |top: u8, fits: &dyn Fn(u8) -> bool| (0..=top).take_while(|&l| fits(l)).last();
    let max = max as usize;
    let (count, max_level, outlines): (usize, Option<u8>, Vec<String>) = if grid == "areas" {
        let text = areas.ok_or("give the GeoJSON file of the areas")?;
        let list = pbfcut::areas::AreaList::parse(&text, "name", &|f| Err(format!("{f}: the page reads one GeoJSON file only")))?;
        let f = list.areas.iter().take(max).map(|a| feature(&a.name, a.boundary.outer_rings().map(|r| ring(&mut r.iter().copied())).collect()));
        (list.areas.len(), None, f.collect())
    } else if grid == "h3" {
        let area = area.as_ref().expect("an area");
        let res = pbfcut::cells::Resolution::try_from(level).map_err(|e| format!("the H3 resolution {level}: {e}"))?;
        let top = highest(15, &|r| pbfcut::cells::cells_at_most(area, pbfcut::cells::Resolution::try_from(r).expect("0 to 15"), limit).is_some());
        // above the limit: no list (a count of `limit` + 1 says "more than the limit")
        let list = pbfcut::cells::cells_at_most(area, res, limit).unwrap_or_default();
        let count = if top.is_some_and(|t| level <= t) { list.len() } else { limit + 1 };
        let f = list.iter().take(max).map(|c| feature(&c.to_string(), vec![ring(&mut c.boundary().iter().map(|p| (p.lng(), p.lat())))]));
        (count, top, f.collect())
    } else {
        let area = area.as_ref().expect("an area");
        let top = highest(30, &|z| pbfcut::tiles::tiles_at_most(area, z, limit).is_some());
        let list = pbfcut::tiles::tiles_at_most(area, level, limit).unwrap_or_default();
        let count = if top.is_some_and(|t| level <= t) { list.len() } else { limit + 1 };
        let f = list.iter().take(max).map(|t| {
            let (w, s, e, n) = t.bounds();
            feature(&t.name(), vec![ring(&mut [(w, s), (e, s), (e, n), (w, n)].into_iter())])
        });
        (count, top, f.collect())
    };
    let area = area.map_or("null".to_string(), |a| feature("area", a.outer_rings().map(|r| ring(&mut r.iter().copied())).collect()));
    let max_level = max_level.map_or("null".to_string(), |l| l.to_string());
    Ok(format!(
        r#"{{"count":{count},"max_level":{max_level},"outlines":{{"type":"FeatureCollection","features":[{}]}},"area":{area}}}"#,
        outlines.join(",")
    ))
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = Date, js_name = now)]
    fn js_sys_now() -> f64;
}

/// The WebAssembly memory: the page shows its size.
#[wasm_bindgen]
pub fn wasm_memory() -> JsValue {
    wasm_bindgen::memory()
}
