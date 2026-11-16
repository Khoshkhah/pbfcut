# pbfcut

pbfcut cuts an OpenStreetMap `.osm.pbf` file into map tiles or into a list of your own areas, with one read of the file
for all outputs. Each output file is a complete `.osm.pbf` file (see [The rules of the cut](explanation/rules.md)).

| Sweden (975 MB), Mac mini | pbfcut | osmium-tool 1.19.1 `extract -c -s smart` |
|---|---|---|
| zoom 6, 21 tiles | 11.2 s, 1.8 GB | 50.9 s, 9.8 GB |
| zoom 8, 206 tiles | 15.2 s, 1.5 GB | stopped after 378 s (218 GB peak footprint), no data |

The two tools give the same elements, with one difference. osmium gives a node exactly on a tile border to the two
tiles. pbfcut gives it to the tile east or south of the border.

osmium keeps a set of ids for each extract, so its memory grows with the number of tiles. pbfcut keeps the tile of each
node in one compressed list for all tiles, and writes the large lists to temporary files.

## Start here

- [Cut a file into tiles](tutorials/cut-to-tiles.md): a tutorial on Monaco.
- [Cut a file into your own areas](tutorials/cut-to-areas.md): a list of areas that overlap, as `osmium extract -c`.
- [The command line](reference/cli.md): all commands and options.
- [Boundary formats](reference/boundaries.md): GeoJSON, polygon files (`.poly`), WKT and boxes.
- [The rules of the cut](explanation/rules.md): what each tile file holds.
- [Speed and memory](explanation/speed-and-memory.md): the time and the memory by zoom, and the memory budget.

## Planned

- H3 cells: all cells of one resolution (`pbfcut h3 IN.osm.pbf --res 7`), or a list of cells of different
  resolutions.
- An online page that cuts a file in the browser.
