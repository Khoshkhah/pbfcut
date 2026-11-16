# Cut a file in your browser

The [online page](https://khoshkhah.github.io/pbfcut/) cuts a `.osm.pbf` file in your browser. You do not install
anything, and the file does not leave your computer.

Use Chrome or Edge on a computer. Only these browsers can save all files into a folder.

## Procedure

1. Open <https://khoshkhah.github.io/pbfcut/>.
2. Select the input file. Do one of these steps:
    - Type the name of a Geofabrik region, for example `Monaco`, and select it. Click **Get**.
    - Select a `.osm.pbf` file on your computer.
3. Select the kind of cut:

    | Kind | Output |
    |---|---|
    | **Tiles** | One file for each map tile, for example `8_136_74.osm.pbf`. |
    | **H3 cells** | One file for each H3 cell, for example `872b02d27ffffff.osm.pbf`. |
    | **Areas** | One file for each feature of a GeoJSON file, with its property `name` as the file name. The areas must not overlap. |

4. Set the **Zoom** or the **Resolution**. The list shows the size of the tiles or cells.
5. Optional: select a **Boundary** file (GeoJSON, `.poly` or WKT). The cut then keeps only the data inside it.
6. Click **Cut**. The page shows each step and its time.
7. Click **Save all to a folder**, and select a folder.

The files are equal to the files of the [command line](../reference/cli.md) with the same settings.

## Limits

- The page can use at most 4 GiB of memory.
- The browser must have free storage of about 3 times the size of the input file.
- The command line is about 1.7 times faster (see [Comparison](../explanation/comparison.md)).

!!! warning
    When you close or refresh the page, it deletes all its files. Save the files before you close the page.

## Notes

- **Get** downloads the file from Geofabrik through a small relay, because Geofabrik does not let other web pages
  read its files. **Download** saves the file on your computer instead: in Chrome and Edge it is as fast as **Get**.
- The first time that you open the page, it loads two times. This lets the cut use all the cores of the computer.
- **Border (m)** adds the data near each tile or cell to its file. **Threads** sets the number of cores that the cut
  uses.
