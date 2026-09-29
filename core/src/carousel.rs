//! The announcer's carousel (PROTOCOL.md §4): a loop over the objects the cell wants, most-wanted
//! first, NACKed symbols at the front. Each object gets `max_passes` full passes, then leaves the
//! loop unless someone wants it again; manifests are repeated at most every `t_always`. When
//! nothing is wanted the carousel is silent.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;

use crate::ids::{NodeId, ShortId};
use crate::store::MemStore;
use crate::Millis;

const MAX_FRONT: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Beacon { round: u16 },
    Symbol { object: ShortId, block: u16, esi: u16, k: u16 },
}

#[derive(Clone, Copy, Debug)]
pub struct CarouselParams {
    pub max_passes: u16,
    pub t_always_ms: Millis,
    pub want_ttl_ms: Millis,
}

#[derive(Clone, Debug)]
pub struct Carousel {
    p: CarouselParams,
    set: Vec<ShortId>,
    idx: usize,
    block: u16,
    esi: u16,
    pub round: u16,
    at_round_start: bool,
    /// Repairs owed: object, block, and how many more symbols were asked for.
    front: VecDeque<(ShortId, u16, u16)>,
    /// Rolling id used to pick repair symbols nobody has heard yet.
    repair_seq: u16,
    wants: BTreeMap<ShortId, BTreeMap<NodeId, Millis>>,
    always: BTreeSet<ShortId>,
    /// Completed passes and the time the last pass ended.
    passes: BTreeMap<ShortId, (u16, Millis)>,
    last_always: Option<Millis>,
    pub rebuilds: u64,
}

impl Carousel {
    pub fn new(p: CarouselParams) -> Self {
        Carousel {
            p,
            set: Vec::new(),
            idx: 0,
            block: 0,
            esi: 0,
            round: 0,
            at_round_start: false,
            front: VecDeque::new(),
            repair_seq: 0,
            wants: BTreeMap::new(),
            always: BTreeSet::new(),
            passes: BTreeMap::new(),
            last_always: None,
            rebuilds: 0,
        }
    }

    pub fn on_want(&mut self, id: ShortId, from: NodeId, now: Millis) {
        self.wants.entry(id).or_default().insert(from, now);
    }

    pub fn on_have(&mut self, id: ShortId, from: NodeId) {
        if let Some(w) = self.wants.get_mut(&id) {
            w.remove(&from);
            if w.is_empty() {
                self.wants.remove(&id);
            }
        }
    }

    /// Someone needs more symbols of an object we hold: owe them that many, once. A second ask
    /// for the same block replaces the first rather than adding to it, because the asker is
    /// telling us what it still lacks, not what to add.
    pub fn on_nack(&mut self, id: ShortId, need: &[(u16, u16)], store: &MemStore) {
        if !store.has_complete(&id) {
            return;
        }
        for &(block, count) in need {
            if store.block_k(&id, block).is_none() || count == 0 {
                continue;
            }
            let room = self.front.len() < MAX_FRONT;
            match self.front.iter_mut().find(|(o, b, _)| *o == id && *b == block) {
                Some(entry) => entry.2 = entry.2.max(count),
                None if room => self.front.push_back((id, block, count)),
                None => {}
            }
        }
    }

    /// A repair symbol id nobody is likely to hold: beyond every carousel pass, and moving on.
    fn repair_esi(&self, k: u16) -> u16 {
        let base = crate::frame::CAROUSEL_PASSES.saturating_mul(k);
        let room = (crate::store::ESI_SPAN as u16).saturating_sub(crate::frame::CAROUSEL_PASSES).max(1) * k;
        base.saturating_add(self.repair_seq % room.max(1))
    }

    pub fn set_always(&mut self, id: ShortId) {
        self.always.insert(id);
        self.last_always = None; // send soon
    }

    pub fn unset_always(&mut self, id: &ShortId) {
        self.always.remove(id);
    }

    pub fn wanted_by(&self, id: &ShortId) -> usize {
        self.wants.get(id).map(|w| w.len()).unwrap_or(0)
    }

    pub fn wanted_ids(&self) -> impl Iterator<Item = &ShortId> {
        self.wants.keys()
    }

    /// Whether the object at the cursor is on its first pass through this cell.
    pub fn current_is_fresh(&self) -> bool {
        match self.set.get(self.idx) {
            Some(id) => self.passes.get(id).map(|p| p.0 == 0).unwrap_or(true),
            None => false,
        }
    }

    pub fn set_ids(&self) -> &[ShortId] {
        &self.set
    }

    fn expire(&mut self, now: Millis) {
        let from = now.saturating_sub(self.p.want_ttl_ms);
        self.wants.retain(|_, w| {
            w.retain(|_, &mut t| t >= from);
            !w.is_empty()
        });
    }

    fn eligible(&self, id: &ShortId, store: &MemStore) -> bool {
        if !store.has_complete(id) {
            return false;
        }
        let Some(w) = self.wants.get(id) else { return false };
        match self.passes.get(id) {
            None => true,
            Some(&(count, last_end)) => count < self.p.max_passes || w.values().any(|&t| t > last_end),
        }
    }

    fn always_due(&self, now: Millis) -> bool {
        !self.always.is_empty() && self.last_always.map(|t| now >= t + self.p.t_always_ms).unwrap_or(true)
    }

    /// Whether `peek` would return something now or at `next_due`.
    pub fn has_work(&self, store: &MemStore) -> bool {
        !self.front.is_empty() || self.idx < self.set.len() || !self.always.is_empty() || self.wants.keys().any(|id| self.eligible(id, store))
    }

    /// When the idle carousel next has something to do (the manifest repetition), if anything.
    pub fn next_due(&self) -> Option<Millis> {
        if self.always.is_empty() {
            None
        } else {
            Some(self.last_always.map(|t| t + self.p.t_always_ms).unwrap_or(0))
        }
    }

    fn rebuild(&mut self, store: &MemStore, now: Millis) {
        self.rebuilds += 1;
        self.expire(now);
        let mut scored: Vec<(usize, ShortId)> = Vec::new();
        let wanted: Vec<ShortId> = self.wants.keys().filter(|id| self.eligible(id, store)).copied().collect();
        let include_always = self.always_due(now) || !wanted.is_empty();
        if include_always {
            for id in &self.always {
                if store.has_complete(id) {
                    scored.push((usize::MAX, *id));
                }
            }
            self.last_always = Some(now);
        }
        for id in wanted {
            if !self.always.contains(&id) {
                scored.push((self.wants.get(&id).map(|w| w.len()).unwrap_or(0), id));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        self.set = scored.into_iter().map(|(_, id)| id).collect();
        self.idx = 0;
        self.block = 0;
        self.esi = 0;
    }

    /// The encoding symbol id offset for the pass this object is on: every pass sends symbols
    /// nobody has heard yet, so a node that missed a third of one pass completes on the next
    /// instead of waiting to hear exactly the pieces it lacks.
    fn pass_offset(&self, id: &ShortId, k: u16) -> u16 {
        let pass = self.passes.get(id).map(|p| p.0).unwrap_or(0) % crate::frame::CAROUSEL_PASSES;
        pass.saturating_mul(k)
    }

    /// Next item to transmit, or None if the carousel is idle.
    pub fn peek(&mut self, store: &MemStore, now: Millis) -> Option<Item> {
        while let Some(&(object, block, count)) = self.front.front() {
            if count > 0 {
                if let Some(k) = store.block_k(&object, block) {
                    if store.has_complete(&object) {
                        return Some(Item::Symbol { object, block, esi: self.repair_esi(k), k });
                    }
                }
            }
            self.front.pop_front();
        }
        loop {
            if self.idx >= self.set.len() {
                self.rebuild(store, now);
                if self.set.is_empty() {
                    return None;
                }
                self.round = self.round.wrapping_add(1);
                self.at_round_start = true;
            }
            if self.at_round_start {
                return Some(Item::Beacon { round: self.round });
            }
            let object = self.set[self.idx];
            match store.block_k(&object, self.block) {
                Some(k) if store.has_complete(&object) => {
                    let esi = self.pass_offset(&object, k).saturating_add(self.esi);
                    return Some(Item::Symbol { object, block: self.block, esi, k });
                }
                _ => self.finish_object(now),
            }
        }
    }

    fn finish_object(&mut self, now: Millis) {
        if let Some(id) = self.set.get(self.idx).copied() {
            let e = self.passes.entry(id).or_insert((0, now));
            e.0 = e.0.saturating_add(1);
            e.1 = now;
        }
        self.idx += 1;
        self.block = 0;
        self.esi = 0;
    }

    /// Advance past the item last returned by `peek` (it was transmitted).
    pub fn advance(&mut self, store: &MemStore, now: Millis) {
        if let Some(entry) = self.front.front_mut() {
            entry.2 = entry.2.saturating_sub(1);
            self.repair_seq = self.repair_seq.wrapping_add(1);
            if entry.2 == 0 {
                self.front.pop_front();
            }
            return;
        }
        if self.at_round_start {
            self.at_round_start = false;
            return;
        }
        if self.idx >= self.set.len() {
            return;
        }
        let object = self.set[self.idx];
        let k = store.block_k(&object, self.block).unwrap_or(0);
        self.esi += 1;
        if self.esi >= k {
            self.esi = 0;
            self.block += 1;
            if store.block_k(&object, self.block).is_none() {
                self.finish_object(now);
            }
        }
    }
}
