//! The OSM elements as the decoder gives them and as the writer takes them.

/// The tags in the order of the PBF file. No tags: an empty list.
pub type RawTags = Vec<(String, String)>;

/// A receiver of the elements of a file: all nodes, ways and relations, with all their tags, in the order of the file.
/// The tags come in the order of the file; no tags is an empty iterator; a member without a role has the role "".
pub trait RawSink {
    fn node<'a>(&mut self, id: i64, lat: f64, lon: f64, tags: &mut dyn Iterator<Item = (&'a str, &'a str)>);
    fn way<'a>(
        &mut self,
        id: i64,
        tags: &mut dyn Iterator<Item = (&'a str, &'a str)>,
        refs: &mut dyn Iterator<Item = i64>,
    );
    fn relation<'a>(
        &mut self,
        id: i64,
        tags: &mut dyn Iterator<Item = (&'a str, &'a str)>,
        members: &mut dyn Iterator<Item = (i64, MemberType, &'a str)>,
    );
}

#[derive(Clone)]
pub struct RawNode {
    pub id: i64,
    pub lon: f64,
    pub lat: f64,
    pub tags: RawTags,
}

#[derive(Clone)]
pub struct RawWay {
    pub id: i64,
    pub tags: RawTags,
    pub refs: Vec<i64>,
}

/// The element type of a relation member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberType {
    Node,
    Way,
    Relation,
}

/// A relation as read: its tags and its members (id, type, role).
#[derive(Clone, Debug, Default)]
pub struct Relation {
    pub id: i64,
    pub tags: Vec<(String, String)>,
    pub members: Vec<(i64, MemberType, String)>,
}
