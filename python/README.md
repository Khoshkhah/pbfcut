# pbfcut

Cut an OpenStreetMap `.osm.pbf` file into map tiles, H3 cells or a list of areas. Each output is a complete `.osm.pbf`
file. The cut is the Rust library of pbfcut: this package only converts the arguments and the results.

```python
import pbfcut

r = pbfcut.cut_tiles("monaco.osm.pbf", "tiles", zoom=14, memory="1G")
print(r["tiles"], [f["file"] for f in r["files"]])
```

Manual: https://khoshkhah.github.io/pbfcut/manual/reference/python/
