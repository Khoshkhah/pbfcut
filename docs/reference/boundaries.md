# Boundary formats

`--boundary FILE` and the polygon files of an area list can have these formats. pbfcut finds the format from the text
of the file, not from its name. All coordinates are longitude, then latitude, in degrees (WGS 84).

| Format | Example |
|---|---|
| GeoJSON | `{"type": "Polygon", "coordinates": [[[7.40, 43.72], [7.45, 43.72], [7.45, 43.76], [7.40, 43.72]]]}` |
| WKT | `POLYGON ((7.40 43.72, 7.45 43.72, 7.45 43.76, 7.40 43.72))` |
| Polygon file (`.poly`) | see below |
| A box (`--bbox`) | `7.40,43.72,7.45,43.76` (west, south, east, north) |

A node exactly on a border is inside, as in osmium.

## GeoJSON

A Polygon or a MultiPolygon, also inside a Feature or a FeatureCollection. A Feature with no geometry and a `bbox`
member is a box. For `--boundary`, all features together are one area. For `--areas`, each feature is one area.

## WKT

A `POLYGON` or a `MULTIPOLYGON`. The first ring of a polygon is its outside, and the other rings are holes.

## Polygon file

The osmosis format. Geofabrik has one for each region, for example
<https://download.geofabrik.de/europe/monaco.poly>.

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

The first line is the name. Each section is one ring, with one point on each line, and ends with `END`. A section
whose name starts with `!` is a hole. The last line is `END`.
