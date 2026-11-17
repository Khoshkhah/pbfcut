# Speed and memory

The memory of the cut follows its memory budget (`--memory`, default 2G), not the number of tiles. All numbers are
from a Mac mini (12 cores, 16 GB) with the Geofabrik files of Sweden (975 MB) and France (5.1 GB). For osmium and the
online page, see [Comparison](comparison.md).

- **Heap**: the most memory that the cut used (from `--steps`).
- **RSS**: the most memory that the system gave to the process. It is larger, because the allocator keeps some freed
  memory.

## Tiles

| File | Zoom | Tiles | Files | Time | Peak RSS | Peak heap |
|---|---|---|---|---|---|---|
| Sweden | 6 | 28 | 21 | 12.3 s | 915 MiB | 358 MiB |
| Sweden | 8 | 242 | 183 | 18.5 s | 954 MiB | 395 MiB |
| Sweden | 10 | 3,520 | 2,221 | 41.9 s | 1,189 MiB | 673 MiB |
| Sweden | 12 | 54,793 | 27,623 | 313 s | 2,588 MiB | 1,267 MiB |
| France | 8 | 143 | 116 | 91 s | 1,305 MiB | 647 MiB |
| France | 10 | 2,156 | 1,431 | 295 s | 2,987 MiB | 1,513 MiB |

The time grows with the zoom because the output grows: a way goes to every tile that it touches, with all its nodes.
At zoom 12, the tile files of Sweden have 495 million nodes from outside their tiles.

## H3 cells

| File | Resolution | Cells | Files | Time | Peak RSS | Peak heap |
|---|---|---|---|---|---|---|
| Sweden | 4 | 937 | 657 | 28.6 s | 1,351 MiB | 537 MiB |
| Sweden | 5 | 6,214 | 3,730 | 38.2 s | 2,024 MiB | 1,251 MiB |
| Sweden | 6 | 42,659 | 21,618 | 93.8 s | 3,930 MiB | 1,610 MiB |

Other programs used the Mac during these runs, so the times can be a little long.

## Areas

| File | Areas | Time | Peak RSS | Peak heap |
|---|---|---|---|---|
| Monaco | 3 | 0.04 s | 24 MiB | 11 MiB |
| Sweden | 10 boxes | 11.7 s | 848 MiB | 350 MiB |

## zlib compression off

With `--no-compression` (Python: `compress=False`; the page: "zlib compression" off), the files have raw blocks, not
zlib blocks. The elements are the same.

| Cut | zlib compression on | zlib compression off |
|---|---|---|
| Sweden, zoom 8, command line | 12.7 s, 1,000 MB | 9.3 to 9.8 s, 1,619 MB |
| Sweden, zoom 8, online page | 20.8 s, 942 MiB | 16.9 s, 1,536 MiB |
| Monaco, zoom 14, command line | 0.04 s, 1 MB | 0.03 s, 2 MB |

Measured on 2026-11-16 on the Mac mini, while other programs used it (load about 6). Use zlib compression off for files
that a program reads one time, soon after the cut; keep it on for files that people keep or send.

## A smaller memory budget

| File | Cut | `--memory` | Time | Peak RSS | Peak heap |
|---|---|---|---|---|---|
| Sweden | zoom 12 | 2G | 313 s | 2,588 MiB | 1,267 MiB |
| Sweden | zoom 12 | 512M | 596 s | 2,603 MiB | 544 MiB |
| Sweden | resolution 6 | 2G | 93.8 s | 3,930 MiB | 1,610 MiB |
| Sweden | resolution 6 | 512M | 179.6 s | 3,037 MiB | 755 MiB |
| France | zoom 10 | 2G | 295 s | 2,987 MiB | 1,513 MiB |
| France | zoom 10 | 512M | 502 s | 3,272 MiB | 681 MiB |

A smaller budget sorts more data on disk, so the cut takes about two times longer. The files are the same. The lists
of the nodes and the ways do not follow the budget (France: about 250 MB), so France needs about 700 MB of heap also
with `--memory 512M`.

## Do the measurement again

`tools/measure.sh` runs each cut two times: one time for the time and the RSS, one time with `--steps` for the heap.

```bash
cargo build --release
tools/measure.sh /tmp/measure sweden-latest.osm.pbf:6,8,10,12
tools/measure.sh /tmp/measure --memory 512M sweden-latest.osm.pbf:12
tools/measure.sh /tmp/measure sweden-latest.osm.pbf:r4,r5,r6     # rN: H3 resolution N
```

A cut at a high zoom writes many gigabytes. Make sure that the disk has space.
