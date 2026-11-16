<h1 align="center">pbfcut</h1>

<p align="center">Cut an OpenStreetMap <code>.osm.pbf</code> file into map tiles, H3 cells or your own areas, in one read.</p>

<p align="center">
  <a href="https://khoshkhah.github.io/pbfcut/"><img alt="Online page" src="https://img.shields.io/badge/online_page-try_it-2563eb?style=flat-square"></a>
  <a href="https://khoshkhah.github.io/pbfcut/manual/"><img alt="Documentation" src="https://img.shields.io/badge/documentation-manual-2563eb?style=flat-square"></a>
  <a href="https://khoshkhah.github.io/pbfcut/manual/explanation/comparison/"><img alt="Comparison" src="https://img.shields.io/badge/comparison-osmium-2563eb?style=flat-square"></a>
  <a href="https://github.com/Khoshkhah/pbfcut/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/Khoshkhah/pbfcut/ci.yml?branch=main&label=CI&style=flat-square"></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-555?style=flat-square"></a>
</p>

- **Fast, with little memory.** Faster than `osmium extract`, with much less memory, also for thousands of tiles.
- **Complete files.** Each way comes with all its nodes, and each multipolygon with all its members.
- **Command line, Python, Rust or browser.** The same code everywhere, also in WebAssembly.

**Try it online:** <https://khoshkhah.github.io/pbfcut/>. No installation, and the file stays on your computer.

## Install

```bash
cargo install --git https://github.com/Khoshkhah/pbfcut     # command line (needs Rust: https://rustup.rs)
pip install pbfcut                                          # Python
```

## Use

```bash
pbfcut tiles sweden-latest.osm.pbf --zoom 8 -o tiles/                     # tiles/8_136_74.osm.pbf, ...
pbfcut h3    sweden-latest.osm.pbf --res 5 -o cells/                      # cells/<H3 index>.osm.pbf, ...
pbfcut areas sweden-latest.osm.pbf --areas counties.geojson -o counties/  # counties/<name>.osm.pbf, ...
```

```python
import pbfcut
pbfcut.cut_tiles("sweden-latest.osm.pbf", "tiles", zoom=8)
```

Get the input files from [Geofabrik](https://download.geofabrik.de). All commands and options are in
[the manual](https://khoshkhah.github.io/pbfcut/manual/reference/cli/).

## Documentation

- [The manual](https://khoshkhah.github.io/pbfcut/manual/): tutorials, reference, and how the cut works.
- [docs/design.md](docs/design.md): the design and the plan, for developers.

MIT license.
