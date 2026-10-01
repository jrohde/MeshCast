//! The announcer's carousel (PROTOCOL.md §4): a loop over the objects the cell wants, the most
//! listeners served per byte first, NACKed symbols at the front. Each object gets `max_passes` full passes, then leaves the
//! loop unless someone wants it again; manifests are repeated at most every `t_always`. When
//! nothing is wanted the carousel is silent.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;

use crate::ids::{NodeId, ShortId};
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
    pub t_always_ms: Millis,
    pub want_ttl_ms: Millis,
    /// Unit of the repetition backoff: the second repetition of an object waits this long after
    /// the pass before, later ones up to eight times as long, however many ask (docs/ABUSE.md).
    pub t_repass_ms: Millis,
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
    always: BTreeSet<ShortId>,
    /// Completed passes, the time the last pass ended, and how often in a row it was passed
    /// again without resting (the backoff level).
    passes: BTreeMap<ShortId, (u16, Millis, u8)>,
    /// When an object was first asked for again after its last pass.
    asked_since: BTreeMap<ShortId, Millis>,
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
            front_set: BTreeSet::new(),
            wants: BTreeMap::new(),
            always: BTreeSet::new(),
            passes: BTreeMap::new(),
            asked_since: BTreeMap::new(),
            last_always: None,
            rebuilds: 0,
        }
    }

    pub fn on_want(&mut self, id: ShortId, from: NodeId, now: Millis) {
        self.wants.entry(id).or_default().insert(from, now);
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

    fn always_due(&self, now: Millis) -> bool {
        !self.always.is_empty() && self.last_always.map(|t| now >= t + self.p.t_always_ms).unwrap_or(true)
    }

    /// Whether `peek` would return something now or at `next_due`.
    pub fn has_work(&self, store: &MemStore, now: Millis) -> bool {
        !self.front.is_empty() || self.idx < self.set.len() || !self.always.is_empty() || self.wants.keys().any(|id| self.eligible(id, store, now))
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
        let include_always = self.always_due(now) || !wanted.is_empty();
        if include_always {
            for id in &self.always {
                if store.has_complete(id) {
                    scored.push((u64::MAX, 1, *id));
                }
            }
            self.last_always = Some(now);
        }
        for id in wanted {
            if !self.always.contains(&id) {
                let listeners = self.wants.get(&id).map(|w| w.len()).unwrap_or(0) as u64;
                let bytes = store.entry(&id).and_then(|e| e.len()).unwrap_or(1).max(1) as u64;
                scored.push((listeners, bytes, id));
            }
        }
        // a before b when a.listeners / a.bytes > b.listeners / b.bytes, compared without division.
        scored.sort_by(|a, b| {
            let (l, r) = (a.0 as u128 * b.1 as u128, b.0 as u128 * a.1 as u128);
            if a.0 == u64::MAX || b.0 == u64::MAX {
                b.0.cmp(&a.0).then(a.2.cmp(&b.2))
            } else {
                r.cmp(&l).then(a.2.cmp(&b.2))
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
