# Speed and memory

All numbers are from a Mac mini (12 cores, 16 GB) with the Geofabrik files of Sweden (975 MB) and France (5.1 GB). The
cut writes all tiles in one read of the file.

For the comparison with osmium and with the online page, see [Comparison](comparison.md).

## Memory by zoom

The memory budget is the default (`--memory 2G`). "Heap" is the most memory that the cut had in use (from `--steps`).
"RSS" is the most memory that the system gave to the process: it is larger, because the allocator keeps some freed
memory.

| File | Zoom | Tiles | Files | Time | Peak RSS | Peak heap |
|---|---|---|---|---|---|---|
| Sweden | 6 | 28 | 21 | 12.3 s | 915 MiB | 358 MiB |
| Sweden | 8 | 242 | 183 | 18.5 s | 954 MiB | 395 MiB |
| Sweden | 10 | 3,520 | 2,221 | 41.9 s | 1,189 MiB | 673 MiB |
| Sweden | 12 | 54,793 | 27,623 | 313 s | 2,588 MiB | 1,267 MiB |
| France | 8 | 143 | 116 | 91 s | 1,305 MiB | 647 MiB |
| France | 10 | 2,156 | 1,431 | 295 s | 2,987 MiB | 1,513 MiB |

The time grows with the zoom because the output grows. Each tile file is complete: a way, a multipolygon or a boundary
goes to every tile that it touches, with all its nodes. At zoom 12, the tile files of Sweden have 495 million nodes from
outside their tiles.

## Memory by H3 resolution

`pbfcut h3`, with the same budget (`--memory 2G`). The cells are all cells that touch the bounding box of the file, so
many cells are in the sea and have no file. Other programs used the Mac mini during these runs (load 4 to 18), so the
times can be a little long.

| File | Resolution | Cells | Files | Time | Peak RSS | Peak heap |
|---|---|---|---|---|---|---|
| Monaco (inside its boundary) | 8 | 13 | 10 | 0.05 s | 42 MiB | not measured |
| Monaco (inside its boundary) | 10 | 222 | 205 | 0.13 s | 114 MiB | not measured |
| Sweden | 4 | 937 | 657 | 28.6 s | 1,351 MiB | 537 MiB |
| Sweden | 5 | 6,214 | 3,730 | 38.2 s | 2,024 MiB | 1,251 MiB |
| Sweden | 6 | 42,659 | 21,618 | 93.8 s | 3,930 MiB | 1,610 MiB |
| Sweden | 6, `--memory 512M` | 42,659 | 21,618 | 179.6 s | 3,037 MiB | 755 MiB |

The cut of the cells is the cut of the tiles, so the memory follows the budget and not the number of cells. With
`--memory 512M`, the peak heap of resolution 6 is less than half, and the time is about two times longer.

## Areas

The cut to a list of areas has the same steps as the cut to tiles. The test of a node is different: the grid of the
area boxes, then the polygon test.

| File | Areas | Files | Time | Peak RSS | Peak heap |
|---|---|---|---|---|---|
| Monaco | 4, they overlap (`tests/data/monaco-areas.json`) | 4 | 0.1 s | 41 MiB | 22 MiB |
| Sweden | 10 boxes, 2 x 5, each 0.3 degrees larger on each side, so they overlap | 10 | 17.3 s | 1,015 MiB | 565 MiB |
| Sweden | tiles of zoom 6, for comparison | 21 | 12.3 s | 953 MiB | 357 MiB |

The 10 boxes of Sweden make 17 sets of areas. A node in an overlap goes to two or four files, so the output is larger
than the output of zoom 6. Measured on the Mac mini (Apple M6, 12 cores, 16 GB), 2026-11-15.

## A smaller memory budget

| File | Zoom | `--memory` | Time | Peak RSS | Peak heap |
|---|---|---|---|---|---|
| Sweden | 12 | 2G | 313 s | 2,588 MiB | 1,267 MiB |
| Sweden | 12 | 512M | 596 s | 2,603 MiB | 544 MiB |
| France | 10 | 2G | 295 s | 2,987 MiB | 1,513 MiB |
| France | 10 | 512M | 502 s | 3,272 MiB | 681 MiB |

With a smaller budget, the cut sorts more data on disk and takes more time. The output files are the same. Some data
does not follow the budget: the lists of the nodes and the ways of the file (France: about 250 MB). Thus France needs
about 700 MB of heap also with `--memory 512M`.

## Do the measurement again

`tools/measure.sh` in the repository runs the cut two times for each zoom: one time for the time and the RSS, one time
with `--steps` for the heap.

```bash
cargo build --release
tools/measure.sh /tmp/measure sweden-latest.osm.pbf:6,8,10,12
tools/measure.sh /tmp/measure --memory 512M sweden-latest.osm.pbf:12
tools/measure.sh /tmp/measure sweden-latest.osm.pbf:r4,r5,r6     # H3 cells: rN is the resolution N
```

The tile files go to `/tmp/measure/run`, and the script removes them after each run. A cut at a high zoom writes many
gigabytes of tile files and temporary files: make sure that the disk has space.
