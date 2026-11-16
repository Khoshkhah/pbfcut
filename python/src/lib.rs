//! The Python module `pbfcut`. It only converts the arguments and the results: the cut is `pbfcut::job` of the library,
//! the same code as the command line (docs/python.md).

use pbfcut::areas::AreaList;
use pbfcut::boundary::Boundary;
use pbfcut::job::{self, Grid};
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::{Path, PathBuf};

create_exception!(pbfcut, Error, PyException, "An error of pbfcut, with the message of the library.");

fn err(e: String) -> PyErr {
    Error::new_err(e)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// A boundary file (GeoJSON, .poly or WKT), or a box (west, south, east, north).
#[derive(FromPyObject)]
enum BoundaryArg {
    Box((f64, f64, f64, f64)),
    Path(PathBuf),
}

fn boundary(b: Option<BoundaryArg>) -> Result<Option<Boundary>, String> {
    match b {
        None => Ok(None),
        Some(BoundaryArg::Box((w, s, e, n))) => Boundary::from_box(&format!("{w},{s},{e},{n}")).map(Some),
        Some(BoundaryArg::Path(p)) => Boundary::parse(&read(&p)?).map_err(|e| format!("{}: {e}", p.display())).map(Some),
    }
}

/// The options of every cut.
struct Options {
    boundary: Option<Boundary>,
    border: f64,
    complete: Vec<String>,
    memory: Option<usize>,
    temp: Option<PathBuf>,
    threads: Option<usize>,
}

/// A memory size: a number of bytes, or a text such as "4G" or "512M".
#[derive(FromPyObject)]
enum Size {
    Bytes(usize),
    Text(String),
}

#[allow(clippy::too_many_arguments)]
fn options(
    boundary_arg: Option<BoundaryArg>,
    border: f64,
    complete: Vec<String>,
    memory: Option<Size>,
    temp: Option<PathBuf>,
    threads: Option<usize>,
) -> PyResult<Options> {
    let memory = match memory {
        None => None,
        Some(Size::Bytes(b)) => Some(b),
        Some(Size::Text(t)) => Some(job::size(&t).map_err(err)?),
    };
    Ok(Options { boundary: boundary(boundary_arg).map_err(err)?, border, complete, memory, temp, threads })
}

/// Run the cut with the GIL released, with the memory budget, the temporary folder and the threads of `o`, then
/// give back the settings before.
fn run(py: Python<'_>, input: PathBuf, out: PathBuf, grid: Grid, o: Options) -> PyResult<Py<PyDict>> {
    let Options { boundary, border, complete, memory, temp, threads } = o;
    let start = std::time::Instant::now();
    let report = py
        .detach(move || -> Result<job::Report, String> {
            let complete: Vec<&str> = complete.iter().map(String::as_str).filter(|t| !t.is_empty()).collect();
            let old_memory = pbfcut::store::memory();
            let old_temp = temp.as_ref().map(|_| pbfcut::store::temp_dir());
            if let Some(m) = memory {
                pbfcut::store::set_memory(m);
            }
            if let Some(d) = &temp {
                std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
                pbfcut::store::set_temp_dir(Some(d.clone()));
            }
            let cut = || job::cut(&input, &out, &grid, boundary, &complete, border);
            let r = match threads {
                Some(n) => rayon::ThreadPoolBuilder::new().num_threads(n).build().map_err(|e| e.to_string())?.install(cut),
                None => cut(),
            };
            pbfcut::store::set_memory(old_memory);
            if let Some(d) = old_temp {
                pbfcut::store::set_temp_dir(Some(d));
            }
            r
        })
        .map_err(err)?;
    let d = PyDict::new(py);
    let files = report
        .files
        .iter()
        .map(|w| -> PyResult<_> {
            let f = PyDict::new(py);
            f.set_item(report.key, &w.name)?;
            f.set_item("file", &w.file)?;
            f.set_item("nodes", w.counts.nodes)?;
            f.set_item("ways", w.counts.ways)?;
            f.set_item("relations", w.counts.relations)?;
            f.set_item("bytes", w.bytes)?;
            Ok(f)
        })
        .collect::<PyResult<Vec<_>>>()?;
    d.set_item(format!("{}s", report.key), report.listed)?;
    d.set_item("files", files)?;
    d.set_item("seconds", start.elapsed().as_secs_f64())?;
    Ok(d.unbind())
}

fn default_complete() -> Vec<String> {
    vec!["multipolygon".into(), "boundary".into()]
}

/// Cut `input` into the map tiles of `zoom` (or of the list file `tiles`) in the folder `out_dir`.
#[pyfunction]
#[pyo3(signature = (input, out_dir, zoom=8, *, tiles=None, boundary=None, border=0.0, complete=default_complete(), memory=None, temp=None, threads=None))]
#[allow(clippy::too_many_arguments)]
fn cut_tiles(
    py: Python<'_>,
    input: PathBuf,
    out_dir: PathBuf,
    zoom: u8,
    tiles: Option<PathBuf>,
    boundary: Option<BoundaryArg>,
    border: f64,
    complete: Vec<String>,
    memory: Option<Size>,
    temp: Option<PathBuf>,
    threads: Option<usize>,
) -> PyResult<Py<PyDict>> {
    let grid = match tiles {
        Some(p) => Grid::Tiles(read(&p).and_then(|t| job::tile_list(&t)).map_err(err)?),
        None => Grid::Zoom(zoom),
    };
    run(py, input, out_dir, grid, options(boundary, border, complete, memory, temp, threads)?)
}

/// Cut `input` into the H3 cells of resolution `res` in the folder `out_dir`.
#[pyfunction]
#[pyo3(signature = (input, out_dir, res, *, boundary=None, border=0.0, complete=default_complete(), memory=None, temp=None, threads=None))]
#[allow(clippy::too_many_arguments)]
fn cut_cells(
    py: Python<'_>,
    input: PathBuf,
    out_dir: PathBuf,
    res: u8,
    boundary: Option<BoundaryArg>,
    border: f64,
    complete: Vec<String>,
    memory: Option<Size>,
    temp: Option<PathBuf>,
    threads: Option<usize>,
) -> PyResult<Py<PyDict>> {
    run(py, input, out_dir, Grid::Res(res), options(boundary, border, complete, memory, temp, threads)?)
}

/// Cut `input` into the areas of the file `areas` (a GeoJSON FeatureCollection or an osmium config) in `out_dir`.
#[pyfunction]
#[pyo3(signature = (input, areas, out_dir, *, name="name", boundary=None, complete=default_complete(), memory=None, temp=None, threads=None))]
#[allow(clippy::too_many_arguments)]
fn cut_areas(
    py: Python<'_>,
    input: PathBuf,
    areas: PathBuf,
    out_dir: PathBuf,
    name: &str,
    boundary: Option<BoundaryArg>,
    complete: Vec<String>,
    memory: Option<Size>,
    temp: Option<PathBuf>,
    threads: Option<usize>,
) -> PyResult<Py<PyDict>> {
    // a file that an osmium config names is relative to the config
    let base = areas.parent().unwrap_or(Path::new("")).to_path_buf();
    let list = read(&areas)
        .and_then(|t| AreaList::parse(&t, name, &|f| read(&base.join(f))))
        .map_err(|e| err(format!("{}: {e}", areas.display())))?;
    run(py, input, out_dir, Grid::Areas(list), options(boundary, 0.0, complete, memory, temp, threads)?)
}

/// Compare the `.osm.pbf` files of two folders by their elements. Gives one line for each file that differs or is in
/// one folder only: an empty list means that the folders are equal.
#[pyfunction]
fn compare(py: Python<'_>, dir_a: PathBuf, dir_b: PathBuf) -> PyResult<Vec<String>> {
    py.detach(|| job::compare(&dir_a, &dir_b)).map(|(_, lines)| lines).map_err(err)
}

#[pymodule]
#[pyo3(name = "pbfcut")]
fn pbfcut_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("Error", m.py().get_type::<Error>())?;
    m.add_function(wrap_pyfunction!(cut_tiles, m)?)?;
    m.add_function(wrap_pyfunction!(cut_cells, m)?)?;
    m.add_function(wrap_pyfunction!(cut_areas, m)?)?;
    m.add_function(wrap_pyfunction!(compare, m)?)?;
    Ok(())
}
