# Comparison

All numbers are from a Mac mini (12 cores, 16 GB) with the Geofabrik file of Sweden (975 MB).

## pbfcut and osmium

| Sweden | pbfcut | osmium-tool 1.19.1 `extract -c -s smart -S types=multipolygon,boundary` |
|---|---|---|
| zoom 6, 21 tiles | 11.2 s, 1.8 GB | 50.9 s, 9.8 GB |
| zoom 8, 206 tiles | 15.2 s, 1.5 GB | stopped after 378 s (218 GB peak footprint), no data |

The two tools give the same elements, with one difference. osmium gives a node exactly on a tile border to the two
tiles. pbfcut gives it to the tile east or south of the border (see [The rules of the cut](rules.md)).

osmium keeps a set of ids for each extract, so its memory grows with the number of tiles. pbfcut keeps the tile of each
node in one compressed list for all tiles, and writes the large lists to temporary files. Thus the memory of pbfcut
follows its memory budget (`--memory`), not the number of tiles (see [Speed and memory](speed-and-memory.md)).

## The command line and the online page

The command line is about 1.6 times faster than the [online page](../tutorials/online-page.md). Sweden at zoom 8, with
12 threads:

| | Time |
|---|---|
| `pbfcut tiles` | 12.7 s (5 runs: 12.6 s to 13.0 s) |
| The online page, Chromium | 19.8 s (2 runs: 19.7 s and 19.9 s) |

The two give the same files. The page runs the same code in WebAssembly, and its temporary files go into the storage
of the browser (OPFS).

## Do the measurement again

`tools/measure.sh` gives the time, the peak RSS and the peak heap of each cut:

```bash
cargo build --release
tools/measure.sh out/ sweden-latest.osm.pbf:6,8
```

For osmium, give one extract for each tile in a configuration file, and measure it with `/usr/bin/time`:

```bash
/usr/bin/time -l osmium extract -c tiles.json -s smart -S types=multipolygon,boundary sweden-latest.osm.pbf
```
