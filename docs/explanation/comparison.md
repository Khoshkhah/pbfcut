# Comparison

pbfcut, its [online page](../tutorials/online-page.md) and osmium cut the same files into map tiles.

## The system

| | |
|---|---|
| Computer | Mac mini (Mac18,5), Apple silicon, 12 cores, 16 GB memory |
| System | macOS 27.2 |
| pbfcut | built with Rust 1.99.1 (`cargo build --release`) |
| The online page | Chromium 140 (headless, Playwright), cross-origin isolated |
| osmium | osmium-tool 1.19.1 |
| Threads | 12 for pbfcut and the page |

## The files

The Geofabrik files of Sweden and France (downloaded on 2026-11-14 and 2026-11-11). The numbers of elements are from
`osmium fileinfo -e`.

| | Sweden | France |
|---|---|---|
| File size | 975 MB | 5,113 MB |
| Nodes | 132,370,187 | 529,233,838 |
| Ways | 12,443,153 | 73,692,922 |
| Roads (ways with a `highway` tag) | 2,315,417 | 12,466,049 |
| Relations | 542,622 | 1,106,136 |
| Area of the Geofabrik extract | 583,973 km² | 792,047 km² |

The area is the area of the polygon of the extract in Geofabrik's `index-v1.json`. The polygon also covers some sea
near the coast.

## pbfcut, the online page and osmium

The time of the cut, and the peak memory:

- pbfcut: the peak resident memory (`/usr/bin/time -l`).
- The online page: the peak WebAssembly memory. The memory of the browser itself is not in this number.
- osmium: the peak resident memory, and the peak memory footprint (with the memory that the system moved to the disk).

| File | Zoom | Tiles with data | pbfcut | Online page | osmium `extract -c -s smart -S types=multipolygon,boundary` |
|---|---|---|---|---|---|
| Sweden | 6 | 21 | 12.2 s, 0.86 GiB | 20.1 s, 0.62 GiB | 50.9 s, 9.8 GB ¹ |
| Sweden | 8 | 183 | 12.8 s, 1.00 GiB | 21.8 s, 0.72 GiB | stopped after 378 s (218 GB peak footprint), no data ¹ |
| France | 6 | 12 | 56.6 s, 1.11 GiB | 93.8 s, 0.85 GiB | 174 s, 7.2 GiB (35.9 GiB peak footprint) |
| France | 8 | 116 | 59.4 s, 1.53 GiB | 98.5 s, 1.20 GiB | stopped by us after 600 s, no data |

¹ Measured on 2026-11-15 with the same osmium and the same Mac. At zoom 8 the configuration had 206 extracts.

Other programs used the Mac during the runs of the page on France (load average 6 to 8), so these two times can be a
little long.

pbfcut and osmium give the same elements, with one difference. osmium gives a node exactly on a tile border to the two
tiles. pbfcut gives it to the tile east or south of the border (see [The rules of the cut](rules.md)).

osmium keeps a set of ids for each extract, so its memory grows with the number of tiles. pbfcut keeps the tile of each
node in one compressed list for all tiles, and writes the large lists to temporary files. Thus the memory of pbfcut
follows its memory budget (`--memory`), not the number of tiles (see [Speed and memory](speed-and-memory.md)).

## The command line and the online page

The command line is about 1.7 times faster than the online page: 12.2 s against 20.1 s (Sweden, zoom 6) and 59.4 s
against 98.5 s (France, zoom 8). The two give the same files. The page runs the same code in WebAssembly, and its
temporary files and tile files go into the storage of the browser (OPFS), not into its memory.

## Do the measurement again

`tools/measure.sh` gives the time, the peak RSS and the peak heap of each cut:

```bash
cargo build --release
tools/measure.sh out/ sweden-latest.osm.pbf:6,8
```

For osmium, give one extract for each tile with data in a configuration file, and measure it with `/usr/bin/time`:

```bash
/usr/bin/time -l osmium extract -c tiles.json -s smart -S types=multipolygon,boundary sweden-latest.osm.pbf
```

The numbers of elements, and the roads:

```bash
osmium fileinfo -e sweden-latest.osm.pbf
osmium tags-filter -R -o roads.osm.pbf sweden-latest.osm.pbf w/highway && osmium fileinfo -e roads.osm.pbf
```
