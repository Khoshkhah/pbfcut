"""Write the regions of Geofabrik for the page from Geofabrik's index (index-v1.json, with the outlines):

- web/www/regions.json: [id, name, parent, file] of each region with a .osm.pbf file;
- web/www/regions/<id>.json: the outline of each region (a GeoJSON MultiPolygon, the same polygon as its .poly file).
  The page uses it as the boundary of a cut of that region, and shows it on the map.

    python3 web/regions.py INDEX.json
"""
import json
import re
import sys
from pathlib import Path

WWW = Path(__file__).resolve().parent / "www"
index = json.load(open(sys.argv[1]))["features"]
regions = [f for f in index if "pbf" in f["properties"].get("urls", {})]
out = [[f["properties"]["id"], f["properties"]["name"], f["properties"].get("parent", ""), f["properties"]["urls"]["pbf"]] for f in regions]
(WWW / "regions.json").write_text(json.dumps(out, separators=(",", ":")))
(WWW / "regions").mkdir(exist_ok=True)
for f in regions:
    # the file name of app.js: the id with each character that is not a letter, a digit, "_" or "-" as "_"
    name = re.sub(r"[^A-Za-z0-9_-]", "_", f["properties"]["id"])
    (WWW / "regions" / f"{name}.json").write_text(json.dumps(f["geometry"], separators=(",", ":")))
print(f"{len(out)} regions")
