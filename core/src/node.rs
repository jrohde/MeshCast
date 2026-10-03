//! The MeshCast node: one event-driven state machine that ties objects, manifests, frames,
//! carousel, election, EtherFatsoen and EtherDiscipline together. No I/O: the host (firmware,
//! station or simulator) feeds [`Event`]s and executes [`Action`]s.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;

use crate::carousel::{Carousel, CarouselParams, Item};
use crate::discipline::{Accounting, Verdict};
use crate::election::{step_up_order, Election, Transition};
use crate::fatsoen::Fatsoen;
use crate::frame::{AnnounceEntry, Beacon, Bulk, CarrierKind, CAP_IP, CAP_MAINS, Class, Frame, FrameType, Gossip, ManifestAnnounce, Nack, PieceSet, WantSet, ASK_LISTENED, HAVE_BUDGET, HAVE_SET_BYTES, MAX_ANNOUNCE_ENTRIES, MAX_GOSSIP_IDS, MAX_NACK_RANGES, MAX_WANT, PHASE_MASK, SYMBOL_SIZE, WANT_BUDGET, WANT_SET_BYTES};
use crate::ids::{ChannelId, NodeId, ShortId};
use crate::manifest::{Collection, CollectionKind, CollectionRef, Manifest, ObjectRef};
use crate::rendition::RenditionTable;
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
    /// Whether this node plays audio codes itself (a phone behind a dongle, a station). A node
    /// that cannot, such as a board with a speaker, wants renditions instead (PROTOCOL.md §1.2).
    pub decodes: bool,
    /// Whether this node can make renditions: it runs the profiles' decoder and encoder.
    pub renders: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CarrierState {
    /// Clear-channel assessment: energy above threshold right now.
    pub busy: bool,
    /// Fraction (permille) of the last window during which the channel was busy with others.
    pub occupancy_permille: u16,
}

/// Diagnostic view of where an object could be fetched from: see `Node::excursion_view`.
pub type ExcursionView = (Vec<(NodeId, bool, i16)>, Vec<(NodeId, NodeId)>);

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
    /// `upload_to`: the announcer an upload frame is meant for (diagnostics).
    Tx { carrier: usize, channel: u8, bytes: Vec<u8>, airtime_ms: u32, class: Class, frame_type: FrameType, upload_to: Option<NodeId> },
    ObjectComplete { id: ShortId, now: Millis },
    Role { carrier: usize, role: Role, announcer: NodeId, now: Millis },
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// Excursions started: following another announcer for an object ours could not get.
    pub excursions: u64,
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
    /// Uploads that put at least one symbol on the air (whole objects and repair answers).
    pub uploads_started: u64,
    /// Of those, repair answers: a list of named symbols rather than a whole object.
    pub repairs_started: u64,
    /// Announcer side: offers granted, and grants that lapsed without progress.
    pub grants_given: u64,
    pub grants_lapsed: u64,
    /// Collection manifests that came with a root manifest (§4): granted with it on the
    /// announcer side, uploaded after it on the holder side.
    pub follow_ups_granted: u64,
    pub follow_ups_sent: u64,
    /// Follower asks for what a manifest just adopted named, sooner than `T_want_min`.
    pub wants_rest: u64,
    /// Pieces and covers fetched to relay for another cell's listeners (PROTOCOL.md §4).
    pub relay_wants: u64,
    /// How well we heard the holder, summed over grants that ended in completion and over grants
    /// that lapsed (dBm; divide by the counts).
    pub grant_rssi_completed: i64,
    pub grants_completed: u64,
    pub grant_rssi_lapsed: i64,
    /// Diagnostics: lapsed grants whose holder follows another announcer, and lapsed grants
    /// that never brought a symbol; grants a holder heard for the first time.
    pub grants_lapsed_foreign: u64,
    pub grants_lapsed_unstarted: u64,
    pub grants_received: u64,
    /// Renditions this node made on demand.
    pub renditions_made: u64,
    /// Rendition asks refused because the programme is not about to play.
    pub renditions_refused: u64,
    /// Carousel symbol frames: [first pass, repeated passes].
    pub carousel_frames: [u64; 2],
    /// Repair answers lined up; most are cancelled by hearing another holder answer first.
    pub repairs_queued: u64,
    pub manifests_adopted: u64,
    pub conflict_reports_sent: u64,
    /// Manifest announcements sent to bring our announcer up to date.
    pub manifest_corrections: u64,
    /// One-symbol NACKs sent to an announcer that had been silent, asking it to show it serves.
    pub probes: u64,
    /// Want sets sent, and the pieces they asked for.
    pub want_sets: u64,
    pub want_set_pieces: u64,
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
    /// Upload phases last announced (announcers only): how its listening time is divided.
    upload_phases: u8,
    /// Frames heard from it while it has been in the table: a node heard once may be a name
    /// someone made up (ABUSE.md), so it counts towards our score only from the second.
    heard_count: u16,
    haves: BTreeSet<ShortId>,
    /// HAVE sets of a manifest we do not hold yet: kept, and read once we adopt that manifest.
    /// A follower about to fetch from another cell often lacks the manifest the sets refer to.
    unread_sets: Vec<PieceSet>,
}

/// One manifest of a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ManifestRef {
    seq: u32,
    short: ShortId,
    len: u32,
}

/// What a node knows of a channel's manifests (PROTOCOL.md §2).
#[derive(Clone, Copy, Debug, Default)]
struct ManifestInfo {
    /// The newest manifest adopted, its signature checked. Its bytes may be gone (evicted while
    /// we did not follow the channel) while we still know its seq.
    adopted: Option<ManifestRef>,
    /// A newer one announced and not yet held. Announcements are not signed, so this never
    /// displaces the adopted one or blocks a real manifest (`rx_announce`).
    announced: Option<ManifestRef>,
}

impl ManifestInfo {
    /// The manifest to hold: the announced one while there is one, else the adopted one.
    fn current(&self) -> Option<ManifestRef> {
        self.announced.or(self.adopted)
    }
}

/// What a root manifest we adopted names, read once when it is adopted: its signature is checked
/// then and not each time we look.
#[derive(Clone, Debug)]
struct RootIndex {
    collections: Vec<CollectionRef>,
    renditions: Option<ObjectRef>,
}

/// A source transmitting an object to the announcer: either a full pass or a NACKed list.
/// A want set before it is packed: its collection manifest, the holder it is granted to (NONE
/// for an open ask) and its phase byte (PROTOCOL.md §3.3).
type SetKey = (ShortId, NodeId, u8);

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
    /// The phase of the announcer's listening time this upload uses, named by its grant or by
    /// the NACK it answers.
    phase: u8,
    /// A manifest, root or collection: it goes before the objects it names (PROTOCOL.md §4).
    manifest: bool,
    /// Its place among the uploads of its rank: lower goes first (PROTOCOL.md §4).
    order: u64,
}

#[derive(Clone, Debug)]
struct Pending {
    class: Class,
    frame_type: FrameType,
    bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default)]
struct Progress {
    /// When we started wanting the object.
    wanted_at: Millis,
    last_progress: Millis,
    last_nack: Millis,
    last_want: Millis,
}

impl CarrierRt {
    /// Start an upload, or line it up behind the one in progress. Urgent work (a repair) goes to
    /// the head of the queue, and a grant from our own announcer `own` before any whole upload
    /// to another cell's (PROTOCOL.md §4). One place decides, so an upload can never be queued
    /// behind nothing.
    fn add_upload(&mut self, u: Upload, urgent: bool, own: NodeId) {
        match (&self.upload, urgent) {
            (None, _) => self.upload = Some(u),
            (Some(_), true) => self.upload_queue.push_front(u),
            (Some(_), false) => {
                // Repairs, then manifests, then our own announcer's grants, then other cells';
                // within each, by `order`.
                let rank = |q: &Upload| if q.list.is_some() { 0 } else if q.manifest { 1 } else if q.to == own { 2 } else { 3 };
                let r = (rank(&u), u.order);
                let at = self.upload_queue.iter().position(|q| (rank(q), q.order) > r).unwrap_or(self.upload_queue.len());
                self.upload_queue.insert(at, u);
            }
        }
    }

    /// Hold back frames of `class` until `until`: content waits on its own clock.
    fn hold(&mut self, class: Class, until: Millis) {
        if class == Class::Content {
            self.content_until = until;
        } else {
            self.pace_until = until;
        }
    }

    /// Drop every upload meant for `to`, running or lined up.
    fn drop_uploads_to(&mut self, to: NodeId) {
        if self.upload.as_ref().map(|u| u.to == to).unwrap_or(false) {
            self.upload = None;
        }
        self.upload_queue.retain(|u| u.to != to);
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
    /// Until when content (carousel symbols, uploads) waits: slots, phases, the rendezvous and
    /// content budgets hold content only, never the control frames queued behind it.
    content_until: Millis,
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
    /// Channels followed whole: every collection, now and later.
    follows: BTreeSet<ChannelId>,
    /// Collections followed one by one, as (channel, cid).
    follows_collections: BTreeSet<(ChannelId, u32)>,
    /// The root manifest of each channel (PROTOCOL.md §2).
    manifests: BTreeMap<ChannelId, ManifestInfo>,
    /// What each root manifest we adopted and hold names, by its short id.
    roots: BTreeMap<ShortId, RootIndex>,
    /// The collection manifest we adopted and hold for each collection. It stays, with its pieces,
    /// until the one a newer root names is held: a new root alone must not cost a follower the
    /// pieces it holds.
    collections: BTreeMap<(ChannelId, u32), ShortId>,
    own_manifests: Vec<(ChannelId, ShortId, u32, u32)>,
    own_objects: BTreeSet<ShortId>,
    /// Our own objects and manifests that nobody else has been seen to carry yet, and since when:
    /// we keep offering and announcing them.
    pending_ack: BTreeMap<ShortId, Millis>,
    wants: BTreeSet<ShortId>,
    /// Objects completed by registration rather than by a symbol, awaiting `on_complete`.
    quiet_complete: Vec<ShortId>,
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
    /// object -> (granted holder, when, phase of our listening time it uploads in).
    grants: BTreeMap<ShortId, (NodeId, Millis, u8)>,
    /// Phases an announcer reserved for answers to its NACK of an object nobody is granted:
    /// object -> (phase, since).
    repair_phases: BTreeMap<ShortId, (u8, Millis)>,
    /// Renditions named by the manifests we hold: rendition -> (the object it is made from,
    /// its metadata).
    renditions: BTreeMap<ShortId, (ShortId, ObjectMeta)>,
    /// Renditions we will want when their slot comes near: (slot start, rendition).
    renditions_due: Vec<(Millis, ShortId)>,
    /// A running excursion: (carrier, the announcer we visit, since when).
    excursion: Option<(usize, NodeId, Millis)>,
    /// Objects our announcer was last heard asking for itself, and when: it cannot repair them.
    ann_asks: BTreeMap<ShortId, Millis>,
    /// Objects our announcer was last heard granting to an uploader, and when: they are coming.
    ann_grants: BTreeMap<ShortId, Millis>,
    /// When we last received a BULK frame of any object: a channel that carries nothing.
    last_bulk_rx: Millis,
    /// When the announcer last heard an upload in each phase.
    phase_heard: [Millis; MAX_UPLOAD_PHASES as usize],
    /// Holder side: grants we received, with the time, so that we still answer the announcer's
    /// NACKs for a while after our full pass is done.
    granted_to_us: BTreeMap<(ShortId, NodeId), Millis>,
    /// Holder side: offers we owe (object, announcer that asked, when to send).
    offers: Vec<(ShortId, NodeId, Millis)>,
    /// Channels whose manifest we hold and our announcer does not (an older seq, or none):
    /// we announce them at `correction_at` unless someone else does first (PROTOCOL.md §2).
    corrections: BTreeSet<ChannelId>,
    correction_at: Option<Millis>,
    /// When we last announced each channel's manifest to correct our announcer.
    corrected: BTreeMap<ChannelId, Millis>,
    /// Nothing is evicted before this: a node that restarts or stops announcing keeps what it
    /// carried for `want_ttl`, in case it announces again (PROTOCOL.md §4).
    keep_until: Millis,
    /// When we began to follow our current announcer: evidence against it counts from then.
    following_since: Millis,
    /// When to ask for the rest: what a manifest we just adopted named (`ask_rest_soon`).
    ask_rest_at: Option<Millis>,
    /// When we last asked our announcer for one symbol as proof that it serves (§5.2).
    last_probe: Millis,
    /// The pieces of every collection manifest we adopted, in the order it lists them: what a
    /// set's bitmap refers to (PROTOCOL.md §3.3).
    pieces: BTreeMap<ShortId, Vec<ShortId>>,
    /// The collection manifests among those whose pieces have no order (singles, PROTOCOL.md §2):
    /// every piece of one is first.
    unordered: BTreeSet<ShortId>,
    /// When a piece of each collection we adopted last arrived: a collection is in progress
    /// while its pieces arrive or are granted (PROTOCOL.md §4).
    moved: BTreeMap<ShortId, Millis>,
    /// Holder: the collection manifests we last listed in a HAVE with each root manifest, which
    /// we upload after it when granted it (PROTOCOL.md §4).
    offered_with: BTreeMap<ShortId, Vec<ShortId>>,
    /// Pieces and covers of collections we do not listen to that another cell's announcer asked
    /// for: we fetch and keep them to hand on.
    relayed: BTreeSet<ShortId>,
    /// When we first heard another cell's announcer ask, for listeners, for an object we lack.
    relay_asks: BTreeMap<ShortId, Millis>,
    /// Announcer: for each root manifest granted on an offer, the holder and what it listed with
    /// it: the collection manifests new in it among those come with it.
    root_follow_ups: BTreeMap<ShortId, (NodeId, Vec<ShortId>)>,
    pub stats: Stats,
}

/// Upper bound on upload phases an announcer hands out: at most this many uploads run at once.
const MAX_UPLOAD_PHASES: u8 = 16;

impl Node {
    pub fn new(cfg: NodeConfig, now: Millis) -> Self {
        let discipline = Accounting::new(cfg.profile, &cfg.rule_choice);
        let ctrl = cfg.carriers.iter().position(|c| c.kind == CarrierKind::LoraControl).unwrap_or(0);
        let cp = CarouselParams { max_passes: cfg.params.max_passes, want_ttl_ms: cfg.params.want_ttl_ms, t_repass_ms: cfg.params.t_want_min_ms, max_askers: cfg.params.max_askers_per_object };
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
                content_until: 0,
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
            follows_collections: BTreeSet::new(),
            manifests: BTreeMap::new(),
            roots: BTreeMap::new(),
            collections: BTreeMap::new(),
            own_manifests: Vec::new(),
            own_objects: BTreeSet::new(),
            pending_ack: BTreeMap::new(),
            wants: BTreeSet::new(),
            quiet_complete: Vec::new(),
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
            repair_phases: BTreeMap::new(),
            phase_heard: [0; MAX_UPLOAD_PHASES as usize],
            renditions: BTreeMap::new(),
            renditions_due: Vec::new(),
            excursion: None,
            ann_asks: BTreeMap::new(),
            ann_grants: BTreeMap::new(),
            last_bulk_rx: 0,
            granted_to_us: BTreeMap::new(),
            offers: Vec::new(),
            corrections: BTreeSet::new(),
            correction_at: None,
            corrected: BTreeMap::new(),
            keep_until: 0,
            following_since: now,
            last_probe: 0,
            ask_rest_at: None,
            pieces: BTreeMap::new(),
            unordered: BTreeSet::new(),
            moved: BTreeMap::new(),
            offered_with: BTreeMap::new(),
            relayed: BTreeSet::new(),
            relay_asks: BTreeMap::new(),
            root_follow_ups: BTreeMap::new(),
            stats,
            cfg,
            now,
        }
    }

    fn carousel_params(&self) -> CarouselParams {
        CarouselParams { max_passes: self.cfg.params.max_passes, want_ttl_ms: self.cfg.params.want_ttl_ms, t_repass_ms: self.cfg.params.t_want_min_ms, max_askers: self.cfg.params.max_askers_per_object }
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

    /// The announcer whose channel `node` listens on: itself if it announces, else the one it
    /// said it follows. A repair for a follower goes where that follower is tuned.
    fn listens_with(&self, node: NodeId) -> NodeId {
        match self.neighbors.get(&node) {
            Some(nb) if !nb.announcer.is_none() => nb.announcer,
            _ => node,
        }
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
            // At most `max_conflicts`, the one reported longest ago giving way.
            if !self.conflicts.contains_key(&other) && self.conflicts.len() >= self.cfg.params.max_conflicts.max(1) {
                if let Some(gone) = self.conflicts.iter().min_by_key(|(_, (t, _, _))| *t).map(|(k, _)| *k) {
                    self.conflicts.remove(&gone);
                }
            }
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

    /// On a hopping carrier a candidate steps up in a meeting dwell, where every candidate is on
    /// one channel and hears the first of them: after the announcers' meeting beacons, in the
    /// order of `step_up_order` over most of the rest of the dwell.
    fn step_up_time(&mut self, now: Millis, score: u16, caps: u8) -> Millis {
        let dwell = self.cfg.params.dwell_ms.max(1);
        let off = dwell / 5 + step_up_order(caps, score, dwell * 7 / 10, &mut self.rng);
        let this = if self.is_meeting_dwell(now / dwell) { (now / dwell) * dwell + off } else { 0 };
        if this > now { this } else { self.next_meeting_start(now) + off }
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

    /// Diagnostic: the running upload on `carrier` (object, target, started) and how many wait.
    pub fn upload_state(&self, carrier: usize) -> (Option<(ShortId, NodeId, bool)>, usize) {
        match self.carriers.get(carrier) {
            Some(c) => (c.upload.as_ref().map(|u| (u.object, u.to, u.started)), c.upload_queue.len()),
            None => (None, 0),
        }
    }

    /// Diagnostic: the phase count we believe `announcer` uses, and ours if we are one.
    pub fn upload_phases_believed(&self, announcer: NodeId) -> u8 {
        self.neighbors.get(&announcer).map(|n| n.upload_phases).unwrap_or(0)
    }
    pub fn upload_phases_now(&self) -> u8 {
        self.upload_phase_count()
    }
    /// Diagnostic: whether `announcer` granted us `object`.
    pub fn upload_granted(&self, object: &ShortId, announcer: NodeId) -> bool {
        self.granted_to_us.contains_key(&(*object, announcer))
    }

    /// Diagnostic: the colour we believe announcer `ann` has on `carrier`, and our own.
    pub fn colour_believed(&self, carrier: usize, ann: NodeId) -> Option<u8> {
        self.carriers.get(carrier).and_then(|c| c.election.as_ref()).and_then(|e| e.colour_of(ann))
    }
    pub fn own_colour(&self) -> u8 {
        self.my_colour()
    }

    /// Diagnostic: (symbols held, total) of `id`, and whether it is complete.
    pub fn object_progress(&self, id: &ShortId) -> Option<(u32, Option<u32>, bool)> {
        self.store.entry(id).map(|e| {
            let (h, t) = e.progress();
            (h, t, e.is_complete())
        })
    }

    /// Diagnostic: wanted objects whose length we do not know, so cannot complete.
    pub fn wants_without_length(&self) -> Vec<ShortId> {
        self.wants.iter().filter(|id| self.store.entry(id).map(|e| e.len().is_none()).unwrap_or(true)).copied().collect()
    }
    /// Diagnostic: the content type we know for `id`.
    pub fn object_kind(&self, id: &ShortId) -> Option<crate::object::ContentType> {
        self.store.entry(id).filter(|e| e.len().is_some()).map(|e| e.kind())
    }

    /// Diagnostic: what we know of channel `chan`'s manifest: (seq, id, adopted, held, wanted).
    pub fn manifest_state(&self, chan: &crate::ids::ChannelId) -> Option<(u32, ShortId, bool, bool, bool)> {
        self.manifests.get(chan).and_then(|i| i.current().map(|r| (r.seq, r.short, i.announced.is_none(), self.store.has_complete(&r.short), self.wants.contains(&r.short))))
    }

    /// Diagnostic: whether `now` is in the rendezvous on `carrier`.
    pub fn in_meeting(&self, carrier: usize, now: Millis) -> bool {
        self.in_rendezvous(carrier, now)
    }

    /// Diagnostic: how often we challenged an announcer, over all carriers.
    pub fn challenges(&self) -> u32 {
        self.carriers.iter().filter_map(|c| c.election.as_ref()).map(|e| e.challenges).sum()
    }

    /// Diagnostic: announcers we heard a beacon from recently on the cell carrier, and for each
    /// whether its HAVE listed `id` and how well we hear it; plus every neighbour that listed it,
    /// with the announcer it follows.
    pub fn excursion_view(&self, id: &ShortId) -> ExcursionView {
        let i = self.cell_carrier();
        let own = self.announcer_of(i);
        let heard: Vec<NodeId> = self.carriers.get(i).and_then(|c| c.election.as_ref()).map(|e| e.heard_ids(self.now, own).collect()).unwrap_or_default();
        let a = heard.iter().map(|a| (*a, self.neighbors.get(a).map(|n| n.haves.contains(id)).unwrap_or(false), self.neighbors.get(a).map(|n| n.rssi).unwrap_or(0))).collect();
        let h = self.neighbors.iter().filter(|(_, n)| n.haves.contains(id)).map(|(k, n)| (*k, n.announcer)).collect();
        (a, h)
    }

    /// Diagnostic: how many objects we want.
    pub fn wants_len(&self) -> usize {
        self.wants.len()
    }

    /// Diagnostic: whether `id` is on our want list.
    pub fn wants_object(&self, id: &ShortId) -> bool {
        self.wants.contains(id)
    }

    /// Diagnostic: whether we hold `id` complete.
    pub fn holds(&self, id: &ShortId) -> bool {
        self.store.has_complete(id)
    }

    /// Diagnostic: the uploads we are running or have lined up on `carrier`, as (object, to).
    pub fn uploads(&self, carrier: usize) -> Vec<(ShortId, NodeId)> {
        let c = &self.carriers[carrier];
        c.upload.iter().chain(c.upload_queue.iter()).map(|u| (u.object, u.to)).collect()
    }

    /// Diagnostic: every wanted object with its progress, grant and timers.
    pub fn want_report(&self) -> alloc::string::String {
        use core::fmt::Write;
        let mut s = alloc::string::String::new();
        for id in &self.wants {
            let p = self.progress.get(id).copied().unwrap_or_default();
            let prog = self.store.entry(id).map(|e| e.progress());
            let _ = write!(s, "{:?} have={:?} grant={:?} last_progress={} last_want={} | ", id, prog, self.grants.get(id), p.last_progress, p.last_want);
        }
        let _ = write!(s, "now={} next_gossip={} offers={}", self.now, self.next_gossip, self.offers.len());
        s
    }

    pub fn wants(&self) -> &BTreeSet<ShortId> {
        &self.wants
    }

    /// The channels followed whole.
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

    /// Diagnostic: the size of every table a node fills from what it hears, by name, for choosing
    /// their caps (docs/ABUSE.md, "Someone else's firmware", item 5).
    pub fn table_sizes(&self) -> Vec<(&'static str, usize)> {
        let mut car = (0, 0, 0, 0);
        for k in self.carriers.iter().filter_map(|c| c.carousel.as_ref()) {
            let (a, b, c, d) = k.table_sizes();
            car = (car.0 + a, car.1 + b, car.2 + c, car.3 + d);
        }
        alloc::vec![
            ("neighbours", self.neighbors.len()),
            ("ids offered by neighbours", self.neighbors.values().map(|n| n.haves.len()).sum()),
            ("channels", self.manifests.len()),
            ("roots", self.roots.len()),
            ("collections", self.collections.len()),
            ("piece lists", self.pieces.len()),
            ("store entries", self.store.len()),
            ("wants", self.wants.len()),
            ("progress", self.progress.len()),
            ("conflicts", self.conflicts.len()),
            ("grants", self.grants.len()),
            ("announcer asks", self.ann_asks.len()),
            ("announcer grants", self.ann_grants.len()),
            ("granted to us", self.granted_to_us.len()),
            ("offers", self.offers.len()),
            ("relay asks", self.relay_asks.len()),
            ("relayed", self.relayed.len()),
            ("corrected", self.corrected.len()),
            ("renditions", self.renditions.len()),
            ("renditions due", self.renditions_due.len()),
            ("pending acks", self.pending_ack.len()),
            ("carousel objects", car.0),
            ("carousel askers", car.1),
            ("carousel passes", car.2),
            ("carousel places", car.3),
        ]
    }

    /// EXPERIMENT (roots oracle): the manifests, root or collection, we want.
    pub fn wanted_manifests(&self) -> Vec<ShortId> {
        self.wants.iter().filter(|w| self.store.entry(w).map(|e| e.kind().is_manifest()).unwrap_or(false)).copied().collect()
    }

    /// EXPERIMENT (backbone oracle): everything we want.
    pub fn wants_list(&self) -> Vec<ShortId> {
        self.wants.iter().copied().collect()
    }

    /// EXPERIMENT (backbone oracle): what we want that one of our own followers asked for.
    pub fn listened_wants(&self) -> Vec<ShortId> {
        self.wants.iter().filter(|w| self.carriers.iter().filter_map(|c| c.carousel.as_ref()).any(|k| k.wanted_by(w) > 0)).copied().collect()
    }

    /// EXPERIMENT (backbone oracle): every symbol we hold of a complete object, as BULK frames.
    pub fn symbols_of(&self, id: &ShortId) -> Vec<Bulk> {
        let mut out = Vec::new();
        let Some(e) = self.store.entry(id) else { return out };
        let (Some(len), true) = (e.len(), e.is_complete()) else { return out };
        for block in 0..e.known_blocks() {
            let k = self.store.block_k(id, block).unwrap_or(0);
            for esi in 0..k {
                let mut buf = alloc::vec![0u8; SYMBOL_SIZE];
                if self.store.get_symbol(id, block, esi, &mut buf) {
                    out.push(Bulk { object: *id, block, esi, len, payload: buf });
                }
            }
        }
        out
    }

    /// Diagnostic: when a wanted object last brought a symbol and when we last asked for it.
    pub fn want_times(&self, id: &ShortId) -> Option<(Millis, Millis)> {
        self.progress.get(id).map(|p| (p.last_progress, p.last_want))
    }

    /// Diagnostic: what the next round of asking names, its sets read by our piece lists. It is
    /// built as for a GOSSIP and counts as asked.
    pub fn ask_now(&mut self) -> Vec<ShortId> {
        let (want, sets) = self.take_ask(false);
        let mut out: Vec<ShortId> = want.iter().map(|(id, _, _)| *id).collect();
        for w in &sets {
            if let Some(list) = self.pieces.get(&w.set.manifest) {
                out.extend(w.set.pieces().filter_map(|k| list.get(k as usize).copied()));
            }
        }
        out
    }

    /// Diagnostic: whether we take our announcer to be asking for `id` itself (PROTOCOL.md §2).
    pub fn announcer_asked_for(&self, id: &ShortId) -> bool {
        self.ann_asks.get(id).map(|t| self.now < *t + self.cfg.params.want_ttl_ms).unwrap_or(false)
    }

    /// Diagnostic: bytes of the objects we hold complete.
    pub fn held_bytes(&self) -> u64 {
        self.store.complete_ids().filter_map(|id| self.store.entry(id).and_then(|e| e.len())).map(|l| l as u64).sum()
    }

    /// Our colour and the size of our conflict set (ourselves included).
    pub fn colouring(&self) -> (u8, u8) {
        (self.my_colour(), self.colours())
    }

    /// Conflict set as (id, colour) for diagnostics.
    pub fn conflict_set(&self) -> Vec<(u32, u8)> {
        self.conflicts.iter().map(|(id, (_, c, _))| (id.0, *c)).collect()
    }

    /// Follow a whole channel: every collection it has, now and later.
    pub fn follow(&mut self, chan: ChannelId) {
        self.follows.insert(chan);
        self.readopt(chan);
    }

    /// Follow one collection of a channel.
    pub fn follow_collection(&mut self, chan: ChannelId, cid: u32) {
        self.follows_collections.insert((chan, cid));
        self.readopt(chan);
    }

    /// Something of `chan` is followed that was not: adopt its root manifest again, so that what
    /// it names for us is registered and wanted.
    fn readopt(&mut self, chan: ChannelId) {
        if let Some(a) = self.manifests.get(&chan).and_then(|i| i.adopted) {
            if let Some(bytes) = self.store.bytes(&a.short).map(|b| b.to_vec()) {
                if let Ok(m) = Manifest::decode(&bytes) {
                    self.adopt_manifest(&m, a.short);
                }
            }
        }
        // Not held: never fetched, or evicted with the channel's objects while we did not follow
        // it. Announcers do not announce a seq we already know, so we ask for it ourselves.
        if let Some(r) = self.manifests.get(&chan).and_then(|i| i.current()) {
            if !self.store.has_complete(&r.short) {
                if self.store.ensure_hint(r.short, r.len, ContentType::Manifest) {
                    self.quiet_complete.push(r.short);
                } else {
                    self.add_want(r.short);
                }
            }
        }
        self.want_refresh = true;
    }

    /// Stop following a channel, whole or any of its collections: its objects are no longer
    /// wanted (unless another followed or served manifest references them) and its manifest is
    /// no longer announced by us.
    pub fn unfollow(&mut self, chan: ChannelId) {
        self.follows.remove(&chan);
        self.follows_collections.retain(|(c, _)| *c != chan);
        self.prune_wants();
    }

    /// Stop following one collection.
    pub fn unfollow_collection(&mut self, chan: ChannelId, cid: u32) {
        self.follows_collections.remove(&(chan, cid));
        self.prune_wants();
    }

    /// Whether we follow `chan`, whole or any of its collections: we then keep its root manifest.
    fn follows_channel(&self, chan: &ChannelId) -> bool {
        self.follows.contains(chan) || self.follows_collections.range((*chan, 0)..=(*chan, u32::MAX)).next().is_some()
    }

    /// Whether we listen to collection `cid` of `chan`.
    fn listens(&self, chan: &ChannelId, cid: u32) -> bool {
        self.follows.contains(chan) || self.follows_collections.contains(&(*chan, cid))
    }

    /// Whether we want collection `cid` of `chan`: we listen to it, or serve it as announcer.
    fn carries(&self, chan: &ChannelId, cid: u32) -> bool {
        self.is_announcing() || self.listens(chan, cid)
    }

    /// The collections the adopted root manifests we hold name, as (channel, collection).
    fn named_collections(&self) -> impl Iterator<Item = (ChannelId, &CollectionRef)> + '_ {
        self.manifests.iter().filter_map(|(c, i)| i.adopted.and_then(|a| self.roots.get(&a.short)).map(|r| (*c, r))).flat_map(|(c, r)| r.collections.iter().map(move |x| (c, x)))
    }

    /// Objects referenced by the latest manifests of the channels and collections we are
    /// interested in.
    fn interesting_objects(&self) -> BTreeSet<ShortId> {
        let mut set = BTreeSet::new();
        let announcing = self.is_announcing();
        for (chan, info) in &self.manifests {
            if !(announcing || self.follows_channel(chan)) {
                continue;
            }
            // The adopted manifest's objects stay until its successor is held: an announcement
            // alone must not cost a follower its window.
            for r in [info.adopted, info.announced].into_iter().flatten() {
                set.insert(r.short);
            }
            let Some(root) = info.adopted.and_then(|a| self.roots.get(&a.short)) else { continue };
            if let Some(t) = root.renditions {
                set.insert(t.id.short());
            }
            // Every collection manifest of a channel we follow at all; the pieces and cover of what
            // we carry, and of the rest what another cell asked us to relay.
            for c in &root.collections {
                set.insert(c.manifest.id.short());
                let carried = self.carries(chan, c.cid);
                if let Some(v) = c.cover {
                    if carried || self.relayed.contains(&v.id.short()) {
                        set.insert(v.id.short());
                    }
                }
                // The collection manifest held, which may be older than the one the root names.
                if let Some(held) = self.collections.get(&(*chan, c.cid)) {
                    set.insert(*held);
                    let pieces = self.pieces.get(held).into_iter().flatten();
                    set.extend(pieces.filter(|p| carried || self.relayed.contains(p)).copied());
                }
            }
        }
        for (r, (parent, _)) in &self.renditions {
            if set.contains(parent) {
                set.insert(*r);
            }
        }
        set
    }

    /// Whether we could make rendition `id`: we render, know it and hold what it is made from.
    fn can_render(&self, id: &ShortId) -> bool {
        self.cfg.renders && self.renditions.get(id).map(|(parent, _)| self.store.has_complete(parent)).unwrap_or(false)
    }

    /// Make rendition `id` from the object it renders, if we can and hold that object. A real
    /// node runs the profile and keeps the result only if it hashes to the signed id; a
    /// platform that does not reproduce the profile bit for bit simply cannot serve renditions.
    fn render(&mut self, id: &ShortId) -> bool {
        if self.store.has_complete(id) {
            return true;
        }
        let Some(&(parent, meta)) = self.renditions.get(id) else { return false };
        if !self.cfg.renders || !self.store.has_complete(&parent) {
            return false;
        }
        self.store.insert_complete(meta, None);
        self.stats.renditions_made += 1;
        true
    }

    /// Whether `id` is a rendition we should not serve now. A rendition is for listening as the
    /// schedule plays: from twice `T_render_ahead` before a programme's slot (to allow for clocks)
    /// until the programme has played. Before that no device needs it yet, after it nobody can
    /// play it on time, and serving either is what a rendition flood asks for (ABUSE.md).
    /// Renditions of unscheduled objects are served on request, like any object.
    fn rendition_not_due(&self, id: &ShortId) -> bool {
        let Some(&(parent, _)) = self.renditions.get(id) else { return false };
        let now = self.now;
        let ahead = 2 * self.cfg.params.t_render_ahead_ms;
        let mut scheduled = false;
        for held in self.collections.values() {
            let Some(m) = self.store.bytes(held).and_then(|b| Collection::decode(b).ok()) else { continue };
            let Some(o) = m.pieces.iter().find(|o| o.id.short() == parent) else { continue };
            let plays_ms = o.kind.codec().map(|c| o.len as u64 * 8 * 1000 / c.bits_per_second().max(1) as u64).unwrap_or(0) as Millis;
            for e in m.schedule.iter().filter(|e| e.object == parent) {
                scheduled = true;
                let slot = self.slot_ms(e.start);
                if now + ahead >= slot && now <= slot + plays_ms {
                    return false;
                }
            }
        }
        scheduled
    }

    /// Local time of a schedule entry's UTC start. The simulation's epoch is UTC 0; a real node
    /// adds the offset from its time source (§6).
    fn slot_ms(&self, utc_s: u64) -> Millis {
        utc_s.saturating_mul(1000) as Millis
    }

    /// Renditions whose slot has come near are wanted now.
    fn renditions_due_check(&mut self) {
        let horizon = self.now + self.cfg.params.t_render_ahead_ms;
        let due: Vec<ShortId> = self.renditions_due.iter().filter(|(t, _)| *t <= horizon).map(|(_, id)| *id).collect();
        if due.is_empty() {
            return;
        }
        self.renditions_due.retain(|(t, _)| *t > horizon);
        for id in due {
            if !self.store.has_complete(&id) {
                self.add_want(id);
                self.want_refresh = true;
            }
        }
    }

    /// Learn the renditions a table names.
    fn load_table(&mut self, table: &ShortId) {
        let Some(t) = self.store.bytes(table).and_then(|b| RenditionTable::decode(b).ok()) else { return };
        for r in t.entries {
            self.renditions.insert(r.id.short(), (r.parent, r.meta()));
        }
        if !self.cfg.decodes {
            self.plan_renditions();
        }
    }

    /// A device that cannot decode wants the sound, not the codes, and only shortly before it
    /// plays it: a radio needs what is on next, not the whole window. Unscheduled objects are
    /// asked for at once (they stand in for what a user picks).
    fn plan_renditions(&mut self) {
        let mut want = Vec::new();
        let mut due = Vec::new();
        for ((chan, cid), held) in &self.collections {
            if !self.listens(chan, *cid) {
                continue;
            }
            let Some(m) = self.store.bytes(held).and_then(|b| Collection::decode(b).ok()) else { continue };
            for o in m.pieces.iter().filter(|o| o.kind.codec().is_some()) {
                let parent = o.id.short();
                let Some((&r, &(_, meta))) = self.renditions.iter().find(|(_, (p, _))| *p == parent) else { continue };
                if self.store.has_complete(&r) || self.wants.contains(&r) || self.renditions_due.iter().any(|(_, id)| *id == r) {
                    continue;
                }
                let slot = m.schedule.iter().find(|e| e.object == parent).map(|e| self.slot_ms(e.start));
                match slot {
                    Some(t) if self.now + self.cfg.params.t_render_ahead_ms < t => due.push((t, r, meta)),
                    _ => want.push((r, meta)),
                }
            }
        }
        for (t, r, meta) in due {
            self.store.ensure(meta);
            self.renditions_due.push((t, r));
        }
        for (r, meta) in want {
            if self.store.ensure(meta) {
                self.quiet_complete.push(r);
            }
            self.add_want(r);
            self.want_refresh = true;
        }
    }

    /// The holder of `id` we hear best among those that said they have it and do not
    /// announce (announcers serve their cells, they do not upload), or NONE.
    fn best_holder(&self, id: &ShortId) -> NodeId {
        self.neighbors
            .iter()
            .filter(|(n, nb)| nb.haves.contains(id) && nb.announcer != **n)
            .max_by_key(|(_, nb)| nb.rssi)
            .map(|(n, _)| *n)
            .unwrap_or(NodeId::NONE)
    }

    /// The holder of `id` we hear best that follows another announcer than ours: a holder in our
    /// own cell answers our announcer's ask, so naming it ourselves would only duplicate that.
    fn best_holder_elsewhere(&self, id: &ShortId) -> NodeId {
        let own = self.announcer_of(self.cell_carrier());
        self.neighbors
            .iter()
            .filter(|(n, nb)| nb.haves.contains(id) && nb.announcer != **n && nb.announcer != own)
            .max_by_key(|(_, nb)| nb.rssi)
            .map(|(n, _)| *n)
            .unwrap_or(NodeId::NONE)
    }

    /// Drop wants for objects that no manifest of interest references any more (unfollowed
    /// channels, or objects that left a channel's manifest), and offers and uploads of them.
    /// Our own objects we go on offering and uploading: a source need not follow its own
    /// channel, and dropped them from its queue whenever it adopted another channel's manifest.
    fn prune_wants(&mut self) {
        let mut keep = self.interesting_objects();
        let stale: Vec<ShortId> = self.wants.iter().filter(|id| !keep.contains(id)).copied().collect();
        for id in stale {
            self.wants.remove(&id);
            self.progress.remove(&id);
            self.grants.remove(&id);
        }
        keep.extend(self.own_objects.iter().copied());
        keep.extend(self.own_manifests.iter().map(|(_, s, _, _)| *s));
        self.offers.retain(|(o, _, _)| keep.contains(o));
        for c in self.carriers.iter_mut() {
            c.upload_queue.retain(|u| keep.contains(&u.object));
        }
    }

    /// Forget objects no manifest of interest references any more (a channel or collection we
    /// unfollowed, or an object that left its collection's window). Own objects are kept.
    fn evict_orphans(&mut self) {
        if self.now < self.keep_until {
            return;
        }
        let keep = self.interesting_objects();
        self.relayed.retain(|id| keep.contains(id));
        let ttl = self.cfg.params.want_ttl_ms;
        let now = self.now;
        self.relay_asks.retain(|_, t| now < *t + ttl);
        let gone: Vec<ShortId> = self.store.ids().filter(|id| !keep.contains(id) && !self.own_objects.contains(id) && !self.own_manifests.iter().any(|(_, s, _, _)| s == *id)).copied().collect();
        for id in gone {
            self.store.remove(&id);
            self.wants.remove(&id);
            self.progress.remove(&id);
            // What we no longer hold we no longer read: a root's index, a collection's pieces.
            self.roots.remove(&id);
            self.unordered.remove(&id);
            self.moved.remove(&id);
            if self.pieces.remove(&id).is_some() {
                self.collections.retain(|_, s| *s != id);
            }
        }
    }

    /// Objects we hold complete that are not referenced by any manifest we know.
    pub fn orphaned_objects(&self) -> Vec<ShortId> {
        let mut keep = BTreeSet::new();
        for r in self.manifests.values().flat_map(|i| [i.adopted, i.announced]).flatten() {
            keep.insert(r.short);
        }
        for (chan, c) in self.named_collections() {
            keep.insert(c.manifest.id.short());
            if let Some(held) = self.collections.get(&(chan, c.cid)) {
                keep.insert(*held);
                keep.extend(self.pieces.get(held).into_iter().flatten().copied());
            }
        }
        self.store.complete_ids().filter(|id| !keep.contains(id)).copied().collect()
    }

    /// Publish a channel: its root manifest, the collection manifests it names and the objects
    /// they list (we own them, complete). A collection manifest already published need not be
    /// passed again.
    pub fn publish(&mut self, root: &Manifest, collections: &[Collection], objects: &[(ObjectMeta, Option<&[u8]>)]) {
        let now = self.now;
        let (meta, bytes) = root.as_object();
        self.store.insert_complete(meta, Some(&bytes));
        let short = meta.id.short();
        let chan = root.channel_id();
        for c in collections {
            let (m, b) = c.as_object();
            let id = m.id.short();
            if !self.store.has_complete(&id) {
                self.store.insert_complete(m, Some(&b));
            }
            if self.own_objects.insert(id) {
                self.pending_ack.insert(id, now);
            }
        }
        for (m, b) in objects {
            if !self.store.has_complete(&m.id.short()) {
                self.store.insert_complete(*m, *b);
            }
            self.own_objects.insert(m.id.short());
            self.pending_ack.insert(m.id.short(), now);
        }
        let old = self.set_root(chan, root, short, meta.len, None);
        self.own_manifests.retain(|(c, _, _, _)| *c != chan);
        self.own_manifests.push((chan, short, root.seq, meta.len));
        if let Some(o) = old {
            self.pending_ack.remove(&o.short);
        }
        self.pending_ack.insert(short, now);
        for c in &root.collections {
            let id = c.manifest.id.short();
            let new = self.collections.get(&(chan, c.cid)) != Some(&id);
            self.adopt_collection(chan, c, new);
        }
        // A collection manifest no root of ours names any more is not worth offering.
        let named: BTreeSet<ShortId> = root.collections.iter().map(|c| c.manifest.id.short()).collect();
        let stale: Vec<ShortId> = self.pending_ack.keys().filter(|id| self.store.entry(id).map(|e| e.kind() == ContentType::Collection).unwrap_or(false) && !named.contains(id)).copied().collect();
        for id in stale {
            self.pending_ack.remove(&id);
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
        self.follows_collections.clear();
        self.manifests.clear();
        self.roots.clear();
        self.collections.clear();
        self.relayed.clear();
        self.pieces.clear();
        self.unordered.clear();
        self.moved.clear();
        self.own_manifests.clear();
        self.renditions.clear();
        self.renditions_due.clear();
        self.own_objects.clear();
        self.pending_ack.clear();
        self.wants.clear();
        self.grants.clear();
        self.repair_phases.clear();
        self.granted_to_us.clear();
        self.offers.clear();
        self.corrected.clear();
        self.stats = Stats { tx_airtime_ms: alloc::vec![0; self.carriers.len()], ..Default::default() };
    }

    /// Time at which the host should call `Tick` next.
    pub fn next_deadline(&self) -> Millis {
        let mut d = self.next_score;
        if let Some(t) = self.renditions_due.iter().map(|(t, _)| *t).min() {
            d = d.min(t.saturating_sub(self.cfg.params.t_render_ahead_ms).max(self.now + 1));
        }
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
        if let Some(t) = self.correction_at {
            d = d.min(t.max(self.now + 1));
        }
        if !self.offers.is_empty() {
            // An offer to another cell's announcer waits for the rendezvous on a hopping carrier.
            let cell = self.cell_carrier();
            let own = self.announcer_of(cell);
            let everyone_listens = !self.hops(cell) || self.in_rendezvous(cell, self.now);
            let meeting = self.next_meeting_start(self.now) + 1;
            for (_, a, at) in &self.offers {
                d = d.min(if *a == own || everyone_listens { *at } else { (*at).max(meeting) });
            }
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
            let has_pending = !c.queue.is_empty() || c.upload.is_some() || (announcing && c.carousel.as_ref().map(|k| k.has_work(&self.store, self.now)).unwrap_or(false));
            if has_pending {
                let mut t = c.busy_until.max(c.pace_until).max(c.fatsoen.backoff_until).max(self.now + 1);
                if c.queue.is_empty() {
                    t = t.max(c.content_until);
                }
                if c.queue.is_empty() && !announcing {
                    if let Some(u) = &c.upload {
                        t = t.max(u.start_at);
                    }
                }
                d = d.min(t);
            }
            if announcing {
                if let Some(t) = c.carousel.as_ref().and_then(|k| k.next_repass(&self.store)) {
                    d = d.min(t.max(self.now + 1));
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
        // Objects that registration completed (every symbol came before the metadata) complete
        // like any other: once, with the same consequences.
        while let Some(id) = self.quiet_complete.pop() {
            self.on_complete(id, &mut out);
        }
        out
    }

    // ---------------------------------------------------------------- helpers

    fn add_want(&mut self, id: ShortId) {
        if self.wants.insert(id) {
            let now = self.now;
            let p = self.progress.entry(id).or_default();
            p.wanted_at = now;
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
            self.excursions(out);
        }
        for i in 0..self.carriers.len() {
            let score = self.score;
            let caps = self.caps();
            let t = match self.carriers[i].election.as_mut() {
                Some(e) => e.tick(now, score, caps, &mut self.rng),
                None => None,
            };
            if let Some(t) = t {
                if t == Transition::BecameCandidate && self.hops(i) {
                    let at = self.step_up_time(now, score, caps);
                    if let Some(e) = self.carriers[i].election.as_mut() {
                        e.step_up_at(at);
                    }
                }
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
        self.renditions_due_check();
        self.follower_want_check();
        self.conflict_report_check();
        self.correction_check();
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
        // Neighbours heard more than once: a made-up name costs a frame each time it is used.
        let heard = self.neighbors.values().filter(|n| n.heard_count >= 2).count();
        let mut s: u32 = (heard.min(64) * 4) as u32;
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
                self.drop_grants_unless_announcing();
                out.push(Action::Role { carrier, role: Role::Candidate, announcer: NodeId::NONE, now: self.now });
            }
            Transition::BecameAnnouncer => {
                let mut car = Carousel::new(self.carousel_params());
                for a in self.manifests.values().filter_map(|i| i.adopted) {
                    car.add_manifest(a.short, true);
                }
                self.carriers[carrier].carousel = Some(car);
                self.carriers[carrier].upload = None;
                // Serve everything any known manifest references; want what we lack, by name and
                // length.
                let to_want: Vec<(ShortId, u32)> = self.manifests.values().filter_map(|i| i.current()).filter(|r| !self.store.has_complete(&r.short)).map(|r| (r.short, r.len)).collect();
                for (id, len) in to_want {
                    if self.store.ensure_hint(id, len, ContentType::Manifest) {
                        self.quiet_complete.push(id);
                    } else {
                        self.add_want(id);
                    }
                }
                // The manifests we hold were adopted by a follower, for the channels it follows
                // and in the form it plays. An announcer serves every channel, so it adopts them
                // again as one: their objects are registered and what it lacks is wanted.
                let held: Vec<ShortId> = self.manifests.values().filter_map(|i| i.adopted).filter(|a| self.store.has_complete(&a.short)).map(|a| a.short).collect();
                for short in held {
                    if let Some(m) = self.store.bytes(&short).and_then(|b| Manifest::decode(b).ok()) {
                        self.adopt_manifest(&m, short);
                    }
                }
                self.next_beacon = self.now;
                self.next_gossip = self.now;
                out.push(Action::Role { carrier, role: Role::Announcer, announcer: self.cfg.id, now: self.now });
            }
            Transition::BecameFollower(to) | Transition::AnnouncerChanged(to) => {
                self.keep_until = self.keep_until.max(self.now + self.cfg.params.want_ttl_ms);
                self.following_since = self.now;
                // What our announcer asked for and granted is about that announcer.
                self.ann_asks.clear();
                self.ann_grants.clear();
                self.carriers[carrier].carousel = Some(Carousel::new(self.carousel_params()));
                self.carriers[carrier].upload = None;
                self.drop_grants_unless_announcing();
                self.want_refresh = true;
                out.push(Action::Role { carrier, role: Role::Follower, announcer: to, now: self.now });
            }
        }
    }

    /// Content crosses a cell boundary wherever a follower of one cell can hear the other cell:
    /// a holder that hears a neighbouring announcer uploads to it when asked (§4), and a
    /// follower that hears a neighbouring announcer with something it wants, which its own
    /// announcer cannot get, goes and fetches it. That is an excursion: follow that announcer,
    /// for what it has rather than how well we hear it, until we have what we came for or it
    /// brings nothing for `T_excursion`; then follow by signal again, and the object is in our
    /// own cell, where our announcer's ask finds it like any holder's.
    fn excursions(&mut self, out: &mut Vec<Action>) {
        let now = self.now;
        let t = self.cfg.params.t_excursion_ms;
        let i = self.cell_carrier();
        if !self.carriers.get(i).map(|c| c.p.kind.is_bulk()).unwrap_or(false) || self.role(i) != Role::Follower {
            self.excursion = None;
            return;
        }
        let own = self.announcer_of(i);
        let since = |p: &Progress| p.wanted_at.max(p.last_progress);
        let ttl = self.cfg.params.want_ttl_ms;
        if let Some((c, to, began)) = self.excursion {
            let pinned = self.carriers[c].election.as_ref().map(|e| e.is_pinned()).unwrap_or(false);
            if own != to || !pinned {
                self.excursion = None;
                return;
            }
            // What we came for and it has: done when none is left, or when none of it moves.
            let haves = self.neighbors.get(&to).map(|n| n.haves.clone()).unwrap_or_default();
            let left: Vec<Millis> = self.wants.iter().filter(|w| haves.contains(w)).map(|w| self.progress.get(w).map(since).unwrap_or(0)).collect();
            if left.is_empty() || left.iter().all(|s| now >= *s + t) {
                self.excursion = None;
                // A visit that brought nothing at all: whatever that announcer lists, it does not
                // serve it to us. Do not go back for a while.
                if !self.progress.values().any(|p| p.last_progress >= began) {
                    let (score, caps) = (self.score, self.caps());
                    if let Some(e) = self.carriers[c].election.as_mut() {
                        e.shun(now, to, now + ttl, score, caps, &mut self.rng);
                    }
                }
                let tr = self.carriers[c].election.as_mut().and_then(|e| e.end_visit(now));
                if let Some(tr) = tr {
                    self.on_transition(c, tr, out);
                }
            }
            return;
        }
        // Our own announcer is held to evidence too (ABUSE.md, "election capture"). An honest
        // announcer does one of three things with what its follower asks for: it serves it, or,
        // lacking it, asks for it itself or grants it to an uploader. One that has done none of
        // them for an object we want, and lists it or not, so that we have never had one symbol
        // of it, for longer than an honest announcer can take to pass an object it was asked for
        // (its repetition ceiling, plus one interval for the ask), does not serve. That counts
        // from when we began to follow it: what we waited for under another announcer, or as
        // one, is no evidence against this one. Or it lists one of our own objects and nobody has
        // been heard sending it for that long since we published it.
        let w = crate::carousel::longest_spacing(self.cfg.params.t_want_min_ms) + self.cfg.params.t_want_min_ms;
        let from = self.following_since;
        let own_haves = self.neighbors.get(&own).map(|n| n.haves.clone()).unwrap_or_default();
        let claims_or_ignores = |x: &ShortId| own_haves.contains(x) || !self.ann_seeking(x);
        let never_served = self.wants.iter().any(|x| claims_or_ignores(x) && self.progress.get(x).map(|p| p.last_progress == 0 && now >= p.wanted_at.max(from) + w).unwrap_or(false))
            || self.pending_ack.iter().any(|(p, since)| own_haves.contains(p) && now >= *since + w);
        // Sooner where the channel is silent: an announcer that lists what we wait for and has not
        // put one symbol of anything on the air for `T_excursion`. Silence alone proves nothing:
        // an honest announcer whose repetitions a WANT flood holds back is silent too, for up to
        // its ceiling. So we ask it for one symbol of what we wait for, every `T_nack_stall`; it
        // answers from the front of its next round whatever the backoff. One that has still sent
        // nothing at all `T_want_min` later serves nothing.
        let quiet_since = self.last_bulk_rx.max(from);
        let waiting = self.wants.iter().find(|x| claims_or_ignores(x) && self.progress.get(x).map(|p| now >= since(p).max(from) + t).unwrap_or(false)).copied();
        let mut silent = false;
        if let (Some(x), true) = (waiting, now >= quiet_since + t) {
            silent = now >= quiet_since + t + self.cfg.params.t_want_min_ms;
            if !silent && now >= self.last_probe + self.cfg.params.t_nack_stall_ms {
                self.probe(x, own);
            }
        }
        if never_served || silent {
            let (score, caps) = (self.score, self.caps());
            let tr = self.carriers[i].election.as_mut().and_then(|e| e.shun(now, own, now + ttl, score, caps, &mut self.rng));
            if let Some(tr) = tr {
                if tr == Transition::BecameCandidate && self.hops(i) {
                    let at = self.step_up_time(now, score, caps);
                    if let Some(e) = self.carriers[i].election.as_mut() {
                        e.step_up_at(at);
                    }
                }
                self.on_transition(i, tr, out);
            }
            return;
        }
        // Stalled, and not merely waiting its turn: a busy cell delivers late; only a cell that
        // cannot get an object sends its followers out for it. An object our announcer lists, it
        // has: we wait for it, or find above that it lies.
        let stalled: Vec<ShortId> = self.wants.iter().filter(|w| !own_haves.contains(w) && !self.ann_granted_within(w, t) && self.progress.get(w).map(|p| now >= since(p) + t).unwrap_or(false)).copied().collect();
        if stalled.is_empty() {
            return;
        }
        let heard: Vec<NodeId> = self.carriers[i].election.as_ref().map(|e| e.heard_ids(now, own).collect()).unwrap_or_default();
        let best = heard
            .iter()
            .filter(|a| !self.carriers[i].election.as_ref().map(|e| e.is_shunned(**a, now)).unwrap_or(false))
            .filter_map(|a| self.neighbors.get(a).map(|n| (*a, n)))
            .filter(|(_, n)| stalled.iter().any(|w| n.haves.contains(w)))
            .max_by_key(|(_, n)| n.rssi)
            .map(|(a, _)| a);
        if let Some(to) = best {
            let tr = self.carriers[i].election.as_mut().and_then(|e| e.visit(now, to));
            if let Some(tr) = tr {
                self.excursion = Some((i, to, now));
                self.stats.excursions += 1;
                // A fresh start for what we came for: the stall clock runs from the visit.
                for w in &stalled {
                    if let Some(p) = self.progress.get_mut(w) {
                        p.wanted_at = now;
                    }
                }
                self.on_transition(i, tr, out);
            }
        }
    }

    /// Whether our announcer has named an uploader for `id` within the last `window`. If it has
    /// not, our cell cannot get the object: a named repair waits `T_grant` for that (a grant
    /// lapses after as long without a symbol), an excursion, which costs more, `T_excursion`.
    fn ann_granted_within(&self, id: &ShortId, window: Millis) -> bool {
        self.ann_grants.get(id).map(|t| self.now < *t + window).unwrap_or(false)
    }

    /// What this node is: the part of the score that does not depend on its role.
    fn caps(&self) -> u8 {
        (if self.cfg.mains { CAP_MAINS } else { 0 }) | (if self.cfg.has_ip { CAP_IP } else { 0 })
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
            caps: self.caps(),
            next_ms: self.cfg.params.election.t_beacon_ms.min(65535) as u16,
            round,
            utc: 0,
            time_quality: 0,
            colour: self.my_colour(),
            colours: self.colours(),
            upload_phases: if self.divides_listening_time(carrier) { self.upload_phase_count() } else { 1 },
            occupancy,
        }
    }

    /// Whether an announcer on `carrier` divides its listening time into upload phases: on every
    /// radio carrier. Under polite access every transmission is short and followed by a pause, so
    /// an upload is spread over minutes and hidden uploaders overlap. Where no regulator caps the
    /// sender, uploads to an announcer wait for the end of the meeting dwell and start together:
    /// 45 % of ESP-NOW's upload frames collided at their announcer (FEASIBILITY.md §15). Under a
    /// duty cycle a holder that sends back to back never finds the channel busy, while one that
    /// does backs off over a window that doubles each time: one source uploaded at a third of its
    /// rate for 25 minutes (FEASIBILITY.md §18). A lone holder has the whole cycle, so dividing
    /// costs it nothing.
    fn divides_listening_time(&self, carrier: usize) -> bool {
        self.carriers[carrier].p.kind != CarrierKind::Ip
    }

    /// The receiver divides its listening time among those it asked to speak: each grant carries
    /// its own phase, and the cycle has as many phases as the highest one in use. One upload gets
    /// all the time; hidden uploaders never overlap.
    fn upload_phase_count(&self) -> u8 {
        self.phases_in_use().map(|p| p + 1).max().unwrap_or(1).max(1)
    }

    /// Phases held by running grants and by reserved repairs.
    fn phases_in_use(&self) -> impl Iterator<Item = u8> + '_ {
        self.grants.values().map(|(_, _, p)| *p).chain(self.repair_phases.values().map(|(p, _)| *p))
    }

    /// `node` no longer announces: stop uploading to it and forget the grants it gave us.
    fn forget_announcer(&mut self, node: NodeId) {
        for c in self.carriers.iter_mut() {
            c.drop_uploads_to(node);
        }
        self.granted_to_us.retain(|(_, a), _| *a != node);
        self.offers.retain(|(_, a, _)| *a != node);
    }

    /// Grants are the announcer's: they name who speaks in its listening time. A node that
    /// stops announcing listens to someone else, so its grants end with its role; kept, they
    /// would ride along in its asks as a follower and keep holders talking to nobody. The same
    /// holds for wants it took on for its cell.
    fn drop_grants_unless_announcing(&mut self) {
        if !self.is_announcing() {
            self.grants.clear();
            self.repair_phases.clear();
            // What we wanted only to serve others goes too.
            self.prune_wants();
        }
    }

    /// The lowest phase no grant or repair uses, if any is left.
    fn free_upload_phase(&self) -> Option<u8> {
        (0..MAX_UPLOAD_PHASES).find(|p| !self.phases_in_use().any(|q| q == *p))
    }

    /// The phase for `holder`: listening time is divided among those who speak, not among the
    /// objects they bring. A holder uploads one object at a time, so every grant to it shares the
    /// phase of its first; a phase per object left most of a cycle idle and made an hour of
    /// music in 3-minute pieces arrive three times later than in one piece (FEASIBILITY.md §13).
    fn phase_for(&self, holder: NodeId) -> Option<u8> {
        self.grants.values().find(|(h, _, _)| *h == holder && !holder.is_none()).map(|(_, _, p)| *p).or_else(|| self.free_upload_phase())
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
            c.content_until = 0;
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
        self.excursion = None;
        self.ann_asks.clear();
        self.ann_grants.clear();
        self.grants.clear();
        self.repair_phases.clear();
        self.phase_heard = [0; MAX_UPLOAD_PHASES as usize];
        self.granted_to_us.clear();
        self.offered_with.clear();
        self.root_follow_ups.clear();
        self.ask_rest_at = None;
        self.offers.clear();
        self.corrections.clear();
        self.correction_at = None;
        self.keep_until = now + p.want_ttl_ms;
        for id in self.own_objects.iter().chain(self.own_manifests.iter().map(|(_, s, _, _)| s)) {
            self.pending_ack.insert(*id, now);
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
        let g = Gossip { node: self.cfg.id, announcer: self.gossip_announcer_field(), announcer_colour: self.announcer_colouring_field().0, announcer_colours: self.announcer_colouring_field().1, heard, have: Vec::new(), have_sets: Vec::new(), want: Vec::new(), sets: Vec::new() };
        let cell = self.cell_carrier();
        self.enqueue(cell, Frame::Gossip(g));
        self.stats.conflict_reports_sent += 1;
        self.report_due = false;
        self.last_report = self.now;
    }

    fn gossip_round(&mut self) {
        let announcing = self.is_announcing();
        // What we have: an announcer what its carousel serves, in a rotation when it does not all
        // fit; anyone else its own objects not yet carried. Pieces of a manifest go as sets.
        let (have, have_sets) = if announcing {
            let ids: Vec<ShortId> = self.store.complete_ids().copied().collect();
            let (have, sets, taken) = self.pack_have(&ids, self.have_cursor);
            self.have_cursor = self.have_cursor.wrapping_add(taken.max(1));
            (have, sets)
        } else {
            let ids: Vec<ShortId> = self.pending_ack.keys().filter(|id| self.store.has_complete(id)).copied().collect();
            self.pack_offer(&ids)
        };
        let (want, sets) = self.take_ask(false);
        if !have.is_empty() || !have_sets.is_empty() || !want.is_empty() || !sets.is_empty() {
            let g = Gossip { node: self.cfg.id, announcer: self.gossip_announcer_field(), announcer_colour: self.announcer_colouring_field().0, announcer_colours: self.announcer_colouring_field().1, heard: self.heard_field(), have, have_sets, want, sets };
            self.enqueue(self.cell_carrier(), Frame::Gossip(g));
        }
        let mut whole = false;
        let entries: Vec<AnnounceEntry> = if announcing {
            let all: Vec<AnnounceEntry> = self
                .manifests
                .iter()
                // Only what it serves: a manifest it adopted and holds. Passing on an announcement
                // it has not verified would spread a false seq through every cell.
                .filter_map(|(c, i)| i.adopted.filter(|a| self.store.has_complete(&a.short)).map(|a| AnnounceEntry { channel: *c, manifest: a.short, seq: a.seq, len: a.len }))
                .collect();
            if all.len() <= MAX_ANNOUNCE_ENTRIES {
                whole = true;
                all
            } else {
                // In channel order from a cursor that steps one entry less than a frame holds:
                // every two neighbours of the list share some frame, so a follower can tell
                // from a frame alone that a channel between them is not on it.
                let n = MAX_ANNOUNCE_ENTRIES;
                let v: Vec<AnnounceEntry> = (0..n).map(|j| all[(self.announce_cursor + j) % all.len()].clone()).collect();
                self.announce_cursor = (self.announce_cursor + n - 1) % all.len();
                v
            }
        } else {
            self.own_manifests
                .iter()
                .filter(|(_, s, _, _)| self.pending_ack.contains_key(s))
                .take(MAX_ANNOUNCE_ENTRIES)
                .map(|(c, s, seq, len)| AnnounceEntry { channel: *c, manifest: *s, seq: *seq, len: *len })
                .collect()
        };
        // An announcer that knows no manifest says so too: its followers then tell it theirs.
        if !entries.is_empty() || whole {
            let cell = self.cell_carrier();
            self.enqueue(cell, Frame::ManifestAnnounce(ManifestAnnounce { node: self.cfg.id, entries: entries.clone(), whole }));
            if self.ctrl != cell {
                self.enqueue(self.ctrl, Frame::ManifestAnnounce(ManifestAnnounce { node: self.cfg.id, entries, whole }));
            }
        }
    }

    /// Ask only for what is not coming: objects with a symbol in the last stall interval are
    /// flowing and are left out. Each entry carries the granted uploader, if any.
    /// Whether a round of asking is dear: on a hopping carrier an announcer asks, and holders
    /// offer to another cell, only in the meeting dwell, once a hop cycle. There a round asks for
    /// everything, in sets; where asking is cheap a round asks for the most listeners per byte
    /// first and comes back as soon as it has arrived, because then the order of arrival matters
    /// more than the number of rounds (FEASIBILITY.md §14).
    fn rounds_are_dear(&self) -> bool {
        self.hops(self.cell_carrier())
    }

    /// Every piece of every manifest we adopted, with its manifest and its place in it.
    fn piece_index(&self) -> BTreeMap<ShortId, (ShortId, u16)> {
        let mut index = BTreeMap::new();
        for (m, list) in &self.pieces {
            for (k, id) in list.iter().enumerate() {
                index.entry(*id).or_insert((*m, k as u16));
            }
        }
        index
    }

    /// Where each piece plays: its place in the list of its collection manifest, or 0 in a
    /// collection without an order, whose every piece is first. The order of asking, uploading
    /// and passing goes by it (PROTOCOL.md §4).
    fn places(&self) -> BTreeMap<ShortId, u16> {
        let mut places = BTreeMap::new();
        for (m, list) in &self.pieces {
            let ordered = !self.unordered.contains(m);
            for (k, id) in list.iter().enumerate() {
                let k = if ordered { k as u16 } else { 0 };
                let e = places.entry(*id).or_insert(k);
                *e = (*e).min(k);
            }
        }
        places
    }

    /// What we ask for in one GOSSIP: manifests, renditions and anything else by name, and the
    /// pieces of a manifest we hold as sets, one per granted holder and phase (open asks have
    /// none). Twenty pieces of one collection take one entry rather than twenty spread over three
    /// rounds, for a follower asking its announcer and for an announcer asking holders alike
    /// (PROTOCOL.md §3.3, FEASIBILITY.md §14). What does not fit waits for the next round.
    fn take_ask(&mut self, new_only: bool) -> (Vec<(ShortId, NodeId, u8)>, Vec<WantSet>) {
        let now = self.now;
        let ids = self.wants_to_ask(new_only);
        let index = if self.rounds_are_dear() { self.piece_index() } else { BTreeMap::new() };
        let mut singles = Vec::new();
        let mut groups: BTreeMap<SetKey, Vec<u16>> = BTreeMap::new();
        for id in ids {
            let (grant, phase) = self.grants.get(&id).map(|(h, _, p)| (*h, *p)).unwrap_or((NodeId::NONE, 0));
            // An announcer marks what its own followers asked for: an ask for listeners, which a
            // follower of another cell may relay, rather than one to fill its library (§3.3).
            let listened = self.is_announcing() && self.carriers.iter().filter_map(|c| c.carousel.as_ref()).any(|k| k.wanted_by(&id) > 0);
            let phase = phase | if listened { ASK_LISTENED } else { 0 };
            match index.get(&id) {
                Some((m, k)) => groups.entry((*m, grant, phase)).or_default().push(*k),
                None => singles.push((id, grant, phase)),
            }
        }
        let mut budget = WANT_BUDGET;
        let mut want = Vec::new();
        for e in singles {
            if budget < 13 || want.len() >= MAX_WANT {
                break;
            }
            budget -= 13;
            want.push(e);
        }
        // Which sets fit (§4, FEASIBILITY.md §19). Every collection gets one before any gets a
        // second: in the order of manifest ids the sets of the lowest ids and their grants filled
        // the budget round after round, and one announcer asked three times in 26 minutes for a
        // source whose manifest id sorted last. Collections in progress, a piece of which arrived
        // in the last `T_want_min` or is granted, go before the others: what is flowing is not
        // asked for, so a collection in progress gives up its place once its uploads run, and as
        // many collections are in flight as arrive; one that stopped arriving takes turns again.
        // Asked for side by side, twelve albums of a band L network shared its uploads and each
        // could be played through only later. Within that, the earliest place first (§2), then
        // the one asked for longest ago.
        let asked = |m: &ShortId, ks: &[u16]| -> Millis {
            let list = self.pieces.get(m);
            ks.iter().filter_map(|k| list.and_then(|l| l.get(*k as usize))).map(|id| self.progress.get(id).map(|p| p.last_want).unwrap_or(0)).min().unwrap_or(0)
        };
        let first = |m: &ShortId, ks: &[u16]| if self.unordered.contains(m) { 0 } else { ks.iter().copied().min().unwrap_or(u16::MAX) };
        let window = self.cfg.params.t_want_min_ms;
        let recent = |t: Millis| t != 0 && now < t + window;
        let moving: BTreeSet<ShortId> = groups
            .keys()
            .map(|g| g.0)
            .filter(|m| {
                self.moved.get(m).map(|t| recent(*t)).unwrap_or(false)
                    || self.pieces.get(m).map(|l| l.iter().any(|id| self.grants.contains_key(id) || self.progress.get(id).map(|p| recent(p.last_progress)).unwrap_or(false))).unwrap_or(false)
            })
            .collect();
        let mut order: Vec<(u16, Millis, SetKey)> = groups.iter().map(|(g, ks)| (first(&g.0, ks), asked(&g.0, ks), *g)).collect();
        order.sort_unstable();
        let mut seen = BTreeSet::new();
        let mut order: Vec<(bool, bool, usize, SetKey)> = order.into_iter().enumerate().map(|(i, (_, _, g))| (!seen.insert(g.0), !moving.contains(&g.0), i, g)).collect();
        order.sort_unstable();
        let mut sets = Vec::new();
        'groups: for (_, _, _, g) in order {
            let (m, grant, phase) = g;
            let mut ks = groups.remove(&g).unwrap_or_default();
            for set in PieceSet::cover(m, &mut ks) {
                if budget < WANT_SET_BYTES {
                    break 'groups;
                }
                budget -= WANT_SET_BYTES;
                sets.push(WantSet { set, grant, phase });
            }
        }
        for (id, _, _) in &want {
            self.progress.entry(*id).or_default().last_want = now;
        }
        for w in &sets {
            let list = self.pieces.get(&w.set.manifest).cloned().unwrap_or_default();
            for k in w.set.pieces() {
                if let Some(id) = list.get(k as usize) {
                    self.progress.entry(*id).or_default().last_want = now;
                }
            }
        }
        self.stats.want_sets += sets.len() as u64;
        self.stats.want_set_pieces += sets.iter().map(|w| w.set.bits.count_ones() as u64).sum::<u64>();
        (want, sets)
    }

    /// A HAVE list as ids and sets, from `skip` on in a rotation over `ids` when not all fit:
    /// pieces of a manifest we hold go as sets, everything else by name. Returns how many of the
    /// rotation's entries were taken.
    fn pack_have(&self, ids: &[ShortId], skip: usize) -> (Vec<ShortId>, Vec<PieceSet>, usize) {
        self.pack_have_after(&[], ids, skip)
    }

    /// A holder's HAVE: each root manifest among `ids` first, each followed by the collection
    /// manifests new in it that we hold, then the rest. What we list with a root we upload after
    /// it if we are granted it, and an announcer that grants it on this list expects them
    /// (PROTOCOL.md §4).
    fn pack_offer(&mut self, ids: &[ShortId]) -> (Vec<ShortId>, Vec<PieceSet>) {
        let mut lead: Vec<ShortId> = Vec::new();
        let mut with: Vec<(ShortId, Vec<ShortId>)> = Vec::new();
        for id in ids {
            let Some(r) = self.roots.get(id) else { continue };
            if lead.len() >= MAX_GOSSIP_IDS {
                break;
            }
            lead.push(*id);
            let mut listed = Vec::new();
            for s in r.collections.iter().filter(|c| c.changed).map(|c| c.manifest.id.short()) {
                if lead.len() < MAX_GOSSIP_IDS && self.store.has_complete(&s) && !lead.contains(&s) {
                    lead.push(s);
                    listed.push(s);
                }
            }
            with.push((*id, listed));
        }
        let (have, sets, _) = self.pack_have_after(&lead, ids, 0);
        self.offered_with.extend(with);
        (have, sets)
    }

    /// `pack_have` behind `lead`, which goes first and by name.
    fn pack_have_after(&self, lead: &[ShortId], ids: &[ShortId], skip: usize) -> (Vec<ShortId>, Vec<PieceSet>, usize) {
        let index = if self.rounds_are_dear() { self.piece_index() } else { BTreeMap::new() };
        let mut groups: BTreeMap<ShortId, Vec<u16>> = BTreeMap::new();
        let mut singles = Vec::new();
        for id in ids.iter().filter(|id| !lead.contains(id)) {
            match index.get(id) {
                Some((m, k)) => groups.entry(*m).or_default().push(*k),
                None => singles.push(*id),
            }
        }
        let mut entries: Vec<Result<PieceSet, ShortId>> = Vec::new();
        for (m, mut ks) in groups {
            entries.extend(PieceSet::cover(m, &mut ks).into_iter().map(Ok));
        }
        entries.extend(singles.into_iter().map(Err));
        let (mut have, mut sets, mut budget, mut taken) = (lead.to_vec(), Vec::new(), HAVE_BUDGET - 8 * lead.len(), 0);
        let n = entries.len();
        for j in 0..n {
            let e = &entries[(skip + j) % n];
            let cost = if e.is_ok() { HAVE_SET_BYTES } else { 8 };
            if cost > budget || (e.is_err() && have.len() >= MAX_GOSSIP_IDS) {
                break;
            }
            budget -= cost;
            taken += 1;
            match e {
                Ok(p) => sets.push(*p),
                Err(id) => have.push(*id),
            }
        }
        (have, sets, taken)
    }

    /// Diagnostic: `g` with its sets unpacked by our piece lists, as we would read it.
    pub fn unpacked(&self, g: &Gossip) -> Gossip {
        self.unpack_sets(g)
    }

    /// Sets in a received GOSSIP, unpacked into ids and WANT entries by the manifests we hold;
    /// pieces of a manifest we do not hold (an older or newer version) are left out, and the
    /// sender asks or offers again once it and we hold the same.
    fn unpack_sets(&self, g: &Gossip) -> Gossip {
        let mut e = g.clone();
        for p in &g.have_sets {
            if let Some(list) = self.pieces.get(&p.manifest) {
                e.have.extend(p.pieces().filter_map(|k| list.get(k as usize).copied()));
            }
        }
        for w in &g.sets {
            if let Some(list) = self.pieces.get(&w.set.manifest) {
                e.want.extend(w.set.pieces().filter_map(|k| list.get(k as usize).map(|id| (*id, w.grant, w.phase))));
            }
        }
        e.have_sets.clear();
        e.sets.clear();
        e
    }

    /// Whether a wanted object belongs in our next ask: it is not arriving (no symbol for
    /// `T_nack_stall`), and if someone is responsible for it, it is not nearly complete either.
    fn askable(&self, id: &ShortId, new_only: bool) -> bool {
        let p = self.progress.get(id).copied().unwrap_or_default();
        if !(self.now >= p.last_progress + self.cfg.params.t_nack_stall_ms || p.last_progress == 0) || (new_only && p.last_want != 0) {
            return false;
        }
        // An object that is nearly complete is repaired by NACK rather than re-asked in full, but
        // only once someone is responsible for it. The want list is where responsibility is
        // assigned, and an object we collected by overhearing a neighbouring cell has no uploader
        // at all: however complete it is, it belongs on the list until it has one.
        !self.left_to_repair(id)
    }

    /// Granted and nearly complete: the rest comes by NACK, not by asking again.
    fn left_to_repair(&self, id: &ShortId) -> bool {
        self.grants.contains_key(id) && matches!(self.store.entry(id).map(|e| e.progress()), Some((have, Some(total))) if self.nearly_complete(have, total))
    }

    /// Whether what is missing of an object is repaired by NACK rather than asked for again: at
    /// least `nack_threshold` of it, or all of it but one symbol. A NACK for one symbol is the
    /// smallest repair there is; under the fraction alone an object of two to four symbols, a
    /// collection manifest, could never be repaired, and one that had lost one of its two symbols
    /// waited minutes for the next round of asking (PROTOCOL.md §4, FEASIBILITY.md §20).
    fn nearly_complete(&self, have: u32, total: u32) -> bool {
        let thr = self.cfg.params.nack_threshold_permille as u64;
        have > 0 && ((have as u64) * 1000 >= thr * total as u64 || have + 1 >= total)
    }

    /// What we want and should ask for now, most listeners per byte first. Lapses grants that
    /// stopped bringing symbols on the way.
    fn wants_to_ask(&mut self, new_only: bool) -> Vec<ShortId> {
        let now = self.now;
        let stall = self.cfg.params.t_nack_stall_ms;
        let t_grant = self.cfg.params.t_grant_ms;
        // A grant lapses after `t_grant` without progress, counted from the grant or from the
        // last symbol it brought, whichever is later. An uploader that delivered once and then
        // fell silent is no more responsible than one that never started.
        let progress = &self.progress;
        let neighbors = &self.neighbors;
        let mut lapsed = 0u64;
        let mut lapsed_rssi = 0i64;
        let (mut foreign, mut unstarted) = (0u64, 0u64);
        let me = self.cfg.id;
        self.grants.retain(|id, (h, t, _)| {
            let p = progress.get(id).copied().unwrap_or_default();
            let keep = now < (*t).max(p.last_progress) + t_grant;
            if !keep {
                lapsed += 1;
                lapsed_rssi += neighbors.get(h).map(|n| n.rssi as i64).unwrap_or(-140);
                if neighbors.get(h).map(|n| n.announcer != me).unwrap_or(true) {
                    foreign += 1;
                }
                if p.last_progress < *t {
                    unstarted += 1;
                }
            }
            keep
        });
        let wants = &self.wants;
        self.repair_phases.retain(|id, (_, t)| {
            let p = progress.get(id).copied().unwrap_or_default();
            wants.contains(id) && now < (*t).max(p.last_progress) + t_grant
        });
        let grants = &self.grants;
        self.root_follow_ups.retain(|r, _| grants.contains_key(r));
        self.stats.grants_lapsed += lapsed;
        self.stats.grant_rssi_lapsed += lapsed_rssi;
        self.stats.grants_lapsed_foreign += foreign;
        self.stats.grants_lapsed_unstarted += unstarted;
        let ids: Vec<ShortId> = self.wants.iter().filter(|id| self.askable(id, new_only)).copied().collect();
        if ids.is_empty() {
            return Vec::new();
        }
        // Where rounds are dear a holder learns every object granted to it at once and uploads
        // them one after the other in its phase; holding grants back until its current upload
        // ended cost a round of asking per object. Where they are cheap a holder whose grant is
        // flowing is not asked for a second object until it is done (FEASIBILITY.md §14).
        let ids: Vec<ShortId> = if self.rounds_are_dear() {
            ids
        } else {
            let busy_holders: Vec<NodeId> = self
                .grants
                .iter()
                .filter(|(id, _)| {
                    let p = self.progress.get(id).copied().unwrap_or_default();
                    p.last_progress != 0 && now < p.last_progress + stall
                })
                .map(|(_, (h, _, _))| *h)
                .collect();
            ids.into_iter().filter(|id| !self.grants.get(id).map(|(h, _, _)| busy_holders.contains(h)).unwrap_or(false)).collect()
        };
        // Ask first for the earlier place (§4): a holder uploads one object at a time, so the
        // order of asking is the order of arriving, and a listener plays a collection from its
        // first piece. Among equal places, the most listeners per byte first (Smith's rule, as
        // in the carousel), so that a small object does not wait behind a large one. Most
        // listeners per byte first put a source's speech before the music it plays between, and
        // a band O town could start playing eight minutes later (FEASIBILITY.md §19).
        let mut ids = ids;
        let key = |id: &ShortId| {
            let listeners = self.carriers.iter().filter_map(|c| c.carousel.as_ref()).map(|k| k.wanted_by(id)).max().unwrap_or(0).max(1) as u128;
            let bytes = self.store.entry(id).and_then(|e| e.len()).unwrap_or(1).max(1) as u128;
            (listeners, bytes)
        };
        let places = self.places();
        let place = |id: &ShortId| places.get(id).copied().unwrap_or(0);
        ids.sort_by(|a, b| {
            let ((la, ba), (lb, bb)) = (key(a), key(b));
            place(a).cmp(&place(b)).then((lb * ba).cmp(&(la * bb))).then(a.cmp(b))
        });
        ids
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
        let full = (self.want_refresh || stalled) && now >= self.next_want_at;
        // What a manifest we just adopted names is the rest of an ask already made, not a new
        // one: asked for soon, at most every `T_gossip_min`, and only what was never asked for.
        // `T_want_min` spaces asking again for what has not come (PROTOCOL.md §4).
        let rest = !full && self.ask_rest_at.map(|t| now >= t).unwrap_or(false) && now >= self.last_want_tx + self.cfg.params.t_gossip_min_ms;
        if full || rest {
            let (want, sets) = self.take_ask(rest);
            self.ask_rest_at = None;
            if want.is_empty() && sets.is_empty() && rest {
                return;
            }
            let g = Gossip { node: self.cfg.id, announcer: self.gossip_announcer_field(), announcer_colour: self.announcer_colouring_field().0, announcer_colours: self.announcer_colouring_field().1, heard: self.heard_field(), have: Vec::new(), have_sets: Vec::new(), want, sets };
            let cell = self.cell_carrier();
            self.enqueue(cell, Frame::Gossip(g));
            self.stats.wants_sent += 1;
            self.last_want_tx = now;
            if full {
                self.want_refresh = false;
                self.next_want_at = now + self.cfg.params.t_want_min_ms;
            } else {
                self.stats.wants_rest += 1;
            }
        }
    }

    /// A manifest we follow brought new wants while we are on an excursion: ask for them soon.
    /// At home our announcer passes new manifests unasked and we ask on our usual cadence; on a
    /// visit only we ask, and every `T_want_min` spent waiting to ask for what the manifest
    /// just fetched names keeps the visit longer.
    fn ask_rest_soon(&mut self) {
        if self.excursion.is_some() {
            self.ask_rest();
        }
    }

    /// Ask for the rest soon: after a random wait of up to `T_offer`, in which an announcer
    /// passing it unasked makes the ask unneeded.
    fn ask_rest(&mut self) {
        if self.ask_rest_at.is_none() && !self.is_announcing() {
            self.ask_rest_at = Some(self.now + self.rng.below(self.cfg.params.t_offer_ms.max(1)));
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
        let stall = self.cfg.params.t_nack_stall_ms;
        let mut to_send: Option<(ShortId, u16, Vec<(u16, u16)>)> = None;
        for id in self.wants.iter() {
            let Some(e) = self.store.entry(id) else { continue };
            let (have, total) = e.progress();
            let Some(total) = total else { continue };
            if !self.nearly_complete(have, total) {
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
            // An announcer's NACK names the phase its answers use: the grant's, or one reserved
            // for this repair.
            let phase = if !announcing {
                0
            } else if let Some((_, _, p)) = self.grants.get(&id) {
                *p
            } else if let Some((p, _)) = self.repair_phases.get(&id) {
                *p
            } else if let Some(p) = self.phase_for(self.best_holder(&id)) {
                self.repair_phases.insert(id, (p, now));
                p
            } else {
                return;
            };
            self.progress.entry(id).or_default().last_nack = now;
            let cell = self.cell_carrier();
            // A follower asks its announcer; but where the announcer is itself asking for the
            // object and has granted it to nobody for `T_grant`, nobody in the cell can repair
            // it, and the follower names the holder it hears best in another cell, whose uploads
            // it overheard.
            let asked = self.ann_asks.get(&id).map(|t| now < *t + self.cfg.params.want_ttl_ms).unwrap_or(false);
            let ann_lacks = asked && !self.ann_granted_within(&id, self.cfg.params.t_grant_ms);
            let answerer = if !announcing {
                if ann_lacks { self.best_holder_elsewhere(&id) } else { NodeId::NONE }
            } else if let Some((h, _, _)) = self.grants.get(&id) {
                *h
            } else {
                self.best_holder(&id)
            };
            self.enqueue(cell, Frame::Nack(Nack { node: self.cfg.id, object: id, block, answerer, phase, missing: ranges }));
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
        if matches!(cand, Cand::Upload(..) | Cand::Carousel(Item::Symbol { .. })) && self.carriers[i].content_until > now {
            return;
        }
        let (bytes, class, frame_type) = match &cand {
            Cand::Queue(j) => {
                let p = &self.carriers[i].queue[*j];
                (p.bytes.clone(), p.class, p.frame_type)
            }
            Cand::Carousel(Item::Beacon { .. }) => {
                let b = self.make_beacon(i);
                (Frame::Beacon(b).encode(), Class::Control, FrameType::Beacon)
            }
            Cand::Carousel(Item::Symbol { object, block, esi, .. }) => {
                let mut buf = alloc::vec![0u8; SYMBOL_SIZE];
                let len = self.store.entry(object).and_then(|e| e.len());
                let (true, Some(len)) = (self.store.get_symbol(object, *block, *esi, &mut buf), len) else {
                    if let Some(car) = self.carriers[i].carousel.as_mut() {
                        car.advance(&self.store, now);
                    }
                    return;
                };
                (Frame::Bulk(Bulk { object: *object, block: *block, esi: *esi, len, payload: buf }).encode(), Class::Content, FrameType::Bulk)
            }
            Cand::Upload(object, block, esi, _) => {
                let mut buf = alloc::vec![0u8; SYMBOL_SIZE];
                let len = self.store.entry(object).and_then(|e| e.len());
                let (true, Some(len)) = (self.store.get_symbol(object, *block, *esi, &mut buf), len) else {
                    self.carriers[i].upload = None;
                    return;
                };
                if let Some(u) = self.carriers[i].upload.as_mut() {
                    if !u.started {
                        u.started = true;
                        self.stats.uploads_started += 1;
                        if u.list.is_some() {
                            self.stats.repairs_started += 1;
                        }
                    }
                }
                (Frame::Bulk(Bulk { object: *object, block: *block, esi: *esi, len, payload: buf }).encode(), Class::Content, FrameType::Bulk)
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
        // End of our upload phase, if we transmit in one: the phase is our turn to speak.
        let mut phase_end: Option<Millis> = None;
        if let Some((colour, colours)) = content_colour {
            // The meeting dwell is control plane only, and announcers that share a channel take
            // turns: content (carousel or upload) runs only in its announcer's slot.
            let dwell = self.cfg.params.dwell_ms.max(1);
            if self.in_rendezvous(i, now) {
                let t = (now / dwell + 1) * dwell;
                self.stats.defer_ms[0] += t - now;
                self.carriers[i].content_until = t;
                return;
            }
            if let Some(t) = self.slot_wait(i, colour, colours, now) {
                self.stats.defer_ms[1] += t.saturating_sub(now);
                self.carriers[i].content_until = t;
                return;
            }
            // An upload transmits only in its own phase of the announcer's listening time, so
            // uploaders that cannot hear each other never overlap there.
            let divides = self.divides_listening_time(i);
            if let (Cand::Upload(..), Some(u), true) = (&cand, self.carriers[i].upload.as_ref(), divides) {
                let announced = self.neighbors.get(&u.to).map(|n| n.upload_phases).unwrap_or(1).max(1) as u64;
                let phase = u.phase as u64;
                let phases = announced.max(phase + 1);
                if phases > 1 {
                    let width = self.cfg.params.t_upload_phase_ms.max(1);
                    let cycle = width * phases;
                    let start = (now / cycle) * cycle + phase * width;
                    let from = if now >= start + width { start + cycle } else { start };
                    if now < from || now + airtime as Millis > from + width {
                        let t = if now < from { from } else { from + cycle };
                        self.stats.defer_ms[1] += t.saturating_sub(now);
                        self.carriers[i].content_until = t;
                        return;
                    }
                    // A cycle's budget is spent inside the phase.
                    self.carriers[i].fatsoen.set_burst_at_least(cycle as u32);
                    phase_end = Some(from + width);
                }
            }
            // The announcer keeps quiet in a phase whose uploader it heard in the last two
            // cycles: it gave that time away, and a half-duplex radio that talks cannot listen.
            if let (Cand::Carousel(Item::Symbol { .. }), true) = (&cand, divides) {
                let width = self.cfg.params.t_upload_phase_ms.max(1);
                let k = self.upload_phase_count() as u64;
                let current = ((now / width) % k) as usize;
                let heard = self.phase_heard.get(current).copied().unwrap_or(0);
                if heard > 0 && now < heard + 2 * k * width {
                    let t = (now / width + 1) * width;
                    self.stats.defer_ms[1] += t - now;
                    self.carriers[i].content_until = t;
                    return;
                }
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
            self.carriers[i].hold(class, now + 1000);
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
            (Cand::Upload(..), Some(u)) => self.channel_for(i, self.listens_with(u.to), now),
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
                    self.carriers[i].hold(class, now + w);
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
                    let mut target = ((toff_min_ms as u64 * p) / (1000 - p)).clamp(airtime as u64, ton_max_ms as u64) as u32;
                    // In an upload phase nobody else speaks to the announcer: the whole phase,
                    // up to one permitted transmission, is ours.
                    if let Some(end) = phase_end {
                        target = target.max((end.saturating_sub(now) as u32).min(ton_max_ms));
                    }
                    let center = self.carriers[i].p.channels.get(channel as usize).copied().unwrap_or(0);
                    if self.discipline.burst_on_ms(band, center, now) + airtime > target {
                        self.carriers[i].content_until = now + toff_min_ms as Millis;
                        self.stats.defer_ms[2] += toff_min_ms as Millis;
                        return;
                    }
                }
            }
            if let Err(w) = self.carriers[i].fatsoen.take_airtime(now, airtime, budget, fresh) {
                self.carriers[i].content_until = now + w;
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
        if matches!(cand, Cand::Carousel(Item::Symbol { .. })) {
            self.stats.carousel_frames[!fresh as usize] += 1;
        }
        self.stats.tx_by_type[frame_type as usize] += 1;
        self.stats.tx_airtime_ms[i] += airtime as u64;
        self.carriers[i].jittered = false;
        let upload_to = match (&cand, self.carriers[i].upload.as_ref()) {
            (Cand::Upload(..), Some(u)) => Some(u.to),
            _ => None,
        };
        self.commit(i, cand);
        out.push(Action::Tx { carrier: i, channel, bytes, airtime_ms: airtime, class, frame_type, upload_to });
    }

    fn commit(&mut self, i: usize, cand: Cand) {
        let now = self.now;
        match cand {
            Cand::Queue(j) => {
                self.carriers[i].queue.remove(j);
            }
            Cand::Carousel(item) => {
                // A source that announces passes its own objects itself: its cell has them, as
                // when it hears someone else send them. Otherwise they stayed pending forever, and
                // it took the next announcer that listed one for a liar (PROTOCOL.md §5.2).
                if let Item::Symbol { object, .. } = item {
                    self.pending_ack.remove(&object);
                }
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
            Frame::Gossip(g) => self.rx_gossip(carrier, g, rssi),
            Frame::ManifestAnnounce(m) => self.rx_announce(m, rssi),
            Frame::Nack(n) => self.rx_nack(n, rssi),
        }
    }

    fn touch(&mut self, id: NodeId, rssi: i16) -> &mut Neighbor {
        let now = self.now;
        // At most `max_neighbours`: a name heard once gives way before one heard twice, and then
        // the one heard longest ago. Names are not checked, and every made-up one was kept for
        // `neighbor_ttl` (docs/ABUSE.md, "Someone else's firmware", item 5).
        if !self.neighbors.contains_key(&id) && self.neighbors.len() >= self.cfg.params.max_neighbours.max(1) {
            if let Some(gone) = self.neighbors.iter().min_by_key(|(_, n)| (n.heard_count >= 2, n.last_heard)).map(|(k, _)| *k) {
                self.neighbors.remove(&gone);
            }
        }
        let n = self.neighbors.entry(id).or_insert(Neighbor { last_heard: now, announcer: NodeId::NONE, score: 0, rssi, colour: 0, upload_phases: 1, heard_count: 0, haves: BTreeSet::new(), unread_sets: Vec::new() });
        n.last_heard = now;
        n.heard_count = n.heard_count.saturating_add(1);
        n.rssi = ((n.rssi as i32 + rssi as i32) / 2) as i16;
        n
    }

    /// At most `max_offered_ids` offered by all neighbours together: what the neighbour heard
    /// longest ago offered is forgotten first (docs/ABUSE.md, "Someone else's firmware", item 5).
    fn bound_offered(&mut self) {
        let cap = self.cfg.params.max_offered_ids.max(1);
        let mut total: usize = self.neighbors.values().map(|n| n.haves.len()).sum();
        while total > cap {
            let Some(id) = self.neighbors.iter().filter(|(_, n)| !n.haves.is_empty()).min_by_key(|(_, n)| n.last_heard).map(|(k, _)| *k) else { break };
            if let Some(n) = self.neighbors.get_mut(&id) {
                total -= n.haves.len();
                n.haves.clear();
                n.unread_sets.clear();
            }
        }
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
            n.upload_phases = b.upload_phases.max(1);
        }
        let me = self.cfg.id;
        let score = self.score;
        let caps = self.caps();
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
        let t = self.carriers[ti].election.as_mut().and_then(|e| e.on_beacon(now, b, rssi, near, me, score, caps));
        if let Some(t) = t {
            self.on_transition(ti, t, out);
        }
    }

    fn rx_bulk(&mut self, _carrier: usize, b: &Bulk, out: &mut Vec<Action>) {
        // Someone is sending this object: any offer of ours for it, and any answer we have not
        // begun, is moot. This is what keeps an ungranted repair to one sender. And if it is one
        // of ours, someone else carries it now.
        self.offers.retain(|(o, _, _)| *o != b.object);
        self.pending_ack.remove(&b.object);
        self.last_bulk_rx = self.now;
        let now = self.now;
        for c in self.carriers.iter_mut() {
            c.cancel_pending(b.object, now);
        }
        let interested = self.wants.contains(&b.object) || self.store.is_known(&b.object) || self.is_announcing();
        if !interested {
            self.stats.bulk_uninterested += 1;
            return;
        }
        let put = self.store.put_symbol(b.object, b.block, b.esi, b.len, &b.payload);
        // A symbol of an object we granted or asked to repair is an upload to us: the phase it
        // uses is in use.
        if matches!(put, Put::New | Put::Complete | Put::Duplicate) && self.is_announcing() {
            let phase = self.grants.get(&b.object).map(|(_, _, p)| *p).or_else(|| self.repair_phases.get(&b.object).map(|(p, _)| *p));
            if let Some(p) = phase {
                self.phase_heard[p as usize] = now;
            }
        }
        match put {
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
        // A manifest we asked for: what it names is the rest of that ask (PROTOCOL.md §4).
        let asked = self.progress.get(&id).map(|p| p.last_want != 0).unwrap_or(false);
        self.progress.remove(&id);
        let now = self.now;
        for (m, list) in &self.pieces {
            if list.contains(&id) {
                self.moved.insert(*m, now);
            }
        }
        // A grant ends when its object arrives.
        self.repair_phases.remove(&id);
        let granted = self.grants.get(&id).copied();
        if let Some((h, _, _)) = self.grants.remove(&id) {
            self.stats.grants_completed += 1;
            self.stats.grant_rssi_completed += self.neighbors.get(&h).map(|n| n.rssi as i64).unwrap_or(-140);
        }
        if self.store.entry(&id).map(|e| e.kind() == ContentType::Renditions).unwrap_or(false) {
            self.load_table(&id);
        }
        match self.store.entry(&id).map(|e| e.kind()) {
            Some(ContentType::Manifest) => {
                if let Some(bytes) = self.store.bytes(&id).map(|b| b.to_vec()) {
                    if let Ok(m) = Manifest::decode(&bytes) {
                        self.adopt_manifest(&m, id);
                    }
                }
                if let (Some((h, _, phase)), true) = (granted, self.is_announcing()) {
                    self.extend_root_grant(id, h, phase);
                }
            }
            Some(ContentType::Collection) => {
                // Read only as the adopted root we hold names it: that root is what makes it
                // authentic (PROTOCOL.md §2).
                let named: Vec<(ChannelId, CollectionRef)> = self.named_collections().filter(|(_, c)| c.manifest.id.short() == id).map(|(chan, c)| (chan, c.clone())).collect();
                for (chan, c) in named {
                    self.adopt_collection(chan, &c, true);
                }
            }
            _ => {}
        }
        if asked && self.store.entry(&id).map(|e| e.kind().is_manifest()).unwrap_or(false) {
            self.ask_rest();
        }
        if self.is_announcing() {
            // Tell sources we have it, and ask for what we still lack, soon.
            self.gossip_soon();
        }
    }

    fn adopt_manifest(&mut self, m: &Manifest, short: ShortId) {
        let chan = m.channel_id();
        let info = self.manifests.get(&chan).copied().unwrap_or_default();
        if let Some(a) = info.adopted {
            if a.seq > m.seq || (a.seq == m.seq && a.short != short) {
                return;
            }
        }
        let len = self.store.entry(&short).and_then(|e| e.len()).unwrap_or(0);
        // A newer announcement still pending stays wanted; one this manifest answers is done.
        let announced = info.announced.filter(|n| n.seq > m.seq);
        let old = self.set_root(chan, m, short, len, announced);
        // Adopted again (followed after the fact, or as a new announcer): as the root, so each
        // collection manifest is passed once more where there is a carousel.
        let again = old.map(|o| o.short == short).unwrap_or(false);
        if !again {
            self.stats.manifests_adopted += 1;
        }
        // You carry what you listen to: objects are registered (and thus collected from the air)
        // only for channels and collections we follow or, as announcer, serve.
        let interested = self.follows_channel(&chan) || self.is_announcing();
        let mut to_want = Vec::new();
        let mut to_adopt = Vec::new();
        if interested {
            // The rendition table: fetched by those who need renditions, known by name to all.
            if let Some(t) = m.renditions {
                let meta = ObjectMeta { id: t.id, len: t.len, kind: ContentType::Renditions };
                if self.store.ensure(meta) {
                    self.quiet_complete.push(t.id.short());
                }
                let needs = (!self.cfg.decodes && self.follows_channel(&chan)) || self.cfg.renders;
                if self.store.has_complete(&t.id.short()) {
                    self.load_table(&t.id.short());
                } else if needs {
                    to_want.push(t.id.short());
                }
            }
            // Every collection manifest of a channel we follow at all, and of every channel we
            // serve: they are small, and they are how we know what another cell asks for when it
            // asks us to relay (§4). Covers only of what we carry.
            let all: Vec<(CollectionRef, bool)> = m.collections.iter().map(|c| (c.clone(), self.carries(&chan, c.cid))).collect();
            for (c, carried) in &all {
                let id = c.manifest.id.short();
                if self.store.ensure(ObjectMeta { id: c.manifest.id, len: c.manifest.len, kind: ContentType::Collection }) {
                    self.quiet_complete.push(id);
                }
                if self.store.has_complete(&id) {
                    let new = again || self.collections.get(&(chan, c.cid)) != Some(&id);
                    to_adopt.push((c.clone(), new));
                } else {
                    to_want.push(id);
                }
                if let (Some(v), true) = (c.cover, *carried) {
                    if self.store.ensure(ObjectMeta { id: v.id, len: v.len, kind: ContentType::Image }) {
                        self.quiet_complete.push(v.id.short());
                    }
                    if !self.store.has_complete(&v.id.short()) {
                        to_want.push(v.id.short());
                    }
                }
            }
        }
        for (c, new) in to_adopt {
            self.adopt_collection(chan, &c, new);
        }
        if self.follows_channel(&chan) && !to_want.is_empty() {
            self.ask_rest_soon();
        }
        for id in to_want {
            self.add_want(id);
        }
        if old.map(|o| o.short != short).unwrap_or(false) || info.announced != announced {
            self.prune_wants();
        }
        if self.follows_channel(&chan) {
            self.want_refresh = true;
        }
    }

    /// Make `m` the adopted root manifest of `chan`: index what it names, and on every carousel
    /// put it in place of the old root. Collections it no longer names have left the channel.
    /// Returns the root it replaces.
    fn set_root(&mut self, chan: ChannelId, m: &Manifest, short: ShortId, len: u32, announced: Option<ManifestRef>) -> Option<ManifestRef> {
        let old = self.manifests.get(&chan).and_then(|i| i.adopted);
        self.manifests.insert(chan, ManifestInfo { adopted: Some(ManifestRef { seq: m.seq, short, len }), announced });
        if let Some(o) = old.filter(|o| o.short != short) {
            self.roots.remove(&o.short);
            for c in self.carriers.iter_mut() {
                if let Some(car) = c.carousel.as_mut() {
                    car.remove_manifest(&o.short);
                }
            }
        }
        self.roots.insert(short, RootIndex { collections: m.collections.clone(), renditions: m.renditions });
        let left: Vec<(ChannelId, u32)> = self.collections.keys().filter(|(c, cid)| *c == chan && !m.collections.iter().any(|x| x.cid == *cid)).copied().collect();
        for k in left {
            if let Some(s) = self.collections.remove(&k) {
                self.drop_collection_manifest(s);
            }
        }
        for c in self.carriers.iter_mut() {
            if let Some(car) = c.carousel.as_mut() {
                car.add_manifest(short, true);
            }
        }
        old
    }

    /// Adopt collection manifest `c` of `chan`, which the adopted root we hold names: learn its
    /// pieces, serve it, and want them if we listen to it or serve it. It takes the place of the
    /// collection's older manifest, whose pieces leave unless it lists them too. `new`: passed
    /// once unasked where there is a carousel.
    fn adopt_collection(&mut self, chan: ChannelId, c: &CollectionRef, new: bool) {
        let short = c.manifest.id.short();
        let Some(col) = self.store.bytes(&short).and_then(|b| Collection::decode(b).ok()) else { return };
        let list: Vec<ShortId> = col.pieces.iter().map(|o| o.id.short()).collect();
        // What neighbours said they have of this collection before we could read it.
        for nb in self.neighbors.values_mut() {
            for p in nb.unread_sets.iter().filter(|p| p.manifest == short) {
                for k in p.pieces() {
                    if let (Some(id), true) = (list.get(k as usize), nb.haves.len() < 512) {
                        nb.haves.insert(*id);
                    }
                }
            }
            nb.unread_sets.retain(|p| p.manifest != short);
        }
        self.bound_offered();
        let ordered = col.kind != CollectionKind::Singles;
        if ordered {
            self.unordered.remove(&short);
        } else {
            self.unordered.insert(short);
        }
        for car in self.carriers.iter_mut().filter_map(|c| c.carousel.as_mut()) {
            car.add_manifest(short, new);
            for (k, id) in list.iter().enumerate() {
                car.set_rank(*id, if ordered { k as u16 } else { 0 });
            }
        }
        self.pieces.insert(short, list);
        let replaced = self.collections.insert((chan, c.cid), short).filter(|o| *o != short);
        if let Some(o) = replaced {
            self.drop_collection_manifest(o);
        }
        let listens = self.listens(&chan, c.cid);
        if self.carries(&chan, c.cid) {
            // A device that cannot decode has no use for the codes of what it listens to.
            let skip_codes = !self.cfg.decodes && listens && !self.is_announcing();
            let mut to_want = Vec::new();
            for o in &col.pieces {
                if skip_codes && o.kind.codec().is_some() {
                    continue;
                }
                if self.store.ensure(o.meta()) {
                    self.quiet_complete.push(o.id.short());
                }
                if !self.store.has_complete(&o.id.short()) {
                    to_want.push(o.id.short());
                }
            }
            if listens && !to_want.is_empty() {
                self.ask_rest_soon();
            }
            for id in to_want {
                self.add_want(id);
            }
            if skip_codes && !self.renditions.is_empty() {
                self.plan_renditions();
            }
        }
        if replaced.is_some() {
            self.prune_wants();
        }
        if listens {
            self.want_refresh = true;
        }
    }

    /// A grant of root manifest `root` covers the collection manifests new in it that its holder
    /// `h` listed with it: it uploads them right after the root, in the same phase, so a root and
    /// what changed in it cost one round of asking rather than two (PROTOCOL.md §4). We take
    /// those we lack as granted to it.
    fn extend_root_grant(&mut self, root: ShortId, h: NodeId, phase: u8) {
        let Some((from, listed)) = self.root_follow_ups.remove(&root) else { return };
        let Some(r) = self.roots.get(&root).filter(|_| from == h) else { return };
        let ids: Vec<ShortId> = r.collections.iter().filter(|c| c.changed).map(|c| c.manifest.id.short()).filter(|s| listed.contains(s) && self.wants.contains(s) && !self.grants.contains_key(s)).collect();
        let now = self.now;
        for s in ids {
            self.grants.insert(s, (h, now, phase));
            self.stats.grants_given += 1;
            self.stats.follow_ups_granted += 1;
        }
    }

    /// What we upload after root manifest `root` when announcer `ann` grants it to us on carrier
    /// `i`: the collection manifests we listed with it, unless `ann` has said it holds one or it
    /// is on its way to it already.
    fn root_follow_ups(&self, root: &ShortId, ann: NodeId, i: usize) -> Vec<ShortId> {
        let Some(listed) = self.offered_with.get(root) else { return Vec::new() };
        let c = &self.carriers[i];
        let held_there = |s: &ShortId| self.neighbors.get(&ann).map(|n| n.haves.contains(s)).unwrap_or(false);
        let sending = |s: &ShortId| c.upload.iter().chain(c.upload_queue.iter()).any(|u| u.object == *s && u.to == ann);
        listed.iter().filter(|s| self.store.has_complete(s) && !held_there(s) && !sending(s)).copied().collect()
    }

    /// What we know of `id` as a piece or a cover of a collection of a channel we follow, if it
    /// is one: what we need to fetch it on another cell's behalf.
    fn relay_meta(&self, id: &ShortId) -> Option<ObjectMeta> {
        for ((chan, _), held) in &self.collections {
            if !self.follows_channel(chan) || !self.pieces.get(held).map(|l| l.contains(id)).unwrap_or(false) {
                continue;
            }
            let col = self.store.bytes(held).and_then(|b| Collection::decode(b).ok())?;
            return col.pieces.iter().find(|o| o.id.short() == *id).map(|o| o.meta());
        }
        self.named_collections().filter(|(chan, _)| self.follows_channel(chan)).find_map(|(_, c)| c.cover.filter(|v| v.id.short() == *id).map(|v| ObjectMeta { id: v.id, len: v.len, kind: ContentType::Image }))
    }

    /// A collection manifest no collection uses any more: its pieces are no longer read by it,
    /// and carousels no longer serve it as a manifest.
    fn drop_collection_manifest(&mut self, s: ShortId) {
        if self.collections.values().any(|v| *v == s) {
            return;
        }
        self.unordered.remove(&s);
        self.moved.remove(&s);
        self.pieces.remove(&s);
        for c in self.carriers.iter_mut() {
            if let Some(car) = c.carousel.as_mut() {
                car.remove_manifest(&s);
            }
        }
    }

    fn rx_gossip(&mut self, _carrier: usize, g: &Gossip, rssi: i16) {
        if g.node == self.cfg.id {
            return;
        }
        let unread: Vec<PieceSet> = g.have_sets.iter().filter(|p| !self.pieces.contains_key(&p.manifest)).copied().collect();
        let unpacked;
        let g = if g.have_sets.is_empty() && g.sets.is_empty() {
            g
        } else {
            unpacked = self.unpack_sets(g);
            &unpacked
        };
        // A node that says it follows someone else is not listening for uploads: whatever we
        // were sending it, and the grants it gave us, ended with its role.
        if g.announcer != g.node {
            self.forget_announcer(g.node);
        }
        {
            let n = self.touch(g.node, rssi);
            n.announcer = g.announcer;
            for h in &g.have {
                if n.haves.len() < 512 {
                    n.haves.insert(*h);
                }
            }
            for p in unread {
                n.unread_sets.retain(|q| !(q.manifest == p.manifest && q.first == p.first));
                if n.unread_sets.len() < 16 {
                    n.unread_sets.push(p);
                }
            }
        }
        self.bound_offered();
        let me = self.cfg.id;
        let now = self.now;
        // An announcer's HAVE lists what its carousel serves; it is not an offer, since
        // announcers do not upload. Only a follower's HAVE is one.
        let offering = g.announcer != g.node;
        // Another holder offered the same objects: our pending offers are redundant.
        if offering && !g.have.is_empty() {
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
            // A want is served by the announcer it names. Announcers that overhear a follower of
            // another cell and serve it too start the same pass at the same instant, and where
            // they are hidden from each other every frame of both collides at that follower.
            let addressed = g.announcer == me;
            let mut new_wants = Vec::new();
            for (w, _, _) in g.want.iter().filter(|_| addressed) {
                // A rendition is for a programme about to play: an ask far ahead of its slot is
                // not a listener's, and the device will ask again when it is due.
                if self.rendition_not_due(w) {
                    self.stats.renditions_refused += 1;
                    continue;
                }
                // An ask is recorded only for an object we can name, one we know or a rendition
                // we know of: anyone can ask for made-up ids, and each would have been kept for
                // `want_ttl` (docs/ABUSE.md, "Someone else's firmware", item 5).
                if self.store.is_known(w) || self.renditions.contains_key(w) {
                    if let Some(car) = self.carriers[i].carousel.as_mut() {
                        car.on_want(*w, g.node, now);
                    }
                }
                // A rendition a follower asks for: make it if we can, else ask for it like any
                // object. An announcer never wants a rendition for itself.
                if !self.store.has_complete(w) && !self.store.is_known(w) {
                    if let Some(&(_, meta)) = self.renditions.get(w) {
                        self.store.ensure(meta);
                    }
                }
                if !self.store.has_complete(w) && self.renditions.contains_key(w) && self.render(w) {
                    continue;
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
                let phase = self.phase_for(g.node);
                if let (true, true, false, Some(phase)) = (offering, self.wants.contains(h), self.grants.contains_key(h), phase) {
                    self.grants.insert(*h, (g.node, now, phase));
                    // A root manifest comes with what its holder listed with it (§4).
                    if self.store.entry(h).map(|e| e.kind() == ContentType::Manifest).unwrap_or(false) {
                        self.root_follow_ups.insert(*h, (g.node, g.have.clone()));
                    }
                    self.stats.grants_given += 1;
                    self.gossip_soon();
                }
            }
            // A report that another announcer is heard somewhere is not a reason to yield to it:
            // we cannot follow what we cannot hear. Announcers yield on hearing a better beacon
            // themselves; reports only feed the colouring (§5.3).
        }
        // Holder side: an announcer (ours or a neighbouring cell's) tells us what it has and
        // wants. An open ask is answered with an offer; only the holder it then grants uploads.
        let from_announcer = g.announcer == g.node;
        for i in 0..self.carriers.len() {
            if !self.carriers[i].p.kind.is_bulk() || self.role(i) == Role::Announcer {
                continue;
            }
            let own = self.announcer_of(i) == g.node;
            if own || from_announcer {
                // An announcer says it has an object of ours. We believe it once we uploaded the
                // object to it; a claim alone is no reason to stop offering (ABUSE.md). That holds
                // for a neighbouring cell's announcer too: a source that uploaded there and later
                // followed it otherwise took it for a liar (FEASIBILITY.md §13).
                for h in &g.have {
                    if self.granted_to_us.contains_key(&(*h, g.node)) {
                        self.pending_ack.remove(h);
                    }
                }
            }
            if own {
                // Only what we know of: what our announcer asks for matters to us only for objects
                // we want or hold, and made-up ids would each be kept for `want_ttl`.
                for (w, grant, _) in g.want.iter().filter(|(w, _, _)| self.store.is_known(w)) {
                    self.ann_asks.insert(*w, now);
                    if !grant.is_none() {
                        self.ann_grants.insert(*w, now);
                    }
                }
            } else if !from_announcer {
                continue;
            }
            // Relaying for another cell (PROTOCOL.md §4): its announcer asks, for its listeners, for
            // a piece or cover of a channel we follow that we lack, and nobody has met the ask for
            // `T_want_min`: we fetch it from our own cell and keep it, to hand it on.
            if from_announcer && !own {
                let asked: Vec<ShortId> = g.want.iter().filter(|(w, grant, phase)| grant.is_none() && phase & ASK_LISTENED != 0 && !self.store.has_complete(w) && !self.wants.contains(w)).map(|(w, _, _)| *w).collect();
                // Only an ask the neighbourhood has not met for `T_want_min`: most are answered by
                // a holder within a round, and relaying those only duplicated the work.
                let wait = self.cfg.params.t_want_min_ms;
                for w in asked {
                    // Only an object of a channel we follow is relayed, so only its asks are kept.
                    let Some(meta) = self.relay_meta(&w) else { continue };
                    let first = *self.relay_asks.entry(w).or_insert(now);
                    if now < first + wait {
                        continue;
                    }
                    self.relayed.insert(w);
                    if self.store.ensure(meta) {
                        self.quiet_complete.push(w);
                    }
                    self.stats.relay_wants += 1;
                    self.add_want(w);
                }
            }
            let mut granted: Vec<Upload> = Vec::new();
            for (w, grant, phase) in g.want.iter() {
                // A rendition we can make counts as held; it is made only when we are granted it,
                // so that one node, not every capable one, spends the work.
                if !self.store.has_complete(w) && !self.can_render(w) {
                    continue;
                }
                if *grant == self.cfg.id && !self.render(w) {
                    continue;
                }
                if *grant == self.cfg.id {
                    // Granted: upload it, after whatever we are already uploading.
                    if self.granted_to_us.insert((*w, g.node), now).is_none() {
                        self.stats.grants_received += 1;
                    }
                    let c = &mut self.carriers[i];
                    // The grant names our phase; a running upload follows it if it changed.
                    for u in c.upload.iter_mut().chain(c.upload_queue.iter_mut()) {
                        if u.object == *w && u.to == g.node {
                            u.phase = *phase & PHASE_MASK;
                        }
                    }
                    let active = c.upload.as_ref().map(|u| u.object == *w && u.to == g.node).unwrap_or(false);
                    let queued = c.upload_queue.iter().any(|u| u.object == *w && u.to == g.node);
                    if !active && !queued && !granted.iter().any(|u| u.object == *w) {
                        let manifest = self.store.entry(w).map(|e| e.kind().is_manifest()).unwrap_or(false);
                        granted.push(Upload { object: *w, to: g.node, start_at: now, started: false, block: 0, esi: 0, list: None, phase: *phase & PHASE_MASK, manifest, order: 0 });
                    }
                } else if grant.is_none() {
                    // Open ask: offer, unless we are already uploading it to this announcer.
                    let uploading = self.carriers[i].upload.as_ref().map(|u| u.object == *w && u.to == g.node).unwrap_or(false);
                    if !uploading && !self.offers.iter().any(|(o, a, _)| o == w && *a == g.node) {
                        // A holder that is uploading offers last: a free holder's offer comes first
                        // and silences it; if nobody else holds the object, it is granted to us and
                        // waits behind our current upload instead of a new round of asking.
                        let busy = self.carriers.iter().any(|c| c.upload.is_some() || !c.upload_queue.is_empty());
                        let t_offer = self.cfg.params.t_offer_ms.max(1);
                        let at = now + self.rng.below(t_offer) + if busy { t_offer } else { 0 };
                        self.offers.push((*w, g.node, at));
                    }
                } else {
                    // Granted to someone else: anything of ours for it that has not begun is
                    // redundant.
                    self.carriers[i].cancel_pending(*w, now);
                    self.offers.retain(|(o, a, _)| !(o == w && *a == g.node));
                }
            }
            // What we were granted we upload the earlier place first, whichever grant brought it,
            // and among equal places smallest first (§4): a listener plays a collection from its
            // first piece. Lined up behind what each earlier grant brought, the first piece of a
            // programme went after 33 later ones (FEASIBILITY.md §19). Anything that is no
            // piece, and every piece of singles, is first.
            if !granted.is_empty() {
                let places = self.places();
                for u in granted.iter_mut() {
                    let len = self.store.entry(&u.object).and_then(|e| e.len()).unwrap_or(u32::MAX);
                    u.order = ((places.get(&u.object).copied().unwrap_or(0) as u64) << 32) | len as u64;
                }
                granted.sort_by_key(|u| u.order);
                let own = self.announcer_of(i);
                // A root manifest brings the collection manifests new in it, right after it: the
                // announcer reads them only once it holds the root.
                let mut all = Vec::with_capacity(granted.len());
                for u in granted.iter() {
                    all.push(u.clone());
                    for s in self.root_follow_ups(&u.object, u.to, i) {
                        if !granted.iter().any(|x| x.object == s) && !all.iter().any(|x: &Upload| x.object == s) {
                            self.granted_to_us.insert((s, u.to), now);
                            self.stats.follow_ups_sent += 1;
                            all.push(Upload { object: s, to: u.to, start_at: now, started: false, block: 0, esi: 0, list: None, phase: u.phase, manifest: true, order: 0 });
                        }
                    }
                }
                let c = &mut self.carriers[i];
                for u in all {
                    c.add_upload(u, false, own);
                }
            }
        }
    }

    /// Send due offers: one small HAVE frame per (object, announcer), suppressed if another
    /// holder's offer for the same object was heard meanwhile.
    fn offer_check(&mut self) {
        let now = self.now;
        // An offer goes out where its announcer listens: our own announcer is on our channel
        // now; on a hopping carrier another cell's announcer hears us only in the rendezvous.
        let cell = self.cell_carrier();
        let own = self.announcer_of(cell);
        let everyone_listens = !self.hops(cell) || self.in_rendezvous(cell, now);
        let sendable = |a: &NodeId, at: &Millis| *at <= now && (*a == own || everyone_listens);
        let due: Vec<(ShortId, NodeId)> = self.offers.iter().filter(|(_, a, at)| sendable(a, at)).map(|(o, a, _)| (*o, *a)).collect();
        if due.is_empty() {
            return;
        }
        self.offers.retain(|(_, a, at)| !sendable(a, at));
        let ids: Vec<ShortId> = due.iter().map(|(o, _)| *o).collect();
        let (have, have_sets) = self.pack_offer(&ids);
        let g = Gossip { node: self.cfg.id, announcer: self.gossip_announcer_field(), announcer_colour: self.announcer_colouring_field().0, announcer_colours: self.announcer_colouring_field().1, heard: self.heard_field(), have, have_sets, want: Vec::new(), sets: Vec::new() };
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
            let interested = self.follows_channel(&e.channel) || announcing;
            if !interested {
                continue;
            }
            let info = self.manifests.get(&e.channel).copied().unwrap_or_default();
            if info.adopted.map(|a| a.seq >= e.seq).unwrap_or(false) {
                continue;
            }
            // Announcements are not signed. The latest heard replaces one still pending, unless
            // that one is arriving: a false announcement delays a real one until the next is
            // heard, and never interrupts a fetch or outlives the real manifest's arrival.
            if let Some(n) = info.announced {
                if n.short == e.manifest || self.arriving(&n.short) {
                    continue;
                }
                self.wants.remove(&n.short);
                self.progress.remove(&n.short);
            }
            self.manifests.insert(e.channel, ManifestInfo { adopted: info.adopted, announced: Some(ManifestRef { seq: e.seq, short: e.manifest, len: e.len }) });
            if self.store.ensure_hint(e.manifest, e.len, ContentType::Manifest) {
                self.quiet_complete.push(e.manifest);
            }
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
        // Whoever announces a seq we hold, or a newer one, has made our correction unneeded.
        for e in &m.entries {
            if self.manifests.get(&e.channel).and_then(|i| i.adopted).map(|a| e.seq >= a.seq).unwrap_or(false) {
                self.corrections.remove(&e.channel);
            }
        }
        if !announcing && m.node == self.announcer_of(self.cell_carrier()) {
            self.check_announcer_current(m);
        }
    }

    /// A follower compares what its announcer announces with the manifests it holds itself, of
    /// channels it follows or publishes. One the announcer announces with an older seq, or
    /// leaves out where the frame shows the gap, is announced back to it, after a random wait
    /// that lets another follower do it first. Otherwise a new announcer whose library is older
    /// than its cell's, a station back from a power cut for example, never learns the newer
    /// manifests: sources announce theirs only until their own announcer has them.
    fn check_announcer_current(&mut self, m: &ManifestAnnounce) {
        let now = self.now;
        let retry = self.cfg.params.t_want_min_ms;
        let held: Vec<(ChannelId, u32, ShortId)> = self
            .manifests
            .iter()
            .filter(|(c, _)| self.follows_channel(c) || self.own_manifests.iter().any(|(o, _, _, _)| o == *c))
            .filter_map(|(c, i)| i.adopted.filter(|a| self.store.has_complete(&a.short)).map(|a| (*c, a.seq, a.short)))
            .collect();
        for (c, seq, short) in held {
            let behind = match m.entries.iter().find(|e| e.channel == c) {
                Some(e) => e.seq < seq,
                None => m.whole || m.entries.windows(2).any(|w| channel_between(&w[0].channel, &c, &w[1].channel)),
            };
            // An announcer that is asking for our manifest knows of it: it announces only what it
            // holds, and is fetching it. While it fetches it asks again every round, so an ask
            // counts for `T_want_min`, not for `want_ttl`: one that asked once and then took an
            // older announcement for the channel's newest was left uncorrected for an hour, and a
            // newcomer in its cell with it (FEASIBILITY.md §20).
            let asking = self.ann_asks.get(&short).map(|t| now < *t + retry).unwrap_or(false);
            if behind && !asking && self.corrected.get(&c).map(|t| now >= t + retry).unwrap_or(true) {
                self.corrections.insert(c);
                if self.correction_at.is_none() {
                    self.correction_at = Some(now + self.rng.below(self.cfg.params.t_offer_ms.max(1)));
                }
            }
        }
    }

    /// Whether our announcer was heard, within `want_ttl`, asking for `id` itself or granting it
    /// to an uploader: what an honest announcer does with an object it lacks.
    fn ann_seeking(&self, id: &ShortId) -> bool {
        let ttl = self.cfg.params.want_ttl_ms;
        let within = |t: Option<&Millis>| t.map(|t| self.now < *t + ttl).unwrap_or(false);
        within(self.ann_asks.get(id)) || within(self.ann_grants.get(id))
    }

    /// Ask our announcer for one symbol of `x`, naming it as the one to answer: proof that it
    /// serves what it lists (PROTOCOL.md §5.2).
    fn probe(&mut self, x: ShortId, own: NodeId) {
        let Some(k) = self.store.block_k(&x, 0) else { return };
        let esi = self.rng.below(k.max(1) as u64) as u16;
        self.last_probe = self.now;
        self.stats.probes += 1;
        let cell = self.cell_carrier();
        self.enqueue(cell, Frame::Nack(Nack { node: self.cfg.id, object: x, block: 0, answerer: own, phase: 0, missing: alloc::vec![(esi, 1)] }));
    }

    /// Whether symbols of `id` arrived within the last `T_nack_stall`: it is being fetched.
    fn arriving(&self, id: &ShortId) -> bool {
        self.progress.get(id).map(|p| p.last_progress > 0 && self.now < p.last_progress + self.cfg.params.t_nack_stall_ms).unwrap_or(false)
    }

    fn correction_check(&mut self) {
        let Some(at) = self.correction_at else { return };
        if self.now < at {
            return;
        }
        self.correction_at = None;
        let now = self.now;
        let entries: Vec<AnnounceEntry> = core::mem::take(&mut self.corrections)
            .into_iter()
            .filter_map(|c| self.manifests.get(&c).and_then(|i| i.adopted).filter(|a| self.store.has_complete(&a.short)).map(|a| AnnounceEntry { channel: c, manifest: a.short, seq: a.seq, len: a.len }))
            .take(MAX_ANNOUNCE_ENTRIES)
            .collect();
        if entries.is_empty() || self.is_announcing() {
            return;
        }
        for e in &entries {
            self.corrected.insert(e.channel, now);
        }
        self.stats.manifest_corrections += 1;
        let cell = self.cell_carrier();
        self.enqueue(cell, Frame::ManifestAnnounce(ManifestAnnounce { node: self.cfg.id, entries, whole: false }));
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
                // A NACK that names another announcer is a follower asking its own for proof
                // (§5.2): that announcer must answer it, not a neighbour on its behalf.
                let other = !n.answerer.is_none() && n.answerer != self.cfg.id && self.neighbors.get(&n.answerer).map(|nb| nb.announcer == n.answerer).unwrap_or(false);
                if other {
                    continue;
                }
                if let Some(car) = self.carriers[i].carousel.as_mut() {
                    car.on_nack(n.object, n.block, &n.missing, &self.store);
                }
            } else {
                // A follower asks its announcer, whose carousel puts the missing symbols at the
                // front of the next round; an announcer asks the neighbourhood, because no
                // carousel serves it. So only an announcer's NACK is answered by holders, and it
                // names which one: its granted uploader, or the holder it hears best. Letting
                // peers answer followers as well turned a 200-node town into a repair storm of
                // nearly a million answers; letting every holder answer an announcer made
                // holders that cannot hear each other collide.
                let asker_is_announcer = self.neighbors.get(&n.node).map(|nb| nb.announcer == n.node).unwrap_or(false);
                let recently = self.granted_to_us.get(&(n.object, n.node)).map(|t| self.now < *t + self.cfg.params.neighbor_ttl_ms).unwrap_or(false);
                if recently {
                    self.granted_to_us.insert((n.object, n.node), self.now);
                }
                // The asker named who answers: if not us, and not "anyone", stay silent. A follower
                // names someone only when its own announcer cannot repair (above), so naming is
                // what lets one holder answer a follower without a storm.
                let named = n.answerer == self.cfg.id;
                if !recently && !asker_is_announcer && !named {
                    continue;
                }
                if !named && !n.answerer.is_none() {
                    continue;
                }
                // Nobody named (the announcer knows no holder): whoever hears the asker best
                // answers first and silences the rest. Such an answer waits in proportion to how
                // many neighbours we hear better than the asker, plus a little jitter.
                let w = self.cfg.params.upload_suppress_ms.max(1);
                let ungranted_delay = (w * self.rssi_rank(rssi)) / 1000 + self.rng.below(w / 10 + 1);
                let phase = n.phase;
                let c = &mut self.carriers[i];
                let granted = named
                    || recently
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
                        u.phase = phase;
                    }
                    Some(u) if u.object == n.object && u.to == n.node => {
                        // A full pass is in progress; it will cover these.
                    }
                    _ if c.upload_queue.iter().any(|u| u.object == n.object && u.to == n.node) => {}
                    _ => {
                        c.add_upload(Upload { object: n.object, to: n.node, start_at, started: false, block: n.block, esi: 0, list: Some(list), phase, manifest: false, order: 0 }, true, NodeId::NONE);
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

/// Whether channel `c` lies strictly between `a` and `b` in ascending order, wrapping past the
/// largest id when `b` is smaller than `a`: neighbours in an announcer's list with `c` not on it.
fn channel_between(a: &ChannelId, c: &ChannelId, b: &ChannelId) -> bool {
    if a < b {
        a < c && c < b
    } else {
        c > a || c < b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges() {
        assert_eq!(compress_ranges(&[0, 1, 2, 5, 7, 8]), alloc::vec![(0, 3), (5, 1), (7, 2)]);
    }
}
