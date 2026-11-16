# Cut a file into H3 cells

This tutorial cuts the OpenStreetMap data of Monaco into the [H3](https://h3geo.org) cells of resolution 8. A cell of
resolution 8 is a hexagon of about 0.7 km².

## Before you start

Do steps 1 and 2 of [Cut a file into tiles](cut-to-tiles.md): install pbfcut and download `monaco-latest.osm.pbf`.

## Procedure

1. Cut the file into the cells of resolution 8:

    ```bash
    pbfcut h3 monaco-latest.osm.pbf --res 8 -o cells/
    ```

2. Look at the folder `cells/`. It has one file for each cell with data. The name of a file is the H3 index of its
   cell, for example `881eb4cb2dfffff.osm.pbf`.

    The file of Monaco also covers some sea, so most of the 672 cells have no data and no file.

## Next steps

- Add `--boundary area.geojson` to use only the cells of an area.
- Add `--border 25` to also give each cell the ways less than 25 m from it.
- Each cell file follows the same rules as a tile file ([The rules of the cut](../explanation/rules.md)).
