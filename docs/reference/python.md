# Python

The Python package runs the same Rust code as the command line, and gives the same files.

```bash
pip install pbfcut
```

There are wheels for Python 3.9 and later on Linux (x86_64, aarch64), macOS (arm64, x86_64) and Windows (x86_64). On
other computers, pip builds the package, and you must have [Rust](https://rustup.rs). The version on GitHub:
`pip install "git+https://github.com/Khoshkhah/pbfcut#subdirectory=python"`.

## Examples

```python
import pbfcut

# map tiles of zoom 8
r = pbfcut.cut_tiles("sweden-latest.osm.pbf", "tiles", zoom=8)
for f in r["files"]:
    print(f["file"], f["nodes"], f["ways"], f["relations"], f["bytes"])

# H3 cells of resolution 9 in a box, with 1 GiB of memory and 4 threads
pbfcut.cut_cells("monaco.osm.pbf", "cells", 9, boundary=(7.40, 43.72, 7.44, 43.76), memory="1G", threads=4)

# a list of areas that do not overlap, named by the property NAME
pbfcut.cut_areas("sweden-latest.osm.pbf", "counties.geojson", "counties", name="NAME")

# two folders with the same elements
assert pbfcut.compare("tiles", "tiles-before") == []
```

## Functions

| Function | Command line |
|---|---|
| `cut_tiles(input, out_dir, zoom=8, *, tiles=None, ...)` | `pbfcut tiles` |
| `cut_cells(input, out_dir, res, ...)` | `pbfcut h3` |
| `cut_areas(input, areas, out_dir, *, name="name", ...)` | `pbfcut areas` |
| `compare(dir_a, dir_b)` | `pbfcut compare` |

The options are keyword arguments, as on [the command line](cli.md):

| Argument | Value |
|---|---|
| `boundary` | A file (GeoJSON, `.poly` or WKT), or a box `(west, south, east, north)`. Default: all data. |
| `border` | Metres. Default: `0.0`. Not for `cut_areas`. |
| `complete` | The relation types to keep whole. Default: `("multipolygon", "boundary")`. |
| `memory` | Bytes, or a text such as `"4G"` or `"512M"`. Default: a quarter of the physical memory, at least 2G (on Windows: 2G). |
| `temp` | The folder of the temporary files. |
| `threads` | The number of threads. Default: all processors. |
| `compress` | `True` (default): zlib blocks. `False`: raw blocks, as `--no-compression`: a faster cut, larger files. |

Paths can be `str` or `pathlib.Path`.

## Results and errors

A cut gives a `dict`, the same as the JSON of the command line, with the size of each file:

```python
{"tiles": 4, "seconds": 0.21, "files": [
    {"tile": "14/8529/5974", "file": "14_8529_5974.osm.pbf", "nodes": 21943, "ways": 3577, "relations": 260, "bytes": 401977},
    ...]}
```

The result has no data: read the files with the OSM library of your choice. `compare` gives one line for each file
that is different; an empty list means equal folders.

An error raises `pbfcut.Error`. The cut releases the GIL, so other Python threads continue. The memory budget is one
setting for the whole process: do not run two cuts with different budgets at the same time.
