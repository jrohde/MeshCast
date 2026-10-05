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
    /// The channel's description of itself, for guides (PROTOCOL.md §2).
    pub card: Option<Card>,
    pub sig: [u8; 64],
}

/// What a channel says it is (PROTOCOL.md §2, "A channel describes itself"). Kept as signed, so
/// that the signature still verifies; what a guide reads goes through the accessors, which
/// read a code they do not know, or a field beyond its limits, as absent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Card {
    /// After `podcast:medium`; bit 7 marks a list of channels.
    pub medium: u8,
    /// RDS programme types (IEC 62106, the European table).
    pub genres: Vec<u8>,
    /// ISO 639-1 codes, or ISO 639-3 where a language has none.
    pub langs: Vec<String>,
    /// ISO 3166-1 country, optionally with an ISO 3166-2 subdivision.
    pub area: Option<String>,
    pub about: Option<String>,
}

/// What a card may hold at most on the air; beyond these the root is invalid. What a guide reads
/// is narrower still (`Card::genres_known` and the others).
const CARD_MAX_GENRES: usize = 16;
const CARD_MAX_LANGS: u64 = 8;
const CARD_MAX_SHORT: usize = 16;
const CARD_MAX_ABOUT: usize = 255;

impl Card {
    /// The medium, 1 to 8, or none.
    pub fn medium_known(&self) -> Option<u8> {
        Some(self.medium & 0x7f).filter(|m| (1..=8).contains(m))
    }

    /// Whether the channel is a list of other channels.
    pub fn is_list(&self) -> bool {
        self.medium & 0x80 != 0 && self.medium_known().is_some()
    }

    /// At most three programme types, 1 to 29: not the alarm codes.
    pub fn genres_known(&self) -> impl Iterator<Item = u8> + '_ {
        self.genres.iter().copied().filter(|g| (1..=29).contains(g)).take(3)
    }

    /// At most three languages, two or three lower-case letters each.
    pub fn langs_known(&self) -> impl Iterator<Item = &str> + '_ {
        self.langs.iter().map(|l| l.as_str()).filter(|l| (2..=3).contains(&l.len()) && l.bytes().all(|b| b.is_ascii_lowercase())).take(3)
    }

    /// A country, optionally with a subdivision: `NL` or `NL-UT`.
    pub fn area_known(&self) -> Option<&str> {
        self.area.as_deref().filter(|a| {
            let b = a.as_bytes();
            b.len() >= 2 && b.len() <= 6 && b[..2].iter().all(|c| c.is_ascii_uppercase()) && (b.len() == 2 || (b.len() >= 4 && b[2] == b'-' && b[3..].iter().all(|c| c.is_ascii_alphanumeric())))
        })
    }

    /// One line of at most 80 bytes.
    pub fn about_known(&self) -> Option<&str> {
        self.about.as_deref().filter(|a| a.len() <= 80 && !a.contains('\n'))
    }

    fn put(&self, e: &mut Encoder<Vec<u8>>) {
        e.array(5).ok();
        e.u8(self.medium).ok();
        e.bytes(&self.genres).ok();
        e.array(self.langs.len() as u64).ok();
        for l in &self.langs {
            e.str(l).ok();
        }
        for s in [&self.area, &self.about] {
            match s {
                Some(s) => {
                    e.str(s).ok();
                }
                None => {
                    e.null().ok();
                }
            }
        }
    }

    fn get(d: &mut Decoder) -> Result<Card, ManifestError> {
        if d.array().map_err(|_| ManifestError::Cbor)? != Some(5) {
            return Err(ManifestError::Cbor);
        }
        let medium = d.u8().map_err(|_| ManifestError::Cbor)?;
        let genres = d.bytes().map_err(|_| ManifestError::Cbor)?;
        if genres.len() > CARD_MAX_GENRES {
            return Err(ManifestError::BadLength);
        }
        let genres = genres.to_vec();
        let n = d.array().map_err(|_| ManifestError::Cbor)?.ok_or(ManifestError::Cbor)?;
        if n > CARD_MAX_LANGS {
            return Err(ManifestError::BadLength);
        }
        let mut langs = Vec::new();
        for _ in 0..n {
            let l = d.str().map_err(|_| ManifestError::Cbor)?;
            if l.len() > CARD_MAX_SHORT {
                return Err(ManifestError::BadLength);
            }
            langs.push(String::from(l));
        }
        let mut text = |max: usize| -> Result<Option<String>, ManifestError> {
            if d.datatype().map_err(|_| ManifestError::Cbor)? == Type::Null {
                d.null().map_err(|_| ManifestError::Cbor)?;
                return Ok(None);
            }
            let s = d.str().map_err(|_| ManifestError::Cbor)?;
            if s.len() > max {
                return Err(ManifestError::BadLength);
            }
            Ok(Some(String::from(s)))
        };
        let area = text(CARD_MAX_SHORT)?;
        let about = text(CARD_MAX_ABOUT)?;
        Ok(Card { medium, genres, langs, area, about })
    }
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
        // Grown as entries arrive, not reserved from the count the input claims: a dozen bytes
        // claiming 4096 pieces made room for all of them (docs/ABUSE.md, item 5).
        let mut pieces = Vec::new();
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
        let mut schedule = Vec::new();
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
    fn body_bytes(chan: &[u8; 32], seq: u32, title: &str, collections: &[CollectionRef], prev: &Option<ObjectId>, renditions: &Option<ObjectRef>, card: &Option<Card>) -> Vec<u8> {
        let mut e = Encoder::new(Vec::new());
        e.array(7).ok();
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
        match card {
            Some(c) => c.put(&mut e),
            None => {
                e.null().ok();
            }
        }
        e.into_writer()
    }

    pub fn sign(key: &SigningKey, seq: u32, title: &str, collections: Vec<CollectionRef>, prev: Option<ObjectId>, renditions: Option<ObjectRef>) -> Manifest {
        Self::sign_card(key, seq, title, collections, prev, renditions, None)
    }

    /// A root with the channel's description of itself (PROTOCOL.md §2).
    pub fn sign_card(key: &SigningKey, seq: u32, title: &str, collections: Vec<CollectionRef>, prev: Option<ObjectId>, renditions: Option<ObjectRef>, card: Option<Card>) -> Manifest {
        let chan = key.verifying_key().to_bytes();
        let body = Self::body_bytes(&chan, seq, title, &collections, &prev, &renditions, &card);
        let sig = key.sign(&body).to_bytes();
        Manifest { chan, seq, title: String::from(title), collections, prev, renditions, card, sig }
    }

    pub fn encode(&self) -> Vec<u8> {
        let body = Self::body_bytes(&self.chan, self.seq, &self.title, &self.collections, &self.prev, &self.renditions, &self.card);
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
        if b.array().map_err(|_| ManifestError::Cbor)? != Some(7) {
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
        // Grown as entries arrive, not reserved from the claimed count (docs/ABUSE.md, item 5).
        let mut collections = Vec::new();
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
        let card = if b.datatype().map_err(|_| ManifestError::Cbor)? == Type::Null {
            b.null().map_err(|_| ManifestError::Cbor)?;
            None
        } else {
            Some(Card::get(&mut b)?)
        };
        let m = Manifest { chan, seq, title, collections, prev, renditions, card, sig };
        if !m.verify() {
            return Err(ManifestError::BadSignature);
        }
        Ok(m)
    }

    pub fn verify(&self) -> bool {
        let Ok(vk) = VerifyingKey::from_bytes(&self.chan) else { return false };
        let body = Self::body_bytes(&self.chan, self.seq, &self.title, &self.collections, &self.prev, &self.renditions, &self.card);
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

    fn podcast_card() -> Card {
        Card { medium: 1, genres: vec![1, 7], langs: vec![String::from("nl"), String::from("en")], area: None, about: None }
    }

    #[test]
    fn a_card_is_signed_with_its_root() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let c = album();
        let m = Manifest::sign_card(&key, 3, "Test channel", vec![c.reference(None, true)], None, None, Some(podcast_card()));
        let back = Manifest::decode(&m.encode()).unwrap();
        assert_eq!(back.card, Some(podcast_card()));
        // Changing the card breaks the signature: only the channel's key describes it.
        let mut bytes = m.encode();
        let pos = bytes.windows(2).position(|w| w == b"nl").unwrap();
        bytes[pos] = b'd';
        bytes[pos + 1] = b'e';
        assert_eq!(Manifest::decode(&bytes), Err(ManifestError::BadSignature));
    }

    #[test]
    fn a_guide_reads_what_it_does_not_know_as_absent() {
        // Codes from a newer vocabulary, the alarm codes and fields over their limits are read as
        // absent, and the root stays valid (PROTOCOL.md §2).
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let card = Card { medium: 0x80 | 42, genres: vec![0, 31, 4, 40, 10, 11, 12], langs: vec![String::from("NL"), String::from("fy"), String::from("nds"), String::from("en"), String::from("de")], area: Some(String::from("NL-UTRECHT")), about: Some(String::from("x").repeat(81)) };
        let m = Manifest::sign_card(&key, 1, "T", vec![], None, None, Some(card));
        let back = Manifest::decode(&m.encode()).unwrap();
        let c = back.card.unwrap();
        assert_eq!(c.medium_known(), None);
        assert!(!c.is_list());
        assert_eq!(c.genres_known().collect::<Vec<_>>(), vec![4, 10, 11]);
        assert_eq!(c.langs_known().collect::<Vec<_>>(), vec!["fy", "nds", "en"]);
        assert_eq!(c.area_known(), None);
        assert_eq!(c.about_known(), None);
        let list = Card { medium: 0x80 | 2, area: Some(String::from("NL-UT")), ..Card::default() };
        assert!(list.is_list() && list.medium_known() == Some(2) && list.area_known() == Some("NL-UT"));
    }

    #[test]
    fn a_card_beyond_its_bounds_makes_the_root_invalid() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let card = Card { genres: vec![1; 17], ..Card::default() };
        let m = Manifest::sign_card(&key, 1, "T", vec![], None, None, Some(card));
        assert_eq!(Manifest::decode(&m.encode()), Err(ManifestError::BadLength));
    }

    #[test]
    fn what_a_card_adds_to_a_root() {
        // PROTOCOL.md §2: a card without `about` is 10 to 25 bytes; a root of one collection with a
        // short title and no card is 185 bytes.
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let c = CollectionRef { cid: 1, kind: CollectionKind::Series, title: String::from("Bulletins of channel 3"), manifest: ObjectRef { id: ObjectId([1; 32]), len: 900 }, cover: None, changed: true };
        let bare = Manifest::sign(&key, 12, "Channel 3", vec![c.clone()], None, None).encode().len();
        assert_eq!(bare, 185);
        let small = Card { medium: 7, genres: vec![1], langs: vec![String::from("nl")], area: None, about: None };
        let big = Card { medium: 1, genres: vec![1, 7, 29], langs: vec![String::from("nl"), String::from("fy"), String::from("nds")], area: Some(String::from("NL-UT")), about: None };
        for (card, lo, hi) in [(small, 10, 10), (big, 24, 24)] {
            let with = Manifest::sign_card(&key, 12, "Channel 3", vec![c.clone()], None, None, Some(card)).encode().len();
            let added = with - bare + 1; // the null it replaces
            assert!(added >= lo && added <= hi, "a card adds {added} bytes");
        }
    }
}
