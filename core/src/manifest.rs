//! Signed channel manifests (PROTOCOL.md §2). CBOR body, Ed25519 signature.

use alloc::string::String;
use alloc::vec::Vec;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use minicbor::{data::Type, Decoder, Encoder};

use crate::ids::{ChannelId, ObjectId, ShortId};
use crate::object::{ContentType, ObjectMeta};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestObject {
    pub id: ObjectId,
    pub len: u32,
    pub kind: ContentType,
    pub title: String,
}

/// An object named by id and length, such as a channel's rendition table.
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub chan: [u8; 32],
    pub seq: u32,
    pub title: String,
    pub objects: Vec<ManifestObject>,
    pub schedule: Vec<ScheduleEntry>,
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

impl Manifest {
    fn body_bytes(chan: &[u8; 32], seq: u32, title: &str, objects: &[ManifestObject], schedule: &[ScheduleEntry], prev: &Option<ObjectId>, renditions: &Option<ObjectRef>) -> Vec<u8> {
        let mut e = Encoder::new(Vec::new());
        e.array(if renditions.is_some() { 7 } else { 6 }).ok();
        e.bytes(chan).ok();
        e.u32(seq).ok();
        e.str(title).ok();
        e.array(objects.len() as u64).ok();
        for o in objects {
            e.array(4).ok();
            e.bytes(&o.id.0).ok();
            e.u32(o.len).ok();
            e.u8(o.kind as u8).ok();
            e.str(&o.title).ok();
        }
        e.array(schedule.len() as u64).ok();
        for s in schedule {
            e.array(3).ok();
            e.bytes(&s.object.0).ok();
            e.u64(s.start).ok();
            e.u32(s.repeat).ok();
        }
        match prev {
            Some(p) => {
                e.bytes(&p.0).ok();
            }
            None => {
                e.null().ok();
            }
        }
        if let Some(r) = renditions {
            e.array(2).ok();
            e.bytes(&r.id.0).ok();
            e.u32(r.len).ok();
        }
        e.into_writer()
    }

    pub fn sign(key: &SigningKey, seq: u32, title: &str, objects: Vec<ManifestObject>, schedule: Vec<ScheduleEntry>, prev: Option<ObjectId>) -> Manifest {
        Self::sign_with_renditions(key, seq, title, objects, schedule, prev, None)
    }

    pub fn sign_with_renditions(key: &SigningKey, seq: u32, title: &str, objects: Vec<ManifestObject>, schedule: Vec<ScheduleEntry>, prev: Option<ObjectId>, renditions: Option<ObjectRef>) -> Manifest {
        let chan = key.verifying_key().to_bytes();
        let body = Self::body_bytes(&chan, seq, title, &objects, &schedule, &prev, &renditions);
        let sig = key.sign(&body).to_bytes();
        Manifest { chan, seq, title: String::from(title), objects, schedule, prev, renditions, sig }
    }

    pub fn encode(&self) -> Vec<u8> {
        let body = Self::body_bytes(&self.chan, self.seq, &self.title, &self.objects, &self.schedule, &self.prev, &self.renditions);
        let mut e = Encoder::new(Vec::new());
        e.array(2).ok();
        e.bytes(&body).ok();
        e.bytes(&self.sig).ok();
        e.into_writer()
    }

    pub fn decode(bytes: &[u8]) -> Result<Manifest, ManifestError> {
        let mut d = Decoder::new(bytes);
        let n = d.array().map_err(|_| ManifestError::Cbor)?;
        if n != Some(2) {
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
        let fields = b.array().map_err(|_| ManifestError::Cbor)?;
        if fields != Some(6) && fields != Some(7) {
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
        let no = b.array().map_err(|_| ManifestError::Cbor)?.ok_or(ManifestError::Cbor)?;
        if no > 4096 {
            return Err(ManifestError::BadLength);
        }
        let mut objects = Vec::with_capacity(no as usize);
        for _ in 0..no {
            if b.array().map_err(|_| ManifestError::Cbor)? != Some(4) {
                return Err(ManifestError::Cbor);
            }
            let idb = b.bytes().map_err(|_| ManifestError::Cbor)?;
            if idb.len() != 32 {
                return Err(ManifestError::BadLength);
            }
            let mut id = [0u8; 32];
            id.copy_from_slice(idb);
            let len = b.u32().map_err(|_| ManifestError::Cbor)?;
            let kind = ContentType::from_u8(b.u8().map_err(|_| ManifestError::Cbor)?);
            let t = String::from(b.str().map_err(|_| ManifestError::Cbor)?);
            objects.push(ManifestObject { id: ObjectId(id), len, kind, title: t });
        }
        let ns = b.array().map_err(|_| ManifestError::Cbor)?.ok_or(ManifestError::Cbor)?;
        if ns > 4096 {
            return Err(ManifestError::BadLength);
        }
        let mut schedule = Vec::with_capacity(ns as usize);
        for _ in 0..ns {
            if b.array().map_err(|_| ManifestError::Cbor)? != Some(3) {
                return Err(ManifestError::Cbor);
            }
            let ob = b.bytes().map_err(|_| ManifestError::Cbor)?;
            if ob.len() != 8 {
                return Err(ManifestError::BadLength);
            }
            let mut o = [0u8; 8];
            o.copy_from_slice(ob);
            let start = b.u64().map_err(|_| ManifestError::Cbor)?;
            let repeat = b.u32().map_err(|_| ManifestError::Cbor)?;
            schedule.push(ScheduleEntry { object: ShortId(o), start, repeat });
        }
        let prev = match b.datatype().map_err(|_| ManifestError::Cbor)? {
            Type::Null => {
                b.null().map_err(|_| ManifestError::Cbor)?;
                None
            }
            _ => {
                let pb = b.bytes().map_err(|_| ManifestError::Cbor)?;
                if pb.len() != 32 {
                    return Err(ManifestError::BadLength);
                }
                let mut p = [0u8; 32];
                p.copy_from_slice(pb);
                Some(ObjectId(p))
            }
        };
        let renditions = if fields == Some(7) {
            if b.array().map_err(|_| ManifestError::Cbor)? != Some(2) {
                return Err(ManifestError::Cbor);
            }
            let rb = b.bytes().map_err(|_| ManifestError::Cbor)?;
            if rb.len() != 32 {
                return Err(ManifestError::BadLength);
            }
            let mut rid = [0u8; 32];
            rid.copy_from_slice(rb);
            let len = b.u32().map_err(|_| ManifestError::Cbor)?;
            Some(ObjectRef { id: ObjectId(rid), len })
        } else {
            None
        };
        let m = Manifest { chan, seq, title, objects, schedule, prev, renditions, sig };
        if !m.verify() {
            return Err(ManifestError::BadSignature);
        }
        Ok(m)
    }

    pub fn verify(&self) -> bool {
        let Ok(vk) = VerifyingKey::from_bytes(&self.chan) else { return false };
        let body = Self::body_bytes(&self.chan, self.seq, &self.title, &self.objects, &self.schedule, &self.prev, &self.renditions);
        let sig = Signature::from_bytes(&self.sig);
        vk.verify(&body, &sig).is_ok()
    }

    pub fn channel_id(&self) -> ChannelId {
        ChannelId::of_pubkey(&self.chan)
    }

    /// The manifest as an object: id = hash of its encoding.
    pub fn as_object(&self) -> (ObjectMeta, Vec<u8>) {
        let bytes = self.encode();
        (ObjectMeta { id: ObjectId::of(&bytes), len: bytes.len() as u32, kind: ContentType::Manifest }, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn sign_encode_decode_verify() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let obj = ManifestObject { id: ObjectId::of(b"track"), len: 540_000, kind: ContentType::Music, title: String::from("Track 1") };
        let table = ObjectRef { id: ObjectId::of(b"rendition table"), len: 120 };
        for renditions in [None, Some(table)] {
            let m = Manifest::sign_with_renditions(&key, 1, "Test channel", vec![obj.clone()], vec![], None, renditions);
            assert_eq!(Manifest::decode(&m.encode()).unwrap(), m);
        }
        let m = Manifest::sign(&key, 1, "Test channel", vec![obj.clone()], vec![ScheduleEntry { object: obj.id.short(), start: 1000, repeat: 0 }], None);
        assert!(m.verify());
        let bytes = m.encode();
        let back = Manifest::decode(&bytes).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.channel_id(), ChannelId::of_pubkey(&key.verifying_key().to_bytes()));
        // Tamper with the title inside the body: signature must fail.
        let mut bad = bytes.clone();
        let pos = bad.windows(4).position(|w| w == b"Test").unwrap();
        bad[pos] = b'X';
        assert_eq!(Manifest::decode(&bad), Err(ManifestError::BadSignature));
    }
}
