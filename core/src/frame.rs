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
/// Bytes of a GOSSIP frame shared by WANT entries (13 each) and want sets (23 each), and by HAVE
/// ids (8 each) and have sets (18 each): what eight WANT entries and twelve HAVE ids took before
/// sets existed, so that no frame grows (PROTOCOL.md §3.3).
pub const WANT_BUDGET: usize = 13 * MAX_WANT;
pub const HAVE_BUDGET: usize = 8 * MAX_GOSSIP_IDS;
pub const WANT_SET_BYTES: usize = 23;
pub const HAVE_SET_BYTES: usize = 18;
/// A set covers this many consecutive pieces of one manifest.
pub const SET_BITS: u16 = 64;

/// Pieces of one manifest: bit `i` of `bits` is piece `first + i` of the manifest whose short id
/// is `manifest`, in the order the manifest lists them (PROTOCOL.md §3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PieceSet {
    pub manifest: ShortId,
    pub first: u16,
    pub bits: u64,
}

impl PieceSet {
    /// The indices of the pieces in this set.
    pub fn pieces(&self) -> impl Iterator<Item = u16> + '_ {
        (0..SET_BITS).filter(move |i| self.bits >> i & 1 == 1).map(move |i| self.first.saturating_add(i))
    }

    /// Sets covering `indices` of `manifest`, each spanning at most `SET_BITS` pieces.
    pub fn cover(manifest: ShortId, indices: &mut [u16]) -> Vec<PieceSet> {
        indices.sort_unstable();
        let mut out = Vec::new();
        let mut i = 0;
        while i < indices.len() {
            let first = indices[i];
            let mut bits = 0u64;
            while i < indices.len() && indices[i] < first.saturating_add(SET_BITS) {
                bits |= 1 << (indices[i] - first);
                i += 1;
            }
            out.push(PieceSet { manifest, first, bits });
        }
        out
    }
}

/// The phase byte of a WANT entry or want set: the upload phase in its low four bits (§4), and in
/// bit 7 an announcer's mark that one of its own followers asked for the object (PROTOCOL.md
/// §3.3): an ask for listeners, which another cell's follower may relay, rather than one to fill
/// the announcer's library.
pub const PHASE_MASK: u8 = 0x0f;
pub const ASK_LISTENED: u8 = 0x80;

/// Pieces wanted, as a WANT entry is for one object: open (`grant` NONE) or granted to one
/// uploader, in the phase of the announcer's listening time it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WantSet {
    pub set: PieceSet,
    pub grant: NodeId,
    pub phase: u8,
}

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

/// Beacon capability bit: the announcer is mains powered.
pub const CAP_MAINS: u8 = 0b10;
/// Beacon capability bit: the announcer has an IP uplink.
pub const CAP_IP: u8 = 0b01;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beacon {
    /// Which bulk carrier kind this announcer runs a carousel on.
    pub carrier: CarrierKind,
    pub announcer: NodeId,
    pub score: u16,
    /// What the announcer is, as opposed to what it happens to experience (PROTOCOL.md §5.1):
    /// `CAP_MAINS | CAP_IP`. Compared as a number, so mains power outranks an uplink.
    pub caps: u8,
    /// Milliseconds until the next beacon from this announcer on this carrier.
    pub next_ms: u16,
    /// Carousel round counter.
    pub round: u16,
    /// The sender's shared time in ms as the frame begins (PROTOCOL.md §6).
    pub time: u64,
    pub time_quality: u8,
    /// The announcer's colour: its rank in its conflict set. On frequency-agile carriers the
    /// colour is the offset on the shared base hop sequence, on single-channel carriers the
    /// time slot. Followers and uploaders derive the announcer's channel and slot from it.
    pub colour: u8,
    /// Number of colours in use around this announcer: how many slots the cycle has.
    pub colours: u8,
    /// How many phases this announcer's listening time is divided into for uploads: each running
    /// upload transmits only in its own phase (PROTOCOL.md §4). 1 means no division.
    pub upload_phases: u8,
    /// Spectrum weather: measured occupancy (percent) on up to four bulk channels.
    pub occupancy: [u8; 4],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bulk {
    pub object: ShortId,
    pub block: u16,
    /// Encoding symbol id. `< k` is a source symbol; `>= k` a repair symbol (v1).
    pub esi: u16,
    /// The object's length in bytes: it fixes the number of blocks and each block's K, so any
    /// symbol is usable without the object's metadata (PROTOCOL.md §3.2).
    pub len: u32,
    pub payload: Vec<u8>,
}

impl Bulk {
    /// K of this symbol's block.
    pub fn k(&self) -> u16 {
        crate::object::block_k(self.len, self.block)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gossip {
    pub node: NodeId,
    /// The announcer this node currently follows (NONE if none heard), its colour and the
    /// number of colours in its cycle, so that holders can reach it without having heard its beacon.
    pub announcer: NodeId,
    pub announcer_colour: u8,
    pub announcer_colours: u8,
    /// Other announcers this node also hears, each with the colour it announced and the number
    /// of colours it believes are in use: the conflict report that drives colouring. The count
    /// travels because a slot cycle only works if the announcers sharing it agree on its length,
    /// and two announcers in conflict often cannot hear each other directly.
    pub heard: Vec<(NodeId, u8, u8)>,
    pub have: Vec<ShortId>,
    /// Pieces held, by manifest and bitmap: an offer of a whole collection, or what an announcer
    /// serves, in one entry.
    pub have_sets: Vec<PieceSet>,
    /// Objects wanted, each with the node granted to upload it (NONE = open ask: holders
    /// answer with a HAVE offer and the announcer grants one of them) and, for a grant, the
    /// phase of the announcer's listening time the upload uses (PROTOCOL.md §4).
    pub want: Vec<(ShortId, NodeId, u8)>,
    /// Pieces wanted by manifest and bitmap: a whole ask, or a whole grant, for a collection in
    /// one entry.
    pub sets: Vec<WantSet>,
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
    /// Entries in ascending channel order, wrapping around once at most (PROTOCOL.md §3.4).
    pub entries: Vec<AnnounceEntry>,
    /// The entries are every manifest the sender serves (flag bit 0): an announcer whose list
    /// fits in one frame. A follower then knows that a channel not in it is unknown to it.
    pub whole: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nack {
    pub node: NodeId,
    pub object: ShortId,
    pub block: u16,
    /// The holder an announcer names to answer; NONE lets any holder answer after a wait.
    pub answerer: NodeId,
    /// The upload phase answers use (PROTOCOL.md §4); set by an announcer, 0 otherwise.
    pub phase: u8,
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
                out.push((b.carrier as u8 & 0x07) | (b.caps & 0x03) << 3);
                out.extend_from_slice(&b.announcer.0.to_le_bytes());
                out.extend_from_slice(&b.score.to_le_bytes());
                out.extend_from_slice(&b.next_ms.to_le_bytes());
                out.extend_from_slice(&b.round.to_le_bytes());
                out.extend_from_slice(&b.time.to_le_bytes());
                out.push(b.time_quality);
                out.push(b.colour);
                out.push(b.colours);
                out.push(b.upload_phases);
                out.extend_from_slice(&b.occupancy);
            }
            Frame::Bulk(b) => {
                out.push((VERSION << 4) | FrameType::Bulk as u8);
                out.push(0);
                out.extend_from_slice(&b.object.0);
                out.extend_from_slice(&b.block.to_le_bytes());
                out.extend_from_slice(&b.esi.to_le_bytes());
                out.extend_from_slice(&b.len.to_le_bytes());
                let mut payload = [0u8; SYMBOL_SIZE];
                let n = b.payload.len().min(SYMBOL_SIZE);
                payload[..n].copy_from_slice(&b.payload[..n]);
                out.extend_from_slice(&payload);
            }
            Frame::Gossip(g) => {
                let nheard = g.heard.len().min(MAX_HEARD);
                let nw = g.want.len().min(MAX_WANT);
                let ns = g.sets.len().min((WANT_BUDGET - 13 * nw) / WANT_SET_BYTES).min(7);
                let nh = g.have.len().min(MAX_GOSSIP_IDS);
                let nhs = g.have_sets.len().min((HAVE_BUDGET - 8 * nh) / HAVE_SET_BYTES).min(7);
                out.push((VERSION << 4) | FrameType::Gossip as u8);
                out.push(nheard as u8 | (ns as u8) << 2 | (nhs as u8) << 5);
                out.extend_from_slice(&g.node.0.to_le_bytes());
                out.extend_from_slice(&g.announcer.0.to_le_bytes());
                out.push(g.announcer_colour);
                out.push(g.announcer_colours);
                out.push(nh as u8);
                out.push(nw as u8);
                for (id, colour, colours) in &g.heard[..nheard] {
                    out.extend_from_slice(&id.0.to_le_bytes());
                    out.push(*colour);
                    out.push(*colours);
                }
                for id in &g.have[..nh] {
                    out.extend_from_slice(&id.0);
                }
                for p in &g.have_sets[..nhs] {
                    out.extend_from_slice(&p.manifest.0);
                    out.extend_from_slice(&p.first.to_le_bytes());
                    out.extend_from_slice(&p.bits.to_le_bytes());
                }
                for (id, grant, phase) in &g.want[..nw] {
                    out.extend_from_slice(&id.0);
                    out.extend_from_slice(&grant.0.to_le_bytes());
                    out.push(*phase);
                }
                for w in &g.sets[..ns] {
                    out.extend_from_slice(&w.set.manifest.0);
                    out.extend_from_slice(&w.set.first.to_le_bytes());
                    out.extend_from_slice(&w.set.bits.to_le_bytes());
                    out.extend_from_slice(&w.grant.0.to_le_bytes());
                    out.push(w.phase);
                }
            }
            Frame::ManifestAnnounce(m) => {
                out.push((VERSION << 4) | FrameType::ManifestAnnounce as u8);
                out.push(m.whole as u8);
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
                out.push(n.phase & 0x0f);
                out.extend_from_slice(&n.node.0.to_le_bytes());
                out.extend_from_slice(&n.object.0);
                out.extend_from_slice(&n.block.to_le_bytes());
                out.extend_from_slice(&n.answerer.0.to_le_bytes());
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
                let caps = (flags >> 3) & 0x03;
                let announcer = NodeId(c.u32()?);
                let score = c.u16()?;
                let next_ms = c.u16()?;
                let round = c.u16()?;
                let time = c.u64()?;
                let time_quality = c.u8()?;
                let colour = c.u8()?;
                let colours = c.u8()?;
                let upload_phases = c.u8()?;
                let occ = c.bytes(4)?;
                let mut occupancy = [0u8; 4];
                occupancy.copy_from_slice(occ);
                Frame::Beacon(Beacon { carrier, announcer, score, caps, next_ms, round, time, time_quality, colour, colours, upload_phases, occupancy })
            }
            2 => {
                let object = c.short()?;
                let block = c.u16()?;
                let esi = c.u16()?;
                let len = c.u32()?;
                let payload = c.bytes(SYMBOL_SIZE)?.to_vec();
                if crate::object::block_k(len, block) == 0 {
                    return Err(DecodeError::BadValue);
                }
                Frame::Bulk(Bulk { object, block, esi, len, payload })
            }
            3 => {
                let nheard = (flags & 0x03) as usize;
                let ns = ((flags >> 2) & 0x07) as usize;
                let nhs = ((flags >> 5) & 0x07) as usize;
                let node = NodeId(c.u32()?);
                let announcer = NodeId(c.u32()?);
                let announcer_colour = c.u8()?;
                let announcer_colours = c.u8()?;
                let nh = c.u8()? as usize;
                let nw = c.u8()? as usize;
                if nh > MAX_GOSSIP_IDS || nw > MAX_WANT || nheard > MAX_HEARD || 13 * nw + WANT_SET_BYTES * ns > WANT_BUDGET || 8 * nh + HAVE_SET_BYTES * nhs > HAVE_BUDGET {
                    return Err(DecodeError::BadLength);
                }
                let mut heard = Vec::with_capacity(nheard);
                for _ in 0..nheard {
                    let id = NodeId(c.u32()?);
                    let colour = c.u8()?;
                    let colours = c.u8()?;
                    heard.push((id, colour, colours));
                }
                let mut have = Vec::with_capacity(nh);
                for _ in 0..nh {
                    have.push(c.short()?);
                }
                let mut have_sets = Vec::with_capacity(nhs);
                for _ in 0..nhs {
                    let manifest = c.short()?;
                    let first = c.u16()?;
                    let bits = c.u64()?;
                    have_sets.push(PieceSet { manifest, first, bits });
                }
                let mut want = Vec::with_capacity(nw);
                for _ in 0..nw {
                    let id = c.short()?;
                    let grant = NodeId(c.u32()?);
                    let phase = c.u8()?;
                    want.push((id, grant, phase));
                }
                let mut sets = Vec::with_capacity(ns);
                for _ in 0..ns {
                    let manifest = c.short()?;
                    let first = c.u16()?;
                    let bits = c.u64()?;
                    let grant = NodeId(c.u32()?);
                    let phase = c.u8()?;
                    sets.push(WantSet { set: PieceSet { manifest, first, bits }, grant, phase });
                }
                Frame::Gossip(Gossip { node, announcer, announcer_colour, announcer_colours, heard, have, have_sets, want, sets })
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
                Frame::ManifestAnnounce(ManifestAnnounce { node, entries, whole: flags & 1 != 0 })
            }
            5 => {
                let node = NodeId(c.u32()?);
                let object = c.short()?;
                let block = c.u16()?;
                let answerer = NodeId(c.u32()?);
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
                Frame::Nack(Nack { node, object, block, answerer, phase: flags & 0x0f, missing })
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
                caps: CAP_MAINS,
                next_ms: 60000,
                round: 7,
                time: 1_700_000_000_000,
                time_quality: 2,
                colour: 3,
                colours: 5,
                upload_phases: 3,
                occupancy: [10, 20, 30, 40],
            }),
            Frame::Bulk(Bulk { object: ShortId([1; 8]), block: 2, esi: 3, len: 500_000, payload: vec![9u8; SYMBOL_SIZE] }),
            Frame::Gossip(Gossip { node: NodeId(1), announcer: NodeId(2), announcer_colour: 1, announcer_colours: 4, heard: vec![(NodeId(7), 0, 3), (NodeId(8), 1, 3), (NodeId(9), 2, 3)], have: vec![ShortId([3; 8]); 12], have_sets: vec![], want: vec![(ShortId([4; 8]), NodeId(5), 2); 8], sets: vec![] }),
            // Sets within both budgets: 1 HAVE id + 4 have sets (80 of 96 bytes), 1 WANT entry +
            // 3 want sets (82 of 104).
            Frame::Gossip(Gossip {
                node: NodeId(1),
                announcer: NodeId(2),
                announcer_colour: 1,
                announcer_colours: 4,
                heard: vec![(NodeId(7), 0, 3), (NodeId(8), 1, 3), (NodeId(9), 2, 3)],
                have: vec![ShortId([3; 8]); 1],
                have_sets: vec![PieceSet { manifest: ShortId([6; 8]), first: 0, bits: u64::MAX }; 4],
                want: vec![(ShortId([4; 8]), NodeId::NONE, ASK_LISTENED | 3); 1],
                sets: vec![WantSet { set: PieceSet { manifest: ShortId([5; 8]), first: 64, bits: 0x8000_0000_0000_0001 }, grant: NodeId(9), phase: 3 }; 3],
            }),
            Frame::ManifestAnnounce(ManifestAnnounce {
                node: NodeId(5),
                entries: vec![AnnounceEntry { channel: ChannelId([6; 8]), manifest: ShortId([7; 8]), seq: 9, len: 1234 }; 8],
                whole: true,
            }),
            Frame::Nack(Nack { node: NodeId(8), object: ShortId([2; 8]), block: 0, answerer: NodeId(3), phase: 5, missing: vec![(0, 3), (10, 1)] }),
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
        let b = Frame::Beacon(Beacon { carrier: CarrierKind::GfskBulk, announcer: NodeId(1), score: 0, caps: 0, next_ms: 0, round: 0, time: 0, time_quality: 0, colour: 0, colours: 1, upload_phases: 1, occupancy: [0; 4] });
        assert_eq!(b.encode().len(), 30);
        let k = Frame::Bulk(Bulk { object: ShortId([0; 8]), block: 0, esi: 0, len: 1, payload: vec![] });
        assert_eq!(k.encode().len(), 220);
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
        let f = Frame::Gossip(Gossip { node: NodeId(1), announcer: NodeId(2), announcer_colour: 0, announcer_colours: 1, heard: vec![], have: vec![], have_sets: vec![], want: vec![], sets: vec![] });
        let mut bytes = f.encode();
        bytes[3] ^= 1;
        assert_eq!(Frame::decode(&bytes), Err(DecodeError::BadCrc));
    }
}
