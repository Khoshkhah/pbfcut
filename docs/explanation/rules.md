# The rules of the cut

Each tile file of pbfcut is a complete `.osm.pbf` file. These rules give the elements of a tile:

1. Each node inside the boundary goes to exactly one tile: the tile that holds it. A node on the line between two
   tiles goes to the tile east or south of the line.
2. A way with a node in a tile goes to that tile, complete: all its nodes, also the nodes outside the tile.
3. A relation with a member in a tile goes to that tile, with its parent relations.
4. A relation of a type in `--complete` (default: `multipolygon` and `boundary`) goes to every tile with a member,
   with all its members. A member way comes with all its nodes. A program can then make the polygons of the tile
   from the tile file alone.
5. A tile with no node inside has no file.

These are the rules of osmium's `smart` strategy (`osmium extract -s smart -S types=multipolygon,boundary`), with one
difference. osmium gives a node exactly on a tile border to the two tiles, because its bounding box includes its edge.

## How the cut works

The cut reads the file two times.

- Read 1 makes the plan. It records the tile of each node as runs of ids (nodes with near ids are usually in one
  place), compressed. Then it finds the tiles of each way, the nodes that each tile needs from outside it, and the
  tiles of each relation. The large lists go to temporary files.
- Read 2 writes each element into the files of its tiles, once, in the order of the input file.

The elements are never all in memory, and the memory does not grow with the number of tiles.
