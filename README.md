<h1 align="center">pbfcut</h1>

<p align="center">Cut an OpenStreetMap <code>.osm.pbf</code> file into map tiles, H3 cells or a list of areas, in one read of the file.</p>

<p align="center">
  <a href="https://khoshkhah.github.io/pbfcut/"><img alt="Online page" src="https://img.shields.io/badge/online_page-try_it-2563eb?style=flat-square"></a>
  <a href="https://khoshkhah.github.io/pbfcut/manual/"><img alt="Documentation" src="https://img.shields.io/badge/documentation-manual-2563eb?style=flat-square"></a>
  <a href="https://khoshkhah.github.io/pbfcut/manual/explanation/comparison/"><img alt="Comparison" src="https://img.shields.io/badge/comparison-osmium-2563eb?style=flat-square"></a>
  <a href="https://github.com/Khoshkhah/pbfcut/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/Khoshkhah/pbfcut/ci.yml?branch=main&label=CI&style=flat-square"></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-555?style=flat-square"></a>
</p>

- **Fast, with little memory.** pbfcut is faster than `osmium extract` and uses much less memory, also for hundreds of
  tiles. See the [comparison](https://khoshkhah.github.io/pbfcut/manual/explanation/comparison/).
- **Complete files.** Each output is a complete `.osm.pbf` file: each way with all its nodes, and each multipolygon and
  boundary with all its members ([the rules](https://khoshkhah.github.io/pbfcut/manual/explanation/rules/)).
- **Command line, Python, Rust library, or browser.** The same Rust code runs natively and in WebAssembly.

**Try it online:** https://khoshkhah.github.io/pbfcut/ (no installation; the file stays on your computer).

## Install

You must have Rust (https://rustup.rs):

```bash
cargo install --git https://github.com/Khoshkhah/pbfcut
```

Cargo puts the command `pbfcut` in `~/.cargo/bin`. The installer of Rust (rustup) adds this folder to `PATH`. Open a new
terminal after the installation. If the shell says `pbfcut: command not found`, add the folder to `PATH`:

```bash
export PATH="$HOME/.cargo/bin:$PATH"     # add this line to ~/.bashrc or ~/.zshrc
```

## Use

Download a region from Geofabrik (https://download.geofabrik.de), then cut it:

```bash
pbfcut tiles sweden-latest.osm.pbf --zoom 8 -o tiles/     # tiles/8_136_74.osm.pbf, ...
pbfcut h3 sweden-latest.osm.pbf --res 5 -o cells/         # cells/<cell index>.osm.pbf, ...
```

| Command | Output |
|---|---|
| `pbfcut tiles IN.osm.pbf --zoom Z -o DIR` | one file for each map tile of zoom `Z` |
| `pbfcut tiles IN.osm.pbf --tiles LIST.txt -o DIR` | one file for each tile of a list (also of different zooms) |
| `pbfcut h3 IN.osm.pbf --res R -o DIR` | one file for each H3 cell of resolution `R` |
| `pbfcut areas IN.osm.pbf --areas AREA.geojson -o DIR` | one area: one file with the data inside the area (a FeatureCollection with one feature) |
| `pbfcut areas IN.osm.pbf --areas AREAS.geojson -o DIR` | one file for each area of a list (as `osmium extract -c`; the areas must not overlap) |
| `pbfcut compare DIR_A DIR_B` | exit code 1 if the files of the two folders have different elements |

Options: `--boundary AREA.geojson`, `--border METRES`, `--complete TYPES`, `--memory SIZE` (for example `512M` on a
computer with little memory). See [the command line](https://khoshkhah.github.io/pbfcut/manual/reference/cli/).

## Use from Python

```bash
pip install pbfcut
```

```python
import pbfcut
r = pbfcut.cut_tiles("sweden-latest.osm.pbf", "tiles", zoom=8, memory="1G")
```

See [Python](https://khoshkhah.github.io/pbfcut/manual/reference/python/).

## Use from Rust

```toml
[dependencies]
pbfcut = { git = "https://github.com/Khoshkhah/pbfcut" }
```

`pbfcut::tilecut::cut_to_tiles` and `cut_to_cells` write the files into a folder. `cut_to_sink` and `cut_cells_to_sink`
give them to a function. `pbfcut::extract::cut_to_pbf` cuts one area.

## Documentation

- The manual: https://khoshkhah.github.io/pbfcut/manual/ (source in `docs/`)
- The online page: source in `web/`
- The design and the plan: [docs/design.md](docs/design.md)

MIT license.
