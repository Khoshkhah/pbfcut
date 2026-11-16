# pbfcut

pbfcut cuts an OpenStreetMap `.osm.pbf` file into map tiles or H3 cells, with one read of the file for all tiles or
cells. Each output file is a complete `.osm.pbf` file (see [The rules of the cut](explanation/rules.md)).

pbfcut is faster than `osmium extract` and uses much less memory: see [Comparison](explanation/comparison.md).

## Start here

- [Cut a file into tiles](tutorials/cut-to-tiles.md): a tutorial on Monaco.
- [Cut a file into H3 cells](tutorials/cut-to-h3-cells.md): the same, with H3 cells.
- [Cut a file in your browser](tutorials/online-page.md): the [online page](https://khoshkhah.github.io/pbfcut/), with
  no installation.
- [The command line](reference/cli.md): all commands and options.
- [The rules of the cut](explanation/rules.md): what each tile file holds.
- [Speed and memory](explanation/speed-and-memory.md): the time and the memory by zoom, and the memory budget.
- [Comparison](explanation/comparison.md): pbfcut and osmium, the command line and the online page.
