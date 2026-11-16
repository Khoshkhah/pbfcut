# Comparison

pbfcut, its [online page](../tutorials/online-page.md) and osmium cut the same files into the same map tiles.

## The results

The time of the cut and the peak memory. For pbfcut and osmium, the memory is the peak RSS (`/usr/bin/time -l`). For
the page, it is the peak WebAssembly memory, with no memory of the browser itself. The footprint of osmium also counts
the memory that the system moved to the disk.

| File | Zoom | Tiles with data | pbfcut | Online page | osmium `extract -c -s smart -S types=multipolygon,boundary` |
|---|---|---|---|---|---|
| Sweden | 6 | 21 | 12.2 s, 0.86 GiB | 20.1 s, 0.62 GiB | 50.9 s, 9.8 GB ¹ |
| Sweden | 8 | 183 | 12.8 s, 1.00 GiB | 21.8 s, 0.72 GiB | stopped after 378 s (218 GB peak footprint), no data ¹ |
| France | 6 | 12 | 56.6 s, 1.11 GiB | 93.8 s, 0.85 GiB | 174 s, 7.2 GiB (35.9 GiB peak footprint) |
| France | 8 | 116 | 59.4 s, 1.53 GiB | 98.5 s, 1.20 GiB | stopped by us after 600 s, no data |

¹ Measured on 2026-11-15 with the same osmium and the same Mac. At zoom 8 the configuration had 206 extracts.

Other programs used the Mac during the runs of the page on France, so these two times can be a little long.

The files have the same elements, with one difference: osmium gives a node exactly on a tile border to the two tiles
([The rules of the cut](rules.md)).

osmium keeps a set of ids for each extract, so its memory grows with the number of tiles. pbfcut keeps the tile of each
node in one compressed list, and writes the large lists to disk. Its memory follows its budget, not the number of
tiles ([Speed and memory](speed-and-memory.md)).

The command line is about 1.7 times faster than the page. The page runs the same code in WebAssembly, and gives the
same files.

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

The area is the polygon of the extract in Geofabrik's `index-v1.json`, with some sea near the coast.

## Do the measurement again

For pbfcut, see [Speed and memory](speed-and-memory.md#do-the-measurement-again). For osmium, write one extract for
each tile with data in a config file:

```bash
/usr/bin/time -l osmium extract -c tiles.json -s smart -S types=multipolygon,boundary sweden-latest.osm.pbf
osmium fileinfo -e sweden-latest.osm.pbf                                    # the numbers of elements
```
