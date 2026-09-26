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

#[derive(Clone, Debug)]
struct Block {
    k: u16,
    have: Vec<u64>,
    count: u16,
}

impl Block {
    fn new(k: u16) -> Self {
        Block { k, have: vec![0u64; (k as usize + 63) / 64], count: 0 }
    }
    fn has(&self, esi: u16) -> bool {
        esi < self.k && (self.have[esi as usize / 64] >> (esi % 64)) & 1 == 1
    }
    fn set(&mut self, esi: u16) -> bool {
        if self.has(esi) {
            return false;
        }
        self.have[esi as usize / 64] |= 1u64 << (esi % 64);
        self.count += 1;
        true
    }
    fn complete(&self) -> bool {
        self.count == self.k
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
    pub fn known_blocks(&self) -> u16 {
        self.blocks.len() as u16
    }
}

#[derive(Clone, Debug)]
pub struct MemStore {
    entries: BTreeMap<ShortId, Entry>,
    keep_bytes_below: usize,
}

impl MemStore {
    pub fn new(keep_bytes_below: usize) -> Self {
        MemStore { entries: BTreeMap::new(), keep_bytes_below }
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
            Self::recheck(e);
        }
    }

    /// Register an object by short id with a length hint (from MANIFEST_ANNOUNCE).
    pub fn ensure_hint(&mut self, short: ShortId, len: u32, mime: Mime) {
        let keep = (len as usize) <= self.keep_bytes_below;
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
            Self::recheck(e);
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
        if k == 0 || k > K_MAX || esi >= k {
            return Put::Rejected;
        }
        let keep_below = self.keep_bytes_below;
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
        Self::recheck(e);
        if e.complete {
            Put::Complete
        } else {
            Put::New
        }
    }

    fn recheck(e: &mut Entry) {
        let Some(len) = e.len() else { return };
        let nb = blocks(len) as usize;
        if e.blocks.len() < nb {
            return;
        }
        let all = (0..nb).all(|b| e.blocks[b].as_ref().map(|x| x.complete() && x.k == block_k(len, b as u16)).unwrap_or(false));
        if !all {
            return;
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

    /// Copy symbol `esi` of `block` into `buf` (must be `SYMBOL_SIZE`). Returns false if absent.
    pub fn get_symbol(&self, short: &ShortId, block: u16, esi: u16, buf: &mut [u8]) -> bool {
        let Some(e) = self.entries.get(short) else { return false };
        let Some(Some(b)) = e.blocks.get(block as usize) else { return false };
        if !b.has(esi) {
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

    pub fn block_k(&self, short: &ShortId, block: u16) -> Option<u16> {
        let e = self.entries.get(short)?;
        if let Some(len) = e.len() {
            let k = block_k(len, block);
            return if k == 0 { None } else { Some(k) };
        }
        e.blocks.get(block as usize).and_then(|b| b.as_ref().map(|b| b.k))
    }

    pub fn missing(&self, short: &ShortId, block: u16) -> Vec<u16> {
        let Some(e) = self.entries.get(short) else { return Vec::new() };
        let Some(k) = self.block_k(short, block) else { return Vec::new() };
        match e.blocks.get(block as usize) {
            Some(Some(b)) => (0..k).filter(|&i| !b.has(i)).collect(),
            _ => (0..k).collect(),
        }
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
        assert_eq!(st.missing(&short, 0).len(), 3);
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
