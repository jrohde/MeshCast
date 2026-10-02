//! Manifests (PROTOCOL.md §2): a channel's signed root manifest names its collections; each
//! collection manifest lists one collection's pieces. CBOR bodies; the root is signed with
//! Ed25519, a collection manifest is authenticated by the root naming its full hash.

use alloc::string::String;
use alloc::vec::Vec;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use minicbor::{data::Type, Decoder, Encoder};

use crate::ids::{ChannelId, ObjectId, ShortId};
use crate::object::{ContentType, ObjectMeta};

/// A piece of a collection: one object, as the collection manifest lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestObject {
    pub id: ObjectId,
    pub len: u32,
    pub kind: ContentType,
    pub title: String,
}

/// An object named by id and length, such as a collection manifest, a cover or a rendition table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectRef {
    pub id: ObjectId,
    pub len: u32,
}

impl ManifestObject {
    pub fn meta(&self) -> ObjectMeta {
        ObjectMeta { id: self.id, len: self.len, kind: self.kind }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleEntry {
    pub object: ShortId,
    /// UTC seconds.
    pub start: u64,
    /// Repeat interval in seconds, 0 = once.
    pub repeat: u32,
}

/// What a collection is (PROTOCOL.md §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CollectionKind {
    /// Pieces in a fixed order.
    Album = 1,
    /// Episodes that come and go: a podcast, a bulletin; with a schedule, a station.
    Series = 2,
    /// Pieces without an order.
    Singles = 3,
}

impl CollectionKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(CollectionKind::Album),
            2 => Some(CollectionKind::Series),
            3 => Some(CollectionKind::Singles),
            _ => None,
        }
    }
}

/// A collection as the root manifest names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionRef {
    /// Chosen by the provider, the same across versions: what a node subscribes to.
    pub cid: u32,
    pub kind: CollectionKind,
    pub title: String,
    /// The collection manifest, by full hash.
    pub manifest: ObjectRef,
    pub cover: Option<ObjectRef>,
    /// Its manifest is new in this root: the collection is new, or its manifest changed since
    /// the previous root. A holder granted this root uploads these after it (PROTOCOL.md §4).
    pub changed: bool,
}

/// One collection's pieces in order, or the window of a series, and its schedule if it has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collection {
    pub cid: u32,
    pub kind: CollectionKind,
    pub title: String,
    pub pieces: Vec<ManifestObject>,
    pub schedule: Vec<ScheduleEntry>,
}

/// A channel's root manifest: its signed index of collections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub chan: [u8; 32],
    pub seq: u32,
    pub title: String,
    pub collections: Vec<CollectionRef>,
    pub prev: Option<ObjectId>,
    /// The channel's rendition table, if it names renditions (PROTOCOL.md §1.2).
    pub renditions: Option<ObjectRef>,
    pub sig: [u8; 64],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestError {
    Cbor,
    BadLength,
    BadSignature,
}

/// Lists longer than this are refused: a decoder must not allocate what a frame-sized attacker asks.
const MAX_LIST: u64 = 4096;

fn put_ref(e: &mut Encoder<Vec<u8>>, r: &Option<ObjectRef>) {
    match r {
        Some(r) => {
            e.array(2).ok();
            e.bytes(&r.id.0).ok();
            e.u32(r.len).ok();
        }
        None => {
            e.null().ok();
        }
    }
}

fn get_id(d: &mut Decoder) -> Result<ObjectId, ManifestError> {
    let b = d.bytes().map_err(|_| ManifestError::Cbor)?;
    if b.len() != 32 {
        return Err(ManifestError::BadLength);
    }
    let mut id = [0u8; 32];
    id.copy_from_slice(b);
    Ok(ObjectId(id))
}

fn get_ref(d: &mut Decoder) -> Result<Option<ObjectRef>, ManifestError> {
    if d.datatype().map_err(|_| ManifestError::Cbor)? == Type::Null {
        d.null().map_err(|_| ManifestError::Cbor)?;
        return Ok(None);
    }
    if d.array().map_err(|_| ManifestError::Cbor)? != Some(2) {
        return Err(ManifestError::Cbor);
    }
    let id = get_id(d)?;
    let len = d.u32().map_err(|_| ManifestError::Cbor)?;
    Ok(Some(ObjectRef { id, len }))
}

fn get_list(d: &mut Decoder) -> Result<u64, ManifestError> {
    let n = d.array().map_err(|_| ManifestError::Cbor)?.ok_or(ManifestError::Cbor)?;
    if n > MAX_LIST {
        return Err(ManifestError::BadLength);
    }
    Ok(n)
}

impl Collection {
    pub fn encode(&self) -> Vec<u8> {
        let mut e = Encoder::new(Vec::new());
        e.array(5).ok();
        e.u32(self.cid).ok();
        e.u8(self.kind as u8).ok();
        e.str(&self.title).ok();
        e.array(self.pieces.len() as u64).ok();
        for o in &self.pieces {
            e.array(5).ok();
            e.bytes(&o.id.0).ok();
            e.u32(o.len).ok();
            e.u8(o.kind as u8).ok();
            e.str(&o.title).ok();
            // Reserved for the integrity data of PROTOCOL.md §1: null until it is specified.
            put_ref(&mut e, &None);
        }
        e.array(self.schedule.len() as u64).ok();
        for s in &self.schedule {
            e.array(3).ok();
            e.bytes(&s.object.0).ok();
            e.u64(s.start).ok();
            e.u32(s.repeat).ok();
        }
        e.into_writer()
    }

    pub fn decode(bytes: &[u8]) -> Result<Collection, ManifestError> {
        let mut d = Decoder::new(bytes);
        if d.array().map_err(|_| ManifestError::Cbor)? != Some(5) {
            return Err(ManifestError::Cbor);
        }
        let cid = d.u32().map_err(|_| ManifestError::Cbor)?;
        let kind = CollectionKind::from_u8(d.u8().map_err(|_| ManifestError::Cbor)?).ok_or(ManifestError::Cbor)?;
        let title = String::from(d.str().map_err(|_| ManifestError::Cbor)?);
        let n = get_list(&mut d)?;
        let mut pieces = Vec::with_capacity(n as usize);
        for _ in 0..n {
            if d.array().map_err(|_| ManifestError::Cbor)? != Some(5) {
                return Err(ManifestError::Cbor);
            }
            let id = get_id(&mut d)?;
            let len = d.u32().map_err(|_| ManifestError::Cbor)?;
            let kind = ContentType::from_u8(d.u8().map_err(|_| ManifestError::Cbor)?);
            let title = String::from(d.str().map_err(|_| ManifestError::Cbor)?);
            // The integrity slot is read and ignored until it is specified.
            let _integrity = get_ref(&mut d)?;
            pieces.push(ManifestObject { id, len, kind, title });
        }
        let ns = get_list(&mut d)?;
        let mut schedule = Vec::with_capacity(ns as usize);
        for _ in 0..ns {
            if d.array().map_err(|_| ManifestError::Cbor)? != Some(3) {
                return Err(ManifestError::Cbor);
            }
            let ob = d.bytes().map_err(|_| ManifestError::Cbor)?;
            if ob.len() != 8 {
                return Err(ManifestError::BadLength);
            }
            let mut o = [0u8; 8];
            o.copy_from_slice(ob);
            let start = d.u64().map_err(|_| ManifestError::Cbor)?;
            let repeat = d.u32().map_err(|_| ManifestError::Cbor)?;
            schedule.push(ScheduleEntry { object: ShortId(o), start, repeat });
        }
        Ok(Collection { cid, kind, title, pieces, schedule })
    }

    /// The collection manifest as an object: id = hash of its encoding.
    pub fn as_object(&self) -> (ObjectMeta, Vec<u8>) {
        let bytes = self.encode();
        (ObjectMeta { id: ObjectId::of(&bytes), len: bytes.len() as u32, kind: ContentType::Collection }, bytes)
    }

    /// How a root manifest names this collection; `changed` if this manifest is new in that root.
    pub fn reference(&self, cover: Option<ObjectRef>, changed: bool) -> CollectionRef {
        let (meta, _) = self.as_object();
        CollectionRef { cid: self.cid, kind: self.kind, title: self.title.clone(), manifest: ObjectRef { id: meta.id, len: meta.len }, cover, changed }
    }
}

impl Manifest {
    fn body_bytes(chan: &[u8; 32], seq: u32, title: &str, collections: &[CollectionRef], prev: &Option<ObjectId>, renditions: &Option<ObjectRef>) -> Vec<u8> {
        let mut e = Encoder::new(Vec::new());
        e.array(6).ok();
        e.bytes(chan).ok();
        e.u32(seq).ok();
        e.str(title).ok();
        e.array(collections.len() as u64).ok();
        for c in collections {
            e.array(6).ok();
            e.u32(c.cid).ok();
            e.u8(c.kind as u8).ok();
            e.str(&c.title).ok();
            put_ref(&mut e, &Some(c.manifest));
            put_ref(&mut e, &c.cover);
            e.bool(c.changed).ok();
        }
        match prev {
            Some(p) => {
                e.bytes(&p.0).ok();
            }
            None => {
                e.null().ok();
            }
        }
        put_ref(&mut e, renditions);
        e.into_writer()
    }

    pub fn sign(key: &SigningKey, seq: u32, title: &str, collections: Vec<CollectionRef>, prev: Option<ObjectId>, renditions: Option<ObjectRef>) -> Manifest {
        let chan = key.verifying_key().to_bytes();
        let body = Self::body_bytes(&chan, seq, title, &collections, &prev, &renditions);
        let sig = key.sign(&body).to_bytes();
        Manifest { chan, seq, title: String::from(title), collections, prev, renditions, sig }
    }

    pub fn encode(&self) -> Vec<u8> {
        let body = Self::body_bytes(&self.chan, self.seq, &self.title, &self.collections, &self.prev, &self.renditions);
        let mut e = Encoder::new(Vec::new());
        e.array(2).ok();
        e.bytes(&body).ok();
        e.bytes(&self.sig).ok();
        e.into_writer()
    }

    pub fn decode(bytes: &[u8]) -> Result<Manifest, ManifestError> {
        let mut d = Decoder::new(bytes);
        if d.array().map_err(|_| ManifestError::Cbor)? != Some(2) {
            return Err(ManifestError::Cbor);
        }
        let body = d.bytes().map_err(|_| ManifestError::Cbor)?;
        let sig_b = d.bytes().map_err(|_| ManifestError::Cbor)?;
        if sig_b.len() != 64 {
            return Err(ManifestError::BadLength);
        }
        let mut sig = [0u8; 64];
        sig.copy_from_slice(sig_b);

        let mut b = Decoder::new(body);
        if b.array().map_err(|_| ManifestError::Cbor)? != Some(6) {
            return Err(ManifestError::Cbor);
        }
        let chan_b = b.bytes().map_err(|_| ManifestError::Cbor)?;
        if chan_b.len() != 32 {
            return Err(ManifestError::BadLength);
        }
        let mut chan = [0u8; 32];
        chan.copy_from_slice(chan_b);
        let seq = b.u32().map_err(|_| ManifestError::Cbor)?;
        let title = String::from(b.str().map_err(|_| ManifestError::Cbor)?);
        let n = get_list(&mut b)?;
        let mut collections = Vec::with_capacity(n as usize);
        for _ in 0..n {
            if b.array().map_err(|_| ManifestError::Cbor)? != Some(6) {
                return Err(ManifestError::Cbor);
            }
            let cid = b.u32().map_err(|_| ManifestError::Cbor)?;
            let kind = CollectionKind::from_u8(b.u8().map_err(|_| ManifestError::Cbor)?).ok_or(ManifestError::Cbor)?;
            let t = String::from(b.str().map_err(|_| ManifestError::Cbor)?);
            let manifest = get_ref(&mut b)?.ok_or(ManifestError::Cbor)?;
            let cover = get_ref(&mut b)?;
            let changed = b.bool().map_err(|_| ManifestError::Cbor)?;
            collections.push(CollectionRef { cid, kind, title: t, manifest, cover, changed });
        }
        let prev = match b.datatype().map_err(|_| ManifestError::Cbor)? {
            Type::Null => {
                b.null().map_err(|_| ManifestError::Cbor)?;
                None
            }
            _ => Some(get_id(&mut b)?),
        };
        let renditions = get_ref(&mut b)?;
        let m = Manifest { chan, seq, title, collections, prev, renditions, sig };
        if !m.verify() {
            return Err(ManifestError::BadSignature);
        }
        Ok(m)
    }

    pub fn verify(&self) -> bool {
        let Ok(vk) = VerifyingKey::from_bytes(&self.chan) else { return false };
        let body = Self::body_bytes(&self.chan, self.seq, &self.title, &self.collections, &self.prev, &self.renditions);
        let sig = Signature::from_bytes(&self.sig);
        vk.verify(&body, &sig).is_ok()
    }

    pub fn channel_id(&self) -> ChannelId {
        ChannelId::of_pubkey(&self.chan)
    }

    /// The root manifest as an object: id = hash of its encoding.
    pub fn as_object(&self) -> (ObjectMeta, Vec<u8>) {
        let bytes = self.encode();
        (ObjectMeta { id: ObjectId::of(&bytes), len: bytes.len() as u32, kind: ContentType::Manifest }, bytes)
    }

    /// The collection this root names by the short id of its manifest.
    pub fn collection_by_manifest(&self, short: &ShortId) -> Option<&CollectionRef> {
        self.collections.iter().find(|c| c.manifest.id.short() == *short)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn album() -> Collection {
        let obj = ManifestObject { id: ObjectId::of(b"track"), len: 43_008, kind: ContentType::Music, title: String::from("Track 1") };
        Collection { cid: 7, kind: CollectionKind::Album, title: String::from("First album"), pieces: vec![obj.clone()], schedule: vec![ScheduleEntry { object: obj.id.short(), start: 1000, repeat: 0 }] }
    }

    #[test]
    fn collection_encode_decode() {
        let c = album();
        assert_eq!(Collection::decode(&c.encode()).unwrap(), c);
        let (meta, bytes) = c.as_object();
        assert_eq!(meta.kind, ContentType::Collection);
        assert_eq!(meta.id, ObjectId::of(&bytes));
    }

    #[test]
    fn sign_encode_decode_verify() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let c = album();
        let cover = ObjectRef { id: ObjectId::of(b"cover"), len: 14_000 };
        let table = ObjectRef { id: ObjectId::of(b"rendition table"), len: 120 };
        for (cover, renditions) in [(None, None), (Some(cover), Some(table))] {
            let m = Manifest::sign(&key, 1, "Test channel", vec![c.reference(cover, true)], None, renditions);
            assert_eq!(Manifest::decode(&m.encode()).unwrap(), m);
        }
        let m = Manifest::sign(&key, 1, "Test channel", vec![c.reference(None, false)], None, None);
        assert!(m.verify());
        let bytes = m.encode();
        let back = Manifest::decode(&bytes).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.channel_id(), ChannelId::of_pubkey(&key.verifying_key().to_bytes()));
        assert_eq!(back.collection_by_manifest(&c.as_object().0.id.short()).map(|r| r.cid), Some(7));
        // Tamper with the title inside the body: signature must fail.
        let mut bad = bytes.clone();
        let pos = bad.windows(4).position(|w| w == b"Test").unwrap();
        bad[pos] = b'X';
        assert_eq!(Manifest::decode(&bad), Err(ManifestError::BadSignature));
    }
}
