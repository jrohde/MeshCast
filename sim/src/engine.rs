//! Discrete-event engine: nodes run the real `meshcast_core::node::Node`; the engine models
//! propagation, sensitivity, capture, half-duplex, CCA busy detection and occupancy.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use meshcast_core::frame::{Frame, FrameType};
use meshcast_core::ids::NodeId;
use meshcast_core::node::{Action, CarrierState, Event, Node, NodeConfig};
use meshcast_core::rng::Rng;
use meshcast_core::Millis;

use crate::metrics::Metrics;
use crate::radio::{distance_loss, Phy, Propagation};

const OCC_WINDOW_MS: Millis = 10_000;
const MAX_AIRTIME_MS: Millis = 5_000;

pub struct SimNode {
    pub node: Node,
    pub alive: bool,
    pub mains: bool,
    wake_seq: u64,
    next_wake: Millis,
    /// Own transmissions per carrier (start, end), recent only.
    own_tx: Vec<VecDeque<(Millis, Millis)>>,
    /// Others' energy above CCA threshold per carrier (start, end), recent only.
    busy: Vec<VecDeque<(Millis, Millis)>>,
    pub airtime_ms: Vec<u64>,
}

struct Transmission {
    id: u64,
    from: usize,
    carrier: usize,
    channel: u8,
    start: Millis,
    end: Millis,
    bytes: Vec<u8>,
    frame_type: FrameType,
    candidates: Vec<(usize, f64)>,
    /// For an upload: the node index it is meant for.
    upload_to: Option<usize>,
    /// For an upload: (phase count the sender believed, the announcer's actual count, granted).
    upload_phase_view: (u8, u8, bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ev {
    Wake(usize, u64),
    TxEnd(u64),
    Kill(usize),
    Revive(usize),
}

pub struct Engine {
    pub nodes: Vec<SimNode>,
    pub phys: Vec<Phy>,
    heap: BinaryHeap<Reverse<(Millis, u64, Ev)>>,
    seq: u64,
    tx_seq: u64,
    /// Distance loss (without pl0) per ordered pair, including per-link shadowing.
    loss: Vec<f32>,
    /// Per node, per carrier: (neighbour, rx power at neighbour in dBm).
    reach: Vec<Vec<Vec<(usize, f64)>>>,
    /// Active or recently ended transmissions keyed by (carrier, channel).
    recent: HashMap<(usize, u8), VecDeque<u64>>,
    txs: HashMap<u64, Transmission>,
    pub now: Millis,
    pub metrics: Metrics,
    pub verbose: bool,
    trace_grants: bool,
    next_sample: Millis,
}

impl Engine {
    pub fn new(configs: Vec<NodeConfig>, positions: Vec<(f64, f64)>, phys: Vec<Phy>, prop: Propagation, seed: u64) -> Self {
        let n = configs.len();
        let mut rng = Rng::new(seed ^ 0xC0FFEE);
        // Symmetric shadowing per link.
        let mut loss = vec![0f32; n * n];
        for i in 0..n {
            for j in (i + 1)..n {
                let d = ((positions[i].0 - positions[j].0).powi(2) + (positions[i].1 - positions[j].1).powi(2)).sqrt();
                let u1 = rng.unit().max(1e-12);
                let u2 = rng.unit();
                let gauss = (-2.0 * u1.ln()).sqrt() * (2.0 * core::f64::consts::PI * u2).cos();
                let l = distance_loss(&prop, d) + gauss * prop.shadow_sigma_db;
                loss[i * n + j] = l as f32;
                loss[j * n + i] = l as f32;
            }
        }
        let mut reach = vec![vec![Vec::new(); phys.len()]; n];
        for i in 0..n {
            for (c, phy) in phys.iter().enumerate() {
                let floor = phy.sensitivity_dbm.min(phy.cca_threshold_dbm);
                for j in 0..n {
                    if i == j {
                        continue;
                    }
                    let rx = phy.tx_dbm - phy.pl0_db - loss[i * n + j] as f64;
                    if rx >= floor {
                        reach[i][c].push((j, rx));
                    }
                }
            }
        }
        let mut nodes = Vec::with_capacity(n);
        for cfg in configs.into_iter() {
            let mains = cfg.mains;
            let node = Node::new(cfg, 0);
            nodes.push(SimNode {
                node,
                alive: true,
                mains,
                wake_seq: 0,
                next_wake: Millis::MAX,
                own_tx: vec![VecDeque::new(); phys.len()],
                busy: vec![VecDeque::new(); phys.len()],
                airtime_ms: vec![0; phys.len()],
            });
        }
        let mut metrics = Metrics::default();
        metrics.per_node_bulk = vec![(0, 0); n];
        let mut e = Engine { nodes, phys, heap: BinaryHeap::new(), seq: 0, tx_seq: 0, loss, reach, recent: HashMap::new(), txs: HashMap::new(), now: 0, metrics, verbose: false, trace_grants: std::env::var("MESHCAST_TRACE_GRANTS").is_ok(), next_sample: 0 };
        for i in 0..n {
            e.schedule_wake(i, 1);
        }
        e
    }

    fn push(&mut self, t: Millis, ev: Ev) {
        self.seq += 1;
        self.heap.push(Reverse((t, self.seq, ev)));
    }

    pub fn schedule_kill(&mut self, node: usize, t: Millis) {
        self.push(t, Ev::Kill(node));
    }

    pub fn schedule_revive(&mut self, node: usize, t: Millis) {
        self.push(t, Ev::Revive(node));
    }

    /// Switch a node off or on right now (battery dead, taken indoors, switched on again).
    pub fn set_alive(&mut self, node: usize, alive: bool) {
        if alive == self.nodes[node].alive {
            return;
        }
        self.nodes[node].alive = alive;
        self.nodes[node].next_wake = Millis::MAX;
        if alive {
            let now = self.now;
            self.nodes[node].node.reboot(now);
            self.schedule_wake(node, now + 1);
        }
    }

    /// Replace a node by a newcomer: same hardware and place, nothing learned.
    pub fn replace_with_newcomer(&mut self, node: usize) {
        let now = self.now;
        self.nodes[node].alive = true;
        self.nodes[node].next_wake = Millis::MAX;
        self.nodes[node].node.factory_reset(now);
        self.schedule_wake(node, now + 1);
    }

    fn schedule_wake(&mut self, i: usize, t: Millis) {
        let t = t.max(self.now + 1);
        if t >= self.nodes[i].next_wake {
            return;
        }
        self.nodes[i].wake_seq += 1;
        self.nodes[i].next_wake = t;
        let s = self.nodes[i].wake_seq;
        self.push(t, Ev::Wake(i, s));
    }

    fn reschedule(&mut self, i: usize) {
        let d = self.nodes[i].node.next_deadline();
        self.schedule_wake(i, d);
    }

    /// Run until `until` (inclusive). May be called repeatedly to interleave scripted events
    /// (publications, subscription changes, failures) with simulation.
    pub fn run(&mut self, until: Millis, sample_every: Millis) {
        let mut next_sample = if self.next_sample == 0 { sample_every } else { self.next_sample };
        loop {
            let Some(Reverse((t, _, _))) = self.heap.peek() else { break };
            if *t > until {
                break;
            }
            let Some(Reverse((t, _, ev))) = self.heap.pop() else { break };
            self.now = t;
            if t >= next_sample {
                self.sample();
                next_sample += sample_every;
            }
            match ev {
                Ev::Wake(i, s) => self.wake(i, s),
                Ev::TxEnd(id) => self.tx_end(id),
                Ev::Kill(i) => {
                    self.nodes[i].alive = false;
                    self.nodes[i].next_wake = Millis::MAX;
                }
                Ev::Revive(i) => {
                    self.nodes[i].alive = true;
                    self.nodes[i].next_wake = Millis::MAX;
                    let now = self.now;
                    self.nodes[i].node.reboot(now);
                    self.schedule_wake(i, self.now + 1);
                }
            }
        }
        self.now = until;
        self.next_sample = next_sample;
    }

    /// After changing a node's protocol state from outside (publish, follow, unfollow), make
    /// sure it wakes up promptly.
    pub fn poke(&mut self, i: usize) {
        if self.nodes[i].alive {
            self.reschedule(i);
        }
    }

    fn sample(&mut self) {
        let now = self.now;
        for i in 0..self.nodes.len() {
            if !self.nodes[i].alive {
                continue;
            }
            for c in 0..self.phys.len() {
                let occ = self.occupancy(i, c, now);
                self.metrics.occupancy_samples.push((now, i, c, occ));
            }
        }
    }

    fn prune(q: &mut VecDeque<(Millis, Millis)>, before: Millis) {
        while let Some(&(_, end)) = q.front() {
            if end < before {
                q.pop_front();
            } else {
                break;
            }
        }
    }

    fn busy_now(&mut self, i: usize, c: usize, now: Millis) -> bool {
        let q = &mut self.nodes[i].busy[c];
        Self::prune(q, now.saturating_sub(OCC_WINDOW_MS));
        q.iter().any(|&(s, e)| s <= now && now < e)
    }

    fn occupancy(&mut self, i: usize, c: usize, now: Millis) -> u16 {
        let from = now.saturating_sub(OCC_WINDOW_MS);
        let q = &mut self.nodes[i].busy[c];
        Self::prune(q, from);
        let mut total = 0u64;
        let mut cur_s = 0u64;
        let mut cur_e = 0u64;
        for &(s, e) in q.iter() {
            let s = s.max(from);
            let e = e.min(now);
            if e <= s {
                continue;
            }
            if s > cur_e {
                total += cur_e - cur_s;
                cur_s = s;
                cur_e = e;
            } else if e > cur_e {
                cur_e = e;
            }
        }
        total += cur_e - cur_s;
        (total * 1000 / OCC_WINDOW_MS).min(1000) as u16
    }

    fn wake(&mut self, i: usize, s: u64) {
        if !self.nodes[i].alive || self.nodes[i].wake_seq != s {
            return;
        }
        self.nodes[i].next_wake = Millis::MAX;
        let now = self.now;
        let mut states = Vec::with_capacity(self.phys.len());
        for c in 0..self.phys.len() {
            let busy = self.busy_now(i, c, now);
            let occ = self.occupancy(i, c, now);
            states.push(CarrierState { busy, occupancy_permille: occ });
        }
        let actions = self.nodes[i].node.handle(Event::Tick { now, carriers: &states });
        self.apply(i, actions);
        self.reschedule(i);
    }

    fn apply(&mut self, i: usize, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Tx { carrier, channel, bytes, airtime_ms, class: _, frame_type, upload_to } => self.tx_start(i, carrier, channel, bytes, airtime_ms, frame_type, upload_to),
                Action::ObjectComplete { id, now } => {
                    if self.trace_grants {
                        eprintln!("OC {} {} {:?} {:?} role={:?}", now, i, id, self.nodes[i].node.object_kind(&id), self.nodes[i].node.role(self.phys.len() - 1));
                    }
                    self.metrics.completions.entry((i, id)).or_insert(now);
                }
                Action::Role { carrier, role, announcer, now } => {
                    let nid = self.nodes[i].node.id();
                    if self.verbose {
                        eprintln!("[{:>9.2} h] node {} carrier {} -> {:?} (announcer {:?})", now as f64 / 3.6e6, i, carrier, role, announcer);
                    }
                    self.metrics.role(now, nid, carrier, role, announcer);
                }
            }
        }
    }

    fn tx_start(&mut self, from: usize, carrier: usize, channel: u8, bytes: Vec<u8>, airtime_ms: u32, frame_type: FrameType, target: Option<NodeId>) {
        let now = self.now;
        let end = now + airtime_ms as Millis;
        self.tx_seq += 1;
        let id = self.tx_seq;
        self.metrics.frames_sent += 1;
        if frame_type == FrameType::Bulk {
            self.metrics.bulk_sent += 1;
            let ann = self.nodes[from].node.role(carrier) == meshcast_core::node::Role::Announcer;
            self.metrics.bulk_sent_by[ann as usize] += 1;
        }
        self.nodes[from].airtime_ms[carrier] += airtime_ms as u64;
        {
            let q = &mut self.nodes[from].own_tx[carrier];
            Self::prune(q, now.saturating_sub(MAX_AIRTIME_MS));
            q.push_back((now, end));
        }
        // An upload frame: which announcer is it for, and can it hear it at all?
        let mut upload_to = None;
        let mut upload_phase_view = (0u8, 0u8, false);
        if frame_type == FrameType::Bulk {
            if let Some(t) = target {
                let j = (t.0 as usize).wrapping_sub(1);
                if j < self.nodes.len() {
                    upload_to = Some(j);
                    let granted = match Frame::decode(&bytes) {
                        Ok(Frame::Bulk(b)) => self.nodes[from].node.upload_granted(&b.object, t),
                        _ => false,
                    };
                    upload_phase_view = (self.nodes[from].node.upload_phases_believed(t), self.nodes[j].node.upload_phases_now(), granted);
                    let reach = &self.reach[from][carrier];
                    let rx = reach.iter().find(|(k, _)| *k == j).map(|(_, r)| *r);
                    let outcome = if !self.nodes[j].alive {
                        Some(5)
                    } else if rx.map(|r| r < self.phys[carrier].sensitivity_dbm).unwrap_or(true) {
                        Some(4)
                    } else if self.nodes[j].node.channel(carrier, now) != channel {
                        let target_ann = self.nodes[j].node.role(carrier) == meshcast_core::node::Role::Announcer;
                        let believed = self.nodes[from].node.colour_believed(carrier, t);
                        let actual = self.nodes[j].node.own_colour();
                        let k = if !target_ann { 0 } else if believed.is_none() { 2 } else if believed != Some(actual) { 1 } else { 3 };
                        self.metrics.upload_wrong_channel[k] += 1;
                        Some(3)
                    } else {
                        None // decided at the end of the frame
                    };
                    if let Some(o) = outcome {
                        self.metrics.upload_outcome[o] += 1;
                    }
                }
            }
        }
        if let Ok(Frame::Gossip(g)) = Frame::decode(&bytes) {
            for (_, h, _) in &g.want {
                let k = (h.0 as usize).wrapping_sub(1);
                if !h.is_none() && k < self.nodes.len() && self.nodes[k].node.role(carrier) == meshcast_core::node::Role::Announcer {
                    self.metrics.grants_to_announcers += 1;
                }
            }
        }
        let phy = &self.phys[carrier];
        let mut candidates = Vec::new();
        let reach = std::mem::take(&mut self.reach[from][carrier]);
        if self.trace_grants {
            match Frame::decode(&bytes) {
                Ok(Frame::Gossip(g)) if g.announcer == g.node => {
                    let meet = self.nodes[from].node.in_meeting(carrier, now);
                    for (o, h, p) in &g.want {
                        if !h.is_none() {
                            eprintln!("GT {} {} {} {:?} {} meet={}", now, from, h.0 - 1, o, p, meet);
                        } else {
                            eprintln!("GA {} {} {:?} meet={} ch={}", now, from, o, meet, channel);
                        }
                    }
                }
                Ok(Frame::ManifestAnnounce(m)) => {
                    eprintln!("MA {} {} n={} meet={} ch={}", now, from, m.entries.len(), self.nodes[from].node.in_meeting(carrier, now), channel);
                }
                Ok(Frame::Gossip(g)) if !g.have.is_empty() => {
                    for o in &g.have {
                        eprintln!("OF {} {} {:?} meet={} ch={}", now, from, o, self.nodes[from].node.in_meeting(carrier, now), channel);
                    }
                }
                Ok(Frame::Bulk(b)) if upload_to.is_some() => {
                    let a = upload_to.unwrap();
                    eprintln!("UP {} {} {} {:?} b={} esi={} k={} ann_holds={} ann_wants={}", now, from, a, b.object, b.block, b.esi, b.k(), self.nodes[a].node.holds(&b.object), self.nodes[a].node.wants_object(&b.object));
                }
                _ => {}
            }
        }
        let trace = self.verbose && std::env::var("MESHCAST_TRACE").is_ok();
        if trace {
            if let Ok(Frame::Gossip(g)) = Frame::decode(&bytes) {
                eprintln!("t={} GOSSIP from {} ann={:?} have={:?} want={:?} heard={:?}", now, from, g.announcer, g.have, g.want, g.heard);
            }
        }
        for &(j, rx) in &reach {
            if !self.nodes[j].alive {
                continue;
            }
            // A receiver hears only the channel it is tuned to.
            let jch = self.nodes[j].node.channel(carrier, now);
            if trace {
                eprintln!("t={} tx from {} carrier {} ch {} {:?} -> node {} ch {} rx {:.1} dBm", now, from, carrier, channel, frame_type, j, jch, rx);
            }
            if jch != channel {
                continue;
            }
            if rx >= phy.cca_threshold_dbm {
                self.nodes[j].busy[carrier].push_back((now, end));
                // Waking a node whose CCA state changed is not needed: it re-checks at its own deadline.
            }
            if rx >= phy.sensitivity_dbm {
                candidates.push((j, rx));
            } else {
                self.metrics.frames_below_sensitivity += 1;
            }
        }
        self.reach[from][carrier] = reach;
        self.recent.entry((carrier, channel)).or_default().push_back(id);
        self.txs.insert(id, Transmission { id, from, carrier, channel, start: now, end, bytes, frame_type, candidates, upload_to, upload_phase_view });
        self.push(end, Ev::TxEnd(id));
    }

    fn rx_dbm(&self, from: usize, to: usize, carrier: usize) -> f64 {
        let n = self.nodes.len();
        self.phys[carrier].tx_dbm - self.phys[carrier].pl0_db - self.loss[from * n + to] as f64
    }

    fn tx_end(&mut self, id: u64) {
        let Some(tx) = self.txs.remove(&id) else { return };
        let now = self.now;
        // Prune the recent list for this (carrier, channel) and collect overlapping transmissions.
        let key = (tx.carrier, tx.channel);
        let mut overlapping: Vec<(usize, Millis, Millis, u64)> = Vec::new();
        if let Some(q) = self.recent.get_mut(&key) {
            q.retain(|other| *other == id || self.txs.get(other).map(|t| t.end + MAX_AIRTIME_MS >= now).unwrap_or(false));
            for other in q.iter() {
                if *other == id {
                    continue;
                }
                if let Some(t) = self.txs.get(other) {
                    if t.start < tx.end && t.end > tx.start {
                        overlapping.push((t.from, t.start, t.end, *other));
                    }
                }
            }
            // Keep this transmission visible to later-ending overlapping ones.
            q.retain(|other| *other != id);
        }
        let phy_capture = self.phys[tx.carrier].capture_db;
        let sens = self.phys[tx.carrier].sensitivity_dbm;
        let mut delivered = 0u64;
        let candidates = tx.candidates.clone();
        let decoded = Frame::decode(&tx.bytes).ok();
        for (j, rx) in candidates {
            if !self.nodes[j].alive {
                continue;
            }
            // Half-duplex: receiver was transmitting on this carrier during the frame.
            let hd = self.nodes[j].own_tx[tx.carrier].iter().any(|&(s, e)| s < tx.end && e > tx.start);
            if hd {
                self.metrics.frames_half_duplex += 1;
                if tx.upload_to == Some(j) {
                    self.metrics.upload_outcome[2] += 1;
                }
                continue;
            }
            let mut worst = f64::NEG_INFINITY;
            let mut worst_from = usize::MAX;
            let mut worst_id = 0u64;
            for &(from2, _, _, id2) in &overlapping {
                if from2 == j || from2 == tx.from {
                    continue;
                }
                let p = self.rx_dbm(from2, j, tx.carrier);
                if p >= sens - 10.0 && p > worst {
                    worst = p;
                    worst_from = from2;
                    worst_id = id2;
                }
            }
            if worst > f64::NEG_INFINITY && rx - worst < phy_capture {
                self.metrics.frames_collided += 1;
                if tx.upload_to == Some(j) {
                    self.metrics.upload_outcome[1] += 1;
                    let other = self.txs.get(&worst_id);
                    let k = match other.map(|t| (t.upload_to, t.frame_type)) {
                        Some((Some(to), _)) if to == j => 0,
                        Some((Some(_), _)) => 1,
                        _ if self.nodes[worst_from].node.role(tx.carrier) == meshcast_core::node::Role::Announcer => 2,
                        _ => 3,
                    };
                    self.metrics.upload_interferer[k] += 1;
                    if self.trace_grants {
                        let o = other.map(|t| (t.start, t.end, t.frame_type)).unwrap_or((0, 0, FrameType::Bulk));
                        eprintln!("UC {} {} {} kind={} tx=[{},{}] other=[{},{}] {:?} from {}", now, tx.from, j, k, tx.start, tx.end, o.0, o.1, o.2, worst_from);
                    }
                    if k == 0 {
                        let views = [tx.upload_phase_view, other.map(|t| t.upload_phase_view).unwrap_or((0, 0, false))];
                        let cause = if views.iter().any(|v| v.0 == 0) {
                            2
                        } else if views.iter().any(|v| !v.2) {
                            1
                        } else if views.iter().any(|v| v.0 != v.1) {
                            0
                        } else {
                            3
                        };
                        self.metrics.upload_collision_cause[cause] += 1;
                    }
                }
                if tx.frame_type == FrameType::Bulk {
                    let a = self.nodes[tx.from].node.role(tx.carrier) == meshcast_core::node::Role::Announcer;
                    let b = self.nodes[worst_from].node.role(tx.carrier) == meshcast_core::node::Role::Announcer;
                    self.metrics.bulk_collision_kinds[a as usize][b as usize] += 1;
                    if !a && !b {
                        let same = match (&decoded, self.txs.get(&worst_id).and_then(|t| Frame::decode(&t.bytes).ok())) {
                            (Some(Frame::Bulk(x)), Some(Frame::Bulk(y))) => x.object == y.object,
                            _ => false,
                        };
                        if same {
                            self.metrics.upload_collision_same_object += 1;
                        } else {
                            self.metrics.upload_collision_other_object += 1;
                        }
                    }
                }
                let meeting = (tx.start / 20_000) % 5 == 0;
                let ft = tx.frame_type as usize;
                if meeting {
                    self.metrics.collided_meeting[ft] += 1;
                } else {
                    self.metrics.collided_other[ft] += 1;
                }
                if tx.frame_type == FrameType::Bulk {
                    self.metrics.per_node_bulk[j].1 += 1;
                }
                continue;
            }
            delivered += 1;
            if tx.upload_to == Some(j) {
                self.metrics.upload_outcome[0] += 1;
                if self.trace_grants {
                    if let Some(Frame::Bulk(b)) = &decoded {
                        eprintln!("UO {} {} {} {:?} esi={} k={} before={:?}", now, tx.from, j, b.object, b.esi, b.k(), self.nodes[j].node.object_progress(&b.object));
                    }
                }
            }
            if self.trace_grants {
                if let Some(Frame::Gossip(g)) = &decoded {
                    if g.announcer == g.node {
                        for (o, h, p) in &g.want {
                            if h.0 as usize == j + 1 {
                                let (up, q) = self.nodes[j].node.upload_state(tx.carrier);
                                let has = self.nodes[j].node.holds(o);
                                eprintln!("GH {} {} {} {:?} {} has={} role={:?} follows={:?} up={:?} q={}", now, tx.from, j, o, p, has, self.nodes[j].node.role(tx.carrier), self.nodes[j].node.announcer_of(tx.carrier), up, q);
                            }
                        }
                    }
                }
            }
            if tx.frame_type == FrameType::Bulk {
                self.metrics.per_node_bulk[j].0 += 1;
            }
            if self.trace_grants && tx.upload_to != Some(j) {
                if let Some(Frame::Bulk(b)) = &decoded {
                    if self.nodes[j].node.wants_object(&b.object) && self.nodes[j].node.role(tx.carrier) == meshcast_core::node::Role::Announcer {
                        eprintln!("BO {} {} {} {:?} esi={} k={} before={:?}", now, tx.from, j, b.object, b.esi, b.k(), self.nodes[j].node.object_progress(&b.object));
                    }
                }
            }
            let actions = match &decoded {
                Some(f) => self.nodes[j].node.handle_frame(now, tx.carrier, f, rx.round() as i16),
                None => self.nodes[j].node.handle(Event::Rx { now, carrier: tx.carrier, bytes: &tx.bytes, rssi_dbm: rx.round() as i16 }),
            };
            self.apply(j, actions);
            self.reschedule(j);
        }
        self.metrics.frames_delivered += delivered;
        if tx.frame_type == FrameType::Bulk {
            self.metrics.bulk_delivered += delivered;
        }
        // Later-ending frames that overlapped this one must still see it: keep an ended stub
        // (no payload, no candidates) until it is older than the longest possible frame.
        let stub = Transmission { id: tx.id, from: tx.from, carrier: tx.carrier, channel: tx.channel, start: tx.start, end: tx.end, bytes: tx.bytes.clone(), frame_type: tx.frame_type, candidates: Vec::new(), upload_to: tx.upload_to, upload_phase_view: tx.upload_phase_view };
        self.txs.insert(id, stub);
        self.recent.entry(key).or_default().push_back(id);
        let cutoff = now.saturating_sub(MAX_AIRTIME_MS);
        let stale: Vec<u64> = self.txs.iter().filter(|(_, t)| t.candidates.is_empty() && t.end < cutoff).map(|(k, _)| *k).collect();
        if stale.len() > 1024 {
            for k in stale {
                self.txs.remove(&k);
            }
        }
    }

}
