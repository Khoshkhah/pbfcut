# pbfcut

pbfcut cuts an OpenStreetMap `.osm.pbf` file into map tiles or H3 cells. It reads the file one time for all tiles or
cells, and each output file is a complete `.osm.pbf` file:

- the nodes in the tile;
- each way with a node in the tile, with all its nodes;
- each relation with a member in the tile, with its parent relations;
- each multipolygon and boundary relation with a member in the tile, with all its members.

Sweden (975 MB) on a Mac mini (12 cores, 16 GB):

| Tiles | pbfcut | osmium-tool 1.19.1 `extract -c -s smart` |
|---|---|---|
| zoom 6, 21 tiles | 11.2 s, 1.8 GB | 50.9 s, 9.8 GB |
| zoom 8, 206 tiles | 15.2 s, 1.5 GB | stopped after 378 s, no data |

The same code builds for WebAssembly (`wasm32-unknown-unknown`), so a browser page can cut a file.

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
files (the list of cells from `pbfcut::cells::cells`). `pbfcut::extract::cut_to_pbf` cuts one area.

MIT license.
