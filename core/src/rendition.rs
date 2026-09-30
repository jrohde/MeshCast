//! Rendition tables (PROTOCOL.md §1.2): the renditions a source names for its audio objects.
//!
//! A rendition is an ordinary object whose bytes are a pure function of another object's bytes
//! under a profile. The source lists them in a table, an object of its own; the manifest names
//! the table by id, so the table is signed through the manifest and each rendition through the
//! table. Only nodes that need renditions fetch it.

use alloc::vec::Vec;

use minicbor::{Decoder, Encoder};

use crate::ids::{ObjectId, ShortId};
use crate::object::{ContentType, ObjectMeta};

/// One rendition: made from `parent` by `profile`, named by its hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rendition {
    pub parent: ShortId,
    pub profile: u8,
    pub id: ObjectId,
    pub len: u32,
}

impl Rendition {
    pub fn meta(&self) -> ObjectMeta {
        ObjectMeta { id: self.id, len: self.len, kind: ContentType::Opus }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenditionTable {
    pub entries: Vec<Rendition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableError;

impl RenditionTable {
    /// CBOR: an array of [parent short id, profile, rendition id, length].
    pub fn encode(&self) -> Vec<u8> {
        let mut e = Encoder::new(Vec::new());
        e.array(self.entries.len() as u64).ok();
        for r in &self.entries {
            e.array(4).ok();
            e.bytes(&r.parent.0).ok();
            e.u8(r.profile).ok();
            e.bytes(&r.id.0).ok();
            e.u32(r.len).ok();
        }
        e.into_writer()
    }

    pub fn decode(bytes: &[u8]) -> Result<RenditionTable, TableError> {
        let mut d = Decoder::new(bytes);
        let n = d.array().map_err(|_| TableError)?.ok_or(TableError)?;
        if n > 4096 {
            return Err(TableError);
        }
        let mut entries = Vec::with_capacity(n as usize);
        for _ in 0..n {
            if d.array().map_err(|_| TableError)? != Some(4) {
                return Err(TableError);
            }
            let p = d.bytes().map_err(|_| TableError)?;
            let profile = d.u8().map_err(|_| TableError)?;
            let i = d.bytes().map_err(|_| TableError)?;
            let len = d.u32().map_err(|_| TableError)?;
            if p.len() != 8 || i.len() != 32 {
                return Err(TableError);
            }
            let mut parent = [0u8; 8];
            parent.copy_from_slice(p);
            let mut id = [0u8; 32];
            id.copy_from_slice(i);
            entries.push(Rendition { parent: ShortId(parent), profile, id: ObjectId(id), len });
        }
        Ok(RenditionTable { entries })
    }

    /// The table as an object: id = hash of its encoding.
    pub fn as_object(&self) -> (ObjectMeta, Vec<u8>) {
        let bytes = self.encode();
        (ObjectMeta { id: ObjectId::of(&bytes), len: bytes.len() as u32, kind: ContentType::Renditions }, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn table_round_trip() {
        let t = RenditionTable {
            entries: vec![
                Rendition { parent: ObjectId::of(b"song").short(), profile: 1, id: ObjectId::of(b"song as opus"), len: 360_000 },
                Rendition { parent: ObjectId::of(b"news").short(), profile: 2, id: ObjectId::of(b"news as opus"), len: 180_000 },
            ],
        };
        let (meta, bytes) = t.as_object();
        assert_eq!(meta.kind, ContentType::Renditions);
        assert_eq!(RenditionTable::decode(&bytes).unwrap(), t);
        assert!(RenditionTable::decode(&bytes[..bytes.len() - 1]).is_err());
    }
}
