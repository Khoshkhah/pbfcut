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

An H3 cell (`pbfcut h3`) has the same rules, with a cell in place of a tile. A node goes to the cell of its point. A
node exactly on the line between two cells goes to one cell only, as H3 gives it.

These are the rules of osmium's `smart` strategy (`osmium extract -s smart -S types=multipolygon,boundary`), with one
difference. osmium gives a node exactly on a tile border to the two tiles, because its bounding box includes its edge.

## Areas

The cut to a list of areas (`pbfcut areas`) uses the same rules, with "area" in place of "tile". Areas can overlap,
so a node can go to more than one area. A node exactly on the border of an area is inside it. Then the area files
have the same elements as the files of `osmium extract -c` with the smart strategy.

The cut keeps one number for each node. For a node in one area, the number is the area. For a node in more than one
area, the number is a set of areas: the cut keeps a table of the sets that it finds. Thus the memory of the cut does
not grow with the overlap. With `--steps`, the line of step 1 gives the number of sets.

## How the cut works

The cut reads the file two times.

- Read 1 makes the plan. It records the tile of each node as runs of ids (nodes with near ids are usually in one
  place), compressed. Then it finds the tiles of each way, the nodes that each tile needs from outside it, and the
  tiles of each relation. The large lists go to temporary files.
- Read 2 writes each element into the files of its tiles, once, in the order of the input file.

The elements are never all in memory, and the memory does not grow with the number of tiles.

### The data of the two reads

```mermaid
flowchart TB
    IN[("<b>Input .osm.pbf</b><br/>sorted by type, then id")]

    subgraph R1["① Read 1: the plan, no element is written"]
        direction TB
        P1["<b>1. The node blocks</b><br/>book of the nodes: the tile of each node inside<br/>the tiles with a file · id file of the nodes"]
        P2["<b>2. The way and relation blocks</b><br/>book of the ways: the tiles of each way<br/>extra rows (node, tile) · id file of the ways · all relations"]
        P3["<b>3. The relations</b><br/>the members in the file, from the id files<br/>the tiles of each relation · extra rows of the complete relations"]
        P4["<b>4. The extra nodes</b><br/>the rows sorted by node id"]
        P1 --> P2 --> P3 --> P4
    end

    subgraph R2["② Read 2: the write, each element once"]
        direction TB
        W1["<b>The nodes</b><br/>to the own tile · to the tiles of the extra rows"]
        W2["<b>The ways</b><br/>to their tiles · to the tiles of the extra rows"]
        W3["<b>The relations</b><br/>to their tiles"]
        W1 --> W2 --> W3
    end

    OUT[("<b>Tile files</b><br/>one complete .osm.pbf for each tile")]
    J[("<b>Junctions files</b><br/>one for each tile")]

    IN --> R1
    R1 --> R2
    IN -. "read again" .-> R2
    R2 --> OUT
    R1 -. "with NodeUses" .-> J

    classDef optional stroke-dasharray: 5 4
    classDef file stroke:#ffd43b,stroke-width:3px
    class J optional
    class IN,OUT,J file
```

Read 1 decodes each block of the input one time and keeps only lists of ids and tiles. Part 3 finds which members of
the relations the file has in the id files, so it reads no block of the input again.

### The threads of read 2

```mermaid
flowchart TB
    subgraph T1["① The calling thread"]
        R["<b>Read</b><br/>the next blocks<br/>and their extra rows"]
    end

    subgraph P1["② The threads of the pool"]
        D["<b>Decode</b><br/>a block"]
        G["<b>Route</b><br/>each element to its tiles,<br/>as compact bytes"]
        D --> G
    end

    subgraph T2["③ The calling thread"]
        C["<b>Collect</b><br/>the elements of each tile<br/>until a block of 8,000 is full"]
    end

    subgraph P2["④ The threads of the pool"]
        K["<b>Pack</b><br/>the full blocks,<br/>with the next batch"]
    end

    subgraph T3["⑤ The calling thread"]
        WR["<b>Write</b><br/>the blocks of each tile<br/>to its file, in input order"]
    end

    R -- "a batch of blocks" --> D
    G -- "the bytes of each tile" --> C
    C -- "the full blocks" --> K
    K --> WR
```

The calling thread reads the next batch while the pool decodes the batch before. The pool packs the full blocks of
the tiles together with the decoding of a later batch. Only the calling thread opens the files: in the browser, a file
of the storage belongs to the thread that opened it.

Each tile file has full blocks of 8,000 elements. One block of the input gives a tile only some elements (France at
zoom 8: about 77 nodes), so the elements of each tile wait until they fill a block. They wait as compact bytes, one
buffer for each tile and input block, and all of them hold an eighth of the memory budget at most. Above that, the
tiles that hold the most write their blocks before they are full. A small budget thus gives other blocks, but the same
elements.
