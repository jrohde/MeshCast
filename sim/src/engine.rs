//! Discrete-event engine: nodes run the real `meshcast_core::node::Node`; the engine models
//! propagation, sensitivity, capture, half-duplex, CCA busy detection and occupancy.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use meshcast_core::frame::{Frame, FrameType};
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ev {
    Wake(usize, u64),
    TxEnd(u64),
    Kill(usize),
    Revive(usize),
    End,
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
        let mut e = Engine { nodes, phys, heap: BinaryHeap::new(), seq: 0, tx_seq: 0, loss, reach, recent: HashMap::new(), txs: HashMap::new(), now: 0, metrics, verbose: false };
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

    pub fn run(&mut self, until: Millis, sample_every: Millis) {
        self.push(until, Ev::End);
        let mut next_sample = sample_every;
        while let Some(Reverse((t, _, ev))) = self.heap.pop() {
            if t > until {
                break;
            }
            self.now = t;
            if t >= next_sample {
                self.sample();
                next_sample += sample_every;
            }
            match ev {
                Ev::End => break,
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
                Action::Tx { carrier, channel, bytes, airtime_ms, class: _, frame_type } => self.tx_start(i, carrier, channel, bytes, airtime_ms, frame_type),
                Action::ObjectComplete { id, now } => {
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

    fn tx_start(&mut self, from: usize, carrier: usize, channel: u8, bytes: Vec<u8>, airtime_ms: u32, frame_type: FrameType) {
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
        let phy = &self.phys[carrier];
        let mut candidates = Vec::new();
        let reach = std::mem::take(&mut self.reach[from][carrier]);
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
        self.txs.insert(id, Transmission { id, from, carrier, channel, start: now, end, bytes, frame_type, candidates });
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
            if tx.frame_type == FrameType::Bulk {
                self.metrics.per_node_bulk[j].0 += 1;
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
        let stub = Transmission { id: tx.id, from: tx.from, carrier: tx.carrier, channel: tx.channel, start: tx.start, end: tx.end, bytes: tx.bytes.clone(), frame_type: tx.frame_type, candidates: Vec::new() };
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
