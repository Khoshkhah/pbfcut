# Rust

The crate `pbfcut` is the library under the command line and the Python package. It is not on crates.io: add it
from GitHub.

```bash
cargo add --git https://github.com/Khoshkhah/pbfcut pbfcut     # add tag = "v0.1.0" in Cargo.toml for a fixed version
```

## Cut to a folder

`pbfcut::job::cut` does what the command line does:

```rust
use pbfcut::boundary::Boundary;
use pbfcut::job::{cut, Grid};
use std::path::Path;

fn main() -> Result<(), String> {
    let report = cut(
        Path::new("monaco.osm.pbf"),
        Path::new("tiles"),
        &Grid::Zoom(14),                          // or Grid::Res(9), Grid::Tiles(list), Grid::Areas(list)
        Some(Boundary::from_box("7.40,43.72,7.44,43.76")?),   // None: all data
        &["multipolygon", "boundary"],
        0.0,                                      // the border in metres
    )?;
    for f in &report.files {
        println!("{} {} nodes, {} bytes", f.file, f.counts.nodes, f.bytes);
    }
    Ok(())
}
```

An error is a `String` with the message of the command line.

## More control

The functions of `pbfcut::tilecut` take a function that opens the input (the cut reads it two times):

| Function | Output |
|---|---|
| `cut_to_tiles(open, &boundary, &tiles, dir, &complete, border_m, None)` | one file for each tile |
| `cut_to_cells(open, &boundary, &cells, dir, &complete, border_m)` | one file for each H3 cell |
| `cut_to_areas(open, &boundary, &areas, dir, &complete)` | one file for each area |
| `cut_to_sink`, `cut_cells_to_sink`, `cut_areas_to_sink` | the bytes of each file to your function, not to a folder |

The lists come from `tiles::tiles(&boundary, zoom)`, `cells::cells(&boundary, resolution)` and
`areas::AreaList::parse`. `extract::cut_to_pbf` cuts one area to any `Write`.

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

- `store::set_memory(bytes)`: the memory budget. Default: 2 GiB.
- `store::set_temp_dir(Some(path))`: the folder of the temporary files.
- Threads: the cut uses the `rayon` pool. For fewer threads, run it in
  `rayon::ThreadPoolBuilder::new().num_threads(n).build()?.install(...)`.

The API documentation: `cargo doc --open --no-deps`.
