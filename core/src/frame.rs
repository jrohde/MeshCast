//! On-air frames (PROTOCOL.md §3). Every frame: `ver:type` byte, `flags` byte, body, CRC-16.
//! All parsers are total: any input yields `Ok(Frame)` or `Err(DecodeError)`, never a panic.

use alloc::vec::Vec;

use crate::crc::crc16;
use crate::ids::{ChannelId, NodeId, ShortId};

pub const VERSION: u8 = 0;
/// Symbol size `T` in bytes. One symbol per BULK frame.
pub const SYMBOL_SIZE: usize = 200;
pub const MAX_GOSSIP_IDS: usize = 12;
/// WANT entries carry a granted uploader (12 bytes each), so fewer fit.
pub const MAX_WANT: usize = 8;
pub const MAX_ANNOUNCE_ENTRIES: usize = 8;
pub const MAX_NACK_RANGES: usize = 40;
pub const MAX_HEARD: usize = 3;
pub const MAX_FRAME: usize = 250;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum CarrierKind {
    LoraControl = 0,
    GfskBulk = 1,
    EspNow = 2,
    Ip = 3,
    /// LoRa used as the bulk carrier itself (sparse, long-range cells): slow but far.
    LoraBulk = 4,
}

impl CarrierKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::LoraControl),
            1 => Some(Self::GfskBulk),
            2 => Some(Self::EspNow),
            3 => Some(Self::Ip),
            4 => Some(Self::LoraBulk),
            _ => None,
        }
    }
    pub fn is_bulk(&self) -> bool {
        !matches!(self, CarrierKind::LoraControl)
    }
}

/// Priority class (EtherFatsoen mechanism 2). Lower value = higher priority.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub enum Class {
    Control = 0,
    Metadata = 1,
    Content = 2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum FrameType {
    Beacon = 1,
    Bulk = 2,
    Gossip = 3,
    ManifestAnnounce = 4,
    Nack = 5,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beacon {
    /// Which bulk carrier kind this announcer runs a carousel on.
    pub carrier: CarrierKind,
    pub announcer: NodeId,
    pub score: u16,
    /// Milliseconds until the next beacon from this announcer on this carrier.
    pub next_ms: u16,
    /// Carousel round counter.
    pub round: u16,
    pub utc: u64,
    pub time_quality: u8,
    /// The announcer's colour: its rank in its conflict set. On frequency-agile carriers the
    /// colour is the offset on the shared base hop sequence, on single-channel carriers the
    /// time slot. Followers and uploaders derive the announcer's channel and slot from it.
    pub colour: u8,
    /// Number of colours in use around this announcer: how many slots the cycle has.
    pub colours: u8,
    /// Spectrum weather: measured occupancy (percent) on up to four bulk channels.
    pub occupancy: [u8; 4],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bulk {
    pub object: ShortId,
    pub block: u16,
    /// Encoding symbol id. `< k` is a source symbol; `>= k` a repair symbol (v1).
    pub esi: u16,
    pub k: u16,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gossip {
    pub node: NodeId,
    /// The announcer this node currently follows (NONE if none heard), its colour and the
    /// number of colours in its cycle, so that holders can reach it without having heard its beacon.
    pub announcer: NodeId,
    pub announcer_colour: u8,
    pub announcer_colours: u8,
    /// Other announcers this node also hears, with the colour each announced: the conflict
    /// report that drives colouring.
    pub heard: Vec<(NodeId, u8)>,
    pub have: Vec<ShortId>,
    /// Objects wanted, each with the node granted to upload it (NONE = open ask: holders
    /// answer with a HAVE offer and the announcer grants one of them).
    pub want: Vec<(ShortId, NodeId)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnounceEntry {
    pub channel: ChannelId,
    pub manifest: ShortId,
    pub seq: u32,
    pub len: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestAnnounce {
    pub node: NodeId,
    pub entries: Vec<AnnounceEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nack {
    pub node: NodeId,
    pub object: ShortId,
    pub block: u16,
    /// Missing source symbols as `(first_esi, count)` ranges.
    pub missing: Vec<(u16, u16)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Beacon(Beacon),
    Bulk(Bulk),
    Gossip(Gossip),
    ManifestAnnounce(ManifestAnnounce),
    Nack(Nack),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    TooShort,
    BadCrc,
    BadVersion,
    BadType,
    BadLength,
    BadValue,
}

impl Frame {
    pub fn frame_type(&self) -> FrameType {
        match self {
            Frame::Beacon(_) => FrameType::Beacon,
            Frame::Bulk(_) => FrameType::Bulk,
            Frame::Gossip(_) => FrameType::Gossip,
            Frame::ManifestAnnounce(_) => FrameType::ManifestAnnounce,
            Frame::Nack(_) => FrameType::Nack,
        }
    }

    pub fn class(&self) -> Class {
        match self {
            Frame::Beacon(_) | Frame::ManifestAnnounce(_) => Class::Control,
            Frame::Gossip(_) | Frame::Nack(_) => Class::Metadata,
            Frame::Bulk(_) => Class::Content,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(MAX_FRAME);
        match self {
            Frame::Beacon(b) => {
                out.push((VERSION << 4) | FrameType::Beacon as u8);
                out.push(b.carrier as u8 & 0x07);
                out.extend_from_slice(&b.announcer.0.to_le_bytes());
                out.extend_from_slice(&b.score.to_le_bytes());
                out.extend_from_slice(&b.next_ms.to_le_bytes());
                out.extend_from_slice(&b.round.to_le_bytes());
                out.extend_from_slice(&b.utc.to_le_bytes());
                out.push(b.time_quality);
                out.push(b.colour);
                out.push(b.colours);
                out.extend_from_slice(&b.occupancy);
            }
            Frame::Bulk(b) => {
                out.push((VERSION << 4) | FrameType::Bulk as u8);
                out.push(0);
                out.extend_from_slice(&b.object.0);
                out.extend_from_slice(&b.block.to_le_bytes());
                out.extend_from_slice(&b.esi.to_le_bytes());
                out.extend_from_slice(&b.k.to_le_bytes());
                let mut payload = [0u8; SYMBOL_SIZE];
                let n = b.payload.len().min(SYMBOL_SIZE);
                payload[..n].copy_from_slice(&b.payload[..n]);
                out.extend_from_slice(&payload);
            }
            Frame::Gossip(g) => {
                let nheard = g.heard.len().min(MAX_HEARD);
                out.push((VERSION << 4) | FrameType::Gossip as u8);
                out.push(nheard as u8);
                out.extend_from_slice(&g.node.0.to_le_bytes());
                out.extend_from_slice(&g.announcer.0.to_le_bytes());
                out.push(g.announcer_colour);
                out.push(g.announcer_colours);
                let nh = g.have.len().min(MAX_GOSSIP_IDS);
                let nw = g.want.len().min(MAX_WANT);
                out.push(nh as u8);
                out.push(nw as u8);
                for (id, colour) in &g.heard[..nheard] {
                    out.extend_from_slice(&id.0.to_le_bytes());
                    out.push(*colour);
                }
                for id in &g.have[..nh] {
                    out.extend_from_slice(&id.0);
                }
                for (id, grant) in &g.want[..nw] {
                    out.extend_from_slice(&id.0);
                    out.extend_from_slice(&grant.0.to_le_bytes());
                }
            }
            Frame::ManifestAnnounce(m) => {
                out.push((VERSION << 4) | FrameType::ManifestAnnounce as u8);
                out.push(0);
                out.extend_from_slice(&m.node.0.to_le_bytes());
                let n = m.entries.len().min(MAX_ANNOUNCE_ENTRIES);
                out.push(n as u8);
                for e in &m.entries[..n] {
                    out.extend_from_slice(&e.channel.0);
                    out.extend_from_slice(&e.manifest.0);
                    out.extend_from_slice(&e.seq.to_le_bytes());
                    out.extend_from_slice(&e.len.to_le_bytes());
                }
            }
            Frame::Nack(n) => {
                out.push((VERSION << 4) | FrameType::Nack as u8);
                out.push(0);
                out.extend_from_slice(&n.node.0.to_le_bytes());
                out.extend_from_slice(&n.object.0);
                out.extend_from_slice(&n.block.to_le_bytes());
                let nr = n.missing.len().min(MAX_NACK_RANGES);
                out.push(nr as u8);
                for (start, count) in &n.missing[..nr] {
                    out.extend_from_slice(&start.to_le_bytes());
                    out.extend_from_slice(&count.to_le_bytes());
                }
            }
        }
        let c = crc16(&out);
        out.extend_from_slice(&c.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Frame, DecodeError> {
        if bytes.len() < 4 {
            return Err(DecodeError::TooShort);
        }
        let (body, crc_bytes) = bytes.split_at(bytes.len() - 2);
        let expect = u16::from_le_bytes([crc_bytes[0], crc_bytes[1]]);
        if crc16(body) != expect {
            return Err(DecodeError::BadCrc);
        }
        if body[0] >> 4 != VERSION {
            return Err(DecodeError::BadVersion);
        }
        let flags = body[1];
        let mut c = Cursor { data: body, pos: 2 };
        let frame = match body[0] & 0x0F {
            1 => {
                let carrier = CarrierKind::from_u8(flags & 0x07).ok_or(DecodeError::BadValue)?;
                let announcer = NodeId(c.u32()?);
                let score = c.u16()?;
                let next_ms = c.u16()?;
                let round = c.u16()?;
                let utc = c.u64()?;
                let time_quality = c.u8()?;
                let colour = c.u8()?;
                let colours = c.u8()?;
                let occ = c.bytes(4)?;
                let mut occupancy = [0u8; 4];
                occupancy.copy_from_slice(occ);
                Frame::Beacon(Beacon { carrier, announcer, score, next_ms, round, utc, time_quality, colour, colours, occupancy })
            }
            2 => {
                let object = c.short()?;
                let block = c.u16()?;
                let esi = c.u16()?;
                let k = c.u16()?;
                let payload = c.bytes(SYMBOL_SIZE)?.to_vec();
                if k == 0 {
                    return Err(DecodeError::BadValue);
                }
                Frame::Bulk(Bulk { object, block, esi, k, payload })
            }
            3 => {
                let nheard = (flags & 0x03) as usize;
                let node = NodeId(c.u32()?);
                let announcer = NodeId(c.u32()?);
                let announcer_colour = c.u8()?;
                let announcer_colours = c.u8()?;
                let nh = c.u8()? as usize;
                let nw = c.u8()? as usize;
                if nh > MAX_GOSSIP_IDS || nw > MAX_WANT || nheard > MAX_HEARD {
                    return Err(DecodeError::BadLength);
                }
                let mut heard = Vec::with_capacity(nheard);
                for _ in 0..nheard {
                    let id = NodeId(c.u32()?);
                    let colour = c.u8()?;
                    heard.push((id, colour));
                }
                let mut have = Vec::with_capacity(nh);
                for _ in 0..nh {
                    have.push(c.short()?);
                }
                let mut want = Vec::with_capacity(nw);
                for _ in 0..nw {
                    let id = c.short()?;
                    let grant = NodeId(c.u32()?);
                    want.push((id, grant));
                }
                Frame::Gossip(Gossip { node, announcer, announcer_colour, announcer_colours, heard, have, want })
            }
            4 => {
                let node = NodeId(c.u32()?);
                let n = c.u8()? as usize;
                if n > MAX_ANNOUNCE_ENTRIES {
                    return Err(DecodeError::BadLength);
                }
                let mut entries = Vec::with_capacity(n);
                for _ in 0..n {
                    let ch = c.bytes(8)?;
                    let mut channel = [0u8; 8];
                    channel.copy_from_slice(ch);
                    let manifest = c.short()?;
                    let seq = c.u32()?;
                    let len = c.u32()?;
                    entries.push(AnnounceEntry { channel: ChannelId(channel), manifest, seq, len });
                }
                Frame::ManifestAnnounce(ManifestAnnounce { node, entries })
            }
            5 => {
                let node = NodeId(c.u32()?);
                let object = c.short()?;
                let block = c.u16()?;
                let nr = c.u8()? as usize;
                if nr > MAX_NACK_RANGES {
                    return Err(DecodeError::BadLength);
                }
                let mut missing = Vec::with_capacity(nr);
                for _ in 0..nr {
                    let start = c.u16()?;
                    let count = c.u16()?;
                    if count == 0 {
                        return Err(DecodeError::BadValue);
                    }
                    missing.push((start, count));
                }
                Frame::Nack(Nack { node, object, block, missing })
            }
            _ => return Err(DecodeError::BadType),
        };
        if c.pos != body.len() {
            return Err(DecodeError::BadLength);
        }
        Ok(frame)
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::TooShort)?;
        if end > self.data.len() {
            return Err(DecodeError::TooShort);
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        let b = self.bytes(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_le_bytes(a))
    }
    fn short(&mut self) -> Result<ShortId, DecodeError> {
        let b = self.bytes(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(ShortId(a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn roundtrip_all_types() {
        let frames = vec![
            Frame::Beacon(Beacon {
                carrier: CarrierKind::GfskBulk,
                announcer: NodeId(42),
                score: 300,
                next_ms: 60000,
                round: 7,
                utc: 1_700_000_000,
                time_quality: 2,
                colour: 3,
                colours: 5,
                occupancy: [10, 20, 30, 40],
            }),
            Frame::Bulk(Bulk { object: ShortId([1; 8]), block: 2, esi: 3, k: 100, payload: vec![9u8; SYMBOL_SIZE] }),
            Frame::Gossip(Gossip { node: NodeId(1), announcer: NodeId(2), announcer_colour: 1, announcer_colours: 4, heard: vec![(NodeId(7), 0), (NodeId(8), 1), (NodeId(9), 2)], have: vec![ShortId([3; 8]); 12], want: vec![(ShortId([4; 8]), NodeId(5)); 8] }),
            Frame::ManifestAnnounce(ManifestAnnounce {
                node: NodeId(5),
                entries: vec![AnnounceEntry { channel: ChannelId([6; 8]), manifest: ShortId([7; 8]), seq: 9, len: 1234 }; 8],
            }),
            Frame::Nack(Nack { node: NodeId(8), object: ShortId([2; 8]), block: 0, missing: vec![(0, 3), (10, 1)] }),
        ];
        for f in frames {
            let bytes = f.encode();
            assert!(bytes.len() <= MAX_FRAME, "{:?} is {} bytes", f.frame_type(), bytes.len());
            let back = Frame::decode(&bytes).unwrap();
            assert_eq!(back, f);
        }
    }

    #[test]
    fn sizes() {
        let b = Frame::Beacon(Beacon { carrier: CarrierKind::GfskBulk, announcer: NodeId(1), score: 0, next_ms: 0, round: 0, utc: 0, time_quality: 0, colour: 0, colours: 1, occupancy: [0; 4] });
        assert_eq!(b.encode().len(), 29);
        let k = Frame::Bulk(Bulk { object: ShortId([0; 8]), block: 0, esi: 0, k: 1, payload: vec![] });
        assert_eq!(k.encode().len(), 218);
    }

    #[test]
    fn garbage_never_panics() {
        let mut rng = crate::rng::Rng::new(7);
        for len in 0..260usize {
            for _ in 0..20 {
                let mut v = vec![0u8; len];
                for b in v.iter_mut() {
                    *b = rng.next_u64() as u8;
                }
                let _ = Frame::decode(&v);
            }
        }
    }

    #[test]
    fn corrupted_crc_rejected() {
        let f = Frame::Gossip(Gossip { node: NodeId(1), announcer: NodeId(2), announcer_colour: 0, announcer_colours: 1, heard: vec![], have: vec![], want: vec![] });
        let mut bytes = f.encode();
        bytes[3] ^= 1;
        assert_eq!(Frame::decode(&bytes), Err(DecodeError::BadCrc));
    }
}
