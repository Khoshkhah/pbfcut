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

/// The output file of [`cut_area`]: its bytes go to `pbfcut.tile_write` as file 0.
struct AreaFile;

impl Write for AreaFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        tile_write(0, buf).map_err(io_err)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Cut the input file (`size` bytes) to one area, `boundary` (GeoJSON), as `pbfcut::extract::cut_to_pbf`: one
/// `.osm.pbf` file named `name`. Gives the JSON report of [`cut`], with one file.
#[wasm_bindgen]
pub fn cut_area(size: f64, boundary: &str, name: &str, memory_mib: u32) -> Result<String, JsValue> {
    let open = setup(size, memory_mib);
    let area = Boundary::parse(boundary)?;
    let started = js_sys_now();
    let (w, c) = pbfcut::extract::cut_to_pbf(open, &area, io::BufWriter::with_capacity(1 << 20, AreaFile))?;
    w.into_inner().map_err(|e| format!("the area file in OPFS: {e}"))?;
    Ok(format!(
        r#"{{"tiles":1,"files":[{{"index":0,"tile":"area","file":"{}","nodes":{},"ways":{},"relations":{}}}],"seconds":{}}}"#,
        name.replace(['"', '\\', '/'], "_"),
        c.nodes,
        c.ways,
        c.relations,
        (js_sys_now() - started) / 1000.0
    ))
}

/// Cut the input file (`size` bytes) into the map tiles of zoom `level` (`grid` "tiles"), or into the H3 cells of
/// resolution `level` (`grid` "h3"), inside `boundary` (GeoJSON; none: the bounding box in the header of the file).
/// `border_m`: the border of each file in metres. `complete`: the types of the relations to complete, separated by
/// commas. `memory_mib`: the memory budget of the cut (0: the default of the library). The files go to
/// `pbfcut.tile_write`. Gives the JSON report of the command `pbfcut tiles` or `pbfcut h3`, with the number of each file.
#[wasm_bindgen]
pub fn cut(size: f64, grid: &str, level: u8, boundary: Option<String>, border_m: f64, complete: &str, memory_mib: u32) -> Result<String, JsValue> {
    let open = setup(size, memory_mib);
    let area = match &boundary {
        Some(b) => Boundary::parse(b)?,
        None => {
            let h = pbfcut::extract::full_header(open()?)?;
            let (w, s, e, n) = h.bbox.ok_or("the file has no bounding box in its header: give a boundary")?;
            let ring = format!("[[{w},{s}],[{e},{s}],[{e},{n}],[{w},{n}],[{w},{s}]]");
            Boundary::parse(&format!(r#"{{"type":"Polygon","coordinates":[{ring}]}}"#))?
        }
    };
    let world = Boundary::parse(WORLD)?;
    let inside = if boundary.is_some() { &area } else { &world };
    let complete: Vec<&str> = complete.split(',').map(str::trim).filter(|t| !t.is_empty()).collect();
    let mut sink = |t: u32, b: &[u8]| tile_write(t, b).map_err(|e| format!("a file in OPFS: {e:?}"));
    let started = js_sys_now();
    // (the name of each tile or cell, its file name), and the numbers of elements
    let (names, counts): (Vec<(String, String)>, _) = if grid == "h3" {
        let res = pbfcut::cells::Resolution::try_from(level).map_err(|e| format!("the H3 resolution {level}: {e}"))?;
        let list = pbfcut::cells::cells(&area, res);
        let counts = pbfcut::tilecut::cut_cells_to_sink(open, inside, &list, &complete, border_m, &mut sink)?;
        (list.iter().map(|c| (c.to_string(), format!("{c}.osm.pbf"))).collect(), counts)
    } else {
        let list = pbfcut::tiles::tiles(&area, level);
        let counts = pbfcut::tilecut::cut_to_sink(open, inside, &list, &complete, border_m, &mut sink)?;
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
