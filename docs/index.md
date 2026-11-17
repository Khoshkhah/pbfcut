# pbfcut

pbfcut cuts an OpenStreetMap `.osm.pbf` file into map tiles, H3 cells or your own areas. It reads the file one time
for all outputs, and each output is a complete `.osm.pbf` file.

It is faster than `osmium extract` and uses much less memory ([Comparison](explanation/comparison.md)).

## Start

| To do this | Read |
|---|---|
| Cut a file with no installation | [Cut a file in your browser](tutorials/browser-page.md) |
| Cut a file into map tiles | [Cut a file into tiles](tutorials/cut-to-tiles.md) |
| Cut a file into H3 cells | [Cut a file into H3 cells](tutorials/cut-to-h3-cells.md) |
| Cut a file into your own areas | [Cut a file into your own areas](tutorials/cut-to-areas.md) |
| See all commands and options | [The command line](reference/cli.md) |
| Cut from a program | [Python](reference/python.md) · [Rust](reference/rust.md) |
| Know what each output file holds | [The rules of the cut](explanation/rules.md) |
