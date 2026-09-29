//! Object store: symbol bitmaps per block, optional byte retention, hash verification.
//!
//! Real nodes keep bytes for everything (flash/SD). The simulator keeps bytes only for small
//! objects (manifests) and trusts the bitmap for large ones, so that thousands of simulated
//! nodes fit in memory. Both behaviours are the same [`MemStore`] with a different threshold.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use crate::frame::SYMBOL_SIZE;
use crate::ids::{ObjectId, ShortId};
use crate::object::{block_k, blocks, ObjectMeta, Mime, K_MAX};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Put {
    /// Symbol stored for the first time.
    New,
    /// Already had it.
    Duplicate,
    /// This symbol completed the object.
    Complete,
    /// Not stored: bad index, k mismatch, or object already complete.
    Rejected,
}

/// How far beyond the source symbols encoding symbol ids may run. A carousel sends fresh
/// symbols on every pass, so the ids of one block span several multiples of K.
pub const ESI_SPAN: u32 = 8;

/// How many distinct encoding symbols decode a block of K source symbols. RaptorQ (RFC 6330)
/// needs K + 2 for a failure probability below one in a million; the source symbols themselves
/// are the first K, so receiving all of those costs nothing extra.
pub fn needed(k: u16) -> u16 {
    if k > 4 {
        k + 2
    } else {
        k
    }
}

#[derive(Clone, Debug)]
struct Block {
    k: u16,
    /// Which encoding symbol ids we hold, over `k * ESI_SPAN` ids. Source symbols are the
    /// first `k`; the rest are repair symbols, each as good as any other.
    have: Vec<u64>,
    count: u16,
    source_count: u16,
}

impl Block {
    fn new(k: u16) -> Self {
        let span = k as usize * ESI_SPAN as usize;
        Block { k, have: vec![0u64; span.div_ceil(64)], count: 0, source_count: 0 }
    }
    fn span(&self) -> u32 {
        self.k as u32 * ESI_SPAN
    }
    fn has(&self, esi: u16) -> bool {
        (esi as u32) < self.span() && (self.have[esi as usize / 64] >> (esi % 64)) & 1 == 1
    }
    fn set(&mut self, esi: u16) -> bool {
        if (esi as u32) >= self.span() || self.has(esi) {
            return false;
        }
        self.have[esi as usize / 64] |= 1u64 << (esi % 64);
        self.count += 1;
        if esi < self.k {
            self.source_count += 1;
        }
        true
    }
    /// How many more encoding symbols this block needs. What comes back will be repair symbols,
    /// which count towards decoding and not towards completing the source set, so this is the
    /// decoding shortfall. Asking for the smaller "source symbols I still lack" would leave the
    /// block short again on arrival and cost another round of asking.
    fn short_by(&self) -> u16 {
        needed(self.k).saturating_sub(self.count)
    }
    /// A systematic code costs nothing when the source symbols all arrive: they are the object.
    /// Otherwise a codec turns any `needed(k)` distinct symbols into it, and a node without one
    /// has to keep waiting for the source symbols themselves.
    fn complete(&self, decoder: bool) -> bool {
        self.source_count == self.k || (decoder && self.count >= needed(self.k))
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub short: ShortId,
    /// Full metadata, once known from a manifest or from publishing.
    pub meta: Option<ObjectMeta>,
    /// Length hint (from MANIFEST_ANNOUNCE) when `meta` is not yet known.
    pub len_hint: Option<u32>,
    pub mime_hint: Mime,
    blocks: Vec<Option<Block>>,
    bytes: Option<Vec<u8>>,
    complete: bool,
    pub verified: bool,
}

impl Entry {
    pub fn len(&self) -> Option<u32> {
        self.meta.map(|m| m.len).or(self.len_hint)
    }
    pub fn mime(&self) -> Mime {
        self.meta.map(|m| m.mime).unwrap_or(self.mime_hint)
    }
    pub fn is_complete(&self) -> bool {
        self.complete
    }
    /// (symbols held, symbols total) if the total is known.
    pub fn progress(&self) -> (u32, Option<u32>) {
        let have: u32 = self.blocks.iter().flatten().map(|b| b.count as u32).sum();
        let total = self.len().map(crate::object::total_symbols);
        (have, total)
    }

    /// How many more encoding symbols this object needs in total.
    pub fn short_by(&self) -> u32 {
        let Some(len) = self.len() else { return u32::MAX };
        (0..blocks(len))
            .map(|b| match self.blocks.get(b as usize).and_then(|x| x.as_ref()) {
                Some(blk) => blk.short_by() as u32,
                None => needed(block_k(len, b)) as u32,
            })
            .sum()
    }
    pub fn known_blocks(&self) -> u16 {
        self.blocks.len() as u16
    }
}

/// Turns enough encoding symbols back into the object. Core does the bookkeeping of which
/// symbols exist and how many are enough; producing the bytes is arithmetic that belongs
/// elsewhere: RaptorQ (RFC 6330) in firmware, a lookup in the simulator. A store without one
/// can only finish an object whose source symbols it holds in full.
pub type Decoder = fn(&ShortId, u32) -> Option<Vec<u8>>;

#[derive(Clone)]
pub struct MemStore {
    entries: BTreeMap<ShortId, Entry>,
    keep_bytes_below: usize,
    decoder: Option<Decoder>,
}

impl core::fmt::Debug for MemStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MemStore").field("entries", &self.entries.len()).finish()
    }
}

impl MemStore {
    pub fn new(keep_bytes_below: usize) -> Self {
        MemStore { entries: BTreeMap::new(), keep_bytes_below, decoder: None }
    }

    pub fn with_decoder(keep_bytes_below: usize, decoder: Decoder) -> Self {
        MemStore { entries: BTreeMap::new(), keep_bytes_below, decoder: Some(decoder) }
    }

    pub fn entry(&self, id: &ShortId) -> Option<&Entry> {
        self.entries.get(id)
    }

    pub fn has_complete(&self, id: &ShortId) -> bool {
        self.entries.get(id).map(|e| e.complete).unwrap_or(false)
    }

    pub fn is_known(&self, id: &ShortId) -> bool {
        self.entries.contains_key(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &ShortId> {
        self.entries.keys()
    }

    pub fn complete_ids(&self) -> impl Iterator<Item = &ShortId> {
        self.entries.iter().filter(|(_, e)| e.complete).map(|(k, _)| k)
    }

    pub fn incomplete_ids(&self) -> impl Iterator<Item = &ShortId> {
        self.entries.iter().filter(|(_, e)| !e.complete).map(|(k, _)| k)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn remove(&mut self, id: &ShortId) -> bool {
        self.entries.remove(id).is_some()
    }

    /// Register an object whose length and mime are known (from a manifest).
    pub fn ensure(&mut self, meta: ObjectMeta) {
        let short = meta.id.short();
        let keep = (meta.len as usize) <= self.keep_bytes_below;
        let decoder = self.decoder;
        let e = self.entries.entry(short).or_insert_with(|| Entry {
            short,
            meta: None,
            len_hint: None,
            mime_hint: meta.mime,
            blocks: Vec::new(),
            bytes: None,
            complete: false,
            verified: false,
        });
        if e.meta.is_none() {
            e.meta = Some(meta);
            let nb = blocks(meta.len) as usize;
            e.blocks.resize(nb, None);
            if keep && e.bytes.is_none() {
                e.bytes = Some(vec![0u8; nb * K_MAX as usize * SYMBOL_SIZE]);
            }
            // A provisional entry may already be complete for all its blocks.
            Self::recheck(e, decoder);
        }
    }

    /// Register an object by short id with a length hint (from MANIFEST_ANNOUNCE).
    pub fn ensure_hint(&mut self, short: ShortId, len: u32, mime: Mime) {
        let keep = (len as usize) <= self.keep_bytes_below;
        let decoder = self.decoder;
        let e = self.entries.entry(short).or_insert_with(|| Entry {
            short,
            meta: None,
            len_hint: None,
            mime_hint: mime,
            blocks: Vec::new(),
            bytes: None,
            complete: false,
            verified: false,
        });
        if e.meta.is_none() && e.len_hint.is_none() {
            e.len_hint = Some(len);
            e.mime_hint = mime;
            let nb = blocks(len) as usize;
            if e.blocks.len() < nb {
                e.blocks.resize(nb, None);
            }
            if keep && e.bytes.is_none() {
                e.bytes = Some(vec![0u8; nb * K_MAX as usize * SYMBOL_SIZE]);
            }
            Self::recheck(e, decoder);
        }
    }

    /// Insert an object we own, complete. `bytes` is kept if below the threshold.
    pub fn insert_complete(&mut self, meta: ObjectMeta, bytes: Option<&[u8]>) {
        let short = meta.id.short();
        let nb = blocks(meta.len) as usize;
        let mut blk = Vec::with_capacity(nb);
        for b in 0..nb {
            let k = block_k(meta.len, b as u16);
            let mut block = Block::new(k);
            for esi in 0..k {
                block.set(esi);
            }
            blk.push(Some(block));
        }
        let keep = (meta.len as usize) <= self.keep_bytes_below;
        let stored = match (keep, bytes) {
            (true, Some(b)) => {
                let mut v = vec![0u8; nb * K_MAX as usize * SYMBOL_SIZE];
                let n = b.len().min(v.len());
                v[..n].copy_from_slice(&b[..n]);
                Some(v)
            }
            _ => None,
        };
        self.entries.insert(
            short,
            Entry { short, meta: Some(meta), len_hint: None, mime_hint: meta.mime, blocks: blk, bytes: stored, complete: true, verified: true },
        );
    }

    pub fn put_symbol(&mut self, short: ShortId, block: u16, esi: u16, k: u16, payload: &[u8]) -> Put {
        if k == 0 || k > K_MAX || (esi as u32) >= k as u32 * ESI_SPAN {
            return Put::Rejected;
        }
        let keep_below = self.keep_bytes_below;
        let decoder = self.decoder;
        let e = self.entries.entry(short).or_insert_with(|| Entry {
            short,
            meta: None,
            len_hint: None,
            mime_hint: Mime::Other,
            blocks: Vec::new(),
            bytes: None,
            complete: false,
            verified: false,
        });
        if e.complete {
            return Put::Rejected;
        }
        if let Some(len) = e.len() {
            if block >= blocks(len) || block_k(len, block) != k {
                return Put::Rejected;
            }
        } else if block as usize >= e.blocks.len() {
            if block >= 64 {
                return Put::Rejected; // provisional entries stay small
            }
            e.blocks.resize(block as usize + 1, None);
        }
        let slot = &mut e.blocks[block as usize];
        if slot.is_none() {
            *slot = Some(Block::new(k));
        }
        let blk = slot.as_mut().unwrap();
        if blk.k != k {
            return Put::Rejected;
        }
        if !blk.set(esi) {
            return Put::Duplicate;
        }
        if let Some(bytes) = e.bytes.as_mut() {
            let off = (block as usize * K_MAX as usize + esi as usize) * SYMBOL_SIZE;
            if off + SYMBOL_SIZE <= bytes.len() {
                let n = payload.len().min(SYMBOL_SIZE);
                bytes[off..off + n].copy_from_slice(&payload[..n]);
            }
        } else if e.len().map(|l| (l as usize) <= keep_below).unwrap_or(false) {
            // Should not happen: ensure() allocates. Ignore.
        }
        Self::recheck(e, decoder);
        if e.complete {
            Put::Complete
        } else {
            Put::New
        }
    }

    fn recheck(e: &mut Entry, decoder: Option<Decoder>) {
        let Some(len) = e.len() else { return };
        let nb = blocks(len) as usize;
        if e.blocks.len() < nb {
            return;
        }
        let can_decode = e.bytes.is_none() || decoder.is_some();
        let all = (0..nb).all(|b| e.blocks[b].as_ref().map(|x| x.complete(can_decode) && x.k == block_k(len, b as u16)).unwrap_or(false));
        if !all {
            return;
        }
        // Bytes we keep and did not receive in full: the decoder turns what we did receive back
        // into the object.
        if let (Some(d), Some(bytes)) = (decoder, e.bytes.as_mut()) {
            let holes = (0..nb).any(|b| e.blocks[b].as_ref().map(|x| x.source_count != x.k).unwrap_or(true));
            if holes {
                match d(&e.short, len) {
                    Some(decoded) => {
                        let n = decoded.len().min(bytes.len());
                        bytes[..n].copy_from_slice(&decoded[..n]);
                    }
                    None => return,
                }
            }
        }
        e.complete = true;
        e.verified = match (e.meta, e.bytes.as_ref()) {
            (Some(m), Some(b)) => ObjectId::of(&b[..m.len as usize]) == m.id,
            _ => true, // trusted: no bytes kept, or no full id to check against
        };
        if !e.verified {
            // Poisoned: drop everything and start over.
            e.complete = false;
            for b in e.blocks.iter_mut() {
                *b = None;
            }
        }
    }

    /// Copy encoding symbol `esi` of `block` into `buf` (must be `SYMBOL_SIZE`). A node that
    /// holds the object can produce any symbol of it, source or repair: having decoded it once,
    /// it can encode it again. A node that is still collecting can only pass on what it holds.
    pub fn get_symbol(&self, short: &ShortId, block: u16, esi: u16, buf: &mut [u8]) -> bool {
        let Some(e) = self.entries.get(short) else { return false };
        let Some(Some(b)) = e.blocks.get(block as usize) else { return false };
        if (esi as u32) >= b.span() {
            return false;
        }
        if !e.complete && !b.has(esi) {
            return false;
        }
        match e.bytes.as_ref() {
            Some(bytes) => {
                let off = (block as usize * K_MAX as usize + esi as usize) * SYMBOL_SIZE;
                if off + SYMBOL_SIZE <= bytes.len() {
                    buf[..SYMBOL_SIZE].copy_from_slice(&bytes[off..off + SYMBOL_SIZE]);
                } else {
                    buf[..SYMBOL_SIZE].fill(0);
                }
            }
            None => buf[..SYMBOL_SIZE].fill(0),
        }
        true
    }

    /// The lowest encoding symbol id of this block that we hold and that is at least `from`.
    /// A node may pass on what it holds even when it does not hold all of it.
    pub fn held_esi_from(&self, short: &ShortId, block: u16, from: u16) -> Option<u16> {
        let e = self.entries.get(short)?;
        let b = e.blocks.get(block as usize)?.as_ref()?;
        if e.complete {
            return if (from as u32) < b.span() { Some(from) } else { None };
        }
        (from as u32..b.span()).map(|x| x as u16).find(|&esi| b.has(esi))
    }

    /// How many encoding symbols of this object we hold, complete or not.
    pub fn held(&self, short: &ShortId) -> u32 {
        self.entries.get(short).map(|e| e.blocks.iter().flatten().map(|b| b.count as u32).sum()).unwrap_or(0)
    }

    pub fn block_k(&self, short: &ShortId, block: u16) -> Option<u16> {
        let e = self.entries.get(short)?;
        if let Some(len) = e.len() {
            let k = block_k(len, block);
            return if k == 0 { None } else { Some(k) };
        }
        e.blocks.get(block as usize).and_then(|b| b.as_ref().map(|b| b.k))
    }

    /// How many more encoding symbols a block needs, and the lowest id we do not hold (which a
    /// sender without a codec uses to pick a useful symbol).
    pub fn block_short_by(&self, short: &ShortId, block: u16) -> u16 {
        let Some(e) = self.entries.get(short) else { return 0 };
        let Some(k) = self.block_k(short, block) else { return 0 };
        match e.blocks.get(block as usize) {
            Some(Some(b)) => b.short_by(),
            _ => needed(k),
        }
    }

    /// The blocks of this object that still need symbols, with how many each needs.
    pub fn short_blocks(&self, short: &ShortId) -> Vec<(u16, u16)> {
        let Some(e) = self.entries.get(short) else { return Vec::new() };
        let Some(len) = e.len() else { return Vec::new() };
        (0..blocks(len))
            .filter_map(|b| {
                let n = self.block_short_by(short, b);
                if n > 0 {
                    Some((b, n))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Full bytes of a complete object whose bytes were retained.
    pub fn bytes(&self, short: &ShortId) -> Option<&[u8]> {
        let e = self.entries.get(short)?;
        if !e.complete {
            return None;
        }
        let len = e.len()? as usize;
        e.bytes.as_ref().map(|b| &b[..len.min(b.len())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::SYMBOL_SIZE;

    #[test]
    fn reassemble_and_verify() {
        let data: Vec<u8> = (0..1234u32).map(|i| (i * 7 % 251) as u8).collect();
        let meta = ObjectMeta { id: ObjectId::of(&data), len: data.len() as u32, mime: Mime::Text };
        let mut st = MemStore::new(1 << 20);
        st.ensure(meta);
        let short = meta.id.short();
        let k = block_k(meta.len, 0);
        assert_eq!(k, 7);
        for esi in (0..k).rev() {
            let off = esi as usize * SYMBOL_SIZE;
            let end = (off + SYMBOL_SIZE).min(data.len());
            let mut sym = vec![0u8; SYMBOL_SIZE];
            sym[..end - off].copy_from_slice(&data[off..end]);
            let r = st.put_symbol(short, 0, esi, k, &sym);
            if esi == 0 {
                assert_eq!(r, Put::Complete);
            } else {
                assert_eq!(r, Put::New);
            }
        }
        assert!(st.has_complete(&short));
        assert_eq!(st.bytes(&short).unwrap(), &data[..]);
        assert_eq!(st.put_symbol(short, 0, 1, k, &[0; SYMBOL_SIZE]), Put::Rejected);
    }

    #[test]
    fn poisoned_symbol_rejected() {
        let data = vec![5u8; 500];
        let meta = ObjectMeta { id: ObjectId::of(&data), len: 500, mime: Mime::Text };
        let mut st = MemStore::new(1 << 20);
        st.ensure(meta);
        let short = meta.id.short();
        let sym = [5u8; SYMBOL_SIZE];
        st.put_symbol(short, 0, 0, 3, &sym);
        st.put_symbol(short, 0, 1, 3, &sym);
        let bad = [6u8; SYMBOL_SIZE];
        assert_eq!(st.put_symbol(short, 0, 2, 3, &bad), Put::New); // completed but failed verification
        assert!(!st.has_complete(&short));
        assert_eq!(st.block_short_by(&short, 0), 3);
    }

    #[test]
    fn any_enough_symbols_decode() {
        // 600 bytes is three source symbols; with a codec any three distinct encoding symbols
        // do, whichever they are, so a receiver that missed two of the first pass completes on
        // the next without asking for anything by name.
        let meta = ObjectMeta { id: ObjectId::of(&[7u8; 600]), len: 600, mime: Mime::Audio };
        let mut st = MemStore::new(0); // keeps no bytes: a node with a codec
        st.ensure(meta);
        let short = meta.id.short();
        assert_eq!(st.put_symbol(short, 0, 0, 3, &[0; SYMBOL_SIZE]), Put::New);
        assert_eq!(st.block_short_by(&short, 0), 2);
        // Two repair symbols from a later pass finish it; symbols 1 and 2 were never heard.
        assert_eq!(st.put_symbol(short, 0, 4, 3, &[0; SYMBOL_SIZE]), Put::New);
        assert_eq!(st.put_symbol(short, 0, 7, 3, &[0; SYMBOL_SIZE]), Put::Complete);
        assert_eq!(st.block_short_by(&short, 0), 0);
        assert!(st.has_complete(&short));
        // A node that keeps the bytes has no codec and needs the source symbols themselves.
        let mut verbatim = MemStore::new(1 << 20);
        verbatim.ensure(meta);
        verbatim.put_symbol(short, 0, 0, 3, &[0; SYMBOL_SIZE]);
        verbatim.put_symbol(short, 0, 4, 3, &[0; SYMBOL_SIZE]);
        verbatim.put_symbol(short, 0, 7, 3, &[0; SYMBOL_SIZE]);
        assert!(!verbatim.has_complete(&short));
    }

    #[test]
    fn provisional_then_ensure() {
        let mut st = MemStore::new(0);
        let short = ShortId([9; 8]);
        assert_eq!(st.put_symbol(short, 0, 0, 2, &[0; SYMBOL_SIZE]), Put::New);
        assert_eq!(st.put_symbol(short, 0, 1, 2, &[0; SYMBOL_SIZE]), Put::New);
        assert!(!st.has_complete(&short));
        st.ensure_hint(short, 300, Mime::Audio);
        assert!(st.has_complete(&short));
    }
}
