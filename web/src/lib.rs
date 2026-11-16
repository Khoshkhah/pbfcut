//! The online page of pbfcut: the tile cut in WebAssembly, in a Web Worker (`www/worker.js`). The worker owns the
//! browser objects: the input file, a pool of OPFS files for the temporary files, and the OPFS file of the tile files.
//! This module keeps only numbers (a file number, a position), and calls the worker through the functions of the
//! object `pbfcut` of the worker. The calls come only from the thread of the cut: the other threads of rayon decode and
//! pack blocks in memory, and do not read or write files.

use std::io::{self, Read, Seek, SeekFrom, Write};

use pbfcut::boundary::{Boundary, WORLD};
use pbfcut::store::{Blob, Store};
use wasm_bindgen::prelude::*;

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

/// Cut the input file (`size` bytes) into the tiles of `zoom` inside `boundary` (GeoJSON; none: the bounding box in
/// the header of the file). `border_m`: the border of each tile in metres. `complete`: the types of the relations to
/// complete, separated by commas. `memory_mib`: the memory budget of the cut (0: the default of the library). The tile
/// files go to `pbfcut.tile_write`. Gives the JSON report of the command `pbfcut tiles`, with the number of each tile.
#[wasm_bindgen]
pub fn cut(size: f64, zoom: u8, boundary: Option<String>, border_m: f64, complete: &str, memory_mib: u32) -> Result<String, JsValue> {
    std::panic::set_hook(Box::new(|p| on_step(&format!("panic: {p}"))));
    let _ = pbfcut::util::STEP_HOOK.set(on_step);
    pbfcut::store::set_temp_store(Some(std::rc::Rc::new(|| Box::new(Opfs) as Box<dyn Store>)));
    if memory_mib > 0 {
        pbfcut::store::set_memory((memory_mib as usize) << 20);
    }
    let len = size as u64;
    let open = || Ok(io::BufReader::with_capacity(4 << 20, Input { pos: 0, len }));
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
    let list = pbfcut::tiles::tiles(&area, zoom);
    let complete: Vec<&str> = complete.split(',').map(str::trim).filter(|t| !t.is_empty()).collect();
    let started = js_sys_now();
    let counts = pbfcut::tilecut::cut_to_sink(open, if boundary.is_some() { &area } else { &world }, &list, &complete, border_m, &mut |t, b| {
        tile_write(t, b).map_err(|e| format!("a tile file in OPFS: {e:?}"))
    })?;
    let files: Vec<String> = list
        .iter()
        .zip(&counts)
        .enumerate()
        .filter(|(_, (_, c))| c.nodes > 0)
        .map(|(i, (t, c))| {
            format!(
                r#"{{"index":{i},"tile":"{}","file":"{}_{}_{}.osm.pbf","nodes":{},"ways":{},"relations":{}}}"#,
                t.name(),
                t.z,
                t.x,
                t.y,
                c.nodes,
                c.ways,
                c.relations
            )
        })
        .collect();
    Ok(format!(r#"{{"tiles":{},"files":[{}],"seconds":{}}}"#, list.len(), files.join(","), (js_sys_now() - started) / 1000.0))
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
