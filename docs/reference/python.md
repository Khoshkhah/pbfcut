# Python

The Python package `pbfcut` gives the cut in Python. The cut is the Rust library of pbfcut, the same code as the
command line: the package only converts the arguments and the results. The files are equal to the files of the
command line.

## Install

```bash
pip install pbfcut
```

The wheels are for Python 3.9 and later, on Linux (x86_64, aarch64), macOS (arm64, x86_64) and Windows (x86_64). On
other computers, pip builds the package from the source. Then you must have Rust (https://rustup.rs).

To install the version on GitHub (you must have Rust):

```bash
pip install "git+https://github.com/Khoshkhah/pbfcut#subdirectory=python"
```

## Examples

Cut a file into the tiles of zoom 8:

```python
import pbfcut

r = pbfcut.cut_tiles("sweden-latest.osm.pbf", "tiles", zoom=8)
for f in r["files"]:
    print(f["tile"], f["file"], f["nodes"], f["ways"], f["relations"], f["bytes"])
```

Cut a file into H3 cells, in a box, with a memory budget of 1 GiB and 4 threads:

```python
r = pbfcut.cut_cells("monaco.osm.pbf", "cells", 9, boundary=(7.40, 43.72, 7.44, 43.76), memory="1G", threads=4)
print(r["cells"], "cells,", len(r["files"]), "files")
```

Cut a file into a list of areas (the areas must not overlap):

```python
r = pbfcut.cut_areas("sweden-latest.osm.pbf", "counties.geojson", "counties", name="NAME")
```

Compare two folders:

```python
assert pbfcut.compare("tiles", "tiles-before") == []
```

## Functions

| Function | Command line |
|---|---|
| `cut_tiles(input, out_dir, zoom=8, *, tiles=None, ...)` | `pbfcut tiles IN --zoom Z` or `--tiles LIST.txt` |
| `cut_cells(input, out_dir, res, ...)` | `pbfcut h3 IN --res R` |
| `cut_areas(input, areas, out_dir, *, name="name", ...)` | `pbfcut areas IN --areas FILE --name PROPERTY` |
| `compare(dir_a, dir_b)` | `pbfcut compare DIR_A DIR_B` |

The options of the cut are keyword arguments. They have the same meaning as the options of
[the command line](cli.md):

| Argument | Option | Value |
|---|---|---|
| `boundary` | `--boundary`, `--bbox` | A file (GeoJSON, `.poly` or WKT), or a tuple `(west, south, east, north)` in degrees. Default: all nodes. |
| `border` | `--border` | Metres. Default: `0.0`. Not for `cut_areas`. |
| `complete` | `--complete` | The relation types to keep whole. Default: `("multipolygon", "boundary")`. An empty list gives no complete relations. |
| `memory` | `--memory` | A number of bytes, or a text such as `"4G"` or `"512M"`. Default: `2G`. |
| `temp` | `--temp` | The folder of the temporary files. |
| `threads` | none | The number of threads of the cut. Default: all processors. |

The paths can be `str` or `pathlib.Path`.

## Results

A cut gives a `dict`, the same as the JSON of the command line, with the size of each file:

```python
{
    "tiles": 4,          # "cells" or "areas": the number of the list, also the ones with no file
    "files": [
        {"tile": "14/8529/5974", "file": "14_8529_5974.osm.pbf", "nodes": 21943, "ways": 3577, "relations": 260, "bytes": 401977},
        # ...
    ],
    "seconds": 0.21,
}
```

The result gives only the names and the numbers, not the data: the memory of the cut stays in its budget. Read the
files with an OSM library of your choice.

`compare` gives one line for each file that is different or is in one folder only. An empty list means that the
folders are equal.

## Errors and threads

An error of the cut gives the exception `pbfcut.Error`, with the message of the library.

The cut releases the GIL: other Python threads continue during the cut. The memory budget and the temporary folder are
settings of the process. Do not run two cuts with different budgets at the same time.
