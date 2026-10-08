//! Announcer election and healing (PROTOCOL.md §5). One instance per bulk carrier.
//!
//! A follower follows the announcer it *receives best* (RSSI), because it must receive that
//! announcer's carousel. Scores decide who steps up when nobody is heard, who yields when two
//! announcers share a cell, and when a much better node challenges the incumbent. "Sharing a
//! cell" is judged by the node relative to its own neighbourhood (see `Node`), not by an absolute
//! signal level.

use alloc::collections::BTreeMap;

use crate::frame::Beacon;
use crate::ids::NodeId;
use crate::params::{ElectionParams, SCORE_MAX};
use crate::rng::Rng;
use crate::Millis;

/// Grace added to the announcer's promised next-beacon time before counting a miss.
const GRACE_MS: Millis = 5_000;

/// When, within a span of time, a candidate steps up: in the order an announcer yields in
/// (§5.2), capability first, then score, then where it stands. The span has a band per capability
/// (mains and uplink, mains, uplink, neither); within its band a candidate waits less the higher
/// its score, and over the last third of the band less the louder it heard the announcer the cell
/// lost (`closeness`, dBm), with a little chance; with no announcer lost, by chance alone. A more
/// capable node can never step up after a less capable one and then displace it, orphaning the
/// followers it had just gathered. Followers are silent, so in a quiet cell a node knows little of
/// its neighbours; how it heard the lost announcer says how near it stood to it, and the nearest
/// is heard by the most of that announcer's followers.
pub fn step_up_order(caps: u8, score: u16, closeness: Option<i16>, span: Millis, rng: &mut Rng) -> Millis {
    let band = span / 4;
    let third = (band / 3).max(1);
    let s = score.min(SCORE_MAX) as u64;
    let rest = match closeness {
        Some(rssi) => {
            let below = (-20 - rssi as i64).clamp(0, 120) as u64;
            third * 7 / 8 * below / 120 + rng.below((third / 8).max(1))
        }
        None => rng.below(third),
    };
    (3 - caps.min(3) as u64) * band + (band * 2 / 3) * (SCORE_MAX as u64 - s) / SCORE_MAX as u64 + rest
}

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
    caps: u8,
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
    /// Diagnostic: how often this node challenged its announcer.
    pub challenges: u32,
    /// On an excursion: following an announcer for what it has, not for how well we hear it.
    pinned: bool,
    /// Announcers that listed what they did not serve us, and until when we ignore them.
    shunned: BTreeMap<NodeId, Millis>,
    /// How we heard the announcer whose loss made us a candidate: our place in the step-up order.
    lost_rssi: Option<i16>,
    /// When we last became announcer.
    announcing_since: Millis,
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
            challenges: 0,
            pinned: false,
            shunned: BTreeMap::new(),
            lost_rssi: None,
            announcing_since: 0,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn is_announcer(&self) -> bool {
        matches!(self.state, State::Announcer)
    }

    fn wait_for(&self, score: u16, caps: u8, rng: &mut Rng) -> Millis {
        step_up_order(caps, score, self.closeness(), self.p.t_base_ms + self.p.t_jitter_ms, rng)
    }

    /// How we heard the announcer the cell lost, if the step-up order goes by it.
    pub fn closeness(&self) -> Option<i16> {
        if self.p.order_by_lost { self.lost_rssi } else { None }
    }

    /// Whether we became announcer within the last `N_miss` beacon intervals: in the same election
    /// as any announcer that also did, before followers have settled on either.
    pub fn is_fresh(&self, now: Millis) -> bool {
        matches!(self.state, State::Announcer) && now < self.announcing_since + self.p.t_beacon_ms * self.p.n_miss as Millis
    }

    fn follow(&mut self, now: Millis, from: NodeId) {
        self.pinned = false;
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
        self.heard.iter().filter(|(id, h)| **id != except && h.last + fresh >= now && !self.is_shunned(**id, now)).max_by_key(|(_, h)| h.rssi).map(|(id, _)| *id)
    }

    /// Whether `id` listed something it did not serve us, recently enough to ignore it.
    pub fn is_shunned(&self, id: NodeId, now: Millis) -> bool {
        self.shunned.get(&id).map(|t| now < *t).unwrap_or(false)
    }

    /// Ignore announcer `id` until `until`: it listed what it did not serve. If it is ours, follow
    /// the best other announcer we hear, or, hearing none, become a candidate: an area whose only
    /// announcer serves nothing has none.
    pub fn shun(&mut self, now: Millis, id: NodeId, until: Millis, my_score: u16, my_caps: u8, rng: &mut Rng) -> Option<Transition> {
        self.shunned.retain(|_, t| now < *t);
        self.shunned.insert(id, until);
        if !matches!(self.state, State::Follower) || self.announcer != id {
            return None;
        }
        if let Some(alt) = self.best_heard(now, id) {
            self.follow(now, alt);
            return Some(Transition::AnnouncerChanged(alt));
        }
        self.lost_rssi = self.heard.get(&id).map(|h| h.rssi);
        let until = now + self.wait_for(my_score, my_caps, rng);
        self.state = State::Candidate { until };
        self.announcer = NodeId::NONE;
        Some(Transition::BecameCandidate)
    }

    /// A beacon heard at `rssi`. `near`: the caller judges this announcer to be in our own cell
    /// (heard at least as well as our typical neighbour); a near-tie yields to the lower id only
    /// then, or when both stepped up in the same election (`fresh_tie`).
    ///
    /// `caps` is what a node is (mains, uplink); `score` adds what it happens to experience in its
    /// role: who it hears, how much budget it has left. Two announcers compare capability, then
    /// score: like with like. A follower compares only capability with its announcer, because a
    /// follower's score is not comparable: it hears what its announcer is too busy transmitting
    /// to hear, and spends nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn on_beacon(&mut self, now: Millis, b: &Beacon, rssi: i16, near: bool, me: NodeId, my_score: u16, my_caps: u8) -> Option<Transition> {
        let (from, score, caps, next_ms, colour, colours) = (b.announcer, b.score, b.caps, b.next_ms, b.colour, b.colours);
        if from == me || from.is_none() || self.is_shunned(from, now) {
            return None;
        }
        let h = self.heard.entry(from).or_insert(Heard { last: now, caps, rssi, score, next_ms, colour, colours });
        h.last = now;
        h.caps = caps;
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
                // More capability counts like a clearly higher score: yield, near or far.
                // Two announcers that stepped up in the same election settle a near-tie by id, near or
                // far: followers chose between them only moments ago, and most hear both. In a
                // square kilometre of 200 nodes two stepped up 3 ms apart, each judged the other to be
                // in another cell, and both announced for four hours (FEASIBILITY.md §36.5).
                let together = self.p.fresh_tie && b.fresh && self.is_fresh(now);
                let yields = match caps.cmp(&my_caps) {
                    core::cmp::Ordering::Greater => true,
                    core::cmp::Ordering::Less => false,
                    core::cmp::Ordering::Equal => d > hy || (d >= -hy && from.0 < me.0 && (near || together)),
                };
                if yields {
                    self.follow(now, from);
                    Some(Transition::BecameFollower(from))
                } else {
                    None
                }
            }
            // A candidate stands down for an announcer, unless it is more capable: then that
            // announcer yields to it (a challenger hears its incumbent all the time).
            State::Candidate { .. } if my_caps > caps => None,
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
                    // Challenge only an announcer that is less capable than us, and only where no
                    // announcer we hear is at least as capable: we would yield to that one, and
                    // follow the weaker one again, and challenge it again.
                    let equal_heard = self.heard.iter().any(|(id, h)| *id != from && h.caps >= my_caps && h.last + self.p.t_beacon_ms * 2 + GRACE_MS >= now);
                    if my_caps > caps && !equal_heard {
                        self.low_count = self.low_count.saturating_add(1);
                    } else {
                        self.low_count = 0;
                    }
                    None
                } else if !self.pinned {
                    let cur = self.heard.get(&self.announcer).map(|h| h.rssi).unwrap_or(i16::MIN);
                    if h_rssi as i32 > cur as i32 + self.p.rssi_hysteresis_db as i32 {
                        self.follow(now, from);
                        Some(Transition::AnnouncerChanged(from))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }

    /// Follow `to` for what it has rather than for how well we hear it: an excursion.
    pub fn visit(&mut self, now: Millis, to: NodeId) -> Option<Transition> {
        if !matches!(self.state, State::Follower) || to == self.announcer {
            return None;
        }
        self.follow(now, to);
        self.pinned = true;
        Some(Transition::AnnouncerChanged(to))
    }

    /// End an excursion: follow the announcer we hear best again.
    pub fn end_visit(&mut self, now: Millis) -> Option<Transition> {
        self.pinned = false;
        if !matches!(self.state, State::Follower) {
            return None;
        }
        match self.best_heard(now, NodeId::NONE) {
            Some(b) if b != self.announcer => {
                self.follow(now, b);
                Some(Transition::AnnouncerChanged(b))
            }
            _ => None,
        }
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    /// Advance the timers: count missed beacons, end candidacies, start challenges.
    pub fn tick(&mut self, now: Millis, my_score: u16, my_caps: u8, rng: &mut Rng) -> Option<Transition> {
        match self.state {
            State::Follower => {
                if self.low_count >= self.p.challenge_beacons {
                    // We are more capable than the incumbent: step up; it will yield. Nobody was
                    // lost, so the step-up order has no announcer to go by.
                    self.low_count = 0;
                    self.challenges += 1;
                    self.lost_rssi = None;
                    let until = now + rng.below(self.p.t_jitter_ms.max(1));
                    self.state = State::Candidate { until };
                    return Some(Transition::BecameCandidate);
                }
                if now >= self.expected_next {
                    self.missed = self.missed.saturating_add(1);
                    self.expected_next = now + self.p.t_beacon_ms;
                    if self.missed >= self.p.n_miss {
                        self.lost_rssi = self.heard.get(&self.announcer).map(|h| h.rssi);
                        self.prune(now);
                        if let Some(alt) = self.best_heard(now, self.announcer) {
                            self.follow(now, alt);
                            return Some(Transition::AnnouncerChanged(alt));
                        }
                        let until = now + self.wait_for(my_score, my_caps, rng);
                        self.state = State::Candidate { until };
                        return Some(Transition::BecameCandidate);
                    }
                }
                None
            }
            State::Candidate { until } => {
                if now >= until {
                    self.state = State::Announcer;
                    self.announcing_since = now;
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

    /// Move a running candidacy to `until` (the caller knows when every candidate listens).
    pub fn step_up_at(&mut self, until: Millis) {
        if let State::Candidate { .. } = self.state {
            self.state = State::Candidate { until };
        }
    }

    /// A candidate steps up `d` later than it would.
    pub fn delay_step_up(&mut self, d: Millis) {
        if let State::Candidate { until } = self.state {
            self.state = State::Candidate { until: until + d };
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
        let h = self.heard.entry(id).or_insert(Heard { last: now, caps: 0, rssi, score: 0, next_ms: self.p.t_beacon_ms as u16, colour, colours });
        h.last = now;
        h.colour = colour;
        h.colours = colours.max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{CarrierKind, CAP_MAINS};

    fn beacon(from: NodeId, score: u16, caps: u8) -> Beacon {
        Beacon { carrier: CarrierKind::GfskBulk, announcer: from, score, caps, next_ms: 60000, round: 0, time: 0, time_quality: 0, colour: 0, colours: 1, upload_phases: 1, occupancy: [0; 4], fresh: false }
    }

    #[test]
    fn silence_then_election_then_yield() {
        let p = ElectionParams::default();
        let mut rng = Rng::new(1);
        let mut e = Election::new(p, 0);
        let mut now = 0;
        let mut became_candidate = None;
        while became_candidate.is_none() && now < 10 * p.t_beacon_ms {
            now += 1000;
            if let Some(Transition::BecameCandidate) = e.tick(now, 100, 0, &mut rng) {
                became_candidate = Some(now);
            }
        }
        let t = became_candidate.unwrap();
        assert!(t >= 3 * p.t_beacon_ms && t <= 4 * p.t_beacon_ms, "{t}");
        let mut ann = None;
        while ann.is_none() {
            now += 1000;
            if let Some(Transition::BecameAnnouncer) = e.tick(now, 100, 0, &mut rng) {
                ann = Some(now);
            }
        }
        assert!(ann.unwrap() - t <= p.t_base_ms + p.t_jitter_ms);
        // A much better node appears: yield.
        let tr = e.on_beacon(now, &beacon(NodeId(9), 400, 0), -80, false, NodeId(5), 100, 0);
        assert_eq!(tr, Some(Transition::BecameFollower(NodeId(9))));
        // Another announcer, weaker signal: keep following 9.
        let tr = e.on_beacon(now + 1, &beacon(NodeId(20), 400, 0), -100, false, NodeId(5), 100, 0);
        assert_eq!(tr, None);
        // A much stronger signal: switch.
        let tr = e.on_beacon(now + 2, &beacon(NodeId(21), 100, 0), -60, false, NodeId(5), 100, 0);
        assert_eq!(tr, Some(Transition::AnnouncerChanged(NodeId(21))));
    }

    #[test]
    fn near_equal_far_announcers_both_persist() {
        let p = ElectionParams::default();
        let mut e = Election::new(p, 0);
        e.state = State::Announcer;
        // Equal score, lower id, but judged to be in another cell: do not yield.
        assert_eq!(e.on_beacon(1000, &beacon(NodeId(1), 300, 0), -100, false, NodeId(5), 300, 0), None);
        // Same, judged to be in our cell: yield.
        assert_eq!(e.on_beacon(2000, &beacon(NodeId(1), 300, 0), -70, true, NodeId(5), 300, 0), Some(Transition::BecameFollower(NodeId(1))));
    }

    #[test]
    fn challenger_steps_up() {
        // A mains-powered follower of a battery announcer.
        let p = ElectionParams::default();
        let mut rng = Rng::new(2);
        let mut e = Election::new(p, 0);
        e.on_beacon(1000, &beacon(NodeId(7), 100, 0), -80, false, NodeId(5), 100, CAP_MAINS);
        for i in 0..p.challenge_beacons as u64 {
            assert_eq!(e.tick(2000 + i, 100, 0, &mut rng), None);
            e.on_beacon(3000 + i, &beacon(NodeId(7), 100, 0), -80, false, NodeId(5), 100, CAP_MAINS);
        }
        assert_eq!(e.tick(4000, 100, 0, &mut rng), Some(Transition::BecameCandidate));
        // The incumbent keeps beaconing; the challenger stays a candidate and steps up.
        assert_eq!(e.on_beacon(5000, &beacon(NodeId(7), 100, 0), -80, false, NodeId(5), 100, CAP_MAINS), None);
        assert_eq!(e.tick(4000 + p.t_jitter_ms, 100, 0, &mut rng), Some(Transition::BecameAnnouncer));
    }

    #[test]
    fn no_challenge_where_an_equal_announcer_is_heard() {
        // A mains follower of a battery announcer that also hears a mains announcer would yield
        // to that one after stepping up, and come back, and challenge again.
        let p = ElectionParams::default();
        let mut rng = Rng::new(4);
        let mut e = Election::new(p, 0);
        for i in 0..2 * p.challenge_beacons as u64 {
            e.on_beacon(1000 + 10 * i, &beacon(NodeId(7), 100, 0), -80, false, NodeId(5), 100, CAP_MAINS);
            e.on_beacon(1001 + 10 * i, &beacon(NodeId(8), 100, CAP_MAINS), -95, false, NodeId(5), 100, CAP_MAINS);
            assert_eq!(e.tick(1002 + 10 * i, 100, 0, &mut rng), None);
        }
    }

    #[test]
    fn a_better_score_alone_does_not_challenge() {
        // Same hardware, but the follower hears more and has spent nothing: that is its role
        // talking, not the node.
        let p = ElectionParams::default();
        let mut rng = Rng::new(3);
        let mut e = Election::new(p, 0);
        for i in 0..2 * p.challenge_beacons as u64 {
            e.on_beacon(1000 + i, &beacon(NodeId(7), 100, 0), -80, false, NodeId(5), 400, 0);
            assert_eq!(e.tick(2000 + i, 400, 0, &mut rng), None);
        }
    }

    #[test]
    fn capability_decides_between_announcers_before_score() {
        let p = ElectionParams::default();
        let mut e = Election::new(p, 0);
        e.state = State::Announcer;
        // A battery announcer with a far better score does not displace a mains one.
        assert_eq!(e.on_beacon(1000, &beacon(NodeId(1), 400, 0), -70, true, NodeId(5), 100, CAP_MAINS), None);
        // A mains announcer in our cell displaces a battery one whatever the scores ...
        let mut e = Election::new(p, 0);
        e.state = State::Announcer;
        assert_eq!(e.on_beacon(1000, &beacon(NodeId(9), 100, CAP_MAINS), -70, true, NodeId(5), 400, 0), Some(Transition::BecameFollower(NodeId(9))));
        // ... near or far, like a clearly higher score.
        let mut e = Election::new(p, 0);
        e.state = State::Announcer;
        assert_eq!(e.on_beacon(1000, &beacon(NodeId(9), 100, CAP_MAINS), -100, false, NodeId(5), 400, 0), Some(Transition::BecameFollower(NodeId(9))));
    }
}
