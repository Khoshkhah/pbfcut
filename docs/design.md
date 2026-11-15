# Design

This document is for developers. It gives the scope of pbfcut, its rules and the plan. The published manual does not
include it.

Status (2026-11-15): M1 is done. The crate, the commands `tiles` and `compare`, the golden tests, CI and the manual
exist. The next step is M1b, the memory budget.

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
  of many areas), `tiles`, `boundary`, `store` (rows in temporary files, the sort on disk), `elements`, `util`.
- Dependencies: `flate2` with `zlib-rs` and `miniz_oxide` (pure Rust, so every target writes the same bytes), `rayon`,
  `serde_json`. M2 adds `h3o` (H3 in pure Rust, the same cell ids as the H3 library 4; it builds for wasm32).
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
| H3 cells | `h3o` `LatLng::to_cell` at the finest resolution of the list, then its parents | no |
| GeoJSON areas | an index of the areas (a grid of their bounding boxes), then the point-in-polygon test of `Boundary` | yes |

For H3 with mixed resolutions, a coarse cell holds the points whose fine cell has it as its parent. The H3 parent
does not exactly cover its children, so the output of a coarse cell is the area of its children, not the hexagon of
the coarse cell. A list of cells must not have a cell and its parent.

GeoJSON areas can overlap, as in osmium, so a node can go to more than one area. The number in the book is then the
number of a set of areas: the cut keeps a table of the sets that it finds (one set for each overlap that has a node).
Without overlap, each set has one area, and the cut is the same as the tile cut.

The rule "each node in exactly one area" holds for map tiles, for H3 cells and for GeoJSON areas that do not overlap.
`pbfcut` writes the number of nodes in more than one area. With `--owned`, it stops with an error when areas overlap.

### 3.2 The options of the library

| Option | `cut_to_tiles` | `cut_to_store`, `cut_to_sink` |
|---|---|---|
| Output | files in a folder | a temporary store, or a sink (the browser) |
| zlib in the tile files | yes | no (a file that a program reads one time, soon after) |
| `border_m` | a way within this distance of a tile goes to that tile too | 0 |
| `NodeUses` | optional: a junctions file for each tile | none |
| Zooms | one zoom when the border is not 0, else mixed | one zoom, or mixed zooms |

`NodeUses` is a trait with two functions: the flags of the nodes of a way, and the flags of a node from all its uses.
With it, the cut writes for each tile a file of the nodes outside the tile with their flags, and the flag `HOME` when
the tile has all the ways of the node. The cut itself does not know about roads.

## 4. The memory budget

Requirements:

- The peak memory does not grow with the number of areas.
- The peak memory grows only a little with the size of the input.
- The peak memory stays in the budget (`--memory`, default 2G). A smaller budget makes the cut use more disk and more
  time. It does not make the cut fail, down to a minimum that the manual gives (planned: 256M).
- All large data goes to temporary files at once, and a step holds only the data that it needs.
- The bytes of the output files do not change with the budget.

Today the cut uses 1.3 to 1.5 GB for France and 1.5 to 1.8 GB for Sweden. Much of the data is already in temporary
files: the id files, the extra nodes (sorted rows), the relations, the parts of the tiles. This data is still in
memory:

| Data | Grows with | Change |
|---|---|---|
| The books of the nodes and the ways (runs of ids, compressed) | the input | In memory while it fits its share of the budget. Else the ways find the areas of their nodes with a sort on disk: the refs (node, way) sorted by node, then a merge with the node book on disk. |
| The write buffers of the temporary files (1 MiB each) | the number of id ranges | Smaller buffers, or a limit for all buffers. |
| The sorts (`store::sort`) | the input | They have a budget now (64 MiB). It becomes their share of `--memory`. |
| The parts that wait before they go to disk | fixed (64 MiB) | Their share of `--memory`. |
| The blocks in decode | the number of threads | At most one block for each thread and 32 MB, as now. |
| The lists of read 1 (`near`, `multi`, the homes) | the input | Into sorted rows on disk, as the extra nodes now. |
| The table of the areas (ids, counts, sets of areas) | the number of areas | Some bytes for each area. It stays in memory. |

The order of the work: first measure each part on Sweden and France (`tools/measure.sh`), then change only the parts
that need it. The sort on disk for the books is the last change, because it costs one more read of the refs.

Measurement and the guard in CI:

- On the Mac mini, natively: the peak RSS and the time for Monaco, Sweden and France, for map tiles at zoom 6, 8, 10
  and 12, and for `--memory` 512M, 2G and 4G. The results go into the manual.
- In CI: the test binary counts the bytes that it allocates (a global allocator that keeps the peak). This is exact
  on every computer, but RSS is not.
  - The peak heap of the Monaco cuts must stay under the budget in `tests/peak_heap.json`. The budget goes down with
    each improvement. It goes up only after a review.
  - The peak heap of Monaco at zoom 17 (many tiles) must be less than 1.2 times the peak at zoom 12 (few tiles).
  - Monaco with `--memory 16M` gives the same files as the goldens.

## 5. The command line

```text
pbfcut tiles   IN.osm.pbf (--zoom Z | --tiles LIST.txt) [OPTIONS] -o DIR
pbfcut h3      IN.osm.pbf (--res R | --cells LIST.txt)  [OPTIONS] -o DIR     (M2)
pbfcut areas   IN.osm.pbf --geojson AREAS.geojson [--owned] [OPTIONS] -o DIR  (M2)
pbfcut compare DIR_A DIR_B

OPTIONS: [--boundary AREA.geojson] [--complete TYPES] [--steps] [--memory SIZE] [--temp DIR]
```

- `h3 --res R`: all H3 cells of resolution `R` (0 to 15) that touch the boundary (no boundary: the bounding box of the
  file). `--cells LIST.txt`: a list of cell ids, one on each line, also of different resolutions. Each file is
  `DIR/<cell id>.osm.pbf`, for example `862b02d27ffffff.osm.pbf`.
- `areas`: each feature of the GeoJSON (Polygon, MultiPolygon, or a `bbox` member) is one area and one output, as
  the extracts of `osmium extract -c`. The file name comes from the property `name` of the feature, else from its
  number.
- `--memory SIZE` (M1b): the memory budget. `--temp DIR` (M1b): the folder of the temporary files.
- Every command writes the numbers of elements of each output to standard output as JSON.

## 6. The manual, GitHub Pages and the online page

The manual:

- mkdocs 1.6 with mkdocs-material 9. `mkdocs build --strict` runs in CI. All text in ASD-STE100 Simplified Technical
  English.
- Tutorials: cut a file to map tiles; to H3 cells; to your own GeoJSON areas.
- How-to guides: use pbfcut from Rust (native and wasm32); replace `osmium extract -c`; cut a large file with little
  memory.
- Reference: the command line, the GeoJSON format of the areas, the output files and the JSON of the counts.
- Explanation: the rules of the cut, how the cut works, the comparison with osmium (with the commands to do the
  measurement again).

GitHub Pages: `pages.yml` deploys the manual at `/pbfcut/manual/`, and from M3 the online page at `/pbfcut/`.
GitHub Pages cannot send the COOP and COEP headers that threads need, so the page uses `coi-serviceworker.js`.
`?v=<sha>` on each relative reference stops the cache of Pages (10 minutes) from giving old files.

The online page (M3):

1. Choose a `.osm.pbf` file. The page reads it from the disk and does not upload it.
2. Choose map tiles (a zoom), H3 cells (a resolution) or a GeoJSON file of areas, and an optional boundary.
3. See the areas on a map before the cut, with their number, and a warning when GeoJSON areas overlap.
4. Start the cut. It runs in WebAssembly with threads, in a worker. The temporary files go to OPFS.
5. See the progress: for each step, the time, the CPU use and the WebAssembly memory. A Stop button stops the cut.
6. Download the files. In Chromium on a computer, into a folder that you choose (`showDirectoryPicker`); else as one
   `.zip` file with no compression (the PBF blocks are already compressed).

The CPU use of a step is the busy time of all threads divided by the time of the step and the number of threads. The
map uses roadstyle's dashboard template and toolbox unchanged. Playwright tests on Monaco run for each kind of area,
on a computer and on a phone viewport (390 x 844), and make sure that the files of the page equal the files of the
command line.

## 7. Tests and CI

- Unit tests in each module.
- Golden tests on Monaco (`tests/golden.rs`): the hash (FNV-1a 64) of each output file, after the header block, for
  five cuts: map tiles at zoom 14; zoom 15 with a 25 m border; the store at zoom 14; the store with zooms 14 and 15;
  one area. The reference is `tests/golden/monaco.txt`. A golden file never changes to make a test pass. The bytes
  are equal on Linux x86-64 and on macOS arm64.
- CI on every push and pull request: `cargo test --release`, the wasm32 build, the command line on Monaco, and
  `mkdocs build --strict`.

## 8. Steps

1. **M1, done.** The crate, `tiles` and `compare`, the golden tests, CI, the manual and Pages.
2. **M1b, the memory budget.** The measurement, `--memory`, `--temp`, the changes of section 4 that the measurement
   asks for, and the peak-heap guard in CI.
3. **M2, H3 cells and GeoJSON areas.** The function "the areas of a point" for H3 and for GeoJSON, the sets of areas
   for overlap, the commands `h3` and `areas`, their golden tests, and a measurement against osmium on Sweden with the
   same areas.
4. **M3, the online page** of section 6, on GitHub Pages.

A step is done only when its part of the manual is done.
