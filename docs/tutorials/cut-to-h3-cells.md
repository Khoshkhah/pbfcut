# Cut a file into H3 cells

This tutorial cuts the OpenStreetMap data of Monaco into the H3 cells of resolution 8. An H3 cell is a hexagon of the
H3 grid (https://h3geo.org). A cell of resolution 8 has an area of about 0.7 km². The cut takes less than one second.

## Before you start

Do steps 1 and 2 of [Cut a file into tiles](cut-to-tiles.md): install pbfcut and download `monaco-latest.osm.pbf`.

## Procedure

1. Cut the file into the cells of resolution 8:

    ```bash
    pbfcut h3 monaco-latest.osm.pbf --res 8 -o cells/
    ```

    pbfcut uses the cells of resolution 8 that touch the bounding box in the header of the file.

2. Look at the result. The folder `cells/` has one file for each cell with a node inside. The name of a file is the
   index of its cell:

    ```text
    881eb4cb2dfffff.osm.pbf  881eb4cb67fffff.osm.pbf  881eb4d825fffff.osm.pbf
    881eb4cb61fffff.osm.pbf  881eb4cb69fffff.osm.pbf  881eb4d82dfffff.osm.pbf
    ...
    ```

    pbfcut writes the numbers of elements of each file to standard output as JSON:

    ```json
    {
      "cells": 672,
      "files": [
        {
          "cell": "881eb4cb2dfffff",
          "file": "881eb4cb2dfffff.osm.pbf",
          "nodes": 355,
          "relations": 4,
          "ways": 6
        },
        ...
      ],
      "seconds": 0.19
    }
    ```

    The box of the Monaco file includes the sea, so most of the 672 cells have no node and no file. The numbers
    change with the date of the file.

3. Cut again, and compare the two folders:

    ```bash
    pbfcut h3 monaco-latest.osm.pbf --res 8 -o cells2/
    pbfcut compare cells/ cells2/
    ```

    The result is `122 files, 0 differ`.

## Next steps

- Add `--boundary area.geojson` to keep only the nodes inside an area. The cut then uses only the cells that touch
  the area.
- Add `--border 25` to give each cell also the ways with a node less than 25 m from it.
- Each cell file has the same rules as a tile file ([The rules of the cut](../explanation/rules.md)).
