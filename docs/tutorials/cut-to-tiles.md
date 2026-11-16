# Cut a file into tiles

This tutorial cuts the OpenStreetMap data of Monaco into the map tiles of zoom 12. It takes about two minutes, and
the cut itself takes less than one second.

## Before you start

You must have Rust. To install it, follow the steps on <https://rustup.rs>.

## Procedure

1. Install pbfcut (about one minute):

    ```bash
    cargo install --git https://github.com/Khoshkhah/pbfcut
    ```

    If the shell then says `pbfcut: command not found`, open a new terminal.

2. Download the data of Monaco (0.7 MB) from Geofabrik:

    ```bash
    curl -LO https://download.geofabrik.de/europe/monaco-latest.osm.pbf
    ```

3. Cut the file into the tiles of zoom 12:

    ```bash
    pbfcut tiles monaco-latest.osm.pbf --zoom 12 -o tiles/
    ```

4. Look at the folder `tiles/`. It has one file for each tile with data, for example `12_2132_1497.osm.pbf`.

    pbfcut also writes the number of nodes, ways and relations of each file as JSON:

    ```json
    {"files": [{"file": "12_2132_1497.osm.pbf", "tile": "12/2132/1497", "nodes": 848, "ways": 18, "relations": 10}, ...],
     "seconds": 0.14, "tiles": 15}
    ```

    The file of Monaco also covers some sea, so 5 of the 15 tiles have no data and no file.

## Next steps

- Add `--boundary area.geojson` to keep only the data inside an area.
- Add `--steps` to see the time and the memory of each step.
- Use `--tiles LIST.txt` to cut to a list of tiles of different zooms ([The command line](../reference/cli.md)).
