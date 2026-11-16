# Cut a file into your own areas

This tutorial cuts the OpenStreetMap data of Monaco into three areas, as `osmium extract -c` does. The areas must
not overlap.

## Before you start

Do steps 1 and 2 of [Cut a file into tiles](cut-to-tiles.md): install pbfcut and download `monaco-latest.osm.pbf`.

## Procedure

1. Write the areas in a file `areas.geojson`. Each feature is one area, and its property `name` is the file name:

    ```json
    {"type": "FeatureCollection", "features": [
      {"type": "Feature", "properties": {"name": "west"},
       "geometry": {"type": "Polygon", "coordinates": [[[7.40, 43.72], [7.419, 43.72], [7.419, 43.76], [7.40, 43.76], [7.40, 43.72]]]}},
      {"type": "Feature", "properties": {"name": "east"},
       "geometry": {"type": "Polygon", "coordinates": [[[7.42, 43.72], [7.45, 43.72], [7.45, 43.76], [7.42, 43.76], [7.42, 43.72]]]}},
      {"type": "Feature", "properties": {"name": "middle"}, "bbox": [7.419, 43.72, 7.42, 43.76], "geometry": null}
    ]}
    ```

    The area `middle` is a box: a `bbox` (west, south, east, north) and no geometry.

2. Cut the file:

    ```bash
    pbfcut areas monaco-latest.osm.pbf --areas areas.geojson -o areas/
    ```

3. Look at the folder `areas/`. It has the files `west.osm.pbf`, `east.osm.pbf` and `middle.osm.pbf`.

## Good to know

- A way with a node in an area goes to that area with all its nodes, also the nodes outside the area.
- A node on a border between two areas goes to the first of the two in the list.
- If two areas overlap, pbfcut stops with an error that names them, and writes no file.
- To take the file names from another property, add `--name PROPERTY`.

## Use an osmium config file

pbfcut also reads the config file of `osmium extract`:

```json
{"directory": "areas", "extracts": [
  {"output": "west.osm.pbf", "bbox": [7.40, 43.72, 7.419, 43.76]},
  {"output": "east.osm.pbf", "polygon": {"file_name": "east.poly", "file_type": "poly"}}
]}
```

```bash
pbfcut areas monaco-latest.osm.pbf --areas config.json
```

The files go into the `directory` of the config. A polygon file can be GeoJSON, `.poly` or WKT
([Boundary formats](../reference/boundaries.md)).
