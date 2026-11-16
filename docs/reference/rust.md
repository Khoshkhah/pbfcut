# Rust

The Rust crate `pbfcut` is the library under the command line and the Python package. They all call the same code.

## Install

The crate is not on crates.io. Add it from GitHub. You must have Rust and Cargo (see the README for the steps):

```bash
cargo add --git https://github.com/Khoshkhah/pbfcut pbfcut
```

Or write it in `Cargo.toml`:

```toml
[dependencies]
pbfcut = { git = "https://github.com/Khoshkhah/pbfcut" }
```

To use a fixed version, add `tag = "v0.1.0"` or `rev = "COMMIT"`.

## Cut to a folder

`pbfcut::job::cut` is the function of the command line and of the Python package. It writes the files to a folder and
gives the names, the element counts and the sizes:

```rust
use pbfcut::boundary::Boundary;
use pbfcut::job::{cut, Grid};
use std::path::Path;

fn main() -> Result<(), String> {
    let boundary = Boundary::parse(&std::fs::read_to_string("monaco.geojson").map_err(|e| e.to_string())?)?;
    let report = cut(
        Path::new("monaco.osm.pbf"),
        Path::new("tiles"),
        &Grid::Zoom(14),          // or Grid::Res(9), Grid::Tiles(list), Grid::Areas(list)
        Some(boundary),           // None: all nodes (the box in the header of the file gives the tiles)
        &["multipolygon", "boundary"],
        0.0,                      // the border in metres (tiles and H3 cells only)
    )?;
    for f in &report.files {
        println!("{} {} nodes, {} bytes", f.file, f.counts.nodes, f.bytes);
    }
    Ok(())
}
```

An error is a `String` with the message that the command line writes.

## Cut with the functions of the cut

For more control, use the functions of `pbfcut::tilecut`. They take a function that opens the input (the cut reads the
file two times), the boundary, the list of outputs and the relation types to keep whole:

| Function | Output |
|---|---|
| `cut_to_tiles(open, &boundary, &tiles, dir, &complete, border_m, None)` | one file for each tile, in `dir` |
| `cut_to_cells(open, &boundary, &cells, dir, &complete, border_m)` | one file for each H3 cell, in `dir` |
| `cut_to_areas(open, &boundary, &areas, dir, &complete)` | one file for each area, in `dir` |
| `cut_to_sink(open, &boundary, &tiles, &complete, border_m, sink)` | the files go to a function that you give, not to a folder |
| `cut_cells_to_sink(...)` | the same, for H3 cells |

Each function gives a `Vec<CutCounts>`: the numbers of nodes, ways and relations of each output, in the order of the list.
The lists come from `pbfcut::tiles::tiles(&boundary, zoom)`, `pbfcut::cells::cells(&boundary, resolution)` and
`pbfcut::areas::AreaList::parse`. `pbfcut::extract::cut_to_pbf` cuts one area and writes it to any `Write`.

```rust
use pbfcut::{boundary::Boundary, tilecut::cut_to_tiles, tiles::tiles};
use std::{fs::File, io::BufReader, path::Path};

fn main() -> Result<(), String> {
    let boundary = Boundary::from_box("7.40,43.72,7.44,43.76")?;
    let list = tiles(&boundary, 14);
    let open = || File::open("monaco.osm.pbf").map(BufReader::new).map_err(|e| e.to_string());
    let counts = cut_to_tiles(open, &boundary, &list, Path::new("tiles"), &["multipolygon", "boundary"], 0.0, None)?;
    println!("{} tiles, {} nodes in the first", list.len(), counts[0].nodes);
    Ok(())
}
```

## Settings

- `pbfcut::store::set_memory(bytes)`: the memory budget of the cut. Default: 2 GiB.
- `pbfcut::store::set_temp_dir(Some(path))`: the folder of the temporary files.
- The cut uses all processors through `rayon`. To use fewer, run it inside a `rayon::ThreadPoolBuilder::new().num_threads(n).build()?.install(...)`.

The memory and the temporary folder are settings of the process.

## The API documentation

```bash
cargo doc --open --no-deps
```
