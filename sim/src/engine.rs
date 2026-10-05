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

/// A node's own clock: what it reads at global time `g`. Its crystal runs `ppm` parts per million
/// fast or slow, and it started counting at `epoch`: a node without GPS, phone or a battery-backed
/// clock knows no time but its own (PROTOCOL.md §6). The default is the simulator's own time.
#[derive(Clone, Copy, Debug, Default)]
pub struct Clock {
    pub epoch: i64,
    pub ppm: i32,
}

impl Clock {
    pub fn local(&self, g: Millis) -> Millis {
        let g = g as i128;
        (self.epoch as i128 + g + g * self.ppm as i128 / 1_000_000).max(0) as Millis
    }

    /// The first global time at which this clock reads `l` or more.
    pub fn global(&self, l: Millis) -> Millis {
        if l == Millis::MAX {
            return Millis::MAX;
        }
        let rate = 1_000_000 + self.ppm as i128;
        let x = l as i128 - self.epoch as i128;
        let mut g = ((x * 1_000_000 + rate - 1).div_euclid(rate)).max(0) as Millis;
        while self.local(g) < l {
            g += 1;
        }
        g
    }
}

pub struct SimNode {
    pub node: Node,
    pub alive: bool,
    pub mains: bool,
    /// An attacker that only sends what it injects: its own protocol never reaches the air.
    pub mute: bool,
    wake_seq: u64,
    next_wake: Millis,
    /// Own transmissions per carrier (start, end), recent only.
    own_tx: Vec<VecDeque<(Millis, Millis)>>,
    /// Others' energy above CCA threshold per carrier (start, end), recent only.
    busy: Vec<VecDeque<(Millis, Millis)>>,
    pub airtime_ms: Vec<u64>,
    pub clock: Clock,
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
    Attack(usize),
}

/// A node that asks for more than it listens to (docs/ABUSE.md): every `period_ms` it sends a
/// WANT, addressed to the announcer it follows, for the next objects of `ids` in turn.
#[derive(Clone, Debug)]
pub struct Attacker {
    pub node: usize,
    pub period_ms: Millis,
    /// Send each WANT under a fresh made-up node id instead of its own.
    pub spoof: bool,
    /// Instead of asking, pose as an announcer that has everything and serves nothing.
    pub lure: bool,
    /// A lure claims the maximum score and full capability.
    pub claim_max: bool,
    /// A lure alternates its beacon and its HAVE, one frame per attempt.
    beaconed: bool,
    cursor: usize,
    rng: Rng,
}

impl Attacker {
    pub fn new(node: usize, period_ms: Millis, spoof: bool, lure: bool, claim_max: bool) -> Self {
        Attacker { node, period_ms, spoof, lure, claim_max, beaconed: false, cursor: 0, rng: Rng::new(0xA77A_C0DE ^ node as u64) }
    }
}

pub struct Engine {
    /// Attackers, and the objects they ask for.
    pub attackers: Vec<Attacker>,
    pub attack_ids: Vec<meshcast_core::ids::ShortId>,
    /// Rendition objects, so their frames can be counted apart.
    pub rendition_ids: std::collections::HashSet<meshcast_core::ids::ShortId>,
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
    /// Symbols to lose on purpose, once each: (receiver, object, symbol), for tests.
    lose: Vec<(usize, meshcast_core::ids::ShortId, u16)>,
    /// MESHCAST_TRACE_RX=<node>: every symbol that node receives.
    trace_rx: Option<usize>,
    /// Experiment: nobody receives on a separate control carrier (MESHCAST_NO_CTRL_RX).
    no_ctrl_rx: bool,
    /// A node switched on again starts its clock from zero.
    clock_restart: bool,
    /// Diagnostic (MESHCAST_TRACE_BUSY=<node index>): who keeps that node's channel busy when it
    /// wants to send, per transmitter, with the minute of the first and last time.
    trace_busy: Option<usize>,
    busy_from: VecDeque<(Millis, Millis, usize)>,
    pub busy_blame: std::collections::BTreeMap<usize, (u64, Millis, Millis)>,
    next_sample: Millis,
    /// Diagnostic (MESHCAST_TRACE_LEAVE): each follower that leaves its announcer for what it does
    /// not get, with who holds that object and who hears whom.
    trace_leave: bool,
    seen_leave: Vec<Millis>,
}

impl Engine {
    pub fn new(configs: Vec<NodeConfig>, positions: Vec<(f64, f64)>, phys: Vec<Phy>, prop: Propagation, seed: u64) -> Self {
        let n = configs.len();
        Self::with_clocks(configs, positions, phys, prop, seed, vec![Clock::default(); n], false)
    }

    /// With a clock per node; `restart`: a node switched on again counts from zero, as one
    /// without a battery-backed clock does.
    pub fn with_clocks(configs: Vec<NodeConfig>, positions: Vec<(f64, f64)>, phys: Vec<Phy>, prop: Propagation, seed: u64, clocks: Vec<Clock>, restart: bool) -> Self {
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
        for (k, cfg) in configs.into_iter().enumerate() {
            let mains = cfg.mains;
            let clock = clocks[k];
            let node = Node::new(cfg, clock.local(0));
            nodes.push(SimNode {
                node,
                alive: true,
                mains,
                mute: false,
                wake_seq: 0,
                next_wake: Millis::MAX,
                own_tx: vec![VecDeque::new(); phys.len()],
                busy: vec![VecDeque::new(); phys.len()],
                airtime_ms: vec![0; phys.len()],
                clock,
            });
        }
        let mut metrics = Metrics::default();
        metrics.per_node_bulk = vec![(0, 0); n];
        let mut e = Engine { attackers: Vec::new(), attack_ids: Vec::new(), rendition_ids: Default::default(), nodes, phys, heap: BinaryHeap::new(), seq: 0, tx_seq: 0, loss, reach, recent: HashMap::new(), txs: HashMap::new(), now: 0, metrics, verbose: false, trace_grants: std::env::var("MESHCAST_TRACE_GRANTS").is_ok(), trace_busy: std::env::var("MESHCAST_TRACE_BUSY").ok().and_then(|v| v.parse().ok()), busy_from: VecDeque::new(), busy_blame: Default::default(), next_sample: 0, trace_leave: std::env::var("MESHCAST_TRACE_LEAVE").is_ok(), seen_leave: vec![0; n], lose: Vec::new(), trace_rx: std::env::var("MESHCAST_TRACE_RX").ok().and_then(|v| v.parse().ok()), no_ctrl_rx: std::env::var("MESHCAST_NO_CTRL_RX").is_ok(), clock_restart: restart };
        for i in 0..n {
            e.schedule_wake(i, 1);
        }
        e
    }

    fn push(&mut self, t: Millis, ev: Ev) {
        self.seq += 1;
        self.heap.push(Reverse((t, self.seq, ev)));
    }

    /// Start the attackers: each sends its first WANT after one period.
    pub fn start_attacks(&mut self) {
        for k in 0..self.attackers.len() {
            let period = self.attackers[k].period_ms.max(2);
            let t = self.now + self.attackers[k].rng.below(period);
            self.push(t, Ev::Attack(k));
        }
    }

    fn attack(&mut self, k: usize) {
        let now = self.now;
        let a = &mut self.attackers[k];
        let (i, period, spoof) = (a.node, a.period_ms, a.spoof);
        // Random timing, on average once a period: a WANT flood, not a jammer aimed at dwell
        // starts (which is what a fixed period from zero turned out to be).
        let next = now + period / 2 + a.rng.below(period.max(2));
        self.push(next, Ev::Attack(k));
        if !self.nodes[i].alive || self.attack_ids.is_empty() {
            return;
        }
        if self.attackers[k].lure {
            self.lure(k);
            return;
        }
        let a = &mut self.attackers[k];
        let n = self.attack_ids.len().min(meshcast_core::frame::MAX_WANT);
        let want: Vec<_> = (0..n).map(|j| (self.attack_ids[(a.cursor + j) % self.attack_ids.len()], NodeId::NONE, 0u8)).collect();
        a.cursor = (a.cursor + n) % self.attack_ids.len();
        let carrier = self.phys.iter().position(|p| p.kind != meshcast_core::frame::CarrierKind::LoraControl).unwrap_or(0);
        let node = &self.nodes[i].node;
        let announcer = node.announcer_of(carrier);
        if announcer.is_none() {
            return;
        }
        let id = if spoof { NodeId(0x8000_0000 | (self.tx_seq as u32 & 0x7fff_ffff)) } else { node.id() };
        let g = meshcast_core::frame::Gossip { node: id, announcer, announcer_colour: 0, announcer_colours: 1, heard: Vec::new(), have: Vec::new(), have_sets: Vec::new(), want, sets: Vec::new() };
        let bytes = Frame::Gossip(g).encode();
        let channel = node.channel(carrier, self.nodes[i].clock.local(now));
        let airtime = self.phys[carrier].to_core().airtime_ms(bytes.len());
        self.metrics.attack_frames += 1;
        self.tx_start(i, carrier, channel, bytes, airtime, FrameType::Gossip, None);
    }

    /// In the rendezvous, where every follower listens, beacon as an announcer and list objects
    /// in HAVE; never carousel anything.
    fn lure(&mut self, k: usize) {
        let now = self.now;
        let i = self.attackers[k].node;
        let carrier = self.phys.iter().position(|p| p.kind != meshcast_core::frame::CarrierKind::LoraControl).unwrap_or(0);
        // Where every follower listens: in the rendezvous on a hopping carrier, at any time on one
        // that does not hop.
        if self.phys[carrier].channels.len() > 1 && !self.nodes[i].node.in_meeting(carrier, self.nodes[i].clock.local(now)) {
            return;
        }
        let a = &mut self.attackers[k];
        let n = self.attack_ids.len().min(meshcast_core::frame::MAX_GOSSIP_IDS);
        let have: Vec<_> = (0..n).map(|j| self.attack_ids[(a.cursor + j) % self.attack_ids.len()]).collect();
        let beacon = !a.beaconed;
        let (score, caps) = if a.claim_max { (meshcast_core::params::SCORE_MAX, meshcast_core::frame::CAP_MAINS | meshcast_core::frame::CAP_IP) } else { (0, 0) };
        a.beaconed = beacon;
        if !beacon {
            a.cursor = (a.cursor + n) % self.attack_ids.len();
        }
        let node = &self.nodes[i].node;
        let id = node.id();
        let channel = node.channel(carrier, self.nodes[i].clock.local(now));
        let kind = self.phys[carrier].kind;
        let b = meshcast_core::frame::Beacon { carrier: kind, announcer: id, score, caps, next_ms: 60_000, round: 0, time: node.shared_time(self.nodes[i].clock.local(now)), time_quality: 1, colour: 0, colours: 1, upload_phases: 1, occupancy: [0; 4] };
        let g = meshcast_core::frame::Gossip { node: id, announcer: id, announcer_colour: 0, announcer_colours: 1, heard: Vec::new(), have, have_sets: Vec::new(), want: Vec::new(), sets: Vec::new() };
        let (f, ft) = if beacon { (Frame::Beacon(b), FrameType::Beacon) } else { (Frame::Gossip(g), FrameType::Gossip) };
        let bytes = f.encode();
        let airtime = self.phys[carrier].to_core().airtime_ms(bytes.len());
        self.metrics.attack_frames += 1;
        self.tx_start(i, carrier, channel, bytes, airtime, ft, None);
    }

    pub fn schedule_kill(&mut self, node: usize, t: Millis) {
        self.push(t, Ev::Kill(node));
    }

    pub fn schedule_revive(&mut self, node: usize, t: Millis) {
        self.push(t, Ev::Revive(node));
    }

    /// Switch a node off or on right now (battery dead, taken indoors, switched on again).
    /// Lose the next reception of symbol `esi` of `object` at `node`, once: a fault on purpose.
    pub fn lose_symbol(&mut self, node: usize, object: meshcast_core::ids::ShortId, esi: u16) {
        self.lose.push((node, object, esi));
    }

    pub fn set_alive(&mut self, node: usize, alive: bool) {
        if alive == self.nodes[node].alive {
            return;
        }
        self.nodes[node].alive = alive;
        self.nodes[node].next_wake = Millis::MAX;
        if alive {
            let now = self.now;
            self.restart_clock(node);
            let local = self.nodes[node].clock.local(now);
            self.nodes[node].node.reboot(local);
            self.schedule_wake(node, now + 1);
        }
    }

    /// Replace a node by a newcomer: same hardware and place, nothing learned.
    pub fn replace_with_newcomer(&mut self, node: usize) {
        let now = self.now;
        self.nodes[node].alive = true;
        self.nodes[node].next_wake = Millis::MAX;
        self.restart_clock(node);
        let local = self.nodes[node].clock.local(now);
        self.nodes[node].node.factory_reset(local);
        self.schedule_wake(node, now + 1);
    }

    /// Diagnostic: pairs of live announcers that hear each other on the control carrier but not on
    /// the bulk carrier, and of those how many share none of their control windows in the coming
    /// hour, and how many fewer than nine in ten (PROTOCOL.md §3, §6). Such a pair crosses roots
    /// and announcements only in a window both are in, for a second at least.
    /// Diagnostic: the parts of the network that carrier `c` joins, every node labelled with the
    /// smallest index in its part; two nodes are joined when each hears the other.
    pub fn components(&self, c: usize) -> Vec<usize> {
        let n = self.nodes.len();
        let mut part: Vec<usize> = (0..n).collect();
        fn root(part: &mut Vec<usize>, mut i: usize) -> usize {
            while part[i] != i {
                part[i] = part[part[i]];
                i = part[i];
            }
            i
        }
        let s = self.phys[c].sensitivity_dbm;
        for a in 0..n {
            for b in a + 1..n {
                if self.rx_dbm(a, b, c) >= s && self.rx_dbm(b, a, c) >= s {
                    let (ra, rb) = (root(&mut part, a), root(&mut part, b));
                    part[ra.max(rb)] = ra.min(rb);
                }
            }
        }
        (0..n).map(|i| root(&mut part, i)).collect()
    }

    pub fn ctrl_only_pairs(&self, ctrl: usize, bulk: usize) -> (usize, usize, usize) {
        let windows = |i: usize| {
            let n = &self.nodes[i];
            let mut v = Vec::new();
            let mut g = self.now;
            while g < self.now + 3_600_000 {
                let Some((s, e)) = n.node.ctrl_window_at(n.clock.local(g)) else { break };
                let (s, e) = (n.clock.global(s), n.clock.global(e));
                v.push((s, e));
                g = e.max(g) + 1;
            }
            v
        };
        let anns: Vec<usize> = (0..self.nodes.len()).filter(|&i| self.nodes[i].alive && self.nodes[i].node.role(bulk) == meshcast_core::node::Role::Announcer).collect();
        let hears = |a: usize, b: usize, c: usize| self.rx_dbm(a, b, c) >= self.phys[c].sensitivity_dbm && self.rx_dbm(b, a, c) >= self.phys[c].sensitivity_dbm;
        let (mut pairs, mut apart, mut partly) = (0, 0, 0);
        for (k, &a) in anns.iter().enumerate() {
            for &b in &anns[k + 1..] {
                if !hears(a, b, ctrl) || hears(a, b, bulk) {
                    continue;
                }
                pairs += 1;
                let (wa, wb) = (windows(a), windows(b));
                let shared = wa.iter().filter(|(s, e)| wb.iter().any(|(t, f)| (*e).min(*f).saturating_sub((*s).max(*t)) >= 1_000)).count();
                if shared == 0 {
                    apart += 1;
                } else if shared * 10 < wa.len() * 9 {
                    partly += 1;
                }
            }
        }
        (pairs, apart, partly)
    }

    /// Diagnostic: how far apart the live nodes' shared times are now (ms), how many nodes are
    /// within one second of the median, and how many live nodes there are (PROTOCOL.md §6).
    pub fn shared_time_spread(&self) -> (u64, usize, usize) {
        let mut t: Vec<u64> = self.nodes.iter().filter(|n| n.alive).map(|n| n.node.shared_time(n.clock.local(self.now))).collect();
        if t.is_empty() {
            return (0, 0, 0);
        }
        t.sort_unstable();
        let med = t[t.len() / 2];
        let near = t.iter().filter(|x| x.abs_diff(med) <= 1_000).count();
        (t[t.len() - 1] - t[0], near, t.len())
    }

    /// A node switched on again without a battery-backed clock counts from zero.
    fn restart_clock(&mut self, i: usize) {
        if self.clock_restart {
            let now = self.now as i128;
            let c = &mut self.nodes[i].clock;
            c.epoch = -((now + now * c.ppm as i128 / 1_000_000) as i64);
        }
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
        let d = self.nodes[i].clock.global(self.nodes[i].node.next_deadline());
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
                Ev::Attack(k) => self.attack(k),
                Ev::Revive(i) => {
                    self.nodes[i].alive = true;
                    self.nodes[i].next_wake = Millis::MAX;
                    let now = self.now;
                    self.restart_clock(i);
                    let local = self.nodes[i].clock.local(now);
                    self.nodes[i].node.reboot(local);
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
            if !self.attackers.iter().any(|a| a.node == i) {
                for (name, size) in self.nodes[i].node.table_sizes() {
                    let peak = self.metrics.table_peaks.entry(name).or_insert(0);
                    *peak = (*peak).max(size);
                }
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
            if busy && self.trace_busy == Some(i) {
                while self.busy_from.front().map(|x| x.1 <= now.saturating_sub(10_000)).unwrap_or(false) {
                    self.busy_from.pop_front();
                }
                for &(s, e, f) in self.busy_from.iter().filter(|x| x.0 <= now && now < x.1) {
                    let _ = (s, e);
                    let b = self.busy_blame.entry(f).or_insert((0, now, now));
                    b.0 += 1;
                    b.2 = now;
                }
            }
            let occ = self.occupancy(i, c, now);
            states.push(CarrierState { busy, occupancy_permille: occ });
        }
        let local = self.nodes[i].clock.local(now);
        let actions = self.nodes[i].node.handle(Event::Tick { now: local, carriers: &states });
        self.apply(i, actions);
        self.reschedule(i);
    }

    fn apply(&mut self, i: usize, actions: Vec<Action>) {
        if self.trace_leave {
            if let Some((t, ann, x)) = self.nodes[i].node.last_leave() {
                if t != self.seen_leave[i] {
                    self.seen_leave[i] = t;
                    self.trace_leave_of(i, ann, x);
                }
            }
        }
        for a in actions {
            match a {
                Action::Tx { .. } if self.nodes[i].mute => {}
                Action::Role { .. } if self.nodes[i].mute => {}
                Action::Tx { carrier, channel, bytes, airtime_ms, class: _, frame_type, upload_to } => self.tx_start(i, carrier, channel, bytes, airtime_ms, frame_type, upload_to),
                Action::ObjectComplete { id, now: _ } => {
                    // When it happened, by the simulator's clock: the node's own may differ.
                    let now = self.now;
                    if self.trace_grants {
                        eprintln!("OC {} {} {:?} {:?} role={:?}", now, i, id, self.nodes[i].node.object_kind(&id), self.nodes[i].node.role(self.phys.len() - 1));
                    }
                    self.metrics.completions.entry((i, id)).or_insert(now);
                    if let Some((k, at)) = self.metrics.evicted_pending.remove(&(i, id)) {
                        self.metrics.evictions[k].1 = Some(now - at);
                    }
                }
                Action::Relayed { id } => {
                    let r = self.metrics.relayers.entry(id).or_default();
                    r.0 += 1;
                    r.1.insert(i);
                }
                Action::Evicted { id, info } => {
                    if self.trace_grants {
                        eprintln!("EV {} {} {:?} relayed={} menu={} role={:?}", self.now, i, id, info.relayed, info.menu, self.nodes[i].node.role(self.phys.len() - 1));
                    }
                    let k = self.metrics.evictions.len();
                    self.metrics.evictions.push((info, None));
                    self.metrics.evicted_pending.insert((i, id), (k, self.now));
                }
                Action::Role { carrier, role, announcer, now: _ } => {
                    let now = self.now;
                    let nid = self.nodes[i].node.id();
                    if self.verbose {
                        eprintln!("[{:>9.2} h] node {} carrier {} -> {:?} (announcer {:?})", now as f64 / 3.6e6, i, carrier, role, announcer);
                    }
                    self.metrics.role(now, nid, carrier, role, announcer);
                    if std::env::var("MESHCAST_TRACE_WANTS_AT_ROLE").ok().and_then(|v| v.parse::<usize>().ok()) == Some(i) {
                        eprintln!("WANTS-AT-ROLE {:.3} h node {} -> {:?} (announcer {:?}): {}", now as f64 / 3.6e6, i, role, announcer, self.nodes[i].node.want_report());
                    }
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
            if !self.rendition_ids.is_empty() {
                if let Ok(Frame::Bulk(b)) = Frame::decode(&bytes) {
                    if self.rendition_ids.contains(&b.object) {
                        self.metrics.bulk_sent_rendition += 1;
                    }
                }
            }
            let ann = self.nodes[from].node.role(carrier) == meshcast_core::node::Role::Announcer;
            self.metrics.bulk_sent_by[ann as usize] += 1;
            if let Ok(Frame::Bulk(b)) = Frame::decode(&bytes) {
                use meshcast_core::object::ContentType;
                let k = match self.nodes[from].node.object_kind(&b.object) {
                    Some(ContentType::Manifest) => 0,
                    Some(ContentType::Collection) => 1,
                    Some(_) => 2,
                    None => 3,
                };
                self.metrics.bulk_sent_kind[ann as usize][k] += 1;
            }
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
                    } else if self.nodes[j].node.channel(carrier, self.nodes[j].clock.local(now)) != channel {
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
            // An announcer's asks for its listeners: open, or met by a grant (FEASIBILITY.md §28).
            if g.announcer == g.node && !self.nodes[from].mute {
                let me = self.nodes[from].node.id();
                let unpacked = self.nodes[from].node.unpacked(&g);
                for (o, h, p) in &unpacked.want {
                    if p & meshcast_core::frame::ASK_LISTENED == 0 {
                        continue;
                    }
                    if !self.metrics.listened_asks.contains_key(&(from, *o)) {
                        let real = self.nodes.iter().any(|n| n.alive && n.node.announcer_of(carrier) == me && n.node.listens_to(o));
                        self.metrics.listened_asks.insert((from, *o), crate::metrics::ListenedAsk { first: now, real_listener: real, ..Default::default() });
                    }
                    let a = self.metrics.listened_asks.get_mut(&(from, *o)).unwrap();
                    if h.is_none() {
                        if a.granted.is_none() {
                            a.open_frames += 1;
                        }
                    } else if a.granted.is_none() {
                        let k = (h.0 as usize).wrapping_sub(1);
                        let foreign = k < self.nodes.len() && self.nodes[k].node.announcer_of(carrier) != me;
                        a.granted = Some((now, foreign));
                    }
                }
            }
        }
        let phy = &self.phys[carrier];
        let mut candidates = Vec::new();
        let reach = std::mem::take(&mut self.reach[from][carrier]);
        if self.trace_grants {
            // Sets are read as their sender means them, piece by piece.
            let decoded = match Frame::decode(&bytes) {
                Ok(Frame::Gossip(g)) => Ok(Frame::Gossip(self.nodes[from].node.unpacked(&g))),
                other => other,
            };
            match decoded {
                Ok(Frame::Gossip(g)) if g.announcer == g.node => {
                    let meet = self.nodes[from].node.in_meeting(carrier, self.nodes[from].clock.local(now));
                    eprintln!("GS {} {} want={} have={} meet={} wants_len={}", now, from, g.want.len(), g.have.len(), meet, self.nodes[from].node.wants_len());
                    if std::env::var("MESHCAST_TRACE_WANTS_AT_ROLE").ok().and_then(|v| v.parse::<usize>().ok()) == Some(from) {
                        eprintln!("WANTS-AT-ASK {:.3} h node {}: {}", now as f64 / 3_600_000.0, from, self.nodes[from].node.want_report());
                    }
                    for (o, h, p) in &g.want {
                        if !h.is_none() {
                            eprintln!("GT {} {} {} {:?} {} meet={}", now, from, h.0 - 1, o, p, meet);
                        } else {
                            eprintln!("GA {} {} {:?} meet={} ch={} wants={}", now, from, o, meet, channel, self.nodes[from].node.wants_len());
                        }
                    }
                }
                Ok(Frame::ManifestAnnounce(m)) => {
                    eprintln!("MA {} {} n={} meet={} ch={}", now, from, m.entries.len(), self.nodes[from].node.in_meeting(carrier, self.nodes[from].clock.local(now)), channel);
                }
                Ok(Frame::Gossip(g)) if !g.have.is_empty() => {
                    for o in &g.have {
                        eprintln!("OF {} {} {:?} meet={} ch={}", now, from, o, self.nodes[from].node.in_meeting(carrier, self.nodes[from].clock.local(now)), channel);
                    }
                }
                Ok(Frame::Bulk(b)) if upload_to.is_none() && b.esi == 0 && self.nodes[from].node.role(carrier) == meshcast_core::node::Role::Announcer => {
                    eprintln!("CB {} {} {:?} block {} len {}", now, from, b.object, b.block, b.len);
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
        if std::env::var("MESHCAST_TRACE_ANNOUNCE").is_ok() {
            if let Ok(Frame::ManifestAnnounce(m)) = Frame::decode(&bytes) {
                let heard: Vec<String> = reach.iter().filter(|(j, rx)| self.nodes[*j].alive && *rx >= phy.sensitivity_dbm && self.nodes[*j].node.channel(carrier, self.nodes[*j].clock.local(now)) == channel).map(|(j, _)| self.nodes[*j].node.id().0.to_string()).collect();
                println!("ANN {} {} carrier {} entries {} -> {}", now, self.nodes[from].node.id().0, carrier, m.entries.len(), heard.join(" "));
            }
        }
        let trace_bcn = std::env::var("MESHCAST_TRACE_BEACONS").is_ok() && matches!(Frame::decode(&bytes), Ok(Frame::Beacon(_)));
        if trace_bcn {
            if let Ok(Frame::Beacon(b)) = Frame::decode(&bytes) {
                let heard: Vec<String> = reach.iter().filter(|(j, rx)| self.nodes[*j].alive && *rx >= phy.sensitivity_dbm).map(|(j, rx)| {
                    let jch = self.nodes[*j].node.channel(carrier, self.nodes[*j].clock.local(now));
                    let deaf = !self.nodes[*j].node.listening(carrier, self.nodes[*j].clock.local(now));
                    format!("{}{}{}:{:.0}/{}", self.nodes[*j].node.id().0, if jch == channel { "" } else { "x" }, if deaf { "d" } else { "" }, rx, self.nodes[*j].node.score())
                }).collect();
                println!("BCN {} {} score {} ch {} colour {}/{} meet {} -> {}", now, self.nodes[from].node.id().0, b.score, channel, b.colour, b.colours, self.nodes[from].node.in_meeting(carrier, self.nodes[from].clock.local(now)), heard.join(" "));
            }
        }
        for &(j, rx) in &reach {
            if !self.nodes[j].alive {
                continue;
            }
            // A receiver hears only the channel it is tuned to.
            let jch = self.nodes[j].node.channel(carrier, self.nodes[j].clock.local(now));
            if trace {
                eprintln!("t={} tx from {} carrier {} ch {} {:?} -> node {} ch {} rx {:.1} dBm", now, from, carrier, channel, frame_type, j, jch, rx);
            }
            if jch != channel {
                continue;
            }
            if rx >= phy.cca_threshold_dbm {
                self.nodes[j].busy[carrier].push_back((now, end));
                if self.trace_busy == Some(j) {
                    self.busy_from.push_back((now, end, from));
                }
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

    /// `OX`: an offer reaching, or failing to reach, an announcer that wants what it offers.
    fn trace_offer(&self, tx: &Transmission, offered: &[meshcast_core::ids::ShortId], j: usize, outcome: &str, by: usize) {
        if offered.is_empty() || self.nodes[j].node.role(tx.carrier) != meshcast_core::node::Role::Announcer {
            return;
        }
        for o in offered {
            if self.nodes[j].node.wants_object(o) {
                let by = if by == usize::MAX { String::from("-") } else { by.to_string() };
                eprintln!("OX {} {} {} {:?} {} by={} meet={}", tx.start, tx.from, j, o, outcome, by, self.nodes[j].node.in_meeting(tx.carrier, self.nodes[j].clock.local(tx.start)));
            }
        }
    }

    /// One line for MESHCAST_TRACE_LEAVE: follower `i` left announcer `ann` waiting for `x`. Every
    /// node that holds `x`, with its role, its announcer, and whether it and `ann` hear each other on
    /// the bulk carrier and how strongly `i` hears it.
    fn trace_leave_of(&self, i: usize, ann: meshcast_core::ids::NodeId, x: meshcast_core::ids::ShortId) {
        let bulk = self.phys.len() - 1;
        let sens = self.phys[bulk].sensitivity_dbm;
        let a = (ann.0 as usize).wrapping_sub(1);
        let ok = a < self.nodes.len();
        let holders: Vec<String> = (0..self.nodes.len()).filter(|&j| self.nodes[j].alive && self.nodes[j].node.holds(&x)).map(|j| {
            let n = &self.nodes[j].node;
            let hears = ok && self.rx_dbm(j, a, bulk) >= sens && self.rx_dbm(a, j, bulk) >= sens;
            format!("{}:{:?}:ann{}:{}:{:.0}", j, n.role(bulk), n.announcer_of(bulk).0, if hears { "heard" } else { "apart" }, self.rx_dbm(j, i, bulk))
        }).collect();
        let (alive, holds, wants) = if ok { let n = &self.nodes[a].node; (self.nodes[a].alive, n.holds(&x), n.wants_object(&x)) } else { (false, false, false) };
        // Bridges: nodes that hear, and are heard by, both `ann` and some holder; and of them, those
        // that know `x` well enough to name it (its length, from a manifest), and those that hold it.
        let hear = |p: usize, q: usize| self.rx_dbm(p, q, bulk) >= sens && self.rx_dbm(q, p, bulk) >= sens;
        let held: Vec<usize> = (0..self.nodes.len()).filter(|&j| self.nodes[j].alive && self.nodes[j].node.holds(&x)).collect();
        let bridges: Vec<usize> = if ok { (0..self.nodes.len()).filter(|&j| j != a && self.nodes[j].alive && hear(j, a) && held.iter().any(|&h| h != j && hear(j, h))).collect() } else { Vec::new() };
        let knowing = bridges.iter().filter(|&&j| self.nodes[j].node.object_kind(&x).is_some()).count();
        let knowers = (0..self.nodes.len()).filter(|&j| self.nodes[j].alive && self.nodes[j].node.object_kind(&x).is_some()).count();
        eprintln!("LEAVE {} {} ann={} alive={} holds={} wants={} relay={} {:?} {:?} knowers={} bridges={} knowing={} holders=[{}]", self.now, i, ann.0, alive, holds, wants, self.nodes[i].node.relays(&x), x, self.nodes[i].node.object_kind(&x), knowers, bridges.len(), knowing, holders.join(" "));
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
        // What this frame offers, as its sender means it, for the offer trace.
        let offered: Vec<meshcast_core::ids::ShortId> = match (&decoded, self.trace_grants) {
            (Some(Frame::Gossip(g)), true) if g.announcer != g.node => self.nodes[tx.from].node.unpacked(g).have,
            _ => Vec::new(),
        };
        for (j, rx) in candidates {
            if !self.nodes[j].alive {
                continue;
            }
            if self.no_ctrl_rx && self.phys[tx.carrier].kind == meshcast_core::frame::CarrierKind::LoraControl && self.phys.len() > 1 {
                continue;
            }
            if let Some(Frame::Bulk(b)) = &decoded {
                if let Some(k) = self.lose.iter().position(|(n, o, e)| *n == j && *o == b.object && *e == b.esi) {
                    self.lose.remove(k);
                    continue;
                }
            }
            // The symbol trace says what became of each frame that reached the node.
            let rx_trace = match (&decoded, self.trace_rx == Some(j)) {
                (Some(Frame::Bulk(b)), true) => Some(format!("RX {} {} from {} {:?} esi={} before={:?}", now, j, tx.from, b.object, b.esi, self.nodes[j].node.object_progress(&b.object))),
                _ => None,
            };
            // One radio: a receiver hears a carrier only while it listens there, the whole frame.
            if !self.nodes[j].node.listening(tx.carrier, self.nodes[j].clock.local(tx.start)) || !self.nodes[j].node.listening(tx.carrier, self.nodes[j].clock.local(tx.end.saturating_sub(1))) {
                self.metrics.frames_not_listening += 1;
                if let Some(t) = &rx_trace {
                    eprintln!("{} lost=not-listening", t);
                }
                continue;
            }
            // A receiver on a hopping carrier retunes at the end of its dwell, frame or no frame.
            if self.nodes[j].node.channel(tx.carrier, self.nodes[j].clock.local(tx.end.saturating_sub(1))) != tx.channel {
                self.metrics.frames_retuned += 1;
                if let Some(t) = &rx_trace {
                    eprintln!("{} lost=retuned", t);
                }
                continue;
            }
            // Half-duplex: receiver was transmitting on this carrier during the frame.
            let hd = self.nodes[j].own_tx[tx.carrier].iter().any(|&(s, e)| s < tx.end && e > tx.start);
            if hd {
                self.metrics.frames_half_duplex += 1;
                if tx.upload_to == Some(j) {
                    self.metrics.upload_outcome[2] += 1;
                    if self.trace_grants {
                        if let Some(Frame::Bulk(b)) = &decoded {
                            eprintln!("UH {} {} {} {:?} esi={}", now, tx.from, j, b.object, b.esi);
                        }
                    }
                }
                self.trace_offer(&tx, &offered, j, "hd", usize::MAX);
                if let Some(t) = &rx_trace {
                    eprintln!("{} lost=half-duplex", t);
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
                        eprintln!("UC {} {} {} kind={} tx=[{},{}] other=[{},{}] {:?} from {} phases(believed,actual)={:?}/{:?}", now, tx.from, j, k, tx.start, tx.end, o.0, o.1, o.2, worst_from, tx.upload_phase_view, other.map(|t| t.upload_phase_view));
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
                let meeting = self.nodes[tx.from].node.in_meeting(tx.carrier, self.nodes[tx.from].clock.local(tx.start));
                let ft = tx.frame_type as usize;
                if meeting {
                    self.metrics.collided_meeting[ft] += 1;
                } else {
                    self.metrics.collided_other[ft] += 1;
                }
                if tx.frame_type == FrameType::Bulk {
                    self.metrics.per_node_bulk[j].1 += 1;
                }
                self.trace_offer(&tx, &offered, j, "col", worst_from);
                if let Some(t) = &rx_trace {
                    eprintln!("{} lost=collision with {}", t, worst_from);
                }
                continue;
            }
            delivered += 1;
            self.trace_offer(&tx, &offered, j, "ok", usize::MAX);
            if let Some(t) = &rx_trace {
                eprintln!("{}", t);
            }
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
            let local = self.nodes[j].clock.local(now);
            let actions = match &decoded {
                Some(f) => self.nodes[j].node.handle_frame(local, tx.carrier, f, rx.round() as i16),
                None => self.nodes[j].node.handle(Event::Rx { now: local, carrier: tx.carrier, bytes: &tx.bytes, rssi_dbm: rx.round() as i16 }),
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
