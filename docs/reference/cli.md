# The command line

```text
pbfcut tiles   IN.osm.pbf (--zoom Z | --tiles LIST.txt) [--boundary AREA.geojson] [--complete TYPES]
               [--memory SIZE] [--temp DIR] [--steps] -o DIR
pbfcut compare DIR_A DIR_B
```

The input file must be sorted by type, then by id (`Sort.Type_then_ID`), as the files of Geofabrik.

## pbfcut tiles

Cuts `IN.osm.pbf` into map tiles (Web Mercator, `z/x/y`), with one read of the file for all tiles.

| Option | Value |
|---|---|
| `--zoom Z` | The tiles of zoom `Z` (0 to 30) that touch the boundary. |
| `--tiles LIST.txt` | A list of tiles, one `z/x/y` on each line. The zooms can be different. A tile must not be in another tile of the list. |
| `--boundary AREA.geojson` | A GeoJSON Polygon or MultiPolygon, also in a Feature or a FeatureCollection. The cut keeps only the nodes inside it. Without it: all nodes, and `--zoom` uses the bounding box in the header of the file. |
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

## pbfcut compare

Compares the `.osm.pbf` files of two folders. Two files are equal when they have the same elements, with the same
tags, coordinates, nodes and members, in the same order. The bytes can be different.

The command writes each file that is different or only in one folder, and then `N files, M differ`. The exit code is
1 when a file is different.
