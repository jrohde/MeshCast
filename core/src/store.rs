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
use crate::object::{block_k, blocks, ObjectMeta, ContentType, K_MAX};

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
    /// Length from a symbol or a MANIFEST_ANNOUNCE while `meta` is not yet known.
    pub len_hint: Option<u32>,
    pub kind_hint: ContentType,
    blocks: Vec<Option<Block>>,
    bytes: Option<Vec<u8>>,
    complete: bool,
    pub verified: bool,
}

impl Entry {
    fn empty(short: ShortId, kind: ContentType) -> Self {
        Entry { short, meta: None, len_hint: None, kind_hint: kind, blocks: Vec::new(), bytes: None, complete: false, verified: false }
    }
    /// Take `len` as the object's length: size the block table and, if we keep this object's
    /// bytes, the buffer. Whatever was held under another length is dropped. A store may keep
    /// only small objects' bytes, but never fails to keep what a node must read (`kind`).
    fn set_len(&mut self, len: u32, keep_below: usize, kind: ContentType) {
        let nb = blocks(len) as usize;
        if self.len().map(|l| l != len).unwrap_or(false) {
            self.blocks.clear();
            self.bytes = None;
            self.complete = false;
        }
        self.blocks.resize(nb, None);
        if ((len as usize) <= keep_below || kind.is_read_by_nodes()) && self.bytes.is_none() {
            self.bytes = Some(vec![0u8; nb * K_MAX as usize * SYMBOL_SIZE]);
        }
    }
    pub fn len(&self) -> Option<u32> {
        self.meta.map(|m| m.len).or(self.len_hint)
    }
    pub fn kind(&self) -> ContentType {
        self.meta.map(|m| m.kind).unwrap_or(self.kind_hint)
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

    /// Register an object whose length and content type are known (from a manifest).
    /// Register an object with full metadata (from a manifest). Returns true if that completed
    /// it: every symbol had already arrived before we knew what the object was.
    pub fn ensure(&mut self, meta: ObjectMeta) -> bool {
        let short = meta.id.short();
        let keep = self.keep_bytes_below;
        let e = self.entries.entry(short).or_insert_with(|| Entry::empty(short, meta.kind));
        let was = e.complete;
        if e.meta.is_none() {
            // The signed metadata is the authority: a different length from a symbol or an
            // announce loses what was held under it.
            e.set_len(meta.len, keep, meta.kind);
            e.meta = Some(meta);
            e.len_hint = None;
            if e.complete {
                // Completed from symbols alone: now the bytes can be checked against the id.
                Self::verify(e);
            } else {
                Self::recheck(e);
            }
        }
        !was && e.complete
    }

    /// Register an object by short id with a length hint (from MANIFEST_ANNOUNCE).
    /// Returns true if the hint completed the object (see `ensure`).
    pub fn ensure_hint(&mut self, short: ShortId, len: u32, kind: ContentType) -> bool {
        let keep = self.keep_bytes_below;
        let e = self.entries.entry(short).or_insert_with(|| Entry::empty(short, kind));
        let was = e.complete;
        if e.meta.is_none() && e.len_hint.is_none() {
            e.set_len(len, keep, kind);
            e.len_hint = Some(len);
            e.kind_hint = kind;
            Self::recheck(e);
        }
        !was && e.complete
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
        let keep = (meta.len as usize) <= self.keep_bytes_below || meta.kind.is_read_by_nodes();
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
            Entry { short, meta: Some(meta), len_hint: None, kind_hint: meta.kind, blocks: blk, bytes: stored, complete: true, verified: true },
        );
    }

    /// Store symbol `esi` of `block` of an object of `len` bytes (the length every symbol
    /// carries). An object not seen before is registered by its first symbol.
    pub fn put_symbol(&mut self, short: ShortId, block: u16, esi: u16, len: u32, payload: &[u8]) -> Put {
        let k = block_k(len, block);
        if k == 0 || esi >= k {
            return Put::Rejected;
        }
        let keep = self.keep_bytes_below;
        let e = self.entries.entry(short).or_insert_with(|| Entry::empty(short, ContentType::Other));
        if e.complete {
            return Put::Rejected;
        }
        match e.len() {
            Some(l) if l != len => return Put::Rejected,
            Some(_) => {}
            None => {
                let kind = e.kind();
                e.set_len(len, keep, kind);
                e.len_hint = Some(len);
            }
        }
        let slot = &mut e.blocks[block as usize];
        if slot.is_none() {
            *slot = Some(Block::new(k));
        }
        if !slot.as_mut().unwrap().set(esi) {
            return Put::Duplicate;
        }
        if let Some(bytes) = e.bytes.as_mut() {
            let off = (block as usize * K_MAX as usize + esi as usize) * SYMBOL_SIZE;
            if off + SYMBOL_SIZE <= bytes.len() {
                let n = payload.len().min(SYMBOL_SIZE);
                bytes[off..off + n].copy_from_slice(&payload[..n]);
            }
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
        Self::verify(e);
    }

    /// Check a complete object's bytes against its id, if we keep them and know the full id.
    fn verify(e: &mut Entry) {
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
        let meta = ObjectMeta { id: ObjectId::of(&data), len: data.len() as u32, kind: ContentType::Text };
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
            let r = st.put_symbol(short, 0, esi, meta.len, &sym);
            if esi == 0 {
                assert_eq!(r, Put::Complete);
            } else {
                assert_eq!(r, Put::New);
            }
        }
        assert!(st.has_complete(&short));
        assert_eq!(st.bytes(&short).unwrap(), &data[..]);
        assert_eq!(st.put_symbol(short, 0, 1, meta.len, &[0; SYMBOL_SIZE]), Put::Rejected);
    }

    #[test]
    fn poisoned_symbol_rejected() {
        let data = vec![5u8; 500];
        let meta = ObjectMeta { id: ObjectId::of(&data), len: 500, kind: ContentType::Text };
        let mut st = MemStore::new(1 << 20);
        st.ensure(meta);
        let short = meta.id.short();
        let sym = [5u8; SYMBOL_SIZE];
        st.put_symbol(short, 0, 0, 500, &sym);
        st.put_symbol(short, 0, 1, 500, &sym);
        let bad = [6u8; SYMBOL_SIZE];
        assert_eq!(st.put_symbol(short, 0, 2, 500, &bad), Put::New); // completed but failed verification
        assert!(!st.has_complete(&short));
        assert_eq!(st.missing(&short, 0).len(), 3);
    }

    fn symbols_of(data: &[u8]) -> Vec<Vec<u8>> {
        data.chunks(SYMBOL_SIZE).map(|c| { let mut s = vec![0u8; SYMBOL_SIZE]; s[..c.len()].copy_from_slice(c); s }).collect()
    }

    #[test]
    fn symbols_alone_complete_an_object_and_registration_verifies_it() {
        let data: Vec<u8> = (0..900u32).map(|i| (i * 13 % 251) as u8).collect();
        let meta = ObjectMeta { id: ObjectId::of(&data), len: data.len() as u32, kind: ContentType::Manifest };
        let short = meta.id.short();
        let syms = symbols_of(&data);
        let mut st = MemStore::new(4096);
        for (esi, sym) in syms.iter().enumerate() {
            let r = st.put_symbol(short, 0, esi as u16, meta.len, sym);
            assert_eq!(r, if esi + 1 == syms.len() { Put::Complete } else { Put::New });
        }
        assert_eq!(st.bytes(&short).unwrap(), &data[..]);
        assert!(!st.ensure(meta), "completion is reported once, by the symbol that completed it");
        assert!(st.has_complete(&short));
        assert!(st.entry(&short).unwrap().verified);
    }

    #[test]
    fn a_manifest_is_kept_whatever_its_size() {
        // A store that keeps only small objects' bytes must still keep a large manifest: a node
        // reads it to know what to want. A channel listing 63 objects outgrew a 4 kB threshold,
        // and nobody ever wanted its objects (FEASIBILITY.md §13).
        let data: Vec<u8> = (0..9000u32).map(|i| (i * 7 % 251) as u8).collect();
        let meta = ObjectMeta { id: ObjectId::of(&data), len: data.len() as u32, kind: ContentType::Manifest };
        let short = meta.id.short();
        let mut st = MemStore::new(4096);
        st.ensure(meta);
        for (esi, sym) in symbols_of(&data).iter().enumerate() {
            st.put_symbol(short, 0, esi as u16, meta.len, sym);
        }
        assert_eq!(st.bytes(&short).unwrap(), &data[..]);
        // Music of the same size is only counted.
        let music = ObjectMeta { id: ObjectId::of(&data[1..]), len: data.len() as u32 - 1, kind: ContentType::Music };
        st.ensure(music);
        for esi in 0..block_k(music.len, 0) {
            st.put_symbol(music.id.short(), 0, esi, music.len, &[0u8; SYMBOL_SIZE]);
        }
        assert!(st.has_complete(&music.id.short()));
        assert!(st.bytes(&music.id.short()).is_none());
    }

    #[test]
    fn registration_rejects_wrong_bytes_collected_without_metadata() {
        let data: Vec<u8> = (0..700u32).map(|i| (i * 31 % 251) as u8).collect();
        let meta = ObjectMeta { id: ObjectId::of(&data), len: data.len() as u32, kind: ContentType::Manifest };
        let short = meta.id.short();
        let mut st = MemStore::new(4096);
        for esi in 0..block_k(meta.len, 0) {
            st.put_symbol(short, 0, esi, meta.len, &[1u8; SYMBOL_SIZE]);
        }
        assert!(st.has_complete(&short), "without the id nothing can be checked yet");
        st.ensure(meta);
        assert!(!st.has_complete(&short), "the signed id exposes the bytes");
        assert_eq!(st.missing(&short, 0).len(), block_k(meta.len, 0) as usize);
    }

    #[test]
    fn a_symbol_that_disagrees_on_length_is_rejected() {
        let mut st = MemStore::new(0);
        let short = ShortId([9; 8]);
        assert_eq!(st.put_symbol(short, 0, 0, 300, &[0; SYMBOL_SIZE]), Put::New);
        assert_eq!(st.put_symbol(short, 0, 1, 900, &[0; SYMBOL_SIZE]), Put::Rejected);
        assert_eq!(st.put_symbol(short, 0, 1, 300, &[0; SYMBOL_SIZE]), Put::Complete);
    }

    #[test]
    fn metadata_with_another_length_resets_the_entry() {
        let mut st = MemStore::new(0);
        let short = ObjectId::of(b"x").short();
        st.put_symbol(short, 0, 0, 300, &[0; SYMBOL_SIZE]);
        let meta = ObjectMeta { id: ObjectId::of(b"x"), len: 1000, kind: ContentType::Music };
        st.ensure(meta);
        assert_eq!(st.missing(&short, 0).len(), 5);
    }
}
