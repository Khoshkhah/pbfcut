# Cut a file in your browser

The online page cuts a `.osm.pbf` file into map tiles in your browser: <https://khoshkhah.github.io/pbfcut/>. The page
does not upload the file. The cut runs in WebAssembly, and its temporary files and the tile files go into the storage
of the site (OPFS), not into the memory of the tab.

Use Chrome or Edge on a computer. Other browsers can do the cut, but only Chrome and Edge can save all files into a
folder. On a phone, the page downloads one file at a time.

## Procedure

1. Open <https://khoshkhah.github.io/pbfcut/>.
2. Select the input file:
    - **A region of Geofabrik**: type the name of the region, for example `Monaco`, and select it in the list.
      Then click one of the two buttons:
        - **Get** puts the file into the storage of the site, with its progress. The cut then uses it.
        - **Download** saves the file on your computer. Then select it as a file on this computer.

        Geofabrik does not let other web pages read its files (it sends no CORS headers). Thus **Get** gets the file
        through a relay: a small Cloudflare Worker that accepts only files of download.geofabrik.de.
    - **A file on this computer**: select the `.osm.pbf` file.
3. Select the kind of cut:
    - **Tiles**: the square tiles of web maps. At zoom z, the map has 2^z × 2^z tiles. One file for each tile, for
      example `8_136_74.osm.pbf`.
    - **H3 cells**: the hexagons of the H3 grid. Each resolution more makes the cells 7 times smaller. One file for
      each cell, with the H3 index as its name, for example `872b02d27ffffff.osm.pbf`.
    - **Areas**: no grid. One file for each feature of a GeoJSON FeatureCollection, named by its property `name`
      (as `pbfcut areas`). The areas can overlap. A file with one Polygon gives one area, named after the file.
4. Set the settings:
    - **Zoom** (tiles) or **Resolution** (H3 cells): the size of the tiles or cells. The list shows the size of each.
    - **Border (m)**: each tile file also gets the data in this border around the tile (default 0).
    - **Threads**: the number of threads of the cut. The default is the number of cores of the computer.
    - **Boundary** (optional): a GeoJSON file in longitude and latitude
      (WGS 84) with a Polygon, a MultiPolygon, a Feature or a FeatureCollection. All its polygons together are the
      area. With no boundary, the tiles or cells cover the bounding box in the header of the file.
5. Click **Cut**. The page shows the time, the step, the WebAssembly memory and the storage of the site. Each step
   of the cut gets a check mark and its time. **Stop** stops the cut.
6. Click **Save all to a folder**, and select a folder. The page writes one file for each tile, for example
   `8_136_74.osm.pbf`. To get one file, click its name in the table.

The tile files are equal to the files of `pbfcut tiles` with the same zoom and boundary (see
[The command line](../reference/cli.md)).

## Threads

Threads need a page that is cross-origin isolated. GitHub Pages cannot send the two headers for this (COOP and COEP),
so a service worker of the page adds them. The page loads two times when you open it for the first time. If the browser
does not let the service worker run, the page uses one thread, and it says so under the settings.

## Speed

The command line is about 1.6 times faster than the online page (see [Comparison](../explanation/comparison.md)).

## Limits

- The WebAssembly memory has a maximum of 4 GiB.
- The storage of the site must hold the temporary files and the tile files. The browser gives each site a part of the
  free disk space. A file of 1 GB needs approximately 3 GB of free storage.
- The page keeps the tile files of the last cut only. A new cut removes them.

!!! warning
    When you refresh or close the page, the page removes all its files from the storage of the site: the downloaded
    region, the temporary files and the tile files. Save the tile files before you refresh or close the page.
