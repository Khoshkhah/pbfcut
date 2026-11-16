# The command line

```text
pbfcut tiles   IN.osm.pbf (--zoom Z | --tiles LIST.txt) [OPTIONS] -o DIR
pbfcut h3      IN.osm.pbf --res R [OPTIONS] -o DIR
pbfcut compare DIR_A DIR_B

OPTIONS: [--boundary AREA.geojson] [--border METRES] [--complete TYPES] [--memory SIZE] [--temp DIR] [--steps]
```

The input file must be sorted by type, then by id (`Sort.Type_then_ID`), as the files of Geofabrik.

## pbfcut tiles

Cuts `IN.osm.pbf` into map tiles (Web Mercator, `z/x/y`), with one read of the file for all tiles.

| Option | Value |
|---|---|
| `--zoom Z` | The tiles of zoom `Z` (0 to 30) that touch the boundary. |
| `--tiles LIST.txt` | A list of tiles, one `z/x/y` on each line. The zooms can be different. A tile must not be in another tile of the list. |
| `--boundary AREA.geojson` | A GeoJSON Polygon or MultiPolygon, also in a Feature or a FeatureCollection. The cut keeps only the nodes inside it. Without it: all nodes, and `--zoom` uses the bounding box in the header of the file. |
| `--border METRES` | A way with a node less than this distance from a tile also goes to that tile. Default: 0. The tiles must have one zoom. |
| `--complete TYPES` | The types of the relations that each tile gets with all their members, separated by commas. Default: `multipolygon,boundary`. An empty value (`--complete ""`) gives no complete relations. |
| `--memory SIZE` | The memory budget of the cut, for example `4G` or `512M` (units `K`, `M`, `G`). Default: `2G`. With a smaller budget, the cut writes more temporary data to disk and takes more time. The output files are the same with each budget. |
| `--temp DIR` | The folder of the temporary files. Default: the temporary folder of the system. The cut of a large file can write temporary files of some times the size of the input file. |
| `--steps` | Write the time, the heap in use and the peak heap of each step to standard error. The peak heap of the cut goes into the JSON (`peak_heap_mib`). The count of the heap makes the cut slower. |
| `-o DIR` | The output folder. pbfcut makes it if it does not exist. |

The output:

- One file `DIR/z_x_y.osm.pbf` for each tile with a node inside. A tile with no node inside has no file.
- Each file is sorted by type, then by id, with no metadata (version, time, user). The blocks have zlib compression.
- Standard output gets a JSON object: `tiles` (the number of tiles in the list), `files` (for each file: `file`, `tile`,
  `nodes`, `ways`, `relations`), `seconds`, and with `--steps` also `peak_heap_mib`.

If an error occurs, pbfcut writes it to standard error and stops with the exit code 1.

## pbfcut h3

Cuts `IN.osm.pbf` into the H3 cells of one resolution, with one read of the file for all cells. The cut is the cut of
`pbfcut tiles`, with a cell in place of a tile: the rules, the options and the output are the same.

| Option | Value |
|---|---|
| `--res R` | The cells of resolution `R` (0 to 15) that touch the boundary. Without `--boundary`: the cells that touch the bounding box in the header of the file. |
| `--border METRES` | A way with a node less than this distance from a cell also goes to that cell. Default: 0. The distance must be less than half the mean edge of a cell (resolution 9: 100 m). |

The other options are the options of `pbfcut tiles`.

The output:

- One file `DIR/<cell index>.osm.pbf` for each cell with a node inside, for example `881eb4cb2dfffff.osm.pbf`. The
  index is the H3 index of the cell in hexadecimal, as in the H3 library 4.
- Standard output gets the JSON of `pbfcut tiles`, with `cells` in place of `tiles` and `cell` in place of `tile`.

A node goes to the cell of its point (the function `latLngToCell` of H3). The cell of a point is the same on each
computer and in WebAssembly.

## pbfcut compare

Compares the `.osm.pbf` files of two folders. Two files are equal when they have the same elements, with the same
tags, coordinates, nodes and members, in the same order. The bytes can be different.

The command writes each file that is different or only in one folder, and then `N files, M differ`. The exit code is
1 when a file is different.
