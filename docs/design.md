# Design

This document is for developers. It gives the scope of pbfcut, its rules and the plan. The published manual does not
include it.

Status (2026-11-16): M1 and M1b are done: the crate, the commands `tiles` and `compare`, the golden tests, CI, the
manual, and the memory budget. From M2, these are done: `pbfcut h3 --res R` (H3 cells of one resolution), the command
`areas` (GeoJSON and osmium config lists of areas that do not overlap) and the boundary formats (GeoJSON, `.poly`, WKT,
`--bbox`). The Python binding is done (section 5.1). From M3, the browser page cuts to tiles, H3 cells and areas. Not
done: the list of cells of mixed resolutions (`--cells`), and on the page the CPU use.

## 1. Purpose

pbfcut reads an OpenStreetMap `.osm.pbf` file one time and writes one complete `.osm.pbf` file for each area. An area
is a map tile, an H3 cell or a GeoJSON polygon. It replaces `osmium extract -c` for many areas:

| Sweden (975 MB), Mac mini | pbfcut | osmium-tool 1.19.1 `extract -c`, `-s smart` |
|---|---|---|
| zoom 6, 21 tiles | 11.2 s, 1.8 GB | 50.9 s, 9.8 GB |
| zoom 8, 206 tiles | 15.2 s, 1.5 GB | stopped after 378 s (218 GB peak footprint), no data |

The two tools give the same elements. One difference: osmium gives a node exactly on a tile border to the two tiles,
because its bounding box includes its edge. pbfcut gives such a node to one tile.

osmium keeps an id set for each extract (its manual: 1 to 2 GB for each extract, at most 500 extracts). pbfcut keeps
the area of each node in one compressed list for all areas.

## 2. The crate

- One crate, `pbfcut`, for the native targets and for `wasm32-unknown-unknown`, with the same code.
- Modules: `decode` (the PBF decoder), `write` (the PBF writer), `extract` (the cut of one area), `tilecut` (the cut
  of many areas), `job` (the jobs of the command: one cut to a folder, with the file names, counts and sizes, and
  `compare`; the command and the Python binding call it), `tiles`, `boundary`, `store` (rows in temporary files, the sort on disk), `elements`, `util`.
- Dependencies: `flate2` with `zlib-rs` and `miniz_oxide` (pure Rust, so every target writes the same bytes), `rayon`,
  `serde_json`, `h3o` (H3 in pure Rust, the same cell ids as the H3 library 4; it builds for wasm32; no `std` feature,
  so its math is `libm` and the cells do not change with the target).
- Natively, the temporary files go into a folder. In the browser, the page gives a `Store` (OPFS) with
  `store::set_temp_store`.
- The output files are sorted by type, then by id (`Sort.Type_then_ID`), without metadata. The bytes do not depend on
  the target.

## 3. The rules of the cut

In this section, an "area" is a map tile, an H3 cell or a GeoJSON area.

1. Each node inside the boundary goes to each area that holds it (section 3.1). With map tiles and H3 cells, that is
   exactly one area.
2. A way with a node in an area goes to that area, complete: all its nodes, also the nodes outside the area.
3. A relation with a member in an area goes to that area, with its parent relations.
4. A relation of a type in `complete` goes to every area with a member, with all its members (ways with all their
   nodes). Then every output file is complete for that type.
5. An area with no node inside has no file.

### 3.1 The area of a node

The cut keeps the area of each node in a "book": runs of node ids, each with one number. The three kinds of areas use
the same book. Only the function "the areas of a point" changes:

| Kind | The areas of a point | Overlap |
|---|---|---|
| Map tiles | `Tile::of` at the finest zoom of the list, then its parents. A point on the line between two tiles goes to the tile east or south of the line. | no |
| H3 cells | `h3o` `LatLng::to_cell` at the resolution of the list (one resolution; the parents for a mixed list are not done) | no |
| GeoJSON areas | an index of the areas (a grid of their bounding boxes), then the point-in-polygon test of `Boundary` | no (an error) |

In the code, `tilecut::Grid` is "the areas of a point" for tiles and cells: the area of a point, the areas near a
point (the border; for a cell, the six neighbours of ring 1, so the border must be less than half the edge of a cell),
and the name of the file. The rest of the cut uses only the numbers of the areas. `cut_to_cells` and
`cut_cells_to_sink` are the cell forms of `cut_to_tiles` and `cut_to_sink`. `h3o` builds with no `std` feature:
its math is then `libm` (pure Rust), so the cell of a point does not change with the target. A cell list
(`cells::cells`) comes from a flood fill over the cells that touch the box of the boundary, then the test
`Boundary::touches_box` with the box of each cell.

For H3 with mixed resolutions, a coarse cell holds the points whose fine cell has it as its parent. The H3 parent
does not exactly cover its children, so the output of a coarse cell is the area of its children, not the hexagon of
the coarse cell. A list of cells must not have a cell and its parent.

The areas of a list must not overlap (Kaveh, 2026-11-16), so the rule "each node in exactly one area" holds for all
three kinds, and the area cut is the tile cut with another `Grid`. `Grid::of` gives the first area of the list that
holds the point, and tests the other areas of its grid cell: a point inside two areas and on the border of neither
sets `overlap`, and the cut stops after step 1 with an error that names the two areas. A point on a shared border
goes to the first area (osmium gives it to the two). Before this rule, PR 6 kept a set of areas for each node in an
overlap; the measurement on Sweden (10 areas) gave the same time and memory with the two designs, so the rule makes
the code shorter, not faster.

### 3.2 The options of the library

| Option | `cut_to_tiles` | `cut_to_store`, `cut_to_sink` |
|---|---|---|
| Output | files in a folder | a temporary store, or a sink (the browser) |
| zlib in the tile files | the option `compress` of each function (the command line and the page: on by default) | the same option (a temporary file that a program reads one time, soon after: off) |
| `border_m` | a way within this distance of a tile goes to that tile too | 0 |
| `NodeUses` | optional: a junctions file for each tile | none |
| Zooms | one zoom when the border is not 0, else mixed | one zoom, or mixed zooms |

`NodeUses` is a trait with two functions: the flags of the nodes of a way, and the flags of a node from all its uses.
With it, the cut writes for each tile a file of the nodes outside the tile with their flags, and the flag `HOME` when
the tile has all the ways of the node. The cut itself does not know about roads.

The home tile (or cell) of a node outside the area is the tile that holds the place of the node, when that tile has a file.
Else it is the first tile (in the order of the tiles) of the ways at the node. The cut reads again only the node blocks
with such nodes (they are few: the ways across the border). A program that makes rows near its own tile, for example the
connector of the dangling paths of duckOSM, then makes all rows of the node. Before 2026-11-17 the home was always the first
tile of the ways: in Tartu by tiles, a connector edge was lost, because the home of its node was the other tile.

## 4. The memory budget

Requirements:

- The peak memory does not grow with the number of areas.
- The peak memory grows only a little with the size of the input.
- The peak memory stays near the budget (`--memory`, default a quarter of the physical memory, at least 2G). A smaller budget makes the cut use more disk and
  more time. It does not make the cut fail.
- All large data goes to temporary files at once, and a step holds only the data that it needs.
- The bytes of the output files do not change with the budget.

### 4.1 How the cut keeps to the budget

`store::memory()` is the budget. The parts of the cut take these shares:

| Data | Share | How |
|---|---|---|
| The sort of the nodes from outside (step 4, read 2) | 1/4 | `store::SortedParts` sorts a bucket (a range of 2^27 node ids) in memory when it fits, else on disk with `store::sort`, and gives its rows in pieces. |
| The member ways of complete relations in each tile (`more`) | 1/4 (its sort) | Sorted rows on disk, read as a stream by step 3 and by read 2. A boundary goes to every tile with a member, so these pairs grow with the number of tiles. |
| The rows in flight in read 2 | 1/8 for each batch | A batch has at most `memory / 8 / 512` extra rows. A block with more rows goes in more jobs, one for each range of tiles. A tile is in one job only, and each job sees all rows of the block, so the bytes do not change. |
| The buffers of a merge on disk | the budget of its sort | `run_buffer`: from 16 KiB to 1 MiB for each run, at most 64 runs at a time. |
| The write buffers of the buckets | fixed | 64 KiB each, about 100 buckets at most. |

These parts do not follow the budget yet. They grow with the input, not with the number of tiles:

- the books of the nodes and the ways (Sweden: 72 MB, France: 243 MB at zoom 10);
- the lists of read 1 (`near`, `multi`, the homes) and the tiles of the relations.

A later change can put the books on disk: the refs (node, way) sorted by node, then a merge with the node book. It
costs one more read of the refs, so it waits until a measurement asks for it.

### 4.2 Measurement and the guard in CI

- `tools/measure.sh` gives the time, the peak RSS and the peak heap for each input and zoom (time and RSS from a run
  without `--steps`, because the count of the heap makes the threads slower). The manual has the results.
- In CI, `tests/memory.rs` counts the bytes that the test binary allocates. This is exact on every computer, but RSS
  is not.
  - The peak heap of Monaco at zoom 12 and zoom 17 must stay under the limits in `tests/peak_heap.json`. Lower a
    limit after an improvement; raise it only after a review of the cause.
  - Monaco at zoom 17 with a budget of 64 KiB (all sorts on disk, many small jobs) gives the same bytes as with 2G.
  - There is no test of zoom 17 against zoom 12 on Monaco: the output of zoom 17 is 24 times the output of zoom 12,
    and the blocks in flight hold their part of it. The growth with the number of tiles is measured on Sweden.

## 5. The command line

```text
pbfcut tiles   IN.osm.pbf (--zoom Z | --tiles LIST.txt) [OPTIONS] -o DIR
pbfcut h3      IN.osm.pbf (--res R | --cells LIST.txt)  [OPTIONS] -o DIR
pbfcut areas   IN.osm.pbf --areas AREAS.geojson|CONFIG.json [--name PROPERTY] [OPTIONS] [-o DIR]
pbfcut compare DIR_A DIR_B

OPTIONS: [--boundary FILE | --bbox W,S,E,N] [--border METRES] [--complete TYPES] [--steps] [--memory SIZE] [--temp DIR]
FILE: GeoJSON, an osmosis .poly file or WKT, found from the text
```

- `h3 --res R` (done): all H3 cells of resolution `R` (0 to 15) that touch the boundary (no boundary: the bounding box
  of the file). `--cells LIST.txt` (not done): a list of cell ids, one on each line, also of different resolutions. Each file is
  `DIR/<cell id>.osm.pbf`, for example `862b02d27ffffff.osm.pbf`.
- `areas` (done): each feature of a GeoJSON FeatureCollection (Polygon, MultiPolygon, or a `bbox` member) is one
  area and one output, as the extracts of `osmium extract -c`. The file name comes from the property `--name`
  (default `name`), else from the `id` of the feature, else from its number. An osmium config file (`extracts`) also
  works: the name from `output`, the area from `bbox`, `polygon` or `multipolygon` (coordinates or a `file_name`).
  `src/areas.rs` reads the list and has the grid of the boxes; `tilecut::Grid::Areas` gives the first area of a point
  and checks that no node is inside two areas. The tile cut keeps the home ways of nodes outside the boundary; the
  area cut does not, so an area file equals osmium.
- `--memory SIZE`: the memory budget. `--temp DIR`: the folder of the temporary files.
- Every command writes the numbers of elements of each output to standard output as JSON.

### 5.1 The Python binding

`python/` is the crate `pbfcut-py` (PyO3, maturin, abi3 for Python 3.9 and later). It is not a member of the library
crate: the library and the command do not depend on it.

- **Thin.** The binding only converts the arguments and the results. All logic is in `pbfcut::job`, so the Python
  files equal the files of the command line (Rust is the design, Python is a binding).
- **API.** `cut_tiles`, `cut_cells`, `cut_areas` and `compare`, with the options of the command line as keyword
  arguments. A cut returns the names, the element counts and the sizes of the files, not the data, so the memory stays
  in the budget.
- **GIL and errors.** The cut releases the GIL. An error is `pbfcut.Error` with the message of the library. The
  types are in `python/pbfcut.pyi`.
- **Process settings.** `--memory` and `--temp` are settings of the process (`store::set_memory`). The binding sets
  them for one cut and restores them. Two cuts at the same time with different settings are not supported.
- **Tests.** `python/tests` compares the Python results with the command line on Monaco, for tiles, H3 cells and areas.
- **Release.** `.github/workflows/python.yml` builds the wheels (Linux x86_64 and aarch64, macOS arm64 and x86_64,
  Windows x86_64), tests each one, and publishes to PyPI on a tag `v*` with trusted publishing (environment `pypi`).
  The tag must equal the version in `python/Cargo.toml`.

## 6. The manual, GitHub Pages and the browser page

The manual:

- mkdocs 1.6 with mkdocs-material 9. `mkdocs build --strict` runs in CI. All text in ASD-STE100 Simplified Technical
  English.
- Tutorials: cut a file to map tiles; to H3 cells; to your own GeoJSON areas.
- How-to guides: use pbfcut from Rust (native and wasm32); replace `osmium extract -c`; cut a large file with little
  memory.
- Reference: the command line, the GeoJSON format of the areas, the output files and the JSON of the counts.
- Explanation: the rules of the cut, how the cut works, the comparison with osmium (with the commands to do the
  measurement again).

GitHub Pages: `pages.yml` deploys the manual at `/pbfcut/manual/`, and from M3 the browser page at `/pbfcut/`.
GitHub Pages cannot send the COOP and COEP headers that threads need, so the page uses `coi-serviceworker.js`.
`?v=<sha>` on each relative reference stops the cache of Pages (10 minutes) from giving old files.

The browser page (M3):

1. Choose a `.osm.pbf` file. The page reads it from the disk and does not upload it.
2. Choose map tiles (a zoom), H3 cells (a resolution) or a GeoJSON file of areas, and an optional boundary.
3. See the areas on a map before the cut, with their number, and an error when GeoJSON areas overlap.
4. Start the cut. It runs in WebAssembly with threads, in a worker. The temporary files go to OPFS.
5. See the progress: for each step, the time, the CPU use and the WebAssembly memory. A Stop button stops the cut.
6. Download the files. In Chromium on a computer, into a folder that you choose (`showDirectoryPicker`); else as one
   `.zip` file with no compression (the PBF blocks are already compressed). The cut computes the CRC-32 of each
   file while it writes the file (`crc32fast` in the sink of `web/src/lib.rs`), so the `.zip` reads no file again.

The CPU use of a step is the busy time of all threads divided by the time of the step and the number of threads. The
map is MapLibre GL from a CDN, with a free OpenStreetMap base map and the outlines of the tiles, cells or areas: the
page has no build step for the map and no dependency on another project. Playwright tests on Monaco run for each kind of area,
on a computer and on a phone viewport (390 x 844), and make sure that the files of the page equal the files of the
command line.

## 7. Tests and CI

- Unit tests in each module.
- Golden tests on Monaco (`tests/golden.rs`): the hash (FNV-1a 64) of each output file, after the header block, for
  five cuts: map tiles at zoom 14; zoom 15 with a 25 m border; the store at zoom 14; the store with zooms 14 and 15;
  one area. The reference is `tests/golden/monaco.txt`. The H3 cells of resolution 9, with no border and with a 25 m
  border, are in `tests/golden/monaco_cells.txt` (`tests/cells.rs`). `tests/cells.rs` also compares each cell file of
  resolution 8 (no boundary) with the cut of one area (`extract::cut_where`, the nodes of the cell) by `pbfcut
  compare`. Inside a boundary the two cuts are different: the cut of many areas gives the ways at a node outside the
  boundary to one "home" area. A golden file never changes to make a test pass. The bytes
  are equal on Linux x86-64 and on macOS arm64.
- CI on every push and pull request: `cargo test --release`, the wasm32 build, the command line on Monaco, and
  `mkdocs build --strict`. The workflow `python.yml` builds the wheels and runs the Python tests on each platform.

## 8. Steps

1. **M1, done.** The crate, `tiles` and `compare`, the golden tests, CI, the manual and Pages.
2. **M1b, done.** The memory budget (section 4), `--memory`, `--temp`, the heap in `--steps`, and the peak-heap guard
   in CI.
3. **M2, H3 cells and GeoJSON areas.** Done: the function "the areas of a point" for H3 and for area lists (the areas
   must not overlap), the commands `h3 --res` and `areas`, the boundary formats, their golden tests, and the
   measurements. Not done: `h3 --cells` (mixed resolutions).
4. **M3, the browser page** of section 6, on GitHub Pages. Done: map tiles, H3 cells and areas (a boundary in all
   formats, a border, the threads),
   a region of Geofabrik or a local file, the progress and the steps, "Save all to a folder", the e2e test
   (`web/e2e.py`). Geofabrik sends no CORS headers: the page gives a download link, and Get puts the
   file into OPFS through a relay, in 8 parts at the same time with `Range` (Luxembourg, 48 MB: 11 s, against 30 s with one
   connection; the dated file of `Content-Location`, one ETag, a part that fails starts again at its last byte; the test
   is `web/get_e2e.py`) (the default is the owner's Cloudflare Worker; `web/serve.py` has one for tests). The map before the cut: the
   plan of the library (`plan` in `web/src/lib.rs`), at most 20,000 files, the outline of a Geofabrik region as the
   default boundary (`web/www/regions/`, from Geofabrik's `index-v1.json` by `web/regions.py`). "Download all as .zip" for each
   browser (`web/www/zip.js`: stored files, CRC-32, ZIP64 above 4 GiB; a Blob of slices of the OPFS file, so no more
   storage). In place of the CPU use (the browser gives no CPU time of a thread): the wait of the thread of the cut
   for the input file and for the storage, for each step (France, zoom 8, on the Mac: less than 1 s of storage in
   each step, so the long steps are work on the threads).

A step is done only when its part of the manual is done.
