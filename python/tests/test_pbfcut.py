"""The Python results equal the results of the command line, on Monaco. PBFCUT_BIN: the command (default: pbfcut on
the PATH, else target/release/pbfcut of the repository)."""

import json
import os
import shutil
import subprocess
from pathlib import Path

import pytest

import pbfcut

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "tests" / "data"
PBF = DATA / "monaco.osm.pbf"
BIN = os.environ.get("PBFCUT_BIN") or shutil.which("pbfcut") or str(ROOT / "target" / "release" / "pbfcut")


def cli(*args):
    return subprocess.run([BIN, *map(str, args)], check=True, capture_output=True, text=True).stdout


def same(py_report, cli_dir, py_dir, cli_out):
    """The files of the two folders have the same elements, and the two reports name the same files and counts."""
    assert pbfcut.compare(py_dir, cli_dir) == []
    cli("compare", py_dir, cli_dir)
    c = json.loads(cli_out)
    files = [{k: v for k, v in f.items() if k != "bytes"} for f in py_report["files"]]
    assert files == c["files"]
    assert {k: v for k, v in py_report.items() if k not in ("files", "seconds")} == {
        k: v for k, v in c.items() if k not in ("files", "seconds")
    }
    for f in py_report["files"]:
        assert f["bytes"] == (py_dir / f["file"]).stat().st_size
    assert py_report["files"], "the cut wrote no file"


def test_tiles(tmp_path):
    out = cli("tiles", PBF, "--zoom", 14, "--boundary", DATA / "monaco.geojson", "-o", tmp_path / "cli")
    r = pbfcut.cut_tiles(PBF, tmp_path / "py", zoom=14, boundary=DATA / "monaco.geojson", memory="256M", threads=2)
    same(r, tmp_path / "cli", tmp_path / "py", out)


def test_no_compression(tmp_path):
    """compress=False equals --no-compression, has the elements of the zlib files, and gives larger files."""
    out = cli("tiles", PBF, "--zoom", 14, "--boundary", DATA / "monaco.geojson", "--no-compression", "-o", tmp_path / "cli")
    r = pbfcut.cut_tiles(PBF, tmp_path / "py", zoom=14, boundary=DATA / "monaco.geojson", compress=False)
    same(r, tmp_path / "cli", tmp_path / "py", out)
    z = pbfcut.cut_tiles(PBF, tmp_path / "zlib", zoom=14, boundary=DATA / "monaco.geojson")
    assert pbfcut.compare(tmp_path / "zlib", tmp_path / "py") == []
    assert 2 * sum(f["bytes"] for f in r["files"]) > 3 * sum(f["bytes"] for f in z["files"])


def test_tiles_bbox_border(tmp_path):
    out = cli("tiles", PBF, "--zoom", 15, "--bbox", "7.40,43.72,7.44,43.76", "--border", 50, "-o", tmp_path / "cli")
    r = pbfcut.cut_tiles(str(PBF), str(tmp_path / "py"), 15, boundary=(7.40, 43.72, 7.44, 43.76), border=50)
    same(r, tmp_path / "cli", tmp_path / "py", out)


def test_cells(tmp_path):
    out = cli("h3", PBF, "--res", 8, "--boundary", DATA / "monaco.geojson", "-o", tmp_path / "cli")
    r = pbfcut.cut_cells(PBF, tmp_path / "py", 8, boundary=DATA / "monaco.geojson")
    same(r, tmp_path / "cli", tmp_path / "py", out)


def test_areas(tmp_path):
    areas = tmp_path / "areas.geojson"
    box = lambda w, s, e, n: {"type": "Polygon", "coordinates": [[[w, s], [e, s], [e, n], [w, n], [w, s]]]}
    areas.write_text(json.dumps({"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {"name": "west"}, "geometry": box(7.40, 43.72, 7.42, 43.76)},
        {"type": "Feature", "properties": {"name": "east"}, "geometry": box(7.42, 43.72, 7.44, 43.76)},
    ]}))
    out = cli("areas", PBF, "--areas", areas, "--complete", "multipolygon", "-o", tmp_path / "cli")
    r = pbfcut.cut_areas(PBF, areas, tmp_path / "py", complete=["multipolygon"])
    same(r, tmp_path / "cli", tmp_path / "py", out)
    assert [f["area"] for f in r["files"]] == ["west", "east"]


def test_errors(tmp_path):
    with pytest.raises(pbfcut.Error, match="res: a number from 0 to 15"):
        pbfcut.cut_cells(PBF, tmp_path, 16)
    with pytest.raises(pbfcut.Error, match="not a size"):
        pbfcut.cut_tiles(PBF, tmp_path, memory="lots")
    with pytest.raises(pbfcut.Error, match="nothing.osm.pbf"):
        pbfcut.cut_tiles(tmp_path / "nothing.osm.pbf", tmp_path, boundary=(7.4, 43.7, 7.5, 43.8))
    a, b = tmp_path / "a", tmp_path / "b"
    with pytest.raises(pbfcut.Error, match="no bounding box"):
        pbfcut.cut_tiles(PBF, a)
    pbfcut.cut_tiles(PBF, a, zoom=13, boundary=DATA / "monaco.geojson")
    pbfcut.cut_tiles(PBF, b, zoom=14, boundary=DATA / "monaco.geojson")
    assert pbfcut.compare(a, b)
