# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`pbfcut` is a Rust library and command. It cuts an OpenStreetMap `.osm.pbf` file into map tiles or areas. One read of the input gives all outputs. Each output is a complete `.osm.pbf` file: the nodes in the area, each way with a node there (with all its nodes), and the relations with a member there.

pbfcut is an independent project. Do not mention the other projects of the owner in the code, the tests, the README or the manual.

`docs/design.md` is the source of truth for scope and decisions. Status: the commands `tiles`, `h3` (H3 cells of one resolution), `areas` (a list of areas that do not overlap, as `osmium extract -c`) and `compare` exist. A list of H3 cells of mixed resolutions is planned and not in the code. `docs/design.md` is for developers: `mkdocs.yml` keeps it out of the published manual.

## Commands

```bash
cargo build --release
cargo test --release                                  # unit tests and the golden tests
cargo test --release --test golden tile_files_equal_the_reference   # one test
cargo clippy --release
```

Run the command:

```bash
cargo run --release -- tiles tests/data/monaco.osm.pbf --zoom 14 --boundary tests/data/monaco.geojson -o out/
cargo run --release -- h3 tests/data/monaco.osm.pbf --res 9 --boundary tests/data/monaco.geojson -o out/
cargo run --release -- areas tests/data/monaco.osm.pbf --areas tests/data/monaco-areas.json -o out/
cargo run --release -- compare DIR_A DIR_B           # exit 1 if the elements of the .osm.pbf files differ
```

`tiles` takes `--zoom Z` or `--tiles LIST.txt` (one `z/x/y` on each line, no tile inside another tile of the list). `--complete` sets the relation types that the cut keeps whole (default `multipolygon,boundary`). `--steps` prints the time of each step to standard error.

The browser page is in `web/` (a crate `pbfcut-web` with wasm-bindgen, and the page in `web/www/`). The worker owns the
browser objects (the input file, a pool of 160 OPFS sync access handles for the temporary files, one OPFS file for the
tile files). The module calls them only from the thread of the cut.

```bash
CARGO_BUILD_JOBS=4 ./web/build.sh                     # web/www/pkg (stable), web/www/pkg-threads (nightly), regions.json
python3 web/serve.py 8800                             # no COOP/COEP headers, as Pages; ?relay=relay gets Geofabrik files
python3 web/e2e.py [--relay]                          # Playwright, desktop and phone, on Monaco (needs cargo build --release)
```

## Python binding

`python/` is the crate `pbfcut-py` (PyO3, maturin, abi3 for Python 3.9 and later). It only converts arguments and results; all logic stays in `pbfcut::job`. Build and test:

```bash
maturin build --release -m python/Cargo.toml -o dist && pip install dist/pbfcut-*.whl
PBFCUT_BIN=target/release/pbfcut pytest python/tests      # the Python results equal the command line
```

`.github/workflows/python.yml` builds and tests the wheels, and publishes them to PyPI on a tag `v*` (trusted publishing). The tag must equal the version in `python/Cargo.toml`.

## Golden tests

`tests/golden.rs` compares the output with the hashes of the reference files (`tests/golden/monaco.txt`, from pbfcut 11276bc). `tests/cells.rs` does the same for the H3 cells (`tests/golden/monaco_cells.txt`). The input is `tests/data/monaco.osm.pbf`. The hash covers the blocks after the header block, because the header names the program.

If a golden test fails, a change altered the result. Find the cause. **Never edit the golden file to make a test pass.**

## Architecture

The library (`src/lib.rs`) has one module for each job. The command in `src/main.rs` is thin: it parses arguments and calls the library.

- `decode`: reads the data blocks of a `.osm.pbf` file in place, with no owned objects. It gives the elements to a `RawSink` (`elements`). `read_block` and `decode_block` are the entry points.
- `write`: `PbfWriter` writes the output. Blocks have 8000 elements, with no metadata, and the header says `Sort.Type_then_ID`.
- `boundary`: GeoJSON, `.poly`, WKT or a box (`Boundary::parse` finds the format from the text), with point-in-area tests by latitude band. `WORLD` is a boundary that holds every point.
- `areas`: the list of areas of `pbfcut areas` (GeoJSON FeatureCollection or osmium config), with a grid of their boxes.
- `tiles`: the Web Mercator tile grid and `tiles(boundary, z)`. Change the tile rule in this one function.
- `cells`: the H3 cells (`h3o`, no `std` feature, so `libm` math): `cell_of`, `cells(boundary, res)`, `near_cell`.
- `extract`: the cut of one area (`cut_to_pbf`, `cut_where`) in four passes. The input must be sorted by type, then id (as the files of Geofabrik).
- `tilecut` (and `tilecut/write.rs`): the cut to many tiles. It is the largest module. It reads the file two times. Read 1 only makes a plan. Read 2 writes each element straight into the files of its tiles. The cut keeps the tile of each node in memory as compressed runs of ids (`Book`). `Grid` is the only part that knows tiles from cells (the area of a point, the border, the file name). `cut_to_tiles`, `cut_to_cells` and `cut_to_areas` write files to a folder. For an area list, the number of a node in the book can be a set of areas (`Grid::Areas`, `Grid::each`). `cut_to_store` and `cut_to_sink` keep the tile files in a temporary store.
- `job`: the jobs of the command: `cut` (one cut to a folder, with the file names and the counts) and `compare`. The command and the Python binding call it.
- `store`: rows in temporary files, with an external sort that has a memory budget. The large lists of the cut live here, so they do not stay in memory.

## Rules that are easy to break

- **The output bytes must not depend on the target.** zlib output uses `miniz_oxide` (pure Rust). zlib input uses `flate2` with `zlib-rs`. Do not change these for a faster native library. The native build and the WebAssembly build must write the same file.
- **The code runs natively and on `wasm32`.** The paths with `#[cfg(target_arch = "wasm32")]` use a `Store` of the page (OPFS) and not temporary folders. Keep both paths working.
- **Memory is the main limit.** The cut keeps lists of ids, never the elements. A change that holds elements in memory can make large inputs (France, Sweden) fail.
- **Behavior must equal the reference files.** One known difference from `osmium extract`: a node exactly on a tile border goes to one tile, not two.
- Code comments and docs use ASD-STE100 Simplified Technical English (see the global instructions).
