# Cut a file into your own areas

This tutorial cuts the OpenStreetMap data of Monaco into three areas, with one read of the file. This is the job of
`osmium extract -c`, and pbfcut reads the same config file. The areas must not overlap.

## Before you start

- You must have pbfcut. To install it, do steps 1 and 2 of [Cut a file into tiles](cut-to-tiles.md).
- You must have the file `monaco-latest.osm.pbf` from step 2 of that tutorial.

## Procedure

1. Write the list of areas in a file `areas.geojson`. Each feature is one area. The property `name` gives the name of
   the output file:

    ```json
    {"type": "FeatureCollection", "features": [
      {"type": "Feature", "properties": {"name": "west"},
       "geometry": {"type": "Polygon", "coordinates": [[[7.40, 43.72], [7.419, 43.72], [7.419, 43.76], [7.40, 43.76], [7.40, 43.72]]]}},
      {"type": "Feature", "properties": {"name": "east"},
       "geometry": {"type": "Polygon", "coordinates": [[[7.42, 43.72], [7.45, 43.72], [7.45, 43.76], [7.42, 43.76], [7.42, 43.72]]]}},
      {"type": "Feature", "properties": {"name": "middle"}, "bbox": [7.419, 43.72, 7.42, 43.76], "geometry": null}
    ]}
    ```

    The third area has no geometry. pbfcut uses its `bbox` member: west, south, east, north.

2. Cut the file:

    ```bash
    pbfcut areas monaco-latest.osm.pbf --areas areas.geojson -o areas/
    ```

3. Look at the result. The folder `areas/` has one file for each area with a node inside:

    ```text
    east.osm.pbf  middle.osm.pbf  west.osm.pbf
    ```

    A way with a node in an area is in the file of that area, with all its nodes, also the nodes outside the area.

    The area `middle` has a border in common with `west` and with `east`. A node on such a border goes to the first of
    the two areas in the list: `west` or `east`. If two areas overlap, pbfcut stops with an error that names them and writes no file.

4. To use another property for the names, give `--name`. For example, with a property `code` in each feature:

    ```bash
    pbfcut areas monaco-latest.osm.pbf --areas areas.geojson --name code -o areas/
    ```

    A feature without the property gets the name from its `id` member. A feature without the two gets the name
    `area-N`, with `N` its number in the list (from 0).

## Use the config file of osmium

If you have a config file of `osmium extract`, give it to `--areas`. pbfcut uses `output` for the file name, and
`bbox`, `polygon` or `multipolygon` for the area:

```json
{
  "directory": "areas",
  "extracts": [
    {"output": "west.osm.pbf", "bbox": [7.40, 43.72, 7.425, 43.76]},
    {"output": "east.osm.pbf", "polygon": {"file_name": "east.poly", "file_type": "poly"}}
  ]
}
```

```bash
pbfcut areas monaco-latest.osm.pbf --areas config.json
```

Without `-o`, pbfcut writes the files into the `directory` of the config. A `file_name` is relative to the folder of
the config file. The file can be GeoJSON, a polygon file (`.poly`) or WKT (see
[Boundary formats](../reference/boundaries.md)).

To compare the result with osmium, run osmium with the same config and the same rules, and then `pbfcut compare`:

```bash
osmium extract -c config.json -s smart -S types=multipolygon,boundary -d osmium-areas/ monaco-latest.osm.pbf
pbfcut compare areas/ osmium-areas/
```

On the Monaco test file, the two tools give the same elements in each file.
