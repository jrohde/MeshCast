//! Announcer election and healing (PROTOCOL.md §5). One instance per bulk carrier.
//!
//! A follower follows the announcer it *receives best* (RSSI), because it must receive that
//! announcer's carousel. Scores decide who steps up when nobody is heard, who yields when two
//! announcers share a cell, and when a much better node challenges the incumbent. "Sharing a
//! cell" is judged by the node relative to its own neighbourhood (see `Node`), not by an absolute
//! signal level.

use alloc::collections::BTreeMap;

use crate::ids::NodeId;
use crate::params::{ElectionParams, SCORE_MAX};
use crate::rng::Rng;
use crate::Millis;

/// Grace added to the announcer's promised next-beacon time before counting a miss.
const GRACE_MS: Millis = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Follower,
    Candidate { until: Millis },
    Announcer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    BecameCandidate,
    BecameAnnouncer,
    /// Stopped being announcer or candidate, now following `NodeId`.
    BecameFollower(NodeId),
    /// Switched from one announcer to another while a follower.
    AnnouncerChanged(NodeId),
}

#[derive(Clone, Copy, Debug)]
struct Heard {
    last: Millis,
    rssi: i16,
    score: u16,
    next_ms: u16,
    colour: u8,
    colours: u8,
}

#[derive(Clone, Debug)]
pub struct Election {
    p: ElectionParams,
    state: State,
    pub announcer: NodeId,
    pub announcer_score: u16,
    expected_next: Millis,
    missed: u8,
    low_count: u8,
    heard: BTreeMap<NodeId, Heard>,
}

impl Election {
    pub fn new(p: ElectionParams, now: Millis) -> Self {
        Election {
            p,
            state: State::Follower,
            announcer: NodeId::NONE,
            announcer_score: 0,
            expected_next: now + p.t_beacon_ms,
            missed: 0,
            low_count: 0,
            heard: BTreeMap::new(),
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn is_announcer(&self) -> bool {
        matches!(self.state, State::Announcer)
    }

    fn wait_for(&self, score: u16, rng: &mut Rng) -> Millis {
        let s = score.min(SCORE_MAX) as u64;
        let base = self.p.t_base_ms * (SCORE_MAX as u64 - s) / SCORE_MAX as u64;
        base + rng.below(self.p.t_jitter_ms.max(1))
    }

    fn follow(&mut self, now: Millis, from: NodeId) {
        let h = self.heard.get(&from).copied();
        self.state = State::Follower;
        self.announcer = from;
        self.announcer_score = h.map(|h| h.score).unwrap_or(0);
        let next_ms = h.map(|h| h.next_ms as Millis).unwrap_or(self.p.t_beacon_ms).max(1000);
        self.expected_next = now + next_ms + GRACE_MS;
        self.missed = 0;
        self.low_count = 0;
    }

    fn prune(&mut self, now: Millis) {
        let ttl = self.p.t_beacon_ms * (self.p.n_miss as Millis + 1);
        self.heard.retain(|_, h| h.last + ttl >= now);
    }

    /// Best alternative announcer heard recently, by RSSI.
    fn best_heard(&self, now: Millis, except: NodeId) -> Option<NodeId> {
        let fresh = self.p.t_beacon_ms * 2 + GRACE_MS;
        self.heard.iter().filter(|(id, h)| **id != except && h.last + fresh >= now).max_by_key(|(_, h)| h.rssi).map(|(id, _)| *id)
    }

    /// `near`: the caller judges this announcer to be in our own cell (heard at least as well
    /// as our typical neighbour); only then does a near-tie yield to the lower id.
    pub fn on_beacon(&mut self, now: Millis, from: NodeId, score: u16, next_ms: u16, rssi: i16, colour: u8, colours: u8, near: bool, me: NodeId, my_score: u16) -> Option<Transition> {
        if from == me || from.is_none() {
            return None;
        }
        let h = self.heard.entry(from).or_insert(Heard { last: now, rssi, score, next_ms, colour, colours });
        h.last = now;
        h.rssi = ((h.rssi as i32 + rssi as i32) / 2) as i16;
        h.score = score;
        h.next_ms = next_ms;
        h.colour = colour;
        h.colours = colours.max(1);
        let h_rssi = h.rssi;
        let hy = self.p.hysteresis as i32;
        match self.state {
            State::Announcer => {
                let d = score as i32 - my_score as i32;
                let yields = d > hy || (d >= -hy && from.0 < me.0 && near);
                if yields {
                    self.follow(now, from);
                    Some(Transition::BecameFollower(from))
                } else {
                    None
                }
            }
            State::Candidate { .. } => {
                self.follow(now, from);
                Some(Transition::BecameFollower(from))
            }
            State::Follower => {
                if self.announcer.is_none() {
                    self.follow(now, from);
                    Some(Transition::AnnouncerChanged(from))
                } else if self.announcer == from {
                    self.announcer_score = score;
                    self.expected_next = now + (next_ms as Millis).max(1000) + GRACE_MS;
                    self.missed = 0;
                    if my_score as i32 > score as i32 + hy {
                        self.low_count = self.low_count.saturating_add(1);
                    } else {
                        self.low_count = 0;
                    }
                    None
                } else {
                    let cur = self.heard.get(&self.announcer).map(|h| h.rssi).unwrap_or(i16::MIN);
                    if h_rssi as i32 > cur as i32 + self.p.rssi_hysteresis_db as i32 {
                        self.follow(now, from);
                        Some(Transition::AnnouncerChanged(from))
                    } else {
                        None
                    }
                }
            }
        }
    }

    /// Someone's gossip names a different announcer than us while we are announcer. Only a
    /// clearly better score (from a beacon we heard) makes us yield; otherwise both persist.
    pub fn on_conflict(&mut self, now: Millis, other: NodeId, other_score: Option<u16>, me: NodeId, my_score: u16) -> Option<Transition> {
        if !self.is_announcer() || other == me || other.is_none() {
            return None;
        }
        let Some(s) = other_score else { return None };
        if s as i32 > my_score as i32 + self.p.hysteresis as i32 {
            self.heard.entry(other).or_insert(Heard { last: now, rssi: i16::MIN / 2, score: s, next_ms: self.p.t_beacon_ms as u16, colour: 0, colours: 1 });
            self.follow(now, other);
            Some(Transition::BecameFollower(other))
        } else {
            None
        }
    }

    pub fn tick(&mut self, now: Millis, my_score: u16, rng: &mut Rng) -> Option<Transition> {
        match self.state {
            State::Follower => {
                if self.low_count >= self.p.challenge_beacons {
                    // We are clearly better than the incumbent: step up; it will yield.
                    self.low_count = 0;
                    let until = now + rng.below(self.p.t_jitter_ms.max(1));
                    self.state = State::Candidate { until };
                    return Some(Transition::BecameCandidate);
                }
                if now >= self.expected_next {
                    self.missed = self.missed.saturating_add(1);
                    self.expected_next = now + self.p.t_beacon_ms;
                    if self.missed >= self.p.n_miss {
                        self.prune(now);
                        if let Some(alt) = self.best_heard(now, self.announcer) {
                            self.follow(now, alt);
                            return Some(Transition::AnnouncerChanged(alt));
                        }
                        let until = now + self.wait_for(my_score, rng);
                        self.state = State::Candidate { until };
                        return Some(Transition::BecameCandidate);
                    }
                }
                None
            }
            State::Candidate { until } => {
                if now >= until {
                    self.state = State::Announcer;
                    self.announcer = NodeId::NONE;
                    self.missed = 0;
                    self.low_count = 0;
                    Some(Transition::BecameAnnouncer)
                } else {
                    None
                }
            }
            State::Announcer => None,
        }
    }

    /// Next time `tick` needs to run.
    pub fn deadline(&self) -> Millis {
        match self.state {
            State::Follower => self.expected_next,
            State::Candidate { until } => until,
            State::Announcer => Millis::MAX,
        }
    }

    pub fn missed(&self) -> u8 {
        self.missed
    }

    /// Other announcers heard recently (within two beacon intervals), excluding `except`.
    pub fn announcers_heard(&self, now: Millis, except: NodeId) -> usize {
        self.heard_ids(now, except).count()
    }

    pub fn heard_ids(&self, now: Millis, except: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.heard_with_colour(now, except).map(|(id, _, _)| id)
    }

    pub fn heard_with_colour(&self, now: Millis, except: NodeId) -> impl Iterator<Item = (NodeId, u8, u8)> + '_ {
        let fresh = self.p.t_beacon_ms * 2 + GRACE_MS;
        self.heard.iter().filter(move |(id, h)| **id != except && h.last + fresh >= now).map(|(id, h)| (*id, h.colour, h.colours))
    }

    /// True if this announcer was not in our heard set before (a new neighbour announcer).
    pub fn is_new(&self, id: NodeId) -> bool {
        !self.heard.contains_key(&id)
    }

    /// Colour last announced by `id`, if we have heard its beacon.
    pub fn colour_of(&self, id: NodeId) -> Option<u8> {
        self.heard.get(&id).map(|h| h.colour)
    }

    /// Colour and colour count last announced by `id`.
    pub fn colouring_of(&self, id: NodeId) -> Option<(u8, u8)> {
        self.heard.get(&id).map(|h| (h.colour, h.colours))
    }

    /// An announcer's gossip also states its colouring; keep it current between beacons, and
    /// treat the gossip as a sighting if we never heard its beacon (a holder must be able to
    /// reach an announcer that asked it).
    pub fn note_colouring(&mut self, now: Millis, id: NodeId, colour: u8, colours: u8, rssi: i16) {
        let h = self.heard.entry(id).or_insert(Heard { last: now, rssi, score: 0, next_ms: self.p.t_beacon_ms as u16, colour, colours });
        h.last = now;
        h.colour = colour;
        h.colours = colours.max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_then_election_then_yield() {
        let p = ElectionParams::default();
        let mut rng = Rng::new(1);
        let mut e = Election::new(p, 0);
        let mut now = 0;
        let mut became_candidate = None;
        while became_candidate.is_none() && now < 10 * p.t_beacon_ms {
            now += 1000;
            if let Some(Transition::BecameCandidate) = e.tick(now, 100, &mut rng) {
                became_candidate = Some(now);
            }
        }
        let t = became_candidate.unwrap();
        assert!(t >= 3 * p.t_beacon_ms && t <= 4 * p.t_beacon_ms, "{t}");
        let mut ann = None;
        while ann.is_none() {
            now += 1000;
            if let Some(Transition::BecameAnnouncer) = e.tick(now, 100, &mut rng) {
                ann = Some(now);
            }
        }
        assert!(ann.unwrap() - t <= p.t_base_ms + p.t_jitter_ms);
        // A much better node appears: yield.
        let tr = e.on_beacon(now, NodeId(9), 400, 60000, -80, 0, 1, false, NodeId(5), 100);
        assert_eq!(tr, Some(Transition::BecameFollower(NodeId(9))));
        // Another announcer, weaker signal: keep following 9.
        let tr = e.on_beacon(now + 1, NodeId(20), 400, 60000, -100, 0, 1, false, NodeId(5), 100);
        assert_eq!(tr, None);
        // A much stronger signal: switch.
        let tr = e.on_beacon(now + 2, NodeId(21), 100, 60000, -60, 0, 1, false, NodeId(5), 100);
        assert_eq!(tr, Some(Transition::AnnouncerChanged(NodeId(21))));
    }

    #[test]
    fn near_equal_far_announcers_both_persist() {
        let p = ElectionParams::default();
        let mut e = Election::new(p, 0);
        e.state = State::Announcer;
        // Equal score, lower id, but judged to be in another cell: do not yield.
        assert_eq!(e.on_beacon(1000, NodeId(1), 300, 60000, -100, 0, 1, false, NodeId(5), 300), None);
        // Same, judged to be in our cell: yield.
        assert_eq!(e.on_beacon(2000, NodeId(1), 300, 60000, -70, 0, 1, true, NodeId(5), 300), Some(Transition::BecameFollower(NodeId(1))));
    }

    #[test]
    fn challenger_steps_up() {
        let p = ElectionParams::default();
        let mut rng = Rng::new(2);
        let mut e = Election::new(p, 0);
        e.on_beacon(1000, NodeId(7), 100, 60000, -80, 0, 1, false, NodeId(5), 400);
        for i in 0..p.challenge_beacons as u64 {
            assert_eq!(e.tick(2000 + i, 400, &mut rng), None);
            e.on_beacon(3000 + i, NodeId(7), 100, 60000, -80, 0, 1, false, NodeId(5), 400);
        }
        assert_eq!(e.tick(4000, 400, &mut rng), Some(Transition::BecameCandidate));
    }
}
