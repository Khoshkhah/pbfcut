# The command line

```text
pbfcut tiles   IN.osm.pbf (--zoom Z | --tiles LIST.txt) [OPTIONS] -o DIR
pbfcut h3      IN.osm.pbf --res R [OPTIONS] -o DIR
pbfcut areas   IN.osm.pbf --areas FILE [--name PROPERTY] [OPTIONS] [-o DIR]
pbfcut compare DIR_A DIR_B
```

The input must be sorted by type, then by id, as the files of Geofabrik are. Its blocks can be zlib blocks (as most
`.osm.pbf` files) or raw blocks, also both in one file. A block with LZMA, LZ4 or Zstandard compression stops the cut
with an error. If an error occurs, pbfcut writes it to
standard error and stops with exit code 1.

## pbfcut tiles

One file for each map tile (Web Mercator `z/x/y`): `DIR/z_x_y.osm.pbf`.

| Option | Value |
|---|---|
| `--zoom Z` | The tiles of zoom `Z` (0 to 30) that touch the boundary. Without a boundary: the box in the header of the file. |
| `--tiles LIST.txt` | A list of tiles, one `z/x/y` on each line. The zooms can be different, but a tile must not be inside another tile of the list. |

## pbfcut h3

One file for each H3 cell: `DIR/<H3 index>.osm.pbf`, for example `881eb4cb2dfffff.osm.pbf`.

| Option | Value |
|---|---|
| `--res R` | The cells of resolution `R` (0 to 15) that touch the boundary. Without a boundary: the box in the header of the file. |

## pbfcut areas

One file for each area of a list: `DIR/NAME.osm.pbf`. The areas must not overlap: if a node is inside two areas,
pbfcut stops with an error that names them.

| Option | Value |
|---|---|
| `--areas FILE` | A GeoJSON FeatureCollection (one area for each feature), or an `osmium extract` config file. |
| `--name PROPERTY` | The property that gives the file name. Default: `name`, else the `id` of the feature, else `area-N`. |
| `-o DIR` | Optional with a config file: the default is its `directory`. |

The names must be different, with no `/` or `\`, and must not start with `.`.

## Options of the cut

These options are the same for `tiles`, `h3` and `areas`.

| Option | Value |
|---|---|
| `-o DIR` | The output folder. pbfcut makes it if necessary. |
| `--boundary FILE` | Keep only the data inside this area: GeoJSON, `.poly` or WKT ([Boundary formats](boundaries.md)). |
| `--bbox W,S,E,N` | A box as the boundary, in degrees. Not together with `--boundary`. |
| `--border METRES` | Also give each tile or cell the ways less than this distance from it. Default: 0. Not for `areas`. For `h3`, less than half the edge of a cell. |
| `--complete TYPES` | The relation types that each file gets with all their members. Default: `multipolygon,boundary`. `""`: none. |
| `--no-compression` | Write raw blocks, not zlib blocks. The cut is about 25 % faster, and the files are about 1.6 times larger (Sweden, zoom 8: 9.5 s and 1,619 MB, against 12.7 s and 1,000 MB). The elements are the same. |
| `--memory SIZE` | The memory budget, for example `4G` or `512M`. Default: `2G`. A smaller budget uses more disk and more time, and gives the same files. |
| `--temp DIR` | The folder of the temporary files. Default: the temporary folder of the system. |
| `--steps` | Write the time and the memory of each step to standard error. This makes the cut slower. |

## The output

- One `.osm.pbf` file for each tile, cell or area with data. The others have no file.
- Each file is sorted by type, then by id, with no metadata (version, time, user). Its blocks are zlib blocks, or raw
  blocks with `--no-compression`. Each OSM program reads both.
- Standard output gets JSON: the number of tiles, cells or areas, and for each file its name and its numbers of
  nodes, ways and relations. With `--steps`, also `peak_heap_mib`.

## pbfcut compare

Compares the `.osm.pbf` files of two folders by their elements (tags, coordinates, nodes, members). The bytes can be
different. It writes each file that is different or in one folder only, then `N files, M differ`. The exit code is 1
when a file is different.
