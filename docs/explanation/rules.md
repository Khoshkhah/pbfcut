# The rules of the cut

Each output file is a complete `.osm.pbf` file. These rules give its elements (a tile, an H3 cell or an area):

1. Each node inside the boundary goes to the one tile that holds it. A node on the line between two tiles goes to
   the tile east or south of the line.
2. A way with a node in a tile goes to that tile with all its nodes, also the nodes outside the tile.
3. A relation with a member in a tile goes to that tile, with its parent relations.
4. A relation of a type in `--complete` (default: `multipolygon` and `boundary`) goes to each tile with a member,
   with all its members. Thus a program can make the polygons of a tile from the tile file alone.
5. A tile with no node inside has no file.

These are the rules of `osmium extract -s smart -S types=multipolygon,boundary`, with one difference: osmium gives a
node exactly on a tile border to the two tiles.

## H3 cells and areas

An H3 cell follows the same rules. A node goes to the cell of its point, as H3 gives it.

An area of a list follows the same rules too. The areas must not overlap:

- A node exactly on the border of an area is inside it.
- A node on a border between two areas goes to the first of the two in the list. osmium gives it to the two.
- A node inside two areas stops the cut with an error that names the two areas. pbfcut then writes no file.

## How the cut works

The cut reads the input two times, and never keeps all elements in memory:

- **Read 1** makes the plan. It records the tile of each node, the tiles of each way and relation, and the nodes
  that each tile needs from outside it. The large lists go to temporary files.
- **Read 2** writes each element into the files of its tiles, in the order of the input.

Thus the memory does not grow with the number of tiles.

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

Read 1 decodes each block of the input one time. Step 3 finds the members of the relations in the id files, so it
does not read the input again.

### The threads of read 2

```mermaid
flowchart TB
    R["<b>① Read</b><br/>the next blocks<br/>and their extra rows"]
    D["<b>② Decode and route</b><br/>each element to its tiles,<br/>as compact bytes"]
    C["<b>③ Collect</b><br/>the elements of each tile<br/>until a block of 8,000 is full"]
    K["<b>④ Pack</b><br/>the full blocks,<br/>with the next batch"]
    W["<b>⑤ Write</b><br/>the blocks of each tile<br/>to its file, in input order"]
    R -- "a batch of blocks" --> D
    D -- "the bytes of each tile" --> C
    C -- "the full blocks" --> K
    K --> W
    classDef caller fill:#eef2ff
    classDef pool fill:#fff7e0
    class R,C,W caller
    class D,K pool
```

Blue: the calling thread. Yellow: the thread pool. The calling thread reads the next batch while the pool decodes the
batch before. Only the calling thread opens the
files: in the browser, a file belongs to the thread that opened it.

Each output file has full blocks of 8,000 elements. The elements of a tile wait in memory as compact bytes until they
fill a block. These buffers use at most one eighth of the memory budget. Above that, the largest buffers are written
before they are full, so a small budget gives other blocks, but the same elements.
