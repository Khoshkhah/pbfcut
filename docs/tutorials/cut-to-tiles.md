# Cut a file into tiles

This tutorial cuts Monaco into map tiles of zoom 14. It takes less than one second.

## Before you start

You must have Rust (1.85 or later) and Git.

## Procedure

1. Install pbfcut:

    ```bash
    cargo install --git https://github.com/Khoshkhah/pbfcut
    ```

2. Get the Monaco file and its boundary from the repository:

    ```bash
    git clone https://github.com/Khoshkhah/pbfcut
    cd pbfcut
    ```

3. Cut the file into the tiles of zoom 14 that touch the boundary:

    ```bash
    pbfcut tiles tests/data/monaco.osm.pbf --zoom 14 --boundary tests/data/monaco.geojson -o out/
    ```

4. Look at the result. The folder `out/` has one file for each tile with a node inside:

    ```text
    14_8529_5973.osm.pbf  14_8529_5974.osm.pbf  14_8530_5973.osm.pbf  14_8530_5974.osm.pbf
    ```

    pbfcut writes the numbers of elements of each file to standard output as JSON:

    ```json
    {
      "files": [
        {
          "file": "14_8529_5974.osm.pbf",
          "nodes": 21943,
          "relations": 260,
          "tile": "14/8529/5974",
          "ways": 3577
        },
        ...
      ],
      "seconds": 0.13,
      "tiles": 4
    }
    ```

5. Cut again, and compare the two folders:

    ```bash
    pbfcut tiles tests/data/monaco.osm.pbf --zoom 14 --boundary tests/data/monaco.geojson -o out2/
    pbfcut compare out/ out2/
    ```

    The result is `4 files, 0 differ`.

## Next steps

- Add `--steps` to see the time of each step of the cut.
- Use `--tiles` with a list of tiles of different zooms ([The command line](../reference/cli.md)).
