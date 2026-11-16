# Boundary formats

`--boundary FILE` and the polygon files of an area list (`--areas`) can have these formats. pbfcut finds the format from
the text of the file, not from its name.

| Format | The text starts with | Example |
|---|---|---|
| GeoJSON | `{` | `{"type": "Polygon", "coordinates": [[[7.40, 43.72], [7.45, 43.72], [7.45, 43.76], [7.40, 43.72]]]}` |
| WKT | `POLYGON` or `MULTIPOLYGON` | `POLYGON ((7.40 43.72, 7.45 43.72, 7.45 43.76, 7.40 43.72))` |
| A box | a number | `7.40,43.72,7.45,43.76` (west, south, east, north) |
| Polygon file (`.poly`) | a name line | see below |

All coordinates are longitude, then latitude, in degrees (WGS 84).

## GeoJSON

A Polygon or a MultiPolygon. It can be in a Feature or in a FeatureCollection: then the boundary is all polygons of all
features. A Feature with no geometry and a `bbox` member (`[west, south, east, north]`) is a box.

For `--boundary`, a FeatureCollection is one area. For `--areas`, each feature is one area.

## Polygon file

The osmosis polygon format. Geofabrik has one for each of its areas, next to the `.osm.pbf` file (for example
`https://download.geofabrik.de/europe/monaco.poly`).

```text
monaco
1
   7.4091   43.7247
   7.4396   43.7247
   7.4396   43.7519
   7.4091   43.7519
END
!2
   7.4200   43.7350
   7.4250   43.7350
   7.4250   43.7400
END
END
```

- The first line is the name of the area.
- Each section is one ring: a name line, one point on each line (longitude and latitude), and `END`.
- A section with a name that starts with `!` is a hole. It goes to the polygon that holds its first point.
- The last line is `END`.

## WKT

A `POLYGON` or a `MULTIPOLYGON`. The first ring of a polygon is its outer ring, and the other rings are holes.

## The box option

`--bbox W,S,E,N` gives a box with no file. You cannot give `--bbox` and `--boundary` together.

## Points on the border

A node exactly on the border of a boundary or of an area is inside it, as in osmium.
