# pbfcut

Cut an OpenStreetMap `.osm.pbf` file into map tiles, H3 cells or your own areas, in one read. Each output is a
complete `.osm.pbf` file: each way comes with all its nodes, and each multipolygon with all its members.

The cut is the Rust code of pbfcut. This package runs it from Python, and gives the same files as the command line.

**Try it online:** <https://khoshkhah.github.io/pbfcut/>. The same cut in the browser, with no installation. The file
stays on your computer.

- **Fast, with little memory.** Faster than `osmium extract`, with much less memory, also for thousands of tiles.
- **Three kinds of cut.** Map tiles (`z_x_y`), H3 cells, or a list of areas (GeoJSON or an `osmium extract` config).
- **A boundary.** Keep only the data inside a GeoJSON, `.poly` or WKT file, or inside a box.
- **Parallel.** The cut uses all processors, and releases the GIL.

## Install

```bash
pip install pbfcut
```

There are wheels for Python 3.9 and later on Linux (x86_64, aarch64), macOS (arm64, x86_64) and Windows (x86_64).

## Examples

Get an input file from [Geofabrik](https://download.geofabrik.de), for example `sweden-latest.osm.pbf`.

```python
import pbfcut

# map tiles of zoom 8: tiles/8_136_74.osm.pbf, ...
r = pbfcut.cut_tiles("sweden-latest.osm.pbf", "tiles", zoom=8)
for f in r["files"]:
    print(f["file"], f["nodes"], f["ways"], f["relations"], f["bytes"])

# H3 cells of resolution 5 in a box, with 1 GiB of memory and 4 threads
pbfcut.cut_cells("sweden-latest.osm.pbf", "cells", 5, boundary=(11.0, 55.3, 19.0, 60.0), memory="1G", threads=4)

# a list of areas that do not overlap, one file for each area, named by the property NAME
pbfcut.cut_areas("sweden-latest.osm.pbf", "counties.geojson", "counties", name="NAME")

# two folders with the same elements
assert pbfcut.compare("tiles", "tiles-before") == []
```

A cut gives a `dict` with the number of tiles, the time, and for each file its name and its numbers of nodes, ways,
relations and bytes. An error raises `pbfcut.Error`.

## Functions

| Function | What it does |
|---|---|
| `cut_tiles(input, out_dir, zoom=8, *, tiles=None, ...)` | One file for each map tile of a zoom, or of a list of tiles |
| `cut_cells(input, out_dir, res, ...)` | One file for each H3 cell of a resolution |
| `cut_areas(input, areas, out_dir, *, name="name", ...)` | One file for each area of a list (the areas must not overlap) |
| `compare(dir_a, dir_b)` | The files of two folders that have different elements |

The options are keyword arguments: `boundary`, `border` (metres), `complete` (the relation types to keep whole),
`memory` (for example `"4G"`), `temp`, `threads` and `compress` (`False`: raw blocks, a faster cut and larger files).

## Speed

On a Mac mini (12 cores, 16 GB), pbfcut 0.2.0:

| File | Zoom | pbfcut | osmium extract |
|---|---|---|---|
| Sweden (975 MB) | 6 | 11.7 s, 0.92 GiB | 53.6 s, 7.95 GiB |
| France (5.1 GB) | 6 | 56.5 s, 1.29 GiB | 176.4 s, 7.02 GiB |
| France (5.1 GB) | 8 | 58.7 s, 1.55 GiB | stopped after 600 s |

The memory is the peak RSS. All numbers are in [the comparison](https://khoshkhah.github.io/pbfcut/manual/explanation/comparison/).

## More

- [The Python reference](https://khoshkhah.github.io/pbfcut/manual/reference/python/)
- [The manual](https://khoshkhah.github.io/pbfcut/manual/): tutorials, the command line, and how the cut works
- [The online page](https://khoshkhah.github.io/pbfcut/): the same cut in the browser, with no installation
- [The source code](https://github.com/Khoshkhah/pbfcut) (MIT license)
