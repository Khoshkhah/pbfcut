"""Cut an OpenStreetMap .osm.pbf file into map tiles, H3 cells or areas (the Rust library of pbfcut)."""

import os
from typing import List, Optional, Sequence, Tuple, TypedDict, Union

__version__: str

Path = Union[str, os.PathLike]
BoundaryArg = Union[Path, Tuple[float, float, float, float]]
"""A boundary file (GeoJSON, osmosis .poly or WKT), or a box (west, south, east, north) in degrees."""
Size = Union[int, str]
"""A number of bytes, or a text such as "4G" or "512M"."""

class Error(Exception):
    """An error of pbfcut, with the message of the library."""

class File(TypedDict, total=False):
    tile: str
    """The tile z/x/y (cut_tiles only)."""
    cell: str
    """The H3 cell index (cut_cells only)."""
    area: str
    """The area name (cut_areas only)."""
    file: str
    """The file name in the output folder."""
    nodes: int
    ways: int
    relations: int
    bytes: int

class Report(TypedDict, total=False):
    tiles: int
    """The number of tiles of the cut, also the ones with no file (cut_tiles only)."""
    cells: int
    """The number of cells of the cut (cut_cells only)."""
    areas: int
    """The number of areas of the cut (cut_areas only)."""
    files: List[File]
    """The files that the cut wrote: the outputs with one node or more."""
    seconds: float

def cut_tiles(
    input: Path,
    out_dir: Path,
    zoom: int = 8,
    *,
    tiles: Optional[Path] = None,
    boundary: Optional[BoundaryArg] = None,
    border: float = 0.0,
    complete: Sequence[str] = ("multipolygon", "boundary"),
    memory: Optional[Size] = None,
    temp: Optional[Path] = None,
    threads: Optional[int] = None,
    compress: bool = True,
) -> Report:
    """Cut input into the map tiles of zoom (or of the list file tiles: one z/x/y on each line) in out_dir."""

def cut_cells(
    input: Path,
    out_dir: Path,
    res: int,
    *,
    boundary: Optional[BoundaryArg] = None,
    border: float = 0.0,
    complete: Sequence[str] = ("multipolygon", "boundary"),
    memory: Optional[Size] = None,
    temp: Optional[Path] = None,
    threads: Optional[int] = None,
    compress: bool = True,
) -> Report:
    """Cut input into the H3 cells of resolution res (0 to 15) in out_dir."""

def cut_areas(
    input: Path,
    areas: Path,
    out_dir: Path,
    *,
    name: str = "name",
    boundary: Optional[BoundaryArg] = None,
    complete: Sequence[str] = ("multipolygon", "boundary"),
    memory: Optional[Size] = None,
    temp: Optional[Path] = None,
    threads: Optional[int] = None,
    compress: bool = True,
) -> Report:
    """Cut input into the areas of a GeoJSON FeatureCollection or an osmium config file, in out_dir."""

def compare(dir_a: Path, dir_b: Path) -> List[str]:
    """Compare the .osm.pbf files of two folders by their elements: one line for each file that differs."""
