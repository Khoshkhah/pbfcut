//! pbfcut: cut an OpenStreetMap `.osm.pbf` file into tiles or areas, with one read of the file for all outputs
//! (docs/design.md). Each output is a complete `.osm.pbf` file: the nodes in the area, each way with a node there with
//! all its nodes, and the relations with a member there.

pub mod areas;
pub mod boundary;
pub mod decode;
pub mod elements;
pub mod extract;
pub mod store;
pub mod tilecut;
pub mod tiles;
pub mod util;
pub mod write;
