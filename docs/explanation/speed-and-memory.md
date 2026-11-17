# Speed and memory

The memory of the cut follows its memory budget (`--memory`, default 2G), not the number of tiles. All numbers are
from a Mac mini (12 cores, 16 GB) with the Geofabrik files of Sweden (975 MB) and France (5.1 GB), with pbfcut
0.2.0 on 2026-11-16, one run at a time. For osmium and the online page, see [Comparison](comparison.md).

- **Heap**: the most memory that the cut used (from `--steps`).
- **RSS**: the most memory that the system gave to the process. It is larger, because the allocator keeps some freed
  memory.

## Tiles

| File | Zoom | Tiles | Files | Time | Peak RSS | Peak heap |
|---|---|---|---|---|---|---|
| Sweden | 6 | 28 | 21 | 11.7 s | 936 MiB | 361 MiB |
| Sweden | 8 | 242 | 183 | 12.5 s | 1,053 MiB | 424 MiB |
| Sweden | 10 | 3,520 | 2,221 | 17.1 s | 1,842 MiB | 1,004 MiB |
| Sweden | 12 | 54,793 | 27,623 | 65.2 s | 4,349 MiB | 1,647 MiB |
| France | 8 | 143 | 116 | 60.7 s | 1,581 MiB | 702 MiB |
| France | 10 | 2,156 | 1,431 | 84.5 s | 4,539 MiB | 1,753 MiB |

The time grows with the zoom because the output grows: a way goes to every tile that it touches, with all its nodes.
At zoom 12, the tile files of Sweden have 495 million nodes from outside their tiles.

## H3 cells

| File | Resolution | Cells | Files | Time | Peak RSS | Peak heap |
|---|---|---|---|---|---|---|
| Sweden | 4 | 937 | 657 | 28.5 s | 1,329 MiB | 538 MiB |
| Sweden | 5 | 6,214 | 3,730 | 37.9 s | 2,065 MiB | 1,252 MiB |
| Sweden | 6 | 42,659 | 21,618 | 93.6 s | 4,564 MiB | 1,610 MiB |

## Areas

| File | Areas | Time | Peak RSS | Peak heap |
|---|---|---|---|---|
| Monaco | 3 | 0.06 s | 25 MiB | 11 MiB |
| Sweden | 10 boxes (no overlap) | 11.5 s | 921 MiB | 352 MiB |

## zlib compression off

With `--no-compression` (Python: `compress=False`; the page: "zlib compression" off), the files have raw blocks, not
zlib blocks. The elements are the same.

| Cut | zlib compression on | zlib compression off |
|---|---|---|
| Sweden, zoom 8, command line | 12.6 s, 1,000 MB | 9.3 s, 1,619 MB |
| Sweden, zoom 8, online page | 21.1 s, 944 MiB | 17.0 s, 1,536 MiB |
| Monaco, zoom 14, command line | 0.04 s, 1 MB | 0.03 s, 2 MB |

Use zlib compression off for files that a program reads one time, soon after the cut. Keep it on for files that people
keep or send.

## The memory budget

| File | Cut | `--memory` | Time | Peak RSS | Peak heap |
|---|---|---|---|---|---|
| Sweden | zoom 12 | 512M | 89.6 s | 2,727 MiB | 838 MiB |
| Sweden | zoom 12 | 2G (default) | 65.2 s | 4,349 MiB | 1,647 MiB |
| Sweden | zoom 12 | 4G | 55.6 s | 5,625 MiB | 3,012 MiB |
| Sweden | zoom 12 | 8G | 44.5 s | 4,860 MiB | 3,516 MiB |
| Sweden | resolution 6 | 512M | 114.6 s | 2,594 MiB | 741 MiB |
| Sweden | resolution 6 | 2G (default) | 93.6 s | 4,564 MiB | 1,610 MiB |
| France | zoom 10 | 512M | 108.0 s | 3,464 MiB | 770 MiB |
| France | zoom 10 | 2G (default) | 84.5 s | 4,539 MiB | 1,753 MiB |
| France | zoom 10 | 4G | 74.8 s | 3,753 MiB | 1,896 MiB |
| France | zoom 10 | 8G | 76.4 s | 3,976 MiB | 2,062 MiB |

A smaller budget sorts more data on disk, so the cut takes 1.2 to 1.4 times longer. A larger budget helps only a cut
that fills the default budget: with 8G, Sweden at zoom 12 is 1.5 times faster, and France at zoom 10 is 1.1 times
faster. The cuts at zoom 8 and lower use less than 1 GiB of heap, so a larger budget does not change them. The files
are the same with each budget. The lists of the nodes and the ways do not follow the budget (France: about 250 MB), so
France needs about 770 MiB of heap also with `--memory 512M`.

## Do the measurement again

`tools/measure.sh` runs each cut two times: one time for the time and the RSS, one time with `--steps` for the heap.

```bash
cargo build --release
tools/measure.sh /tmp/measure sweden-latest.osm.pbf:6,8,10,12
tools/measure.sh /tmp/measure --memory 512M sweden-latest.osm.pbf:12
tools/measure.sh /tmp/measure sweden-latest.osm.pbf:r4,r5,r6     # rN: H3 resolution N
```

A cut at a high zoom writes many gigabytes. Make sure that the disk has space.
