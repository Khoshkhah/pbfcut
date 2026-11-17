# Cut a file in your browser

The [online page](https://khoshkhah.github.io/pbfcut/) cuts a `.osm.pbf` file in your browser. You do not install
anything, and the file does not leave your computer.

It works in each browser, also on a phone. Chrome and Edge on a computer can also save all files into a folder.

## Procedure

1. Open <https://khoshkhah.github.io/pbfcut/>.
2. Select the input file. Do one of these steps:
    - Type some letters of a Geofabrik region, for example `mona`. One match is selected at once. With more
      matches, press Enter for the first one, or select one with the arrow keys and Enter or Tab. Click **Get**.
    - Select a `.osm.pbf` file on your computer.
3. Select the kind of cut:

    | Kind | Output |
    |---|---|
    | **Tiles** | One file for each map tile, for example `8_136_74.osm.pbf`. |
    | **H3 cells** | One file for each H3 cell, for example `872b02d27ffffff.osm.pbf`. |
    | **Areas** | One file for each feature of a GeoJSON file, with its property `name` as the file name. The areas must not overlap. |

4. Set the **Zoom** or the **Resolution**. The list shows the size of the tiles or cells. A zoom or resolution
   that gives more than 20,000 files for the area is off, and the page lowers a choice that is too high.
5. Optional: select a **Boundary** file (GeoJSON, `.poly` or WKT). The cut then keeps only the data inside it.
   Without it, a Geofabrik region uses the outline of the region, also a local file with a Geofabrik name such as
   `estonia-latest.osm.pbf`.
6. Look at the map. It shows the tiles, cells or areas of the cut and the boundary, with their number. If the
   boundary or the areas file has an error, the page shows it here.
7. Click **Cut**. The page shows each step and its time. Under a step, "read" and "storage" are the time that it
   waited for the input file and for the storage of the site; the rest is work on the threads.
8. Click **Download all as .zip** (each browser), or **Save all to a folder** and select a folder (Chrome and Edge).
   The `.zip` holds the files as they are, with no more compression. It takes no more storage in the browser.

The files are equal to the files of the [command line](../reference/cli.md) with the same settings.

## Limits

- The page can use at most 4 GiB of memory.
- The browser must have free storage of about 3 times the size of the input file.
- The command line is about 1.7 times faster (see [Comparison](../explanation/comparison.md)).

!!! warning
    When you close or refresh the page, it deletes all its files. Save the files before you close the page.

## Notes

- **Cancel** stops a **Get** or a **Download**; no part of the file stays.
- **Get** downloads the file from Geofabrik through a small relay, because Geofabrik does not let other web pages
  read its files. **Download** saves the file on your computer instead: in Chrome and Edge it is as fast as **Get**.
- The first time that you open the page, it loads two times. This lets the cut use all the cores of the computer.
- **zlib compression** (on by default) gives normal `.osm.pbf` files. Off, the cut is about 20 % faster and the files
  are about 1.6 times larger; each OSM program reads them too.
- The input file can have zlib blocks (as most files) or raw blocks. LZMA, LZ4 and Zstandard blocks stop the cut with
  an error.
- **Border (m)** adds the data near each tile or cell to its file. **Threads** sets the number of cores that the cut
  uses.
