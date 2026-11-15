# Design

Status: accepted (2026-11-15). Kaveh chose the name `pbfcut` (repository `Khoshkhah/pbfcut`, crate and command
`pbfcut`). M1 is done: the crate, the command `tiles` and `compare`, the golden tests and CI.

Kaveh's requirements: one read of the input gives all outputs, for three kinds of areas:

- map tiles (one zoom, or mixed zooms);
- H3 cells (one resolution, or mixed resolutions);
- a list of areas from GeoJSON, as the extracts of osmium. The areas can overlap.

## 1. Purpose

The tile cut of duckOSM reads a region file and writes one complete `.osm.pbf` file for each tile. It is now a part of
`duckosm-core`, but it does not use the network, the tables or DuckDB. Other tools can use it as a replacement for
`osmium extract`:

| Sweden (975 MB), Mac mini | duckOSM tile cut (e3ce06a) | osmium-tool 1.19.1 `extract -c`, `-s smart` |
|---|---|---|
| zoom 6, 21 tiles | 11.2 s, 1.8 GB | 50.9 s, 9.8 GB |
| zoom 8, 206 tiles | 15.2 s, 1.5 GB | stopped after 378 s (218 GB peak footprint), no data |

The two cuts give the same elements. One difference: osmium gives a node exactly on a tile border to the two tiles,
because its bounding box includes its edge. Our cut gives such a node to one tile.

The new repository holds only the cut: the PBF decoder, the PBF writer, the tiles, the boundary and the cut. duckOSM
and duckOSM Studio use it as a Git dependency at one `rev`.

## 2. What moves and what stays

The source is duckOSM `main` at e3ce06a, `rust/duckosm-core/src/`.

### 2.1 Into the new crate

| duckOSM file | Lines | New module | What it holds |
|---|---|---|---|
| `pbfraw.rs` | 456 | `decode` | `read_block`, `decode_block`, `decode_block_tags`, `Buffers` |
| `cutfile.rs` (without `write_pbf`, `write_xml`) | about 330 | `write` | `PbfWriter`, the blocks, `BLOCK`, the zlib blobs (`miniz_oxide`) |
| `cutstream.rs` | 652 | `extract` | `each_block`, `header`, `CutCounts`, `cut_where`, `cut_to_pbf` (one area, four passes) |
| `tilecut.rs`, `tilecut/write.rs` | 1895 | `tilecut` | `cut_to_tiles`, `cut_to_store`, `cut_to_store_build`, `cut_to_sink`, `TileFiles`, `Chunks`, the junctions file |
| `tiles.rs` | 90 | `tiles` | `Tile`, `tiles`, `near_tile` |
| `boundary.rs` | 341 | `boundary` | `Boundary` (GeoJSON Polygon and MultiPolygon) |
| `spill.rs`, lines 1-480 and 766-857 | about 570 | `store` | `Store`, `Blob`, `MemStore`, `DirStore`, `set_temp_store`, `temp_store`, `Codec`, `Writer`, `Rows`, `sort`, `sort_bytes`, `sorted`, `Buckets`, `SortedParts` |
| `raw.rs`: `RawSink`, `RawNode`, `RawWay`, `RawTags` | about 40 | `elements` | the elements and the receiver of the decoder |
| `graph.rs`: `Relation`, `MemberType` | about 15 | `elements` | the relation as read |
| `util.rs`: `step`, `quiet`, `STEP_HOOK`, `trim_memory`, `FxHashMap`, `FxHashSet` | about 50 | `util` | the progress hook and the hash maps |
| `examples/tile_cut.rs` | about 150 | `pbfcut compare` | the element-equal check of two folders of tile files |

Total: about 4600 lines of Rust, with their unit tests.

### 2.2 In duckOSM

- `pbf.rs` (the `osmpbf` reader, `read_area_raw`, `BlockIndex`).
- `raw.rs` without the element types: `Raw`, the fixes, `read_part`, `RawPatch`.
- `cutfile::write_pbf` and `cutfile::write_xml` (they read `Raw`). They use `pbfcut::write::PbfWriter`.
- `spill.rs`: `EdgeCodec` and the module `join` (the network step). They use `pbfcut::store`.
- `network.rs`: `JunctionUses`, `global_junction`. They give the junction flags to the cut through a hook (section 4).
- `quack.rs`, `duckosm-build` (`union.rs`, `quack_tiles.rs`, `cut_file`), and all tests that compare the cut with
  `read_area_raw` (they need `osmpbf`).

duckOSM keeps its old paths with re-exports, so its callers do not change:

```rust
// duckosm-core/src/lib.rs
pub use pbfcut::{boundary, tiles};
pub mod pbfraw { pub use pbfcut::decode::*; }
pub mod cutstream { pub use pbfcut::extract::*; }
pub mod tilecut { pub use pbfcut::tilecut::*; }
```

`spill.rs` and `util.rs` re-export the moved parts. Then there is one temporary store and one step hook for duckOSM,
for Studio and for the cut.

## 3. The crate

- One crate, `pbfcut`, for the native targets and for `wasm32-unknown-unknown`. The code is the same code that Studio
  builds now.
- Dependencies: `flate2` (with `zlib-rs`, pure Rust), `miniz_oxide`, `rayon`, `serde_json` (the boundary), `h3o`
  (H3 in pure Rust, the same cell ids as the H3 library 4 that Kaveh's Python projects use). No C library, no
  `osmpbf`, no DuckDB. `h3o` 0.11 already builds for `wasm32-unknown-unknown`: Studio builds it now through
  `duckosm-build`.
- Natively, the temporary files go into a folder. In the browser, the page gives a `Store` (OPFS), as Studio does now
  with `set_temp_store`.
- The output tile files are sorted by type, then by id (`Sort.Type_then_ID`), without metadata. The bytes do not
  depend on the target.

## 4. The rules of the cut

In this section, an "area" is a map tile, an H3 cell or a GeoJSON area. These rules apply to every variant:

1. Each node inside the boundary goes to each area that holds it (section 4.1). With map tiles and H3 cells, that is
   exactly one area.
2. A way with a node in an area goes to that area, complete: all its nodes, also the nodes outside the area.
3. A relation with a member in an area goes to that area, with its parent relations.
4. A relation of a type in `complete` goes to every area with a member, with all its members (ways with all their
   nodes). Then every output file is complete for that type. quack uses `multipolygon` and `boundary`.
5. An area with no node inside has no file.

### 4.1 The area of a node

The cut now keeps the tile of each node in a "book": runs of node ids, each with one number. The three kinds of areas
use the same book. Only the function "the areas of a point" changes:

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

quack's rule "each row in exactly one area" holds for map tiles, for H3 cells and for GeoJSON areas that do not
overlap. `pbfcut` writes the number of nodes in more than one area. With `--owned`, it stops with an error when areas
overlap.

The variants are options of one function. They are not different code:

| Option | quack (`cut_to_store`, `cut_to_sink`) | The build by tiles (`cut_to_tiles`, `cut_to_store_build`) |
|---|---|---|
| `complete` | `multipolygon`, `boundary` | `multipolygon`, or none |
| zlib in the tile files | no (a temporary file that is read soon) | yes |
| border (`border_m`) | 0 | `2 * snap_m + 5` m: a way near the border of a tile goes to that tile too |
| junctions file | no | yes |
| zooms | one zoom, or mixed zooms (no tile in another tile) | one zoom when the border is not 0 |

The junctions file is the only part of the cut that knows about roads. In the new crate, the cut does not decide
which node is a junction. duckOSM gives two functions through a trait:

```rust
pub trait NodeUses: Sync {
    /// The uses of the nodes of a way (node, flags), from its tags and its nodes.
    fn of_way(&self, tags: &[(&str, &str)], refs: &[i64], out: &mut Vec<(i64, u8)>);
    /// The flags of a node from all its uses; 0: no row in the junctions file.
    fn of_node(&self, uses: &mut dyn Iterator<Item = u8>) -> u8;
}
```

The cut adds the flag `HOME` and writes the file in the format `DOSMJN02`, as now. duckOSM implements the trait with
`JunctionUses::of_way` and `global_junction`. With no `NodeUses`, the cut writes no junctions file.

## 4b. The memory budget

Kaveh's requirement: the native cut also has a memory limit. All large data goes to temporary files at once, and a
step holds only the data that it needs. These are the targets:

- The peak memory does not grow with the number of areas. osmium keeps an id set for each extract: 218 GB at 206
  tiles of Sweden.
- The peak memory grows only a little with the size of the input.
- The peak memory stays in the budget (`--memory`, default 2G). A smaller budget makes the cut use more disk and more
  time. It does not make the cut fail, down to a minimum that the manual gives (planned: 256M).

Today (e3ce06a), the native cut uses 1.3 to 1.5 GB for France and 1.5 to 1.8 GB for Sweden. Much of the data is
already in temporary files: the id files, the extra nodes (sorted rows), the relations, the parts of the tiles. This
data is still in memory:

| Data | Grows with | Change |
|---|---|---|
| The books of the nodes and the ways (runs of ids, compressed) | the input | In memory while it fits its share of the budget. Else the ways find the areas of their nodes with a sort on disk: the refs (node, way) sorted by node, then a merge with the node book on disk. |
| A write buffer for each area (64 KB) | the number of areas | One pool for all buffers, with a limit. When the pool is full, the largest buffer goes to the part file of its area. 10,000 tiles then need no 640 MB of buffers. |
| The sorts (`store::sort`) | the input | They have a budget now. It becomes their share of `--memory`. |
| The blocks in decode | the number of threads | At most one block for each thread and 32 MB, as now. |
| The lists of read 1 (`near`, `multi`, the homes) | the input | Into sorted rows on disk, as the extra nodes now. |
| The table of the areas (ids, counts, sets of areas) | the number of areas | Some bytes for each area. It stays in memory. |

The cut divides the budget among these parts at the start, and writes the division with `--steps`. The order of the
work: first measure each part on Sweden and France, then change only the parts that need it. The sort on disk for the
books is the last change, because it costs one more read of the refs.

The bytes of the output files do not change with the budget. The tests make sure of this.

### Measurement and the guard in CI

- On the Mac mini, natively: the peak RSS (`/usr/bin/time -l`) and the time for Monaco, Sweden and France, for map
  tiles at zoom 6, 8 and 10, for H3 cells at resolution 4, 5 and 6, and for `--memory` 512M, 2G and 4G. The results go
  into the manual (the comparison with osmium).
- In CI: the test binary counts the bytes that it allocates (a global allocator that keeps the peak). This is exact on
  every computer, but RSS is not.
  - The peak heap of the Monaco cuts must stay under the budget in `tests/peak_heap.json`. The budget changes down
    with each improvement. It goes up only with Kaveh's OK.
  - The peak heap of Monaco at zoom 17 (many tiles) must be less than 1.2 times the peak at zoom 12 (few tiles). This
    test fails when the memory grows with the number of areas.
  - Monaco with `--memory 16M` gives the same files as the goldens. This test runs the paths on disk.

## 5. The command line

One read of the input file gives all output files.

```text
pbfcut tiles   IN.osm.pbf (--zoom Z | --tiles LIST.txt) [OPTIONS] -o DIR
pbfcut h3      IN.osm.pbf (--res R | --cells LIST.txt)  [OPTIONS] -o DIR
pbfcut areas   IN.osm.pbf --geojson AREAS.geojson [--owned] [OPTIONS] -o DIR
pbfcut compare DIR_A DIR_B

OPTIONS: [--boundary AREA.geojson] [--complete multipolygon,boundary] [--no-zlib] [--memory SIZE] [--temp DIR]
```

- `--memory SIZE` (for example `4G`): the memory budget of the cut (section 4b). Default: 2G.
- `--temp DIR`: the folder of the temporary files. Default: the temporary folder of the system.

- `tiles --zoom`: the tiles of one zoom that touch the boundary (no boundary: the world). `--tiles`: a list of tiles
  `z/x/y`, one on each line, also of different zooms. A tile must not be in another tile of the list. Each file is
  `DIR/z_x_y.osm.pbf`.
- `h3 --res`: the cells of one resolution that touch the boundary. `--cells`: a list of H3 cell ids, one on each line,
  also of different resolutions. Each file is `DIR/<cell id>.osm.pbf` (for example `862b02d27ffffff.osm.pbf`, as the
  files of h3-routing-platform).
- `areas`: each feature of the GeoJSON (Polygon, MultiPolygon, or a `bbox` member) is one area and one output, as the
  extracts of `osmium extract -c`. The file name comes from the property `name` of the feature, else from its number.
  One feature gives the cut of one area (`cut_where`, the rules of osmium's `smart` cut).
- `compare`: the same files with the same elements (the check of `examples/tile_cut.rs`).
- Every command writes the numbers of elements of each output to standard output as JSON.

## 5a. The manual, GitHub Pages and the online page

Kaveh's requirements: a good manual, GitHub Pages, and an online dashboard.

### The manual

- mkdocs 1.6 with mkdocs-material 9, as duckOSM and Studio. `mkdocs build --strict` runs in CI.
- All text in ASD-STE100 Simplified Technical English.
- The parts (Diátaxis):
  - Tutorials: cut Monaco to map tiles; cut an area to H3 cells; cut with your own GeoJSON areas.
  - How-to guides: use `pbfcut` from Rust (native and wasm32); replace `osmium extract -c`; cut a large file
    with little memory.
  - Reference: the command line, the GeoJSON format of the areas, the output files and the JSON of the counts, the
    rules of the cut (section 4), the library API (a link to `cargo doc`).
  - Explanation: how the cut works (the two reads, the book, the id files), and the comparison with osmium (section
    1, with the commands to do the measurement again).

### GitHub Pages

- `pages.yml`, as in Studio: on each push to `main`, it builds the wasm module, the online page and the manual, and
  deploys them. The page is at `https://khoshkhah.github.io/pbfcut/`, the manual at `/manual/`.
- GitHub Pages cannot send the COOP and COEP headers that threads need. The page uses `coi-serviceworker.js`, as
  Studio. `serve.py` sends the headers for the local tests.
- `version_refs.py` adds `?v=<sha>` to each relative reference, as in Studio, because Pages keeps files in its cache
  for 10 minutes.

### The online page

The steps of the person:

1. Choose a `.osm.pbf` file. The page reads it from the disk (a `File`), and does not upload it.
2. Choose the kind of areas: map tiles (a zoom), H3 cells (a resolution), or a GeoJSON file of areas. An optional
   boundary limits the tiles and the cells.
3. Look at the areas on the map before the cut. The page shows their number. It shows a warning when GeoJSON areas
   overlap.
4. Start the cut. It runs in WebAssembly, with threads, in a worker. The temporary files go to OPFS, as in Studio's
   `quack.html`.
5. Look at the progress: for each step, the time, the CPU use and the WebAssembly memory. A Stop button stops the cut.
6. Download the output files. In Chromium on a computer, the page writes them into a folder that the person chooses
   (`showDirectoryPicker`). Else it gives one `.zip` file. The zip has no compression, because the PBF blocks are
   already compressed.

How the page gets the numbers:

- The step names come from the step hook of the cut (`util::step`), as in Studio's `quack.html`.
- The CPU use of a step is the busy time of all threads divided by the time of the step and the number of threads.
  The cut adds the busy time of each thread of its pool to a counter for each step. This is the same natively, so the
  command line can show it with `--steps`.
- The memory is the size of the WebAssembly memory at the end of each step.

The map uses roadstyle's dashboard template and toolbox unchanged, as Studio does. `build.sh` copies the dashboard
at one roadstyle commit. The tiles, the cells and the areas are one layer on that map. If the page needs a part that
the dashboard does not have, that is a change in roadstyle, not a patch in pbfcut.

Tests: Playwright e2e tests on Monaco, for each kind of area, on a computer viewport and on a phone viewport (390 x
844). A test makes sure that the files that the page gives equal the files of the command line (`pbfcut compare`).
Each page change needs the phone-viewport check before a merge.

## 6. How duckOSM and Studio use it

- duckOSM: `pbfcut = { git = "https://github.com/Khoshkhah/pbfcut.git", rev = "<sha>" }` in
  `rust/duckosm-core/Cargo.toml`.
- duckOSM Studio: no direct dependency. It gets `pbfcut` through `duckosm-core`, and `Cargo.lock` holds the `rev`.
  The calls `duckosm_core::tilecut::cut_to_sink` and `duckosm_core::cutstream::cut_to_pbf` stay the same.
- A change of the cut: merge it in `pbfcut`, move the pin in duckOSM, then move the duckOSM pin in Studio. This is the
  same as the roadstyle and duckOSM pins of Studio now.

## 7. Tests and CI

- The unit tests of the moved files move with them.
- Golden test on Monaco (`tests/data/monaco.osm.pbf`, `monaco.geojson`): the SHA-256 of each tile file and each
  junctions file that duckOSM e3ce06a gives for these cuts:
  1. quack: the world boundary, zoom 14, `complete = multipolygon, boundary`, no zlib.
  2. The build by tiles: the Monaco boundary, zoom 14, border 25 m, `complete = multipolygon`, with junctions.
  3. Mixed zooms: zoom 13 and zoom 15 tiles that do not overlap.
  4. `cut_to_pbf` of the Monaco boundary.

  `tools/golden` made the golden file (`tests/golden/duckosm-e3ce06a.txt`) one time, from duckOSM e3ce06a. The hash
  covers the blocks after the header block, because the header names the program that wrote the file. A golden file
  never changes to make a test pass. The bytes are equal on Linux x86-64 and on macOS arm64.
- In duckOSM, all present tests stay and must pass after the move: the cut against `read_area_raw`, the build by
  tiles against the whole build, and the quack goldens.
- CI on every push and pull request: `cargo test --release`, `cargo build --target wasm32-unknown-unknown`,
  `mkdocs build --strict`. No osmium in CI. `cargo fmt --check` and `cargo clippy -D warnings` come after M2: until then
  the code stays line for line as in duckOSM, so that a change there is easy to copy here. The comparison with osmium is a script for the Mac mini.

## 8. Steps

1. **M1, the new repository.** Copy the files of section 2.1 from duckOSM e3ce06a, with their history if possible
   (`git filter-repo` on the paths). Add the trait `NodeUses`, the command `tiles` and `compare`, the golden tests and
   CI. Add the manual with the first tutorial and the reference of `tiles`, and `pages.yml` for the manual. duckOSM
   does not change.
2. **M1b, the memory budget.** `--memory`, `--temp`, the pool of write buffers, the lists of read 1 on disk, the
   peak-heap guard in CI, and the measurement on the Mac mini (section 4b). The outputs stay equal to the goldens.
3. **M2, duckOSM uses the crate.** One pull request: the dependency, the re-exports, `NodeUses` in duckOSM, and the
   moved files removed. All duckOSM tests and goldens must pass with no change.
4. **M3, Studio.** Move the duckOSM pin. The page e2e tests (`quack_e2e.py`, `e2e.py`) must pass.
5. **M4, H3 cells and GeoJSON areas.** The function "the areas of a point" for H3 and for GeoJSON, the sets of areas
   for overlap, the commands `h3` and `areas`, and their golden tests on Monaco. Then a measurement against osmium on
   Sweden on the Mac mini, with the same GeoJSON areas. The manual gets their tutorials and reference.
6. **M5, the online page.** The page of section 5a, the CPU use of each step, the e2e tests on a computer and on a
   phone, and the page on GitHub Pages.

The manual grows with each step: a step is done only when its part of the manual is done.

From M1 to M2, a change to the cut in duckOSM must also go to the new repository. After M2, the cut changes only in
the new repository. The session "Tool scalability for large extracts" changes `tilecut.rs` now. M1 starts at its last
merged commit, and M2 waits until it has no open change to the cut files.

## 9. Decisions for Kaveh

1. The name: `pbfcut` (decided by Kaveh, 2026-11-15).
2. Your OK to create `github.com/Khoshkhah/pbfcut` now (public, MIT, as duckOSM).
3. Your OK for the design, before code moves out of duckOSM (M2).
