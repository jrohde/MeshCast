//! The announcer's carousel (PROTOCOL.md §4): a loop over the objects the cell wants, the most
//! listeners served per byte first, NACKed symbols at the front. Each object gets `max_passes` full passes, then leaves the
//! loop unless someone wants it again. A manifest is passed once unasked when it is new, and
//! otherwise only when wanted, before anything else. When nothing is wanted the carousel is
//! silent.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;

use crate::ids::{NodeId, ShortId};
use crate::object::ContentType;
use crate::store::MemStore;
use crate::Millis;

const MAX_FRONT: usize = 4096;

/// Repetitions back off to at most `t_repass` × 2^MAX_DOUBLINGS (80 minutes at the draft 10):
/// longer bounded an attacker more tightly but left a follower who joined during an attack
/// waiting for hours (FEASIBILITY.md §11).
const MAX_DOUBLINGS: u8 = 3;

/// The longest an honest announcer waits before it passes an object again that it was asked for:
/// the repetition ceiling.
pub fn longest_spacing(t_repass_ms: Millis) -> Millis {
    t_repass_ms << MAX_DOUBLINGS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Beacon { round: u16 },
    Symbol { object: ShortId, block: u16, esi: u16, k: u16 },
}

#[derive(Clone, Copy, Debug)]
pub struct CarouselParams {
    pub max_passes: u16,
    pub want_ttl_ms: Millis,
    /// Unit of the repetition backoff: the second repetition of an object waits this long after
    /// the pass before, later ones up to eight times as long, however many ask (docs/ABUSE.md).
    pub t_repass_ms: Millis,
    /// Askers kept per object (docs/ABUSE.md, "Someone else's firmware", item 5).
    pub max_askers: usize,
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
    front: VecDeque<(ShortId, u16, u16)>,
    front_set: BTreeSet<(ShortId, u16, u16)>,
    wants: BTreeMap<ShortId, BTreeMap<NodeId, Millis>>,
    /// Manifests this carousel serves: whenever one is passed, it goes before anything else.
    manifests: BTreeSet<ShortId>,
    /// Manifests not passed since they became new: each gets one pass without being asked.
    fresh: BTreeSet<ShortId>,
    /// An object's place in its collection: among objects serving as many listeners per byte,
    /// the earlier one goes first, because it plays first.
    rank: BTreeMap<ShortId, u16>,
    /// Completed passes, the time the last pass ended, and how often in a row it was passed
    /// again without resting (the backoff level).
    passes: BTreeMap<ShortId, (u16, Millis, u8)>,
    /// When an object was first asked for again after its last pass.
    asked_since: BTreeMap<ShortId, Millis>,
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
            front_set: BTreeSet::new(),
            wants: BTreeMap::new(),
            manifests: BTreeSet::new(),
            fresh: BTreeSet::new(),
            rank: BTreeMap::new(),
            passes: BTreeMap::new(),
            asked_since: BTreeMap::new(),
            rebuilds: 0,
        }
    }

    pub fn on_want(&mut self, id: ShortId, from: NodeId, now: Millis) {
        // At most `max_askers` per object, the one that asked longest ago giving way: the count
        // only orders the carousel, and every made-up name would have been kept for `want_ttl`.
        let askers = self.wants.entry(id).or_default();
        if !askers.contains_key(&from) && askers.len() >= self.p.max_askers.max(1) {
            if let Some(oldest) = askers.iter().min_by_key(|(_, t)| **t).map(|(n, _)| *n) {
                askers.remove(&oldest);
            }
        }
        askers.insert(from, now);
        if self.passes.contains_key(&id) {
            self.asked_since.entry(id).or_insert(now);
        }
    }

    pub fn on_have(&mut self, id: ShortId, from: NodeId) {
        if let Some(w) = self.wants.get_mut(&id) {
            w.remove(&from);
            if w.is_empty() {
                self.wants.remove(&id);
            }
        }
    }

    pub fn on_nack(&mut self, id: ShortId, block: u16, ranges: &[(u16, u16)], store: &MemStore) {
        if !store.has_complete(&id) {
            return;
        }
        let Some(k) = store.block_k(&id, block) else { return };
        for &(start, count) in ranges {
            for esi in start..start.saturating_add(count).min(k) {
                if self.front.len() >= MAX_FRONT {
                    return;
                }
                let key = (id, block, esi);
                if self.front_set.insert(key) {
                    self.front.push_back(key);
                }
            }
        }
    }

    /// A manifest this carousel serves: passed when asked for, before anything else, and once
    /// soon unasked if it is `new`.
    pub fn add_manifest(&mut self, id: ShortId, new: bool) {
        self.manifests.insert(id);
        if new {
            self.fresh.insert(id);
        }
    }

    /// `id` is piece `k` of a manifest this carousel serves.
    pub fn set_rank(&mut self, id: ShortId, k: u16) {
        self.rank.insert(id, k);
    }

    pub fn remove_manifest(&mut self, id: &ShortId) {
        self.manifests.remove(id);
        self.fresh.remove(id);
    }

    /// Diagnostic: objects asked for, askers over all of them, objects with a pass record, and
    /// places known.
    pub fn table_sizes(&self) -> (usize, usize, usize, usize) {
        (self.wants.len(), self.wants.values().map(|w| w.len()).sum(), self.passes.len(), self.rank.len())
    }

    pub fn wanted_by(&self, id: &ShortId) -> usize {
        self.wants.get(id).map(|w| w.len()).unwrap_or(0)
    }

    /// Askers of `id` that asked at or after `since`.
    pub fn wanted_by_since(&self, id: &ShortId, since: Millis) -> usize {
        self.wants.get(id).map(|w| w.values().filter(|t| **t >= since).count()).unwrap_or(0)
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

    fn eligible(&self, id: &ShortId, store: &MemStore, now: Millis) -> bool {
        if !store.has_complete(id) {
            return false;
        }
        let Some(w) = self.wants.get(id) else { return false };
        match self.passes.get(id) {
            None => true,
            Some(&(count, last_end, level)) => count < self.p.max_passes || (w.values().any(|&t| t > last_end) && now >= last_end + self.spacing(level)),
        }
    }

    /// How long an object waits after a pass before it is passed again: the first repetition at
    /// once, then ever more slowly, twice as long each time, until the object has rested.
    fn spacing(&self, level: u8) -> Millis {
        if level == 0 {
            0
        } else {
            self.p.t_repass_ms << (level - 1).min(MAX_DOUBLINGS)
        }
    }

    /// When the next object that is asked for again may be passed again, if any waits.
    pub fn next_repass(&self, store: &MemStore) -> Option<Millis> {
        self.wants
            .iter()
            .filter(|(id, _)| store.has_complete(id))
            .filter_map(|(id, w)| {
                let &(count, last_end, level) = self.passes.get(id)?;
                (count >= self.p.max_passes && w.values().any(|&t| t > last_end)).then_some(last_end + self.spacing(level))
            })
            .min()
    }

    /// Whether `peek` would return something now.
    pub fn has_work(&self, store: &MemStore, now: Millis) -> bool {
        !self.front.is_empty() || self.idx < self.set.len() || !self.fresh.is_empty() || self.wants.keys().any(|id| self.eligible(id, store, now))
    }

    fn rebuild(&mut self, store: &MemStore, now: Millis) {
        self.rebuilds += 1;
        self.expire(now);
        // (listeners, bytes, id). Manifests go first; then Smith's rule: one transmitter serving
        // many waiting listeners minimises their total wait by sending the most listeners per
        // byte first. Among objects of one size that is simply most-wanted first; a small object
        // no longer waits behind a large one, and every wanted object is still sent every round.
        let mut scored: Vec<(u64, u64, ShortId)> = Vec::new();
        let wanted: Vec<ShortId> = self.wants.keys().filter(|id| self.eligible(id, store, now)).copied().collect();
        // A pass asked for again climbs one backoff level, unless nobody had asked for the
        // object for twice its current spacing after its last pass, which starts it over.
        for id in &wanted {
            let max = self.p.max_passes;
            let spacing = self.passes.get(id).map(|p| self.spacing(p.2).max(self.p.t_repass_ms));
            let asked = self.asked_since.remove(id);
            if let (Some(p), Some(spacing)) = (self.passes.get_mut(id), spacing) {
                if p.0 >= max {
                    let rested = asked.map(|t| t >= p.1 + 2 * spacing).unwrap_or(true);
                    p.2 = if rested { 0 } else { p.2.saturating_add(1) };
                }
            }
        }
        // A new manifest once, unasked; after that a manifest is passed when it is wanted, like
        // any object. Repeating every manifest on a timer and in every round cost up to 29 % of all
        // frames, more where channels list many objects, and delivered nothing sooner: a node that
        // lacks a manifest learns its id from MANIFEST_ANNOUNCE and asks (FEASIBILITY.md §13).
        // Root manifests before collection manifests: a node reads a collection manifest only
        // once it holds the root that names it, so the other way round it would miss it.
        let level = |id: &ShortId| if store.entry(id).map(|e| e.kind() == ContentType::Collection).unwrap_or(false) { 2 } else { 1 };
        let fresh = core::mem::take(&mut self.fresh);
        for id in &fresh {
            if store.has_complete(id) {
                scored.push((u64::MAX, level(id), *id));
            }
        }
        for id in wanted {
            if !self.manifests.contains(&id) {
                let listeners = self.wants.get(&id).map(|w| w.len()).unwrap_or(0) as u64;
                let bytes = store.entry(&id).and_then(|e| e.len()).unwrap_or(1).max(1) as u64;
                scored.push((listeners, bytes, id));
            } else if !fresh.contains(&id) {
                scored.push((u64::MAX, level(&id), id));
            }
        }
        // a before b when a.listeners / a.bytes > b.listeners / b.bytes, compared without division.
        let rank = |id: &ShortId| self.rank.get(id).copied().unwrap_or(u16::MAX);
        scored.sort_by(|a, b| {
            let (l, r) = (a.0 as u128 * b.1 as u128, b.0 as u128 * a.1 as u128);
            if a.0 == u64::MAX || b.0 == u64::MAX {
                b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2))
            } else {
                r.cmp(&l).then(rank(&a.2).cmp(&rank(&b.2))).then(a.2.cmp(&b.2))
            }
        });
        self.set = scored.into_iter().map(|(_, _, id)| id).collect();
        self.idx = 0;
        self.block = 0;
        self.esi = 0;
    }

    /// Next item to transmit, or None if the carousel is idle.
    pub fn peek(&mut self, store: &MemStore, now: Millis) -> Option<Item> {
        while let Some(&(object, block, esi)) = self.front.front() {
            if let Some(k) = store.block_k(&object, block) {
                if store.has_complete(&object) {
                    return Some(Item::Symbol { object, block, esi, k });
                }
            }
            self.front.pop_front();
            self.front_set.remove(&(object, block, esi));
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
                    return Some(Item::Symbol { object, block: self.block, esi: self.esi, k });
                }
                _ => self.finish_object(now),
            }
        }
    }

    fn finish_object(&mut self, now: Millis) {
        if let Some(id) = self.set.get(self.idx).copied() {
            let e = self.passes.entry(id).or_insert((0, now, 0));
            e.0 = e.0.saturating_add(1);
            e.1 = now;
        }
        self.idx += 1;
        self.block = 0;
        self.esi = 0;
    }

    /// Advance past the item last returned by `peek` (it was transmitted).
    pub fn advance(&mut self, store: &MemStore, now: Millis) {
        if let Some(key) = self.front.pop_front() {
            self.front_set.remove(&key);
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
