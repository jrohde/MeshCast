//! The MeshCast node: one event-driven state machine that ties objects, manifests, frames,
//! carousel, election, EtherFatsoen and EtherDiscipline together. No I/O: the host (firmware,
//! station or simulator) feeds [`Event`]s and executes [`Action`]s.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;

use crate::carousel::{Carousel, CarouselParams, Item};
use crate::discipline::{Accounting, Verdict};
use crate::election::{Election, Transition};
use crate::fatsoen::Fatsoen;
use crate::frame::{AnnounceEntry, Beacon, Bulk, CarrierKind, Class, Frame, FrameType, Gossip, ManifestAnnounce, Nack, MAX_ANNOUNCE_ENTRIES, MAX_GOSSIP_IDS, MAX_NACK_RANGES, SYMBOL_SIZE};
use crate::ids::{ChannelId, NodeId, ShortId};
use crate::manifest::Manifest;
use crate::object::{Mime, ObjectMeta};
use crate::params::{Params, SCORE_MAX};
use crate::profile::{Access, RegionProfile};
use crate::rng::Rng;
use crate::store::{MemStore, Put};
use crate::Millis;

#[derive(Clone, Debug)]
pub struct CarrierParams {
    pub kind: CarrierKind,
    pub bitrate_bps: u32,
    /// Fixed per-frame overhead (preamble, sync word, turnaround) in milliseconds.
    pub overhead_ms: u32,
    /// Band index in the region profile, None for carriers without accounting (ESP-NOW, IP).
    pub band: Option<usize>,
    /// Centre frequencies the carrier may use; more than one means frequency agility.
    pub channels: Vec<u32>,
    pub bw_hz: u32,
    pub tx_dbm: i8,
    /// Beacons at or above this RSSI mean "same cell" for the announcer tie-break.
    pub near_rssi_dbm: i16,
}

impl CarrierParams {
    pub fn airtime_ms(&self, bytes: usize) -> u32 {
        let bits = bytes as u64 * 8;
        ((bits * 1000 + self.bitrate_bps as u64 - 1) / self.bitrate_bps as u64) as u32 + self.overhead_ms
    }
}

#[derive(Clone, Debug)]
pub struct NodeConfig {
    pub id: NodeId,
    pub mains: bool,
    pub has_ip: bool,
    pub profile: &'static RegionProfile,
    /// Which access rule to use per band (index into `Band::access`).
    pub rule_choice: Vec<usize>,
    pub carriers: Vec<CarrierParams>,
    pub params: Params,
    pub seed: u64,
    pub keep_bytes_below: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CarrierState {
    /// Clear-channel assessment: energy above threshold right now.
    pub busy: bool,
    /// Fraction (permille) of the last window during which the channel was busy with others.
    pub occupancy_permille: u16,
}

pub enum Event<'a> {
    Tick { now: Millis, carriers: &'a [CarrierState] },
    Rx { now: Millis, carrier: usize, bytes: &'a [u8], rssi_dbm: i16 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Follower,
    Candidate,
    Announcer,
}

#[derive(Clone, Debug)]
pub enum Action {
    Tx { carrier: usize, channel: u8, bytes: Vec<u8>, airtime_ms: u32, class: Class, frame_type: FrameType },
    ObjectComplete { id: ShortId, now: Millis },
    Role { carrier: usize, role: Role, announcer: NodeId, now: Millis },
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub tx_frames: [u64; 3],
    pub tx_by_type: [u64; 6],
    pub tx_airtime_ms: Vec<u64>,
    pub rx_frames: u64,
    pub rx_bad: u64,
    pub cca_deferrals: u64,
    pub discipline_waits: u64,
    pub symbols_new: u64,
    pub symbols_dup: u64,
    pub symbols_rejected: u64,
    pub nacks_sent: u64,
    pub wants_sent: u64,
    pub uploads_started: u64,
    pub manifests_adopted: u64,
}

#[derive(Clone, Debug)]
struct Neighbor {
    last_heard: Millis,
    announcer: NodeId,
    score: u16,
    haves: BTreeSet<ShortId>,
}

#[derive(Clone, Copy, Debug)]
struct ManifestInfo {
    seq: u32,
    short: ShortId,
    len: u32,
    adopted: bool,
}

/// A source transmitting an object to the announcer: either a full pass or a NACKed list.
#[derive(Clone, Debug)]
struct Upload {
    object: ShortId,
    /// The announcer that asked; determines the hop channel on agile carriers.
    to: NodeId,
    /// Suppression: do not start before this time, and give up if someone else is heard
    /// uploading the same object meanwhile.
    start_at: Millis,
    block: u16,
    esi: u16,
    list: Option<VecDeque<(u16, u16)>>,
}

#[derive(Clone, Debug)]
struct Pending {
    class: Class,
    frame_type: FrameType,
    bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default)]
struct Progress {
    last_progress: Millis,
    last_nack: Millis,
    last_want: Millis,
}

struct CarrierRt {
    p: CarrierParams,
    election: Option<Election>,
    carousel: Option<Carousel>,
    fatsoen: Fatsoen,
    queue: Vec<Pending>,
    busy_until: Millis,
    pace_until: Millis,
    upload: Option<Upload>,
    /// Dropped because a frame could never be legal on this carrier.
    dropped: u64,
    /// Last hop-cycle dwell index at which a dwell-start beacon was queued.
    last_dwell: u64,
    /// The next queued frame has already waited its random jitter.
    jittered: bool,
    /// Airtime of MeshCast frames decoded on this carrier since the last EtherFatsoen window.
    rx_air_ms: u64,
}

enum Cand {
    Queue(usize),
    Carousel(Item),
    Upload(ShortId, u16, u16, u16),
}

pub struct Node {
    cfg: NodeConfig,
    now: Millis,
    pub store: MemStore,
    follows: BTreeSet<ChannelId>,
    manifests: BTreeMap<ChannelId, ManifestInfo>,
    own_manifests: Vec<(ChannelId, ShortId, u32, u32)>,
    own_objects: BTreeSet<ShortId>,
    pending_ack: BTreeSet<ShortId>,
    wants: BTreeSet<ShortId>,
    neighbors: BTreeMap<NodeId, Neighbor>,
    carriers: Vec<CarrierRt>,
    ctrl: usize,
    discipline: Accounting,
    rng: Rng,
    score: u16,
    next_beacon: Millis,
    next_gossip: Millis,
    last_gossip: Millis,
    next_score: Millis,
    want_refresh: bool,
    last_want_tx: Millis,
    have_cursor: usize,
    want_cursor: usize,
    announce_cursor: usize,
    progress: BTreeMap<ShortId, Progress>,
    pub stats: Stats,
}

impl Node {
    pub fn new(cfg: NodeConfig, now: Millis) -> Self {
        let discipline = Accounting::new(cfg.profile, &cfg.rule_choice);
        let ctrl = cfg.carriers.iter().position(|c| c.kind == CarrierKind::LoraControl).unwrap_or(0);
        let cp = CarouselParams { max_passes: cfg.params.max_passes, t_always_ms: cfg.params.t_always_ms, want_ttl_ms: cfg.params.want_ttl_ms };
        let mut carriers = Vec::with_capacity(cfg.carriers.len());
        for c in &cfg.carriers {
            let bulk = c.kind.is_bulk();
            carriers.push(CarrierRt {
                p: c.clone(),
                election: if bulk { Some(Election::new(cfg.params.election, c.near_rssi_dbm, now)) } else { None },
                carousel: if bulk { Some(Carousel::new(cp)) } else { None },
                fatsoen: Fatsoen::new(cfg.params.fatsoen, now),
                queue: Vec::new(),
                busy_until: 0,
                pace_until: 0,
                upload: None,
                dropped: 0,
                last_dwell: u64::MAX,
                jittered: false,
                rx_air_ms: 0,
            });
        }
        let mut stats = Stats::default();
        stats.tx_airtime_ms = alloc::vec![0; cfg.carriers.len()];
        let seed = cfg.seed ^ (cfg.id.0 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let p = cfg.params;
        Node {
            store: MemStore::new(cfg.keep_bytes_below),
            follows: BTreeSet::new(),
            manifests: BTreeMap::new(),
            own_manifests: Vec::new(),
            own_objects: BTreeSet::new(),
            pending_ack: BTreeSet::new(),
            wants: BTreeSet::new(),
            neighbors: BTreeMap::new(),
            carriers,
            ctrl,
            discipline,
            rng: Rng::new(seed),
            score: 0,
            next_beacon: now + p.election.t_beacon_ms,
            next_gossip: now + p.t_gossip_ms,
            last_gossip: 0,
            next_score: now,
            want_refresh: false,
            last_want_tx: 0,
            have_cursor: 0,
            want_cursor: 0,
            announce_cursor: 0,
            progress: BTreeMap::new(),
            stats,
            cfg,
            now,
        }
    }

    fn carousel_params(&self) -> CarouselParams {
        CarouselParams { max_passes: self.cfg.params.max_passes, t_always_ms: self.cfg.params.t_always_ms, want_ttl_ms: self.cfg.params.want_ttl_ms }
    }

    // ---------------------------------------------------------------- public API

    pub fn id(&self) -> NodeId {
        self.cfg.id
    }

    pub fn now(&self) -> Millis {
        self.now
    }

    pub fn score(&self) -> u16 {
        self.score
    }

    pub fn carriers(&self) -> impl Iterator<Item = &CarrierParams> {
        self.carriers.iter().map(|c| &c.p)
    }

    pub fn role(&self, carrier: usize) -> Role {
        match self.carriers.get(carrier).and_then(|c| c.election.as_ref()).map(|e| e.state()) {
            Some(crate::election::State::Announcer) => Role::Announcer,
            Some(crate::election::State::Candidate { .. }) => Role::Candidate,
            _ => Role::Follower,
        }
    }

    pub fn announcer_of(&self, carrier: usize) -> NodeId {
        match self.carriers.get(carrier).and_then(|c| c.election.as_ref()) {
            Some(e) if e.is_announcer() => self.cfg.id,
            Some(e) => e.announcer,
            None => NodeId::NONE,
        }
    }

    /// Current channel index on a frequency-agile carrier: the hop schedule of the announcer we
    /// follow (or our own). A node that follows nobody scans: it stays on each channel for a whole
    /// hop cycle, so that any announcer, which visits every channel once per cycle and beacons at
    /// each dwell start, is heard within one cycle.
    pub fn channel(&self, carrier: usize, now: Millis) -> u8 {
        let c = &self.carriers[carrier];
        let n = c.p.channels.len().max(1) as u64;
        if n == 1 {
            return 0;
        }
        self.channel_for(carrier, self.announcer_of(carrier), now)
    }

    /// Channel of `ann`'s hop sequence at `now`, with the common meeting dwell every
    /// `meet_every` dwells; a node following nobody scans slowly outside meeting dwells.
    pub fn channel_for(&self, carrier: usize, ann: NodeId, now: Millis) -> u8 {
        let c = &self.carriers[carrier];
        let n = c.p.channels.len().max(1) as u64;
        if n == 1 {
            return 0;
        }
        let dwell = self.cfg.params.dwell_ms.max(1);
        let di = now / dwell;
        if self.is_meeting_dwell(di) {
            return hop_channel(MEETING_ID, di, n);
        }
        if ann.is_none() {
            return ((now / (dwell * n)) % n) as u8;
        }
        hop_channel(ann, di, n)
    }

    fn is_meeting_dwell(&self, dwell_index: u64) -> bool {
        let m = self.cfg.params.meet_every.max(1);
        dwell_index % m == 0
    }

    /// Start of the next meeting dwell strictly after `now`.
    fn next_meeting_start(&self, now: Millis) -> Millis {
        let dwell = self.cfg.params.dwell_ms.max(1);
        let m = self.cfg.params.meet_every.max(1);
        let di = now / dwell + 1;
        let next = di.div_ceil(m) * m;
        next * dwell
    }

    /// Whether the cell carrier hops, in which case cell-wide control traffic is timed to the
    /// meeting dwell so that every cell hears it.
    fn agile(&self) -> bool {
        self.carriers.get(self.cell_carrier()).map(|c| c.p.channels.len() > 1).unwrap_or(false)
    }

    pub fn is_announcing(&self) -> bool {
        self.carriers.iter().any(|c| c.election.as_ref().map(|e| e.is_announcer()).unwrap_or(false))
    }

    pub fn wants(&self) -> &BTreeSet<ShortId> {
        &self.wants
    }

    pub fn follows(&self) -> &BTreeSet<ChannelId> {
        &self.follows
    }

    pub fn neighbor_count(&self) -> usize {
        self.neighbors.len()
    }

    pub fn occupancy(&self, carrier: usize) -> u16 {
        self.carriers[carrier].fatsoen.occupancy()
    }

    pub fn rate(&self, carrier: usize) -> u16 {
        self.carriers[carrier].fatsoen.rate()
    }

    pub fn dropped(&self, carrier: usize) -> u64 {
        self.carriers[carrier].dropped
    }

    pub fn follow(&mut self, chan: ChannelId) {
        self.follows.insert(chan);
        if let Some(info) = self.manifests.get(&chan).copied() {
            if info.adopted {
                if let Some(bytes) = self.store.bytes(&info.short).map(|b| b.to_vec()) {
                    if let Ok(m) = Manifest::decode(&bytes) {
                        self.adopt_manifest(&m, info.short);
                    }
                }
            } else if !self.store.has_complete(&info.short) {
                self.add_want(info.short);
            }
        }
        self.want_refresh = true;
    }

    /// Publish a channel manifest and the objects it references (we own them, complete).
    pub fn publish(&mut self, manifest: &Manifest, objects: &[(ObjectMeta, Option<&[u8]>)]) {
        let (meta, bytes) = manifest.as_object();
        self.store.insert_complete(meta, Some(&bytes));
        let short = meta.id.short();
        let chan = manifest.channel_id();
        self.manifests.insert(chan, ManifestInfo { seq: manifest.seq, short, len: meta.len, adopted: true });
        self.own_manifests.retain(|(c, _, _, _)| *c != chan);
        self.own_manifests.push((chan, short, manifest.seq, meta.len));
        self.pending_ack.insert(short);
        for (m, b) in objects {
            self.store.insert_complete(*m, *b);
            self.own_objects.insert(m.id.short());
            self.pending_ack.insert(m.id.short());
        }
        for c in self.carriers.iter_mut() {
            if let Some(car) = c.carousel.as_mut() {
                car.set_always(short);
            }
        }
        self.gossip_soon();
    }

    /// Time at which the host should call `Tick` next.
    pub fn next_deadline(&self) -> Millis {
        let mut d = self.next_score;
        if self.is_announcing() {
            d = d.min(self.next_beacon);
        }
        if self.is_announcing() || !self.pending_ack.is_empty() {
            d = d.min(self.next_gossip);
        }
        if !self.wants.is_empty() {
            // Stall checks (WANT / NACK) are time-based; poll them at the stall granularity.
            d = d.min(self.now + self.cfg.params.t_nack_stall_ms);
        }
        for c in &self.carriers {
            if let Some(e) = &c.election {
                d = d.min(e.deadline());
            }
            let announcing = c.election.as_ref().map(|e| e.is_announcer()).unwrap_or(false);
            let has_pending = !c.queue.is_empty() || c.upload.is_some() || (announcing && c.carousel.as_ref().map(|k| k.has_work(&self.store)).unwrap_or(false));
            if has_pending {
                let mut t = c.busy_until.max(c.pace_until).max(c.fatsoen.backoff_until).max(self.now + 1);
                if c.queue.is_empty() && !announcing {
                    if let Some(u) = &c.upload {
                        t = t.max(u.start_at);
                    }
                }
                d = d.min(t);
            }
            if announcing {
                if let Some(due) = c.carousel.as_ref().and_then(|k| k.next_due()) {
                    d = d.min(due);
                }
                if c.p.channels.len() > 1 {
                    let dwell = self.cfg.params.dwell_ms.max(1);
                    d = d.min((self.now / dwell + 1) * dwell);
                }
            }
        }
        d.max(self.now + 1)
    }

    pub fn handle(&mut self, ev: Event<'_>) -> Vec<Action> {
        let mut out = Vec::new();
        match ev {
            Event::Tick { now, carriers } => self.tick(now, carriers, &mut out),
            Event::Rx { now, carrier, bytes, rssi_dbm } => self.rx(now, carrier, bytes, rssi_dbm, &mut out),
        }
        out
    }

    // ---------------------------------------------------------------- helpers

    fn add_want(&mut self, id: ShortId) {
        if self.wants.insert(id) {
            let now = self.now;
            let p = self.progress.entry(id).or_default();
            p.last_progress = now;
            p.last_want = 0;
            if self.is_announcing() {
                self.gossip_soon();
            }
        }
    }

    fn gossip_soon(&mut self) {
        let t = self.now.max(self.last_gossip + self.cfg.params.t_gossip_min_ms);
        if t < self.next_gossip {
            self.next_gossip = t;
        }
    }

    // ---------------------------------------------------------------- tick

    fn tick(&mut self, now: Millis, states: &[CarrierState], out: &mut Vec<Action>) {
        self.now = now;
        for (i, c) in self.carriers.iter_mut().enumerate() {
            let occ = states.get(i).map(|s| s.occupancy_permille).unwrap_or(0);
            if c.fatsoen.maybe_window(now, occ, c.rx_air_ms) {
                c.rx_air_ms = 0;
            }
        }
        if now >= self.next_score {
            self.expire_neighbors();
            self.score = self.compute_score();
            self.next_score = now + self.cfg.params.t_score_ms;
        }
        for i in 0..self.carriers.len() {
            let score = self.score;
            let t = match self.carriers[i].election.as_mut() {
                Some(e) => e.tick(now, score, &mut self.rng),
                None => None,
            };
            if let Some(t) = t {
                self.on_transition(i, t, out);
            }
        }
        if now >= self.next_beacon {
            self.next_beacon = now + self.cfg.params.election.t_beacon_ms;
            for i in 0..self.carriers.len() {
                if self.role(i) == Role::Announcer {
                    let b = self.make_beacon(i);
                    self.enqueue(i, Frame::Beacon(b));
                }
            }
        }
        // Frequency-agile carriers: a beacon at every dwell start so scanners can find us.
        for i in 0..self.carriers.len() {
            if self.carriers[i].p.channels.len() > 1 && self.role(i) == Role::Announcer {
                let d = now / self.cfg.params.dwell_ms.max(1);
                if self.carriers[i].last_dwell != d {
                    self.carriers[i].last_dwell = d;
                    if !self.carriers[i].queue.iter().any(|p| p.frame_type == FrameType::Beacon) {
                        let b = self.make_beacon(i);
                        self.enqueue(i, Frame::Beacon(b));
                    }
                }
            }
        }
        if now >= self.next_gossip {
            if self.agile() && !self.is_meeting_dwell(now / self.cfg.params.dwell_ms.max(1)) {
                // Hold cell-wide gossip for the meeting dwell, when other cells listen too.
                self.next_gossip = self.next_meeting_start(now) + self.rng.below(self.cfg.params.dwell_ms / 4);
            } else {
                self.next_gossip = now + self.cfg.params.t_gossip_ms;
                if self.is_announcing() || !self.pending_ack.is_empty() {
                    self.last_gossip = now;
                    self.gossip_round();
                }
            }
        }
        self.follower_want_check();
        self.nack_check();
        for i in 0..self.carriers.len() {
            let busy = states.get(i).map(|s| s.busy).unwrap_or(false);
            self.try_tx(i, busy, out);
        }
    }

    fn expire_neighbors(&mut self) {
        let from = self.now.saturating_sub(self.cfg.params.neighbor_ttl_ms);
        self.neighbors.retain(|_, n| n.last_heard >= from);
    }

    fn compute_score(&self) -> u16 {
        let mut s: u32 = (self.neighbors.len().min(64) * 4) as u32;
        if self.cfg.mains {
            s += 64;
        }
        // Unused regulatory budget on the primary bulk carrier.
        if let Some(c) = self.carriers.iter().find(|c| c.p.kind.is_bulk()) {
            if let Some(band) = c.p.band {
                let budget = self.discipline.budget_permille(band, c.p.channels.len() as u16) as u64;
                let limit = 3_600_000 * budget / 1000;
                let used = self.discipline.used_ms(band, self.now, 3_600_000);
                if limit > 0 {
                    s += (64 * limit.saturating_sub(used) / limit) as u32;
                }
            } else {
                s += 64;
            }
        }
        s += self.store.complete_ids().count().min(32) as u32;
        if self.cfg.has_ip {
            s += 16;
        }
        s.min(SCORE_MAX as u32) as u16
    }

    fn on_transition(&mut self, carrier: usize, t: Transition, out: &mut Vec<Action>) {
        match t {
            Transition::BecameCandidate => {
                out.push(Action::Role { carrier, role: Role::Candidate, announcer: NodeId::NONE, now: self.now });
            }
            Transition::BecameAnnouncer => {
                let mut car = Carousel::new(self.carousel_params());
                for info in self.manifests.values() {
                    car.set_always(info.short);
                }
                self.carriers[carrier].carousel = Some(car);
                self.carriers[carrier].upload = None;
                // Serve everything any known manifest references; want what we lack.
                let to_want: Vec<ShortId> = self.manifests.values().filter(|i| !self.store.has_complete(&i.short)).map(|i| i.short).collect();
                for id in to_want {
                    self.add_want(id);
                }
                self.next_beacon = self.now;
                self.next_gossip = self.now;
                out.push(Action::Role { carrier, role: Role::Announcer, announcer: self.cfg.id, now: self.now });
            }
            Transition::BecameFollower(to) | Transition::AnnouncerChanged(to) => {
                self.carriers[carrier].carousel = Some(Carousel::new(self.carousel_params()));
                self.carriers[carrier].upload = None;
                self.want_refresh = true;
                out.push(Action::Role { carrier, role: Role::Follower, announcer: to, now: self.now });
            }
        }
    }

    fn make_beacon(&self, carrier: usize) -> Beacon {
        let c = &self.carriers[carrier];
        let round = c.carousel.as_ref().map(|k| k.round).unwrap_or(0);
        let mut occupancy = [0u8; 4];
        for (i, cr) in self.carriers.iter().filter(|c| c.p.kind.is_bulk()).take(4).enumerate() {
            occupancy[i] = (cr.fatsoen.occupancy() / 10).min(100) as u8;
        }
        Beacon {
            carrier: c.p.kind,
            announcer: self.cfg.id,
            score: self.score,
            next_ms: self.cfg.params.election.t_beacon_ms.min(65535) as u16,
            round,
            utc: 0,
            time_quality: 0,
            channel: self.channel(carrier, self.now),
            occupancy,
        }
    }

    fn enqueue(&mut self, carrier: usize, f: Frame) {
        let class = f.class();
        let frame_type = f.frame_type();
        let bytes = f.encode();
        self.carriers[carrier].queue.push(Pending { class, frame_type, bytes });
    }

    fn primary_bulk(&self) -> Option<usize> {
        self.carriers.iter().position(|c| c.p.kind.is_bulk())
    }

    /// Cell-local control traffic (gossip, NACK, beacons) travels on the bulk carrier: a cell is
    /// what hears each other there. The long-range control carrier only carries discovery.
    fn cell_carrier(&self) -> usize {
        self.primary_bulk().unwrap_or(self.ctrl)
    }

    /// Reboot: lose volatile protocol state (roles, queues, timers) but keep the library.
    pub fn reboot(&mut self, now: Millis) {
        self.now = now;
        let p = self.cfg.params;
        let cp = self.carousel_params();
        for c in self.carriers.iter_mut() {
            if c.election.is_some() {
                c.election = Some(Election::new(p.election, c.p.near_rssi_dbm, now));
                c.carousel = Some(Carousel::new(cp));
            }
            c.fatsoen = Fatsoen::new(p.fatsoen, now);
            c.queue.clear();
            c.busy_until = 0;
            c.pace_until = 0;
            c.upload = None;
        }
        self.neighbors.clear();
        self.next_beacon = now + p.election.t_beacon_ms;
        self.next_gossip = now + p.t_gossip_ms;
        self.next_score = now;
        self.want_refresh = true;
        self.last_want_tx = 0;
        self.progress.clear();
        for id in self.own_objects.iter().chain(self.own_manifests.iter().map(|(_, s, _, _)| s)) {
            self.pending_ack.insert(*id);
        }
    }

    fn gossip_announcer_field(&self) -> NodeId {
        match self.primary_bulk() {
            Some(i) => self.announcer_of(i),
            None => NodeId::NONE,
        }
    }

    fn gossip_round(&mut self) {
        let announcing = self.is_announcing();
        let mut have: Vec<ShortId> = Vec::new();
        if announcing {
            let ids: Vec<ShortId> = self.store.complete_ids().copied().collect();
            if !ids.is_empty() {
                for j in 0..ids.len().min(MAX_GOSSIP_IDS) {
                    have.push(ids[(self.have_cursor + j) % ids.len()]);
                }
                self.have_cursor = (self.have_cursor + MAX_GOSSIP_IDS) % ids.len();
            }
        } else {
            have = self.pending_ack.iter().filter(|id| self.store.has_complete(id)).take(MAX_GOSSIP_IDS).copied().collect();
        }
        let want = self.take_wants();
        if !have.is_empty() || !want.is_empty() {
            let g = Gossip { node: self.cfg.id, announcer: self.gossip_announcer_field(), have, want };
            self.enqueue(self.cell_carrier(), Frame::Gossip(g));
        }
        let entries: Vec<AnnounceEntry> = if announcing {
            let all: Vec<AnnounceEntry> = self
                .manifests
                .iter()
                .map(|(c, i)| AnnounceEntry { channel: *c, manifest: i.short, seq: i.seq, len: i.len })
                .collect();
            if all.is_empty() {
                Vec::new()
            } else {
                let n = all.len().min(MAX_ANNOUNCE_ENTRIES);
                let v: Vec<AnnounceEntry> = (0..n).map(|j| all[(self.announce_cursor + j) % all.len()].clone()).collect();
                self.announce_cursor = (self.announce_cursor + n) % all.len();
                v
            }
        } else {
            self.own_manifests
                .iter()
                .filter(|(_, s, _, _)| self.pending_ack.contains(s))
                .take(MAX_ANNOUNCE_ENTRIES)
                .map(|(c, s, seq, len)| AnnounceEntry { channel: *c, manifest: *s, seq: *seq, len: *len })
                .collect()
        };
        if !entries.is_empty() {
            let cell = self.cell_carrier();
            self.enqueue(cell, Frame::ManifestAnnounce(ManifestAnnounce { node: self.cfg.id, entries: entries.clone() }));
            if self.ctrl != cell {
                self.enqueue(self.ctrl, Frame::ManifestAnnounce(ManifestAnnounce { node: self.cfg.id, entries }));
            }
        }
    }

    fn take_wants(&mut self) -> Vec<ShortId> {
        let ids: Vec<ShortId> = self.wants.iter().copied().collect();
        if ids.is_empty() {
            return Vec::new();
        }
        let n = ids.len().min(MAX_GOSSIP_IDS);
        let v: Vec<ShortId> = (0..n).map(|j| ids[(self.want_cursor + j) % ids.len()]).collect();
        self.want_cursor = (self.want_cursor + n) % ids.len();
        let now = self.now;
        for id in &v {
            self.progress.entry(*id).or_default().last_want = now;
        }
        v
    }

    /// Followers stay silent unless they have wants that the announcer is not serving.
    fn follower_want_check(&mut self) {
        if self.is_announcing() || self.wants.is_empty() {
            return;
        }
        let Some(pb) = self.primary_bulk() else { return };
        if self.announcer_of(pb).is_none() {
            return;
        }
        let now = self.now;
        let stall = self.cfg.params.t_want_min_ms;
        let stalled = self.wants.iter().all(|id| {
            let p = self.progress.get(id).copied().unwrap_or_default();
            now >= p.last_progress.max(p.last_want) + stall
        });
        if (self.want_refresh || stalled) && now >= self.last_want_tx + self.cfg.params.t_want_min_ms.min(stall) {
            let want = self.take_wants();
            let g = Gossip { node: self.cfg.id, announcer: self.gossip_announcer_field(), have: Vec::new(), want };
            let cell = self.cell_carrier();
            self.enqueue(cell, Frame::Gossip(g));
            self.stats.wants_sent += 1;
            self.want_refresh = false;
            self.last_want_tx = now;
        }
    }

    /// Any node (follower or announcer) that is nearly complete on an object and sees no progress
    /// asks for the missing symbols. The carousel or the uploading source answers.
    fn nack_check(&mut self) {
        let Some(pb) = self.primary_bulk() else { return };
        let announcing = self.role(pb) == Role::Announcer;
        if !announcing && self.announcer_of(pb).is_none() {
            return;
        }
        let now = self.now;
        let thr = self.cfg.params.nack_threshold_permille as u64;
        let stall = self.cfg.params.t_nack_stall_ms;
        let mut to_send: Option<(ShortId, u16, Vec<(u16, u16)>)> = None;
        for id in self.wants.iter() {
            let Some(e) = self.store.entry(id) else { continue };
            let (have, total) = e.progress();
            let Some(total) = total else { continue };
            if (have as u64) * 1000 < thr * total as u64 {
                continue;
            }
            let p = self.progress.get(id).copied().unwrap_or_default();
            if now < p.last_progress + stall || now < p.last_nack + stall {
                continue;
            }
            for block in 0..e.known_blocks() {
                let missing = self.store.missing(id, block);
                if !missing.is_empty() {
                    to_send = Some((*id, block, compress_ranges(&missing)));
                    break;
                }
            }
            if to_send.is_some() {
                break;
            }
        }
        if let Some((id, block, ranges)) = to_send {
            self.progress.entry(id).or_default().last_nack = now;
            let cell = self.cell_carrier();
            self.enqueue(cell, Frame::Nack(Nack { node: self.cfg.id, object: id, block, missing: ranges }));
            self.stats.nacks_sent += 1;
        }
    }

    /// Share of airtime the bulk carrier may pace itself to: the regulatory budget minus a
    /// reserve for control frames on the same band, or the self-imposed share where no
    /// regulatory duty cycle exists.
    fn budget_for(&self, carrier: usize) -> u16 {
        let c = &self.carriers[carrier];
        let share = self.cfg.params.fatsoen.max_own_share;
        let reserve = 1000u32.saturating_sub(self.cfg.params.control_reserve as u32);
        match c.p.band {
            Some(b) => match self.discipline.rule(b) {
                Access::DutyCycle { .. } | Access::Polite { .. } => {
                    let budget = self.discipline.budget_permille(b, c.p.channels.len() as u16) as u32;
                    (budget * reserve / 1000) as u16
                }
                _ => share,
            },
            None => share,
        }
    }

    fn try_tx(&mut self, i: usize, cca_busy: bool, out: &mut Vec<Action>) {
        let now = self.now;
        {
            let c = &self.carriers[i];
            if c.busy_until > now || c.fatsoen.backoff_until > now || c.pace_until > now {
                return;
            }
        }
        let is_ann = self.role(i) == Role::Announcer;
        let cand = {
            let c = &mut self.carriers[i];
            if !c.queue.is_empty() {
                let mut best = 0;
                for (j, p) in c.queue.iter().enumerate() {
                    if p.class < c.queue[best].class {
                        best = j;
                    }
                }
                Some(Cand::Queue(best))
            } else if is_ann {
                c.carousel.as_mut().and_then(|k| k.peek(&self.store, now)).map(Cand::Carousel)
            } else if let Some(u) = c.upload.as_mut() {
                if u.start_at > now {
                    return;
                }
                match &mut u.list {
                    Some(list) => loop {
                        match list.front().copied() {
                            Some((block, esi)) => match self.store.block_k(&u.object, block) {
                                Some(k) if esi < k => break Some(Cand::Upload(u.object, block, esi, k)),
                                _ => {
                                    list.pop_front();
                                }
                            },
                            None => {
                                c.upload = None;
                                break None;
                            }
                        }
                    },
                    None => match self.store.block_k(&u.object, u.block) {
                        Some(k) => Some(Cand::Upload(u.object, u.block, u.esi, k)),
                        None => {
                            c.upload = None;
                            None
                        }
                    },
                }
            } else {
                None
            }
        };
        let Some(cand) = cand else { return };
        let (bytes, class, frame_type) = match &cand {
            Cand::Queue(j) => {
                let p = &self.carriers[i].queue[*j];
                (p.bytes.clone(), p.class, p.frame_type)
            }
            Cand::Carousel(Item::Beacon { .. }) => {
                let b = self.make_beacon(i);
                (Frame::Beacon(b).encode(), Class::Control, FrameType::Beacon)
            }
            Cand::Carousel(Item::Symbol { object, block, esi, k }) => {
                let mut buf = alloc::vec![0u8; SYMBOL_SIZE];
                if !self.store.get_symbol(object, *block, *esi, &mut buf) {
                    if let Some(car) = self.carriers[i].carousel.as_mut() {
                        car.advance(&self.store, now);
                    }
                    return;
                }
                (Frame::Bulk(Bulk { object: *object, block: *block, esi: *esi, k: *k, payload: buf }).encode(), Class::Content, FrameType::Bulk)
            }
            Cand::Upload(object, block, esi, k) => {
                let mut buf = alloc::vec![0u8; SYMBOL_SIZE];
                if !self.store.get_symbol(object, *block, *esi, &mut buf) {
                    self.carriers[i].upload = None;
                    return;
                }
                (Frame::Bulk(Bulk { object: *object, block: *block, esi: *esi, k: *k, payload: buf }).encode(), Class::Content, FrameType::Bulk)
            }
        };
        let airtime = self.carriers[i].p.airtime_ms(bytes.len());
        let is_upload = matches!(cand, Cand::Upload(..));
        if !self.carriers[i].fatsoen.allows(class, is_upload) {
            self.carriers[i].pace_until = now + 1000;
            return;
        }
        // Random jitter before control/metadata frames (see Params::tx_jitter_ms). Content is
        // paced by the token bucket already; beacons at dwell starts must not drift.
        if class != Class::Content && frame_type != FrameType::Beacon && !self.carriers[i].jittered {
            let j = self.rng.below(self.cfg.params.tx_jitter_ms.max(1));
            if j > 0 {
                self.carriers[i].jittered = true;
                self.carriers[i].pace_until = now + j;
                return;
            }
        }
        let channel = match (&cand, self.carriers[i].upload.as_ref()) {
            (Cand::Upload(..), Some(u)) => self.channel_for(i, u.to, now),
            _ => self.channel(i, now),
        };
        let center = self.carriers[i].p.channels.get(channel as usize).copied().unwrap_or(0);
        let bw = self.carriers[i].p.bw_hz;
        if let Some(band) = self.carriers[i].p.band {
            match self.discipline.may_transmit(band, now, airtime, center, bw) {
                Verdict::Ok => {}
                Verdict::Wait(w) => {
                    self.carriers[i].pace_until = now + w.min(60_000);
                    self.stats.discipline_waits += 1;
                    return;
                }
                Verdict::Never => {
                    self.carriers[i].dropped += 1;
                    self.commit(i, cand);
                    return;
                }
            }
        }
        if class == Class::Content {
            let budget = self.budget_for(i);
            if let Err(w) = self.carriers[i].fatsoen.take_airtime(now, airtime, budget, is_upload) {
                self.carriers[i].pace_until = now + w;
                return;
            }
        }
        if cca_busy {
            self.carriers[i].fatsoen.cca_busy(now, &mut self.rng);
            self.stats.cca_deferrals += 1;
            return;
        }
        self.carriers[i].fatsoen.cca_clear();
        self.carriers[i].busy_until = now + airtime as Millis;
        if let Some(band) = self.carriers[i].p.band {
            self.discipline.record(band, now, airtime, center, bw);
        }
        self.stats.tx_frames[class as usize] += 1;
        self.stats.tx_by_type[frame_type as usize] += 1;
        self.stats.tx_airtime_ms[i] += airtime as u64;
        self.carriers[i].jittered = false;
        self.commit(i, cand);
        out.push(Action::Tx { carrier: i, channel, bytes, airtime_ms: airtime, class, frame_type });
    }

    fn commit(&mut self, i: usize, cand: Cand) {
        let now = self.now;
        match cand {
            Cand::Queue(j) => {
                self.carriers[i].queue.remove(j);
            }
            Cand::Carousel(_) => {
                if let Some(car) = self.carriers[i].carousel.as_mut() {
                    car.advance(&self.store, now);
                }
            }
            Cand::Upload(object, _, _, k) => {
                let Some(u) = self.carriers[i].upload.as_mut() else { return };
                match &mut u.list {
                    Some(list) => {
                        list.pop_front();
                        if list.is_empty() {
                            self.carriers[i].upload = None;
                        }
                    }
                    None => {
                        u.esi += 1;
                        if u.esi >= k {
                            u.esi = 0;
                            u.block += 1;
                            if self.store.block_k(&object, u.block).is_none() {
                                self.carriers[i].upload = None;
                            }
                        }
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------------- rx

    fn rx(&mut self, now: Millis, carrier: usize, bytes: &[u8], rssi: i16, out: &mut Vec<Action>) {
        let f = match Frame::decode(bytes) {
            Ok(f) => f,
            Err(_) => {
                self.now = now;
                self.stats.rx_bad += 1;
                return;
            }
        };
        self.rx_frame(now, carrier, &f, rssi, out);
    }

    /// Deliver an already-decoded frame (hosts that decode once for many receivers, such as
    /// the simulator, use this; radios use `Event::Rx`).
    pub fn handle_frame(&mut self, now: Millis, carrier: usize, f: &Frame, rssi: i16) -> Vec<Action> {
        let mut out = Vec::new();
        self.rx_frame(now, carrier, f, rssi, &mut out);
        out
    }

    fn rx_frame(&mut self, now: Millis, carrier: usize, f: &Frame, rssi: i16, out: &mut Vec<Action>) {
        self.now = now;
        self.stats.rx_frames += 1;
        if let Some(c) = self.carriers.get_mut(carrier) {
            let len = match f {
                Frame::Bulk(_) => 2 + 14 + SYMBOL_SIZE + 2,
                Frame::Beacon(_) => 28,
                other => other.encode().len(),
            };
            c.rx_air_ms += c.p.airtime_ms(len) as u64;
        }
        match f {
            Frame::Beacon(b) => self.rx_beacon(carrier, b, rssi, out),
            Frame::Bulk(b) => self.rx_bulk(carrier, b, out),
            Frame::Gossip(g) => self.rx_gossip(carrier, g, out),
            Frame::ManifestAnnounce(m) => self.rx_announce(m),
            Frame::Nack(n) => self.rx_nack(n),
        }
    }

    fn touch(&mut self, id: NodeId) -> &mut Neighbor {
        let now = self.now;
        let n = self.neighbors.entry(id).or_insert(Neighbor { last_heard: now, announcer: NodeId::NONE, score: 0, haves: BTreeSet::new() });
        n.last_heard = now;
        n
    }

    fn rx_beacon(&mut self, carrier: usize, b: &Beacon, rssi: i16, out: &mut Vec<Action>) {
        if b.announcer == self.cfg.id {
            return;
        }
        let Some(ti) = self.carriers.iter().position(|c| c.p.kind == b.carrier) else { return };
        if carrier != ti {
            // A beacon relayed on another carrier says nothing about whether we can receive
            // this announcer's carousel. Cells are defined by the bulk carrier.
            return;
        }
        {
            let n = self.touch(b.announcer);
            n.score = b.score;
            n.announcer = b.announcer;
        }
        let me = self.cfg.id;
        let score = self.score;
        let now = self.now;
        // Lonely: nobody but this other announcer has been heard on any carrier, so there is
        // no follower to orphan by yielding.
        let lonely = self.neighbors.len() <= 1;
        let t = self.carriers[ti].election.as_mut().and_then(|e| e.on_beacon(now, b.announcer, b.score, b.next_ms, rssi, lonely, me, score));
        if let Some(t) = t {
            self.on_transition(ti, t, out);
        }
    }

    fn rx_bulk(&mut self, _carrier: usize, b: &Bulk, out: &mut Vec<Action>) {
        // Someone else is sending this object: a pending upload of ours is redundant.
        for c in self.carriers.iter_mut() {
            if let Some(u) = &c.upload {
                if u.object == b.object && u.start_at > self.now {
                    c.upload = None;
                }
            }
        }
        let interested = self.wants.contains(&b.object) || self.store.is_known(&b.object) || self.is_announcing();
        if !interested {
            return;
        }
        match self.store.put_symbol(b.object, b.block, b.esi, b.k, &b.payload) {
            Put::New => {
                self.stats.symbols_new += 1;
                let now = self.now;
                self.progress.entry(b.object).or_default().last_progress = now;
            }
            Put::Complete => {
                self.stats.symbols_new += 1;
                self.on_complete(b.object, out);
            }
            Put::Duplicate => self.stats.symbols_dup += 1,
            Put::Rejected => self.stats.symbols_rejected += 1,
        }
    }

    fn on_complete(&mut self, id: ShortId, out: &mut Vec<Action>) {
        out.push(Action::ObjectComplete { id, now: self.now });
        self.wants.remove(&id);
        self.progress.remove(&id);
        let is_manifest = self.store.entry(&id).map(|e| e.mime() == Mime::Manifest).unwrap_or(false);
        if is_manifest {
            if let Some(bytes) = self.store.bytes(&id).map(|b| b.to_vec()) {
                if let Ok(m) = Manifest::decode(&bytes) {
                    self.adopt_manifest(&m, id);
                }
            }
        }
        if self.is_announcing() {
            // Tell sources we have it, and ask for what we still lack, soon.
            self.gossip_soon();
        }
    }

    fn adopt_manifest(&mut self, m: &Manifest, short: ShortId) {
        let chan = m.channel_id();
        if let Some(cur) = self.manifests.get(&chan) {
            if cur.seq > m.seq || (cur.seq == m.seq && cur.adopted && cur.short != short) {
                return;
            }
        }
        let len = self.store.entry(&short).and_then(|e| e.len()).unwrap_or(0);
        let old = self.manifests.insert(chan, ManifestInfo { seq: m.seq, short, len, adopted: true });
        if old.map(|o| o.adopted && o.short == short).unwrap_or(false) {
            // Re-adoption (e.g. follow() after the fact): only the wants below matter.
        } else {
            self.stats.manifests_adopted += 1;
        }
        for c in self.carriers.iter_mut() {
            if let Some(car) = c.carousel.as_mut() {
                if let Some(o) = old {
                    if o.short != short {
                        car.unset_always(&o.short);
                    }
                }
                car.set_always(short);
            }
        }
        let interested = self.follows.contains(&chan) || self.is_announcing();
        let mut to_want = Vec::new();
        for o in &m.objects {
            self.store.ensure(o.meta());
            if interested && !self.store.has_complete(&o.id.short()) {
                to_want.push(o.id.short());
            }
        }
        for id in to_want {
            self.add_want(id);
        }
        if self.follows.contains(&chan) {
            self.want_refresh = true;
        }
    }

    fn rx_gossip(&mut self, _carrier: usize, g: &Gossip, out: &mut Vec<Action>) {
        if g.node == self.cfg.id {
            return;
        }
        {
            let n = self.touch(g.node);
            n.announcer = g.announcer;
            for h in &g.have {
                if n.haves.len() < 512 {
                    n.haves.insert(*h);
                }
            }
        }
        let me = self.cfg.id;
        let score = self.score;
        let now = self.now;
        let other_score = self.neighbors.get(&g.announcer).map(|n| n.score);
        for i in 0..self.carriers.len() {
            if self.role(i) != Role::Announcer {
                continue;
            }
            let mut new_wants = Vec::new();
            for w in &g.want {
                if let Some(car) = self.carriers[i].carousel.as_mut() {
                    car.on_want(*w, g.node, now);
                }
                if !self.store.has_complete(w) && self.store.is_known(w) {
                    new_wants.push(*w);
                }
            }
            for w in new_wants {
                self.add_want(w);
            }
            for h in &g.have {
                if let Some(car) = self.carriers[i].carousel.as_mut() {
                    car.on_have(*h, g.node);
                }
            }
            if !g.announcer.is_none() && g.announcer != me {
                let t = self.carriers[i].election.as_mut().and_then(|e| e.on_conflict(now, g.announcer, other_score, me, score));
                if let Some(t) = t {
                    self.on_transition(i, t, out);
                }
            }
        }
        // Holder side: an announcer (ours or a neighbouring cell's) tells us what it has and
        // wants. Any holder may answer; a random wait plus suppression keeps it to one uploader.
        let from_announcer = g.announcer == g.node;
        for i in 0..self.carriers.len() {
            if !self.carriers[i].p.kind.is_bulk() || self.role(i) == Role::Announcer {
                continue;
            }
            let own = self.announcer_of(i) == g.node;
            if own {
                for h in &g.have {
                    self.pending_ack.remove(h);
                }
            } else if !from_announcer {
                continue;
            }
            if self.carriers[i].upload.is_none() {
                for w in &g.want {
                    if self.store.has_complete(w) {
                        let max = if own { self.cfg.params.upload_suppress_ms / 4 } else { self.cfg.params.upload_suppress_ms };
                        let start_at = now + self.rng.below(max.max(1));
                        self.carriers[i].upload = Some(Upload { object: *w, to: g.node, start_at, block: 0, esi: 0, list: None });
                        self.stats.uploads_started += 1;
                        break;
                    }
                }
            }
        }
    }

    fn rx_announce(&mut self, m: &ManifestAnnounce) {
        if m.node == self.cfg.id {
            return;
        }
        self.touch(m.node);
        let announcing = self.is_announcing();
        for e in &m.entries {
            let interested = self.follows.contains(&e.channel) || announcing;
            if !interested {
                continue;
            }
            if let Some(cur) = self.manifests.get(&e.channel) {
                if cur.seq >= e.seq {
                    continue;
                }
            }
            self.manifests.insert(e.channel, ManifestInfo { seq: e.seq, short: e.manifest, len: e.len, adopted: false });
            self.store.ensure_hint(e.manifest, e.len, Mime::Manifest);
            if self.store.has_complete(&e.manifest) {
                if let Some(bytes) = self.store.bytes(&e.manifest).map(|b| b.to_vec()) {
                    if let Ok(man) = Manifest::decode(&bytes) {
                        self.adopt_manifest(&man, e.manifest);
                    }
                }
            } else {
                self.add_want(e.manifest);
                self.want_refresh = true;
            }
        }
    }

    fn rx_nack(&mut self, n: &Nack) {
        if n.node == self.cfg.id {
            return;
        }
        self.touch(n.node);
        if !self.store.has_complete(&n.object) {
            return;
        }
        for i in 0..self.carriers.len() {
            if !self.carriers[i].p.kind.is_bulk() {
                continue;
            }
            if self.role(i) == Role::Announcer {
                if let Some(car) = self.carriers[i].carousel.as_mut() {
                    car.on_nack(n.object, n.block, &n.missing, &self.store);
                }
            } else if self.announcer_of(i) == n.node || self.neighbors.get(&n.node).map(|nb| nb.announcer == n.node).unwrap_or(false) {
                // An announcer is missing pieces of something we have: send exactly those.
                let Some(k) = self.store.block_k(&n.object, n.block) else { continue };
                let mut list: VecDeque<(u16, u16)> = VecDeque::new();
                for &(start, count) in &n.missing {
                    for esi in start..start.saturating_add(count).min(k) {
                        list.push_back((n.block, esi));
                    }
                }
                if list.is_empty() {
                    continue;
                }
                match self.carriers[i].upload.as_mut() {
                    Some(u) if u.object == n.object && u.list.is_some() => {
                        u.list.as_mut().unwrap().extend(list);
                    }
                    Some(u) if u.object == n.object => {
                        // A full pass is in progress; it will cover these.
                    }
                    _ => {
                        let start_at = self.now + self.rng.below((self.cfg.params.upload_suppress_ms / 4).max(1));
                        self.carriers[i].upload = Some(Upload { object: n.object, to: n.node, start_at, block: n.block, esi: 0, list: Some(list) });
                        self.stats.uploads_started += 1;
                    }
                }
            }
        }
    }
}

/// Announcer id used for the common meeting-dwell sequence.
pub const MEETING_ID: NodeId = NodeId(0xFFFF_FFFF);

/// Pseudo-random hop sequence per announcer (splitmix64 of announcer id and dwell index).
/// Two announcers' sequences coincide on about one dwell in `n`, which is how they discover
/// each other; followers and uploaders compute the same sequence for their announcer.
pub fn hop_channel(announcer: NodeId, dwell_index: u64, n: u64) -> u8 {
    let mut z = (announcer.0 as u64) << 32 ^ dwell_index ^ 0x9E37_79B9_7F4A_7C15;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z % n.max(1)) as u8
}

fn compress_ranges(missing: &[u16]) -> Vec<(u16, u16)> {
    let mut out: Vec<(u16, u16)> = Vec::new();
    for &m in missing {
        match out.last_mut() {
            Some((start, count)) if *start + *count == m => *count += 1,
            _ => {
                if out.len() >= MAX_NACK_RANGES {
                    break;
                }
                out.push((m, 1));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges() {
        assert_eq!(compress_ranges(&[0, 1, 2, 5, 7, 8]), alloc::vec![(0, 3), (5, 1), (7, 2)]);
    }
}
