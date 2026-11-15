# pbfcut

pbfcut cuts an OpenStreetMap `.osm.pbf` file into map tiles. It reads the file one time for all tiles, and each tile
file is a complete `.osm.pbf` file:

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

## Install and use

```bash
cargo install --git https://github.com/Khoshkhah/pbfcut
```

```bash
pbfcut tiles region.osm.pbf --zoom 8 -o tiles/
```

The manual: https://khoshkhah.github.io/pbfcut/manual/ (source in `docs/`). The design and the plan:
[docs/design.md](docs/design.md).

## Where it comes from

The code is the tile cut of [duckOSM](https://github.com/Khoshkhah/duckOSM) at commit e3ce06a. The test
`tests/golden.rs` makes sure that pbfcut gives the same tile files as that commit. duckOSM and
[duckOSM Studio](https://github.com/Khoshkhah/duckosm-studio) will use pbfcut as a Git dependency.

MIT license.
