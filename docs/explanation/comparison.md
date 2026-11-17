# Comparison

pbfcut, its [browser page](../tutorials/browser-page.md) and osmium cut the same files into the same map tiles.

## The results

Each tool has the same three numbers:

- **Time**: from the start to the end of the cut.
- **Peak RSS**: the most physical memory in use at one time.
- **Peak footprint**: the most memory in use at one time, also the memory that macOS compressed or moved to the disk.

For pbfcut and osmium, `/usr/bin/time -l` gives the two memory numbers. For the browser page, they are the sum of all
processes of the browser, sampled every second. osmium is `osmium extract -c -s smart -S types=multipolygon,boundary`.

| File | Zoom | Tool | Time | Peak RSS | Peak footprint |
|---|---|---|---|---|---|
| Sweden | 6 | pbfcut | 11.7 s | 0.92 GiB | 0.71 GiB |
| | | browser page | 20.5 s | 1.72 GiB | 1.14 GiB |
| | | osmium | 53.6 s | 7.95 GiB | 55.4 GiB |
| Sweden | 8 | pbfcut | 12.7 s | 1.02 GiB | 0.80 GiB |
| | | browser page | 21.7 s | 1.87 GiB | 1.25 GiB |
| | | osmium | failed after 370 s (1) | 9.27 GiB | 203 GiB |
| France | 6 | pbfcut | 56.5 s | 1.29 GiB | 1.07 GiB |
| | | browser page | 93.6 s | 1.92 GiB | 1.24 GiB |
| | | osmium | 176.4 s | 7.02 GiB | 35.9 GiB |
| France | 8 | pbfcut | 58.7 s | 1.55 GiB | 1.15 GiB |
| | | browser page | 98.0 s | 2.24 GiB | 1.42 GiB |
| | | osmium | stopped after 600 s (2) | 7.07 GiB | 120 GiB |

1. osmium ended with an error, and its files were not complete.
2. We stopped osmium after 600 s. Its memory numbers are sampled every second.

Measured on 2026-11-16, one run at a time, with pbfcut 0.2.0. Sweden has 21 tiles with data at zoom 6 and 183 at zoom 8; France has 12
and 116.

The files have the same elements, with one difference: osmium gives a node exactly on a tile border to the two tiles
([The rules of the cut](rules.md)).

osmium keeps a set of ids for each extract, so its memory grows with the number of tiles. pbfcut keeps the tile of each
node in one compressed list, and writes the large lists to disk. Its memory follows its budget, not the number of
tiles ([Speed and memory](speed-and-memory.md)).

The command line is about 1.6 to 1.8 times faster than the page. The page runs the same code in WebAssembly, and gives
the same files. Its WebAssembly memory alone is smaller than the numbers above: 0.63 to 1.17 GiB.

## The system

| | |
|---|---|
| Computer | Mac mini (Mac18,5), Apple silicon, 12 cores, 16 GB memory |
| System | macOS 27.2 |
| pbfcut | built with Rust 1.99.1 (`cargo build --release`) |
| The browser page | Chromium 140 (headless, Playwright), cross-origin isolated |
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
