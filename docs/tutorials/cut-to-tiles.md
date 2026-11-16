# Cut a file into tiles

This tutorial downloads the OpenStreetMap data of Monaco and cuts it into map tiles of zoom 12. The cut takes less
than one second.

## Before you start

You must have Rust and its package manager Cargo. To install them, follow the steps on https://rustup.rs. Then load
Cargo in your terminal (a new terminal does this by itself):

```bash
. "$HOME/.cargo/env"
cargo --version      # must print a version
```

## Procedure

1. Install pbfcut. Cargo downloads the source code and compiles it (about one minute):

    ```bash
    cargo install --git https://github.com/Khoshkhah/pbfcut
    ```

    Cargo puts the command `pbfcut` in `~/.cargo/bin`. Open a new terminal after the installation. If the shell says
    `pbfcut: command not found`, add this folder to `PATH`:

    ```bash
    export PATH="$HOME/.cargo/bin:$PATH"     # add this line to ~/.bashrc or ~/.zshrc
    ```

2. Download the data of an area from Geofabrik (https://download.geofabrik.de). This tutorial uses Monaco, a file of
   about 0.7 MB:

    ```bash
    curl -LO https://download.geofabrik.de/europe/monaco-latest.osm.pbf
    ```

    You can use the file of any other area. A larger file takes more time.

3. Cut the file into the tiles of zoom 12:

    ```bash
    pbfcut tiles monaco-latest.osm.pbf --zoom 12 -o tiles/
    ```

    The files of Geofabrik have a bounding box in their header. pbfcut uses the tiles of zoom 12 in this box.

4. Look at the result. The folder `tiles/` has one file for each tile with a node inside:

    ```text
    12_2132_1493.osm.pbf  12_2132_1497.osm.pbf  12_2133_1496.osm.pbf  12_2134_1496.osm.pbf
    12_2132_1494.osm.pbf  12_2133_1494.osm.pbf  12_2133_1497.osm.pbf  12_2134_1497.osm.pbf
    12_2132_1495.osm.pbf  12_2133_1495.osm.pbf
    ```

    The area of the Monaco file includes the sea near Monaco, so some tiles are tiles of the sea.

    pbfcut writes the numbers of elements of each file to standard output as JSON:

    ```json
    {
      "files": [
        {
          "file": "12_2132_1497.osm.pbf",
          "nodes": 848,
          "relations": 10,
          "tile": "12/2132/1497",
          "ways": 18
        },
        ...
      ],
      "seconds": 0.14,
      "tiles": 15
    }
    ```

    The numbers change with the date of the file.

5. Cut again, and compare the two folders:

    ```bash
    pbfcut tiles monaco-latest.osm.pbf --zoom 12 -o tiles2/
    pbfcut compare tiles/ tiles2/
    ```

    The result is `10 files, 0 differ`.

## Next steps

- Add `--boundary area.geojson` to keep only the nodes inside an area.
- Add `--steps` to see the time and the memory of each step of the cut.
- Use `--tiles` with a list of tiles of different zooms ([The command line](../reference/cli.md)).
