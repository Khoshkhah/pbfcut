# pbfcut

pbfcut cuts an OpenStreetMap `.osm.pbf` file into map tiles or H3 cells. It reads the file one time for all tiles or
cells, and each output file is a complete `.osm.pbf` file. In this list, an area is a tile or a cell:

- the nodes in the area;
- each way with a node in the area, with all its nodes;
- each relation with a member in the area, with its parent relations;
- each multipolygon and boundary relation with a member in the area, with all its members.

| Command | Output |
|---|---|
| `pbfcut tiles IN.osm.pbf --zoom Z -o DIR` | `DIR/z_x_y.osm.pbf` for each map tile (Web Mercator) of zoom `Z` with data |
| `pbfcut tiles IN.osm.pbf --tiles LIST.txt -o DIR` | the same, for a list of tiles, also of different zooms |
| `pbfcut h3 IN.osm.pbf --res R -o DIR` | `DIR/<cell index>.osm.pbf` for each H3 cell of resolution `R` with data, for example `881eb4cb2dfffff.osm.pbf` |
| `pbfcut compare DIR_A DIR_B` | exit code 1 if the elements of the files of the two folders are different |

All cuts take `--boundary AREA.geojson` (only the nodes inside), `--border METRES` (also the ways with a node near the
area), `--complete TYPES` and `--memory SIZE`. The H3 cells come from `h3o`, a pure-Rust H3 library with the cell ids
of the H3 library 4.

Sweden (975 MB) on a Mac mini (12 cores, 16 GB):

| Tiles | pbfcut | osmium-tool 1.19.1 `extract -c -s smart` |
|---|---|---|
| zoom 6, 21 tiles | 11.2 s, 1.8 GB | 50.9 s, 9.8 GB |
| zoom 8, 206 tiles | 15.2 s, 1.5 GB | stopped after 378 s, no data |

The same code builds for WebAssembly (`wasm32-unknown-unknown`). The online page cuts a file in your browser, with no
installation: https://khoshkhah.github.io/pbfcut/ (source in `web/`). The command line is about 1.6 times faster than
the online page (Sweden, zoom 8, 12 threads, Mac mini: 12.7 s against 19.8 s in Chromium).

The H3 cells of resolution 6 (21,618 files) take 94 s and 1.6 GB of heap; with `--memory 512M`, 180 s and 755 MB
([Speed and memory](https://khoshkhah.github.io/pbfcut/manual/explanation/speed-and-memory/)).

Planned: a list of H3 cells of mixed resolutions, GeoJSON areas that can overlap (as the extracts of `osmium extract -c`).

## Install and use

You must have Rust and Cargo (https://rustup.rs). Cargo compiles pbfcut from its source code:

```bash
cargo install --git https://github.com/Khoshkhah/pbfcut
```

Download the `.osm.pbf` file of an area from Geofabrik (https://download.geofabrik.de), then cut it:

```bash
pbfcut tiles sweden-latest.osm.pbf --zoom 8 -o tiles/
pbfcut h3 sweden-latest.osm.pbf --res 5 -o cells/
```

On a computer with little memory, add `--memory 512M`: the cut then uses less memory and more time, and gives the same
files.

The manual: https://khoshkhah.github.io/pbfcut/manual/ (source in `docs/`). The design and the plan:
[docs/design.md](docs/design.md).

## Use it from Rust

```toml
[dependencies]
pbfcut = { git = "https://github.com/Khoshkhah/pbfcut" }
```

`pbfcut::tilecut::cut_to_tiles` writes the tile files into a folder, and `pbfcut::tilecut::cut_to_cells` the cell
files (the list of cells from `pbfcut::cells::cells`). `cut_to_sink` and `cut_cells_to_sink` give the same files to a
function, for example in the browser. `pbfcut::extract::cut_to_pbf` cuts one area.

MIT license.
