//! The MeshCast node: one event-driven state machine that ties objects, manifests, frames,
//! carousel, election, EtherFatsoen and EtherDiscipline together. No I/O: the host (firmware,
//! station or simulator) feeds [`Event`]s and executes [`Action`]s.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;

use crate::carousel::{Carousel, CarouselParams, Item};
use crate::discipline::{Accounting, Verdict};
use crate::election::{Election, Transition};
use crate::fatsoen::Fatsoen;
use crate::frame::{AnnounceEntry, Beacon, Bulk, CarrierKind, Class, Frame, FrameType, Gossip, ManifestAnnounce, Nack, MAX_ANNOUNCE_ENTRIES, MAX_GOSSIP_IDS, MAX_NACK_RANGES, MAX_WANT, SYMBOL_SIZE};
use crate::ids::{ChannelId, NodeId, ShortId};
use crate::manifest::Manifest;
use crate::object::{ContentType, ObjectMeta};
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
    /// Uploads that put at least one symbol on the air.
    pub uploads_started: u64,
    /// Repair answers lined up; most are cancelled by hearing another holder answer first.
    pub repairs_queued: u64,
    pub manifests_adopted: u64,
    pub conflict_reports_sent: u64,
    pub conflicts_noted: u64,
    /// New symbols that arrived for an object we had asked for and had an uploader assigned to,
    /// against ones that simply came past. The split says whether a node is being served or is
    /// living off what it overhears.
    pub symbols_served: u64,
    pub symbols_overheard: u64,
    /// Why a ready content frame was not sent, in milliseconds of deferral:
    /// [meeting dwell, announcer slot, regulatory, token bucket, class gate, CCA].
    pub defer_ms: [u64; 6],
    /// Bulk frames received for objects we do not want, know or serve: airtime spent on us for nothing.
    pub bulk_uninterested: u64,
}

#[derive(Clone, Debug)]
struct Neighbor {
    last_heard: Millis,
    announcer: NodeId,
    score: u16,
    /// How well we hear this node (averaged RSSI of its frames).
    rssi: i16,
    /// Colour last announced (announcers only).
    colour: u8,
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
    /// Whether any symbol of it has reached the air.
    started: bool,
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

impl CarrierRt {
    /// Start an upload, or line it up behind the one in progress. Urgent work (a repair) goes to
    /// the head of the queue. One place decides, so an upload can never be queued behind nothing.
    fn add_upload(&mut self, u: Upload, urgent: bool) {
        match (&self.upload, urgent) {
            (None, _) => self.upload = Some(u),
            (Some(_), true) => self.upload_queue.push_front(u),
            (Some(_), false) => self.upload_queue.push_back(u),
        }
    }

    /// Drop everything we have lined up for `object` that has not begun. An upload that has
    /// started is left alone: stopping halfway would waste what it already sent.
    fn cancel_pending(&mut self, object: ShortId, now: Millis) {
        if self.upload.as_ref().map(|u| u.object == object && u.start_at > now).unwrap_or(false) {
            self.upload = self.upload_queue.pop_front();
        }
        self.upload_queue.retain(|u| !(u.object == object && u.start_at > now));
    }
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
    /// Further granted uploads, served one at a time after the active one.
    upload_queue: VecDeque<Upload>,
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
    /// Earliest time for the next WANT frame; starts at a random phase so that followers that
    /// booted together do not all ask at once.
    next_want_at: Millis,
    have_cursor: usize,
    announce_cursor: usize,
    progress: BTreeMap<ShortId, Progress>,
    /// Announcers reported to be heard together with us by some follower (or heard by us):
    /// our conflict set, with the time of the last report and the colour they announced.
    conflicts: BTreeMap<NodeId, (Millis, u8, u8)>,
    /// A follower that just heard a new announcer reports the conflict once, soon.
    report_due: bool,
    last_report: Millis,
    /// Announcer side: uploader granted per wanted object, with the time of the grant.
    grants: BTreeMap<ShortId, (NodeId, Millis)>,
    /// Holder side: grants we received, with the time, so that we still answer the announcer's
    /// NACKs for a while after our full pass is done.
    granted_to_us: BTreeMap<(ShortId, NodeId), Millis>,
    /// Holder side: offers we owe (object, announcer that asked, when to send).
    offers: Vec<(ShortId, NodeId, Millis)>,
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
                election: if bulk { Some(Election::new(cfg.params.election, now)) } else { None },
                carousel: if bulk { Some(Carousel::new(cp)) } else { None },
                fatsoen: Fatsoen::new(cfg.params.fatsoen, now),
                queue: Vec::new(),
                busy_until: 0,
                pace_until: 0,
                upload: None,
                upload_queue: VecDeque::new(),
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
            next_want_at: now + Rng::new(seed ^ 0x5eed).below(p.t_want_min_ms.max(1)),
            have_cursor: 0,
            announce_cursor: 0,
            progress: BTreeMap::new(),
            conflicts: BTreeMap::new(),
            report_due: false,
            last_report: 0,
            grants: BTreeMap::new(),
            granted_to_us: BTreeMap::new(),
            offers: Vec::new(),
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

    /// Channel of announcer `ann` at `now`. Outside the meeting dwell every announcer follows
    /// one shared pseudo-random base sequence shifted by its colour, so announcers in conflict
    /// (different colours) are never on the same channel. A node following nobody scans slowly.
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
        let colour = if ann == self.cfg.id { self.my_colour() } else { c.election.as_ref().and_then(|e| e.colour_of(ann)).unwrap_or(0) };
        ((hop_channel(BASE_ID, di, n) as u64 + colour as u64) % n) as u8
    }

    /// A colour maps to a channel (`colour mod n`) and, when there are more colours than
    /// channels, to a time slot on that channel (`colour div n`). Single-channel carriers are
    /// simply `n = 1`: every colour is a slot.
    fn slot_of(&self, carrier: usize, colour: u8, colours: u8) -> (u64, u64) {
        let n = self.carriers[carrier].p.channels.len().max(1) as u64;
        let slots = (colours.max(1) as u64).div_ceil(n).max(1);
        (colour as u64 / n, slots)
    }

    /// Our colour: greedy distributed colouring. Lower ids keep their colour; we take the
    /// smallest colour not announced by any conflicting announcer with a lower id.
    fn my_colour(&self) -> u8 {
        let taken: Vec<u8> = self.conflicts.iter().filter(|(id, _)| id.0 < self.cfg.id.0).map(|(_, (_, c, _))| *c).collect();
        (0u8..=254).find(|c| !taken.contains(c)).unwrap_or(255)
    }

    /// Length of the slot cycle: one more than the highest colour we know of, but never less
    /// than what any announcer we conflict with believes. Announcers that take turns must agree
    /// on the length of the cycle or their turns overlap, and each of them sees a different part
    /// of the conflict graph, so the larger count is passed on until they agree.
    fn colours(&self) -> u8 {
        let mut k = self.my_colour().saturating_add(1);
        for (_, (_, colour, colours)) in self.conflicts.iter() {
            k = k.max(colour.saturating_add(1)).max(*colours);
        }
        k
    }

    fn note_conflict(&mut self, other: NodeId, colour: u8, colours: u8) {
        if other != self.cfg.id && !other.is_none() {
            self.conflicts.insert(other, (self.now, colour, colours));
            self.stats.conflicts_noted += 1;
        }
    }

    fn expire_conflicts(&mut self) {
        let from = self.now.saturating_sub(self.cfg.params.conflict_ttl_ms);
        self.conflicts.retain(|_, (t, _, _)| *t >= from);
    }

    /// Announcers that share a channel take turns: our carousel runs only in our slot.
    /// Returns when the next slot of ours starts, or None if it is ours now.
    ///
    /// Taking turns is for carriers where nothing else bounds what everyone adds up to. Where
    /// the regulator already caps every transmitter (a duty cycle, or polite access with its
    /// cumulative limit), that cap is the bound, and adding turns on top only buys idle time:
    /// in a town on one 250 kHz channel it cost five announcers eight ninths of their airtime.
    fn slot_wait(&self, carrier: usize, colour: u8, colours: u8, now: Millis) -> Option<Millis> {
        if let Some(b) = self.carriers[carrier].p.band {
            if matches!(self.discipline.rule(b), Access::DutyCycle { .. } | Access::Polite { .. }) {
                return None;
            }
        }
        let (mine, k) = self.slot_of(carrier, colour, colours);
        self.turn_wait(mine, k, now)
    }

    /// Wait for turn `mine` of `k` in a cycle of `k × T_slot`; None if it is our turn now.
    fn turn_wait(&self, mine: u64, k: u64, now: Millis) -> Option<Millis> {
        if k <= 1 {
            return None;
        }
        let slot = self.cfg.params.t_slot_ms.max(1);
        let cur = (now / slot) % k;
        if cur == mine {
            None
        } else {
            let cycle_start = (now / (slot * k)) * slot * k;
            let mut t = cycle_start + mine * slot;
            if t <= now {
                t += slot * k;
            }
            Some(t)
        }
    }

    /// Whether this carrier hops: more than one channel means announcers can be separated in
    /// frequency, and that every cell needs a common moment to hear the others.
    fn hops(&self, carrier: usize) -> bool {
        self.carriers.get(carrier).map(|c| c.p.channels.len() > 1).unwrap_or(false)
    }

    fn is_meeting_dwell(&self, dwell_index: u64) -> bool {
        let m = self.cfg.params.meet_every.max(1);
        dwell_index % m == 0
    }

    /// Whether `now` falls in this carrier's rendezvous: the dwell on the common sequence, where
    /// every cell is on one channel and can hear every other. A carrier that does not hop has no
    /// rendezvous, because its cells never leave each other's channel in the first place.
    fn in_rendezvous(&self, carrier: usize, now: Millis) -> bool {
        self.hops(carrier) && self.is_meeting_dwell(now / self.cfg.params.dwell_ms.max(1))
    }

    /// Start of the next meeting dwell strictly after `now`.
    fn next_meeting_start(&self, now: Millis) -> Millis {
        let dwell = self.cfg.params.dwell_ms.max(1);
        let m = self.cfg.params.meet_every.max(1);
        let di = now / dwell + 1;
        let next = di.div_ceil(m) * m;
        next * dwell
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

    /// What this node holds: objects it knows of, objects it holds complete, and for the rest
    /// how many symbols each still lacks as a fraction of the object in tenths of a per cent.
    /// An announcer that is chronically a hair short of many objects is an announcer that is
    /// filling up by overhearing rather than by being served.
    pub fn inventory(&self) -> (usize, usize, Vec<u16>) {
        let mut known = 0;
        let mut complete = 0;
        let mut short = Vec::new();
        for id in self.store.ids() {
            let Some(e) = self.store.entry(id) else { continue };
            known += 1;
            if e.is_complete() {
                complete += 1;
                continue;
            }
            if let (have, Some(total)) = e.progress() {
                if total > 0 {
                    let missing = total.saturating_sub(have);
                    short.push(((missing as u64 * 1000) / total as u64).min(1000) as u16);
                }
            }
        }
        (known, complete, short)
    }

    /// Our colour and the size of our conflict set (ourselves included).
    pub fn colouring(&self) -> (u8, u8) {
        (self.my_colour(), self.colours())
    }

    /// Conflict set as (id, colour) for diagnostics.
    pub fn conflict_set(&self) -> Vec<(u32, u8)> {
        self.conflicts.iter().map(|(id, (_, c, _))| (id.0, *c)).collect()
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

    /// Stop following a channel: its objects are no longer wanted (unless another followed or
    /// served manifest references them) and its manifest is no longer announced by us.
    pub fn unfollow(&mut self, chan: ChannelId) {
        self.follows.remove(&chan);
        self.prune_wants();
    }

    /// Objects referenced by the latest manifest of a channel we are interested in.
    fn interesting_objects(&self) -> BTreeSet<ShortId> {
        let mut set = BTreeSet::new();
        let announcing = self.is_announcing();
        for (chan, info) in &self.manifests {
            if !(announcing || self.follows.contains(chan)) {
                continue;
            }
            set.insert(info.short);
            if let Some(bytes) = self.store.bytes(&info.short) {
                if let Ok(m) = Manifest::decode(bytes) {
                    for o in &m.objects {
                        set.insert(o.id.short());
                    }
                }
            }
        }
        set
    }

    /// Drop wants for objects that no manifest of interest references any more (unfollowed
    /// channels, or objects that left a channel's manifest).
    fn prune_wants(&mut self) {
        let keep = self.interesting_objects();
        let stale: Vec<ShortId> = self.wants.iter().filter(|id| !keep.contains(id)).copied().collect();
        for id in stale {
            self.wants.remove(&id);
            self.progress.remove(&id);
            self.grants.remove(&id);
        }
        self.offers.retain(|(o, _, _)| keep.contains(o));
        for c in self.carriers.iter_mut() {
            c.upload_queue.retain(|u| keep.contains(&u.object));
        }
    }

    /// Forget objects no manifest of interest references any more (a channel we unfollowed, or
    /// an object that left its channel's window). Own objects are kept.
    fn evict_orphans(&mut self) {
        let keep = self.interesting_objects();
        let gone: Vec<ShortId> = self.store.ids().filter(|id| !keep.contains(id) && !self.own_objects.contains(id) && !self.own_manifests.iter().any(|(_, s, _, _)| s == *id)).copied().collect();
        for id in gone {
            self.store.remove(&id);
            self.wants.remove(&id);
            self.progress.remove(&id);
        }
    }

    /// Objects we hold complete that are not referenced by any manifest we know.
    pub fn orphaned_objects(&self) -> Vec<ShortId> {
        let mut keep = BTreeSet::new();
        for info in self.manifests.values() {
            keep.insert(info.short);
            if let Some(bytes) = self.store.bytes(&info.short) {
                if let Ok(m) = Manifest::decode(bytes) {
                    for o in &m.objects {
                        keep.insert(o.id.short());
                    }
                }
            }
        }
        self.store.complete_ids().filter(|id| !keep.contains(id)).copied().collect()
    }

    /// Publish a channel manifest and the objects it references (we own them, complete).
    pub fn publish(&mut self, manifest: &Manifest, objects: &[(ObjectMeta, Option<&[u8]>)]) {
        let (meta, bytes) = manifest.as_object();
        self.store.insert_complete(meta, Some(&bytes));
        let short = meta.id.short();
        let chan = manifest.channel_id();
        let old = self.manifests.insert(chan, ManifestInfo { seq: manifest.seq, short, len: meta.len, adopted: true });
        self.own_manifests.retain(|(c, _, _, _)| *c != chan);
        self.own_manifests.push((chan, short, manifest.seq, meta.len));
        if let Some(o) = old {
            self.pending_ack.remove(&o.short);
        }
        self.pending_ack.insert(short);
        for (m, b) in objects {
            if !self.store.has_complete(&m.id.short()) {
                self.store.insert_complete(*m, *b);
            }
            self.own_objects.insert(m.id.short());
            self.pending_ack.insert(m.id.short());
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
        self.prune_wants();
        self.gossip_soon();
    }

    /// Wipe everything a node learned: a freshly flashed dongle, or the same hardware handed to
    /// someone else. Keeps only its identity and its region profile.
    pub fn factory_reset(&mut self, now: Millis) {
        self.reboot(now);
        self.store = MemStore::new(self.cfg.keep_bytes_below);
        self.follows.clear();
        self.manifests.clear();
        self.own_manifests.clear();
        self.own_objects.clear();
        self.pending_ack.clear();
        self.wants.clear();
        self.grants.clear();
        self.granted_to_us.clear();
        self.offers.clear();
        self.stats = Stats { tx_airtime_ms: alloc::vec![0; self.carriers.len()], ..Default::default() };
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
            if self.is_announcing() && self.hops(self.cell_carrier()) {
                d = d.min(self.next_meeting_start(self.now) + 1);
            }
        }
        if let Some(t) = self.offers.iter().map(|(_, _, at)| *at).min() {
            d = d.min(t);
        }
        if !self.is_announcing() {
            d = d.min(self.last_report + self.cfg.params.conflict_ttl_ms / 2);
            if self.report_due {
                d = d.min((self.last_report + self.cfg.params.t_report_min_ms).max(self.now + 1));
            }
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
                    // A hopping announcer beacons at every dwell start.
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
            let p = self.progress.entry(id).or_default();
            p.last_progress = 0;
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
            self.expire_conflicts();
            self.evict_orphans();
            let ttl = self.cfg.params.neighbor_ttl_ms;
            self.granted_to_us.retain(|_, t| *t + ttl >= now);
            // Announcers we hear directly are in conflict with us too.
            let heard: Vec<(NodeId, u8, u8)> = self.carriers.iter().filter_map(|c| c.election.as_ref()).flat_map(|e| e.heard_with_colour(now, self.cfg.id).collect::<Vec<_>>()).collect();
            if self.is_announcing() {
                for (id, colour, colours) in heard {
                    self.note_conflict(id, colour, colours);
                }
            }
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
            if self.hops(i) && self.role(i) == Role::Announcer {
                let d = now / self.cfg.params.dwell_ms.max(1);
                if self.carriers[i].last_dwell != d {
                    self.carriers[i].last_dwell = d;
                    if !self.carriers[i].queue.iter().any(|p| p.frame_type == FrameType::Beacon) {
                        let b = self.make_beacon(i);
                        self.enqueue(i, Frame::Beacon(b));
                        // Announcers cannot hear each other, so spread their dwell-start
                        // beacons over the first part of the dwell.
                        let j = self.rng.below((self.cfg.params.dwell_ms / 10).max(1));
                        let c = &mut self.carriers[i];
                        c.pace_until = c.pace_until.max(now + j);
                    }
                }
            }
        }
        if now >= self.next_gossip {
            let cell = self.cell_carrier();
            if self.hops(cell) && !self.in_rendezvous(cell, now) {
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
        self.conflict_report_check();
        self.offer_check();
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
            colour: self.my_colour(),
            colours: self.colours(),
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
                c.election = Some(Election::new(p.election, now));
                c.carousel = Some(Carousel::new(cp));
            }
            c.fatsoen = Fatsoen::new(p.fatsoen, now);
            c.queue.clear();
            c.busy_until = 0;
            c.pace_until = 0;
            c.upload = None;
            c.upload_queue.clear();
        }
        self.neighbors.clear();
        self.next_beacon = now + p.election.t_beacon_ms;
        self.next_gossip = now + p.t_gossip_ms;
        self.next_score = now;
        self.want_refresh = true;
        self.last_want_tx = 0;
        self.next_want_at = now + self.rng.below(p.t_want_min_ms.max(1));
        self.progress.clear();
        self.conflicts.clear();
        self.grants.clear();
        self.granted_to_us.clear();
        self.offers.clear();
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

    /// Colouring (colour, colour count) of the announcer we follow (or our own).
    fn announcer_colouring_field(&self) -> (u8, u8) {
        let Some(i) = self.primary_bulk() else { return (0, 1) };
        let ann = self.announcer_of(i);
        if ann == self.cfg.id {
            (self.my_colour(), self.colours())
        } else {
            self.carriers[i].election.as_ref().and_then(|e| e.colouring_of(ann)).unwrap_or((0, 1))
        }
    }

    /// Other announcers we hear on the cell carrier besides the one we follow (or besides
    /// ourselves): the conflict report.
    fn heard_field(&self) -> Vec<(NodeId, u8, u8)> {
        let Some(i) = self.primary_bulk() else { return Vec::new() };
        let except = self.announcer_of(i);
        match self.carriers[i].election.as_ref() {
            Some(e) => e.heard_with_colour(self.now, except).filter(|(id, _, _)| *id != self.cfg.id).take(crate::frame::MAX_HEARD).collect(),
            None => Vec::new(),
        }
    }

    /// A follower that hears more than one announcer is the only node that knows they
    /// conflict, so it says so: once when it first hears a new one, and again every half
    /// conflict lifetime while the situation lasts, so that the announcers' colouring persists.
    fn conflict_report_check(&mut self) {
        if self.is_announcing() {
            return;
        }
        let refresh = self.cfg.params.conflict_ttl_ms / 2;
        let due = self.report_due || self.now >= self.last_report + refresh;
        if !due || self.now < self.last_report + self.cfg.params.t_report_min_ms {
            return;
        }
        let heard = self.heard_field();
        if heard.is_empty() {
            self.report_due = false;
            self.last_report = self.now;
            return;
        }
        let g = Gossip { node: self.cfg.id, announcer: self.gossip_announcer_field(), announcer_colour: self.announcer_colouring_field().0, announcer_colours: self.announcer_colouring_field().1, heard, have: Vec::new(), want: Vec::new() };
        let cell = self.cell_carrier();
        self.enqueue(cell, Frame::Gossip(g));
        self.stats.conflict_reports_sent += 1;
        self.report_due = false;
        self.last_report = self.now;
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
            let g = Gossip { node: self.cfg.id, announcer: self.gossip_announcer_field(), announcer_colour: self.announcer_colouring_field().0, announcer_colours: self.announcer_colouring_field().1, heard: self.heard_field(), have, want };
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

    /// Ask only for what is not coming: objects with a symbol in the last stall interval are
    /// flowing and are left out. Each entry carries the granted uploader, if any.
    fn take_wants(&mut self) -> Vec<(ShortId, NodeId)> {
        let now = self.now;
        let stall = self.cfg.params.t_nack_stall_ms;
        let t_grant = self.cfg.params.t_grant_ms;
        // A granted uploader that has not delivered anything loses the grant.
        let progress = &self.progress;
        self.grants.retain(|id, (_, t)| {
            let p = progress.get(id).copied().unwrap_or_default();
            now < *t + t_grant || p.last_progress > *t
        });
        let thr = self.cfg.params.nack_threshold_permille as u64;
        let ids: Vec<ShortId> = self
            .wants
            .iter()
            .filter(|id| {
                let p = self.progress.get(id).copied().unwrap_or_default();
                if !(now >= p.last_progress + stall || p.last_progress == 0) {
                    return false;
                }
                // An object that is nearly complete is repaired by NACK rather than re-asked
                // in full, but only once someone is responsible for it. The want list is where
                // responsibility is assigned, and an object we collected by overhearing a
                // neighbouring cell has no uploader at all: however complete it is, it belongs
                // on the list until it has one.
                if !self.grants.contains_key(id) {
                    return true;
                }
                match self.store.entry(id).map(|e| e.progress()) {
                    Some((have, Some(total))) => (have as u64) * 1000 < thr * total as u64,
                    _ => true,
                }
            })
            .copied()
            .collect();
        if ids.is_empty() {
            return Vec::new();
        }
        // One object per granted holder at a time: a holder whose grant is flowing is busy, so
        // other objects granted to it are not asked for now.
        let busy_holders: Vec<NodeId> = self
            .grants
            .iter()
            .filter(|(id, _)| {
                let p = self.progress.get(id).copied().unwrap_or_default();
                p.last_progress != 0 && now < p.last_progress + stall
            })
            .map(|(_, (h, _))| *h)
            .collect();
        let ids: Vec<ShortId> = ids.into_iter().filter(|id| !self.grants.get(id).map(|(h, _)| busy_holders.contains(h)).unwrap_or(false)).collect();
        if ids.is_empty() {
            return Vec::new();
        }
        let n = ids.len().min(MAX_WANT);
        let v: Vec<ShortId> = ids.into_iter().take(n).collect();
        for id in &v {
            self.progress.entry(*id).or_default().last_want = now;
        }
        v.into_iter().map(|id| (id, self.grants.get(&id).map(|(g, _)| *g).unwrap_or(NodeId::NONE))).collect()
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
        if (self.want_refresh || stalled) && now >= self.next_want_at {
            let want = self.take_wants();
            let g = Gossip { node: self.cfg.id, announcer: self.gossip_announcer_field(), announcer_colour: self.announcer_colouring_field().0, announcer_colours: self.announcer_colouring_field().1, heard: self.heard_field(), have: Vec::new(), want };
            let cell = self.cell_carrier();
            self.enqueue(cell, Frame::Gossip(g));
            self.stats.wants_sent += 1;
            self.want_refresh = false;
            self.last_want_tx = now;
            self.next_want_at = now + self.cfg.params.t_want_min_ms;
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
        // An announcer's uploader may sit in another cell, on another hop sequence: on agile
        // carriers the announcer's NACKs go out in the meeting dwell, like its gossip.
        let cell = self.cell_carrier();
        if announcing && self.hops(cell) && !self.in_rendezvous(cell, self.now) {
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

    /// Share of airtime the bulk carrier may pace itself to. Two ceilings, the lower wins:
    /// the regulatory budget minus a reserve for control frames on the same band, and a fair
    /// share of the channel's occupancy target among the announcers that share our airtime.
    ///
    /// After colouring, a conflicting announcer has a different colour, so it sits on another
    /// channel or in another slot and does not share our airtime at all; only announcers that
    /// colouring could not separate from us (same colour) divide the target. Dividing by every
    /// announcer we hear, as an earlier version did, throttled well-separated announcers to a
    /// fraction of a budget they were entitled to. What colouring has not yet covered is caught
    /// by the reactive side of EtherFatsoen: the AIMD rate still halves on measured occupancy.
    fn budget_for(&self, carrier: usize) -> u16 {
        let c = &self.carriers[carrier];
        let reserve = 1000u32.saturating_sub(self.cfg.params.control_reserve as u32);
        let regulatory = match c.p.band {
            Some(b) => match self.discipline.rule(b) {
                Access::DutyCycle { .. } | Access::Polite { .. } => {
                    let budget = self.discipline.budget_permille(b, c.p.channels.len() as u16) as u32;
                    (budget * reserve / 1000) as u16
                }
                _ => self.cfg.params.fatsoen.max_own_share,
            },
            None => self.cfg.params.fatsoen.max_own_share,
        };
        let mine = self.my_colour();
        let sharing = self.conflicts.values().filter(|(_, colour, _)| *colour == mine).count() as u32;
        let fair = self.cfg.params.fatsoen.occ_high_own as u32 / (sharing + 1);
        regulatory.min(fair as u16).max(1)
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
            } else if let Some(u) = {
                if c.upload.is_none() {
                    c.upload = c.upload_queue.pop_front();
                }
                c.upload.as_mut()
            } {
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
                if let Some(u) = self.carriers[i].upload.as_mut() {
                    if !u.started {
                        u.started = true;
                        self.stats.uploads_started += 1;
                    }
                }
                (Frame::Bulk(Bulk { object: *object, block: *block, esi: *esi, k: *k, payload: buf }).encode(), Class::Content, FrameType::Bulk)
            }
        };
        let airtime = self.carriers[i].p.airtime_ms(bytes.len());
        // Content travels in its announcer's slot: our own colouring for the carousel, the
        // target announcer's (from its beacons) for an upload.
        let content_colour = match (&cand, self.carriers[i].upload.as_ref()) {
            (Cand::Carousel(Item::Symbol { .. }), _) => Some((self.my_colour(), self.colours())),
            (Cand::Upload(..), Some(u)) => Some(self.carriers[i].election.as_ref().and_then(|e| e.colouring_of(u.to)).unwrap_or((0, 1))),
            _ => None,
        };
        if let Some((colour, colours)) = content_colour {
            // The meeting dwell is control plane only, and announcers that share a channel take
            // turns: content (carousel or upload) runs only in its announcer's slot.
            let dwell = self.cfg.params.dwell_ms.max(1);
            if self.in_rendezvous(i, now) {
                let t = (now / dwell + 1) * dwell;
                self.stats.defer_ms[0] += t - now;
                self.carriers[i].pace_until = t;
                return;
            }
            if let Some(t) = self.slot_wait(i, colour, colours, now) {
                self.stats.defer_ms[1] += t.saturating_sub(now);
                self.carriers[i].pace_until = t;
                return;
            }
            // Taking turns means spending a cycle's worth of budget inside one slot.
            let (_, slots) = self.slot_of(i, colour, colours);
            if slots > 1 {
                let cycle = slots * self.cfg.params.t_slot_ms.max(1);
                self.carriers[i].fatsoen.set_burst_at_least(cycle as u32);
            }
        }
        // Fresh content (first copy into the cell) has right of way over repetition.
        let fresh = match &cand {
            Cand::Upload(..) => true,
            Cand::Carousel(Item::Symbol { .. }) => self.carriers[i].carousel.as_ref().map(|k| k.current_is_fresh()).unwrap_or(false),
            _ => false,
        };
        if !self.carriers[i].fatsoen.allows(class, fresh) {
            self.carriers[i].pace_until = now + 1000;
            if class == Class::Content {
                self.stats.defer_ms[4] += 1000;
            }
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
                    // Waiting is for a radio that has one channel. On a frequency-agile carrier
                    // the next dwell is a different channel with its own budget, so never defer
                    // past the end of this dwell.
                    let mut w = w.min(60_000);
                    if self.hops(i) {
                        let dwell = self.cfg.params.dwell_ms.max(1);
                        w = w.min((now / dwell + 1) * dwell - now);
                    }
                    self.carriers[i].pace_until = now + w;
                    self.stats.discipline_waits += 1;
                    self.stats.defer_ms[2] += w;
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
            // Politeness bounds a transmission before the law does. Under polite access every
            // transmission costs a fixed pause afterwards, so a node that sends one frame at a
            // time throws away most of the channel; but a node that transmits for the full
            // second the law allows is deaf for that second, and an announcer must hear its
            // uploaders. So a transmission lasts just long enough to earn its own pause at our
            // allowed duty: burst = pause x p / (1 - p). Take what you need, not what you may.
            if let Some(band) = self.carriers[i].p.band {
                if let Access::Polite { ton_max_ms, toff_min_ms, .. } = self.discipline.rule(band) {
                    let p = budget.min(999) as u64;
                    let target = ((toff_min_ms as u64 * p) / (1000 - p)).clamp(airtime as u64, ton_max_ms as u64) as u32;
                    let center = self.carriers[i].p.channels.get(channel as usize).copied().unwrap_or(0);
                    if self.discipline.burst_on_ms(band, center, now) + airtime > target {
                        self.carriers[i].pace_until = now + toff_min_ms as Millis;
                        self.stats.defer_ms[2] += toff_min_ms as Millis;
                        return;
                    }
                }
            }
            if let Err(w) = self.carriers[i].fatsoen.take_airtime(now, airtime, budget, fresh) {
                self.carriers[i].pace_until = now + w;
                self.stats.defer_ms[3] += w;
                return;
            }
        }
        if cca_busy {
            let until = self.carriers[i].fatsoen.cca_busy(now, &mut self.rng);
            self.stats.cca_deferrals += 1;
            self.stats.defer_ms[5] += until.saturating_sub(now);
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
                            self.carriers[i].upload = self.carriers[i].upload_queue.pop_front();
                        }
                    }
                    None => {
                        u.esi += 1;
                        if u.esi >= k {
                            u.esi = 0;
                            u.block += 1;
                            if self.store.block_k(&object, u.block).is_none() {
                                self.carriers[i].upload = self.carriers[i].upload_queue.pop_front();
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
            Frame::Gossip(g) => self.rx_gossip(carrier, g, rssi, out),
            Frame::ManifestAnnounce(m) => self.rx_announce(m, rssi),
            Frame::Nack(n) => self.rx_nack(n, rssi),
        }
    }

    fn touch(&mut self, id: NodeId, rssi: i16) -> &mut Neighbor {
        let now = self.now;
        let n = self.neighbors.entry(id).or_insert(Neighbor { last_heard: now, announcer: NodeId::NONE, score: 0, rssi, colour: 0, haves: BTreeSet::new() });
        n.last_heard = now;
        n.rssi = ((n.rssi as i32 + rssi as i32) / 2) as i16;
        n
    }

    /// How many of our neighbours we hear better than `rssi`, as a fraction in thousandths.
    /// Zero means we hear this signal better than anyone else we know. It is a rank, so it needs
    /// no absolute signal levels and works on any carrier.
    fn rssi_rank(&self, rssi: i16) -> u64 {
        let n = self.neighbors.len();
        if n == 0 {
            return 0;
        }
        let better = self.neighbors.values().filter(|nb| nb.rssi > rssi).count();
        (better as u64 * 1000) / n as u64
    }

    /// RSSI of our typical (median) neighbour: the yardstick for "same cell". A node that has
    /// heard nobody else considers any peer as close as its typical neighbour.
    fn typical_neighbor_rssi(&self, except: NodeId) -> i16 {
        let mut v: Vec<i16> = self.neighbors.iter().filter(|(id, _)| **id != except).map(|(_, n)| n.rssi).collect();
        if v.is_empty() {
            return i16::MIN;
        }
        v.sort_unstable();
        v[v.len() / 2]
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
            let n = self.touch(b.announcer, rssi);
            n.score = b.score;
            n.announcer = b.announcer;
            n.colour = b.colour;
        }
        let me = self.cfg.id;
        let score = self.score;
        let now = self.now;
        // Same cell: we hear this announcer at least as well as our typical neighbour.
        let near = rssi >= self.typical_neighbor_rssi(b.announcer);
        // A follower that hears a new neighbouring announcer, or sees one change colour, is the
        // only node that can tell the announcers: report soon.
        let known_colour = self.carriers[ti].election.as_ref().and_then(|e| e.colour_of(b.announcer));
        let changed = known_colour.map(|c| c != b.colour).unwrap_or(true);
        if changed && !self.announcer_of(ti).is_none() {
            self.report_due = true;
        }
        let t = self.carriers[ti].election.as_mut().and_then(|e| e.on_beacon(now, b.announcer, b.score, b.next_ms, rssi, b.colour, b.colours, near, me, score));
        if let Some(t) = t {
            self.on_transition(ti, t, out);
        }
    }

    fn rx_bulk(&mut self, _carrier: usize, b: &Bulk, out: &mut Vec<Action>) {
        // Someone is sending this object: any offer of ours for it, and any answer we have not
        // begun, is moot. This is what keeps an ungranted repair to one sender.
        self.offers.retain(|(o, _, _)| *o != b.object);
        let now = self.now;
        for c in self.carriers.iter_mut() {
            c.cancel_pending(b.object, now);
        }
        let interested = self.wants.contains(&b.object) || self.store.is_known(&b.object) || self.is_announcing();
        if !interested {
            self.stats.bulk_uninterested += 1;
            return;
        }
        match self.store.put_symbol(b.object, b.block, b.esi, b.k, &b.payload) {
            Put::New => {
                self.stats.symbols_new += 1;
                if self.grants.contains_key(&b.object) {
                    self.stats.symbols_served += 1;
                } else {
                    self.stats.symbols_overheard += 1;
                }
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
        let is_manifest = self.store.entry(&id).map(|e| e.kind() == ContentType::Manifest).unwrap_or(false);
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
        // You carry what you listen to: objects are registered (and thus collected from the air)
        // only for channels we follow or, as announcer, serve.
        let interested = self.follows.contains(&chan) || self.is_announcing();
        let mut to_want = Vec::new();
        if interested {
            for o in &m.objects {
                self.store.ensure(o.meta());
                if !self.store.has_complete(&o.id.short()) {
                    to_want.push(o.id.short());
                }
            }
        }
        for id in to_want {
            self.add_want(id);
        }
        if old.map(|o| o.short != short).unwrap_or(false) {
            self.prune_wants();
        }
        if self.follows.contains(&chan) {
            self.want_refresh = true;
        }
    }

    fn rx_gossip(&mut self, _carrier: usize, g: &Gossip, rssi: i16, out: &mut Vec<Action>) {
        if g.node == self.cfg.id {
            return;
        }
        {
            let n = self.touch(g.node, rssi);
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
        // Another holder offered the same objects: our pending offers are redundant.
        if !g.have.is_empty() {
            self.offers.retain(|(o, _, _)| !g.have.contains(o));
        }
        if g.announcer == g.node {
            for c in self.carriers.iter_mut() {
                if let Some(e) = c.election.as_mut() {
                    e.note_colouring(now, g.node, g.announcer_colour, g.announcer_colours, rssi);
                }
            }
        }
        if self.is_announcing() {
            // A follower of ours hears other announcers, or a follower of another announcer
            // hears us: either way those announcers collide with us at that node.
            if g.announcer == me {
                for (h, colour, colours) in &g.heard {
                    self.note_conflict(*h, *colour, *colours);
                }
            } else if g.heard.iter().any(|(h, _, _)| *h == me) {
                // A follower of another announcer hears us: that announcer, and every other
                // announcer it hears, collide with us at that node.
                self.note_conflict(g.announcer, g.announcer_colour, g.announcer_colours);
                for (h, colour, colours) in &g.heard {
                    self.note_conflict(*h, *colour, *colours);
                }
            }
        }
        for i in 0..self.carriers.len() {
            if self.role(i) != Role::Announcer {
                continue;
            }
            let mut new_wants = Vec::new();
            for (w, _) in &g.want {
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
                // An offer for something we want: grant the first holder that offers.
                if self.wants.contains(h) && !self.grants.contains_key(h) {
                    self.grants.insert(*h, (g.node, now));
                    self.gossip_soon();
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
        // wants. An open ask is answered with an offer; only the holder it then grants uploads.
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
            for (w, grant) in g.want.iter() {
                if !self.store.has_complete(w) {
                    continue;
                }
                if *grant == self.cfg.id {
                    // Granted: upload it, after whatever we are already uploading.
                    self.granted_to_us.insert((*w, g.node), now);
                    let c = &mut self.carriers[i];
                    let active = c.upload.as_ref().map(|u| u.object == *w && u.to == g.node).unwrap_or(false);
                    let queued = c.upload_queue.iter().any(|u| u.object == *w && u.to == g.node);
                    if !active && !queued {
                        c.add_upload(Upload { object: *w, to: g.node, start_at: now, started: false, block: 0, esi: 0, list: None }, false);
                    }
                } else if grant.is_none() {
                    // Open ask: offer, unless we are already uploading it to this announcer.
                    let uploading = self.carriers[i].upload.as_ref().map(|u| u.object == *w && u.to == g.node).unwrap_or(false);
                    if !uploading && !self.offers.iter().any(|(o, a, _)| o == w && *a == g.node) {
                        let at = now + self.rng.below(self.cfg.params.t_offer_ms.max(1));
                        self.offers.push((*w, g.node, at));
                    }
                } else {
                    // Granted to someone else: anything of ours for it that has not begun is
                    // redundant.
                    self.carriers[i].cancel_pending(*w, now);
                    self.offers.retain(|(o, a, _)| !(o == w && *a == g.node));
                }
            }
        }
    }

    /// Send due offers: one small HAVE frame per (object, announcer), suppressed if another
    /// holder's offer for the same object was heard meanwhile.
    fn offer_check(&mut self) {
        let now = self.now;
        let due: Vec<(ShortId, NodeId)> = self.offers.iter().filter(|(_, _, at)| *at <= now).map(|(o, a, _)| (*o, *a)).collect();
        if due.is_empty() {
            return;
        }
        self.offers.retain(|(_, _, at)| *at > now);
        let have: Vec<ShortId> = due.iter().map(|(o, _)| *o).take(MAX_GOSSIP_IDS).collect();
        let g = Gossip { node: self.cfg.id, announcer: self.gossip_announcer_field(), announcer_colour: self.announcer_colouring_field().0, announcer_colours: self.announcer_colouring_field().1, heard: self.heard_field(), have, want: Vec::new() };
        let cell = self.cell_carrier();
        self.enqueue(cell, Frame::Gossip(g));
    }

    fn rx_announce(&mut self, m: &ManifestAnnounce, rssi: i16) {
        if m.node == self.cfg.id {
            return;
        }
        self.touch(m.node, rssi);
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
            self.store.ensure_hint(e.manifest, e.len, ContentType::Manifest);
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

    fn rx_nack(&mut self, n: &Nack, rssi: i16) {
        if n.node == self.cfg.id {
            return;
        }
        self.touch(n.node, rssi);
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
            } else {
                // A follower asks its announcer, whose carousel puts the missing symbols at the
                // front of the next round; an announcer asks the neighbourhood, because no
                // carousel serves it. So only an announcer's NACK is answered by arbitrary
                // holders: its granted uploader at once, any other holder after a wait. Letting
                // peers answer followers as well turned a 200-node town into a repair storm of
                // nearly a million answers.
                let asker_is_announcer = self.neighbors.get(&n.node).map(|nb| nb.announcer == n.node).unwrap_or(false);
                let recently = self.granted_to_us.get(&(n.object, n.node)).map(|t| self.now < *t + self.cfg.params.neighbor_ttl_ms).unwrap_or(false);
                if recently {
                    self.granted_to_us.insert((n.object, n.node), self.now);
                }
                if !recently && !asker_is_announcer {
                    continue;
                }
                // Whoever hears the asker best answers first and silences the rest: an ungranted
                // repair waits in proportion to how many neighbours we hear better than the
                // asker, plus a little jitter to separate equals.
                let w = self.cfg.params.upload_suppress_ms.max(1);
                let ungranted_delay = (w * self.rssi_rank(rssi)) / 1000 + self.rng.below(w / 10 + 1);
                let c = &mut self.carriers[i];
                let granted = recently
                    || c.upload.as_ref().map(|u| u.object == n.object && u.to == n.node).unwrap_or(false)
                    || c.upload_queue.iter().any(|u| u.object == n.object && u.to == n.node);
                let start_at = if granted { self.now } else { self.now + ungranted_delay };
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
                match c.upload.as_mut() {
                    Some(u) if u.object == n.object && u.to == n.node && u.list.is_some() => {
                        u.list.as_mut().unwrap().extend(list);
                    }
                    Some(u) if u.object == n.object && u.to == n.node => {
                        // A full pass is in progress; it will cover these.
                    }
                    _ if c.upload_queue.iter().any(|u| u.object == n.object && u.to == n.node) => {}
                    _ => {
                        c.add_upload(Upload { object: n.object, to: n.node, start_at, started: false, block: n.block, esi: 0, list: Some(list) }, true);
                        self.stats.repairs_queued += 1;
                    }
                }
            }
        }
    }
}

/// Key of the common meeting-dwell sequence.
pub const MEETING_ID: NodeId = NodeId(0xFFFF_FFFF);
/// Key of the shared base sequence that coloured announcers shift.
pub const BASE_ID: NodeId = NodeId(0xFFFF_FFFE);

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
