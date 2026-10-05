//! Protocol parameters (PROTOCOL.md §8, ETHERFATSOEN.md §4). Draft values; the simulator tunes them.

use crate::Millis;

#[derive(Clone, Copy, Debug)]
pub struct ElectionParams {
    pub t_beacon_ms: Millis,
    pub n_miss: u8,
    pub t_base_ms: Millis,
    pub t_jitter_ms: Millis,
    /// Takeover hysteresis in score units.
    pub hysteresis: u16,
    /// A follower switches announcer only for a signal this much stronger (dB).
    pub rssi_hysteresis_db: u8,
    /// Beacons in a row with a clearly lower score than ours before we challenge the incumbent.
    pub challenge_beacons: u8,
}

impl Default for ElectionParams {
    fn default() -> Self {
        ElectionParams { t_beacon_ms: 60_000, n_miss: 3, t_base_ms: 60_000, t_jitter_ms: 10_000, hysteresis: SCORE_MAX / 10, rssi_hysteresis_db: 6, challenge_beacons: 3 }
    }
}

/// Maximum value of the election score (see `node::Node::compute_score`).
pub const SCORE_MAX: u16 = 256 + 64 + 64 + 32 + 16;

#[derive(Clone, Copy, Debug)]
pub struct FatsoenParams {
    pub window_ms: Millis,
    /// EWMA weight in 1/256.
    pub alpha: u16,
    pub occ_high: u16,
    pub occ_low: u16,
    pub occ_content: u16,
    pub rate_min: u16,
    pub rate_max: u16,
    pub rate_step: u16,
    pub backoff_base_ms: Millis,
    pub backoff_max_attempt: u8,
    /// Own share of airtime allowed on carriers without a regulatory duty cycle (permille).
    pub max_own_share: u16,
    /// Token-bucket burst, in milliseconds of airtime.
    pub burst_ms: u32,
    /// Thresholds for occupancy caused by recognisable MeshCast frames ("fair among ourselves"):
    /// looser than the foreign-energy thresholds above ("polite to strangers").
    pub occ_high_own: u16,
    pub occ_low_own: u16,
    pub occ_content_own: u16,
    pub rate_min_own: u16,
}

impl Default for FatsoenParams {
    fn default() -> Self {
        FatsoenParams {
            window_ms: 10_000,
            alpha: 77, // 0.3
            occ_high: 300,
            occ_low: 150,
            occ_content: 250,
            rate_min: 50,
            rate_max: 1000,
            rate_step: 50,
            backoff_base_ms: 50,
            backoff_max_attempt: 8,
            max_own_share: 500,
            burst_ms: 2000,
            occ_high_own: 500,
            occ_low_own: 350,
            occ_content_own: 450,
            rate_min_own: 250,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub election: ElectionParams,
    pub fatsoen: FatsoenParams,
    /// Gossip interval for announcers and for sources with something pending.
    pub t_gossip_ms: Millis,
    /// Minimum interval between WANT frames from a follower.
    pub t_want_min_ms: Millis,
    /// How long a WANT stays valid at the announcer.
    pub want_ttl_ms: Millis,
    /// Follower may NACK when at least this fraction (permille) of an object is present ...
    pub nack_threshold_permille: u16,
    /// ... and no progress for this many carousel rounds.
    pub nack_rounds: u16,
    /// Dwell time per channel on frequency-agile carriers.
    pub dwell_ms: Millis,
    /// Score recomputation interval.
    pub t_score_ms: Millis,
    /// Neighbours not heard for this long are forgotten.
    pub neighbor_ttl_ms: Millis,
    /// Caps on what a node keeps of what it hears under names nobody checks (docs/ABUSE.md,
    /// "Someone else's firmware", item 5): neighbours, the ids they offered in all, announcers
    /// reported in conflict, and askers kept per object by a carousel.
    pub max_neighbours: usize,
    pub max_offered_ids: usize,
    pub max_conflicts: usize,
    pub max_askers_per_object: usize,
    /// Another cell's asks a node keeps to relay (PROTOCOL.md §4).
    pub max_relay_asks: usize,
    /// Share of the regulatory budget (permille) kept free for control frames when the control
    /// and bulk carriers share a band.
    pub control_reserve: u16,
    /// An object leaves the carousel after this many full passes unless re-wanted.
    pub max_passes: u16,
    /// A node NACKs an object that is ≥ threshold complete after this long without progress.
    pub t_nack_stall_ms: Millis,
    /// Minimum interval between two gossip rounds of the same node.
    pub t_gossip_min_ms: Millis,
    /// Random delay before any control/metadata frame on a bulk carrier, so that nodes whose
    /// CCA cannot see each other (edge of range) do not transmit in lock-step.
    pub tx_jitter_ms: Millis,
    /// On frequency-agile carriers, every `meet_every`-th dwell is a meeting dwell on a common
    /// channel where all announcers and holders can hear each other.
    pub meet_every: u64,
    /// Holders wait a random time up to this before uploading to an announcer that asked, and
    /// give up if they hear someone else uploading the same object meanwhile.
    pub upload_suppress_ms: Millis,
    /// Time slot on single-channel carriers when announcers in conflict take turns.
    pub t_slot_ms: Millis,
    /// Length of one upload phase: uploaders to one announcer take turns in phases this long,
    /// one permitted transmission under polite access.
    pub t_upload_phase_ms: Millis,
    /// A device that cannot decode asks for a rendition this long before the object's slot in
    /// the schedule (PROTOCOL.md §1.2); an unscheduled object is asked for at once.
    pub t_render_ahead_ms: Millis,
    /// A reported conflict between announcers is forgotten after this long.
    pub conflict_ttl_ms: Millis,
    /// Minimum interval between two conflict reports from the same follower.
    pub t_report_min_ms: Millis,
    /// A granted uploader that has not delivered a symbol within this time loses the grant.
    pub t_grant_ms: Millis,
    /// Holders answer an open ask with an offer after a random delay up to this.
    pub t_offer_ms: Millis,
    /// A follower whose want has brought no symbol for this long follows, until it has the
    /// object, another announcer it heard offering it (PROTOCOL.md §4, excursions).
    pub t_excursion_ms: Millis,
    /// A node with one sub-GHz radio for its control and bulk carriers listens on the control
    /// carrier only in a window of `t_ctrl_window_ms` every `t_ctrl_period_ms`, and every node
    /// sends control-carrier frames only then (PROTOCOL.md §3). A window of 0 disables them.
    pub t_ctrl_period_ms: Millis,
    pub t_ctrl_window_ms: Millis,
    /// How long a node that knows no shared time listens on the control carrier before it may step
    /// up as announcer by its own clock: two control periods and a window, the longest a node can
    /// wait for a whole window and the time told in it (PROTOCOL.md §3, §6).
    pub t_acquire_ms: Millis,
    /// An announcer tells its time on the control carrier in every window, wherever nodes listen there
    /// only in the window (PROTOCOL.md §6). Off: never; kept to measure against.
    pub tells_time: bool,
    /// How far a frame keeps from the edges of a hop dwell and of the control window: room for
    /// clocks a little apart (PROTOCOL.md §6).
    pub t_guard_ms: Millis,
    /// Each control period's window sits where the period's number puts it, so that groups whose
    /// shared times differ meet in a window now and then (PROTOCOL.md §3, §6). Off: in the middle of
    /// every period, as before; kept to measure against.
    pub ctrl_window_wanders: bool,
    /// An announcer serves (keeps current, passes unasked) only the channels a follower of its cell
    /// asked for within `cell_keep_ms` or announced, and those it follows (PROTOCOL.md §2). Off: every
    /// channel it hears of, as before; kept to measure against.
    pub cell_menu: bool,
    /// How long a follower's ask keeps a channel served (PROTOCOL.md §2, §8).
    pub cell_keep_ms: Millis,
    /// An announcer whose list does not fit one frame announces, in the round after it adopted a
    /// root and never two rounds running, the roots it adopted last instead of the next stretch in
    /// channel order (PROTOCOL.md §3.4). Off: channel order only; kept to measure against.
    pub announce_recent: bool,
    /// A follower leaves an announcer that neither serves nor gets what it wants (PROTOCOL.md §5.2,
    /// "Asking is not getting"). Off: only one that lists it and does not send it; kept to
    /// measure against (FEASIBILITY.md §31).
    pub leave_lacking: bool,
    /// A follower waits for an object twice as long under each next announcer, for every
    /// announcer it left for it that did not list it (PROTOCOL.md §5.2). Off: as long each time;
    /// kept to measure against.
    pub leave_backoff: bool,
    /// An announcer fetches every piece of every channel it hears of, asked for or not. Off: it
    /// fetches what a follower asks for (PROTOCOL.md §2, FEASIBILITY.md §27); kept to measure against.
    pub proactive: bool,
    /// A node relays for another cell's listeners what it does not follow too, and keeps the menu it
    /// hears of channels it does not follow, to name what they ask for (PROTOCOL.md §4).
    pub relay_unfollowed: bool,
    /// How long another cell's ask for its listeners goes unmet before a node relays it
    /// (PROTOCOL.md §4, `T_relay_wait`).
    pub t_relay_wait_ms: Millis,
    /// A node relays another cell's ask once asks of its age were met by others, before twice that
    /// age, less often than `relay_risk_permille` of the time, as it learned from the asks it heard,
    /// and at the latest after `want_ttl`; `T_relay_wait` is what it expects before it has heard
    /// any (PROTOCOL.md §4). Off: it relays after `T_relay_wait`, as before; kept to measure against.
    pub relay_learn: bool,
    pub relay_risk_permille: u16,
    /// A relay ends when the cell that asked for it holds the object: what we have not fetched yet
    /// we no longer want (PROTOCOL.md §4). Off: as before; kept to measure against.
    pub relay_withdraw: bool,
    /// What gives way to the carry budget is chosen by evidence: the menu of a channel we neither
    /// follow nor serve is of use once it names another cell's ask, not when it arrives, and gives
    /// way first; then relays another cell's announcer lists as held; then the least recently used
    /// (PROTOCOL.md §4).
    /// Off: arriving is use and the least recently used goes first, as before.
    pub evict_by_evidence: bool,
    /// Bytes a node keeps of what it does not listen to, for others (PROTOCOL.md §4, `carry_budget`):
    /// what has not been of use for `want_ttl` gives way, least recently used first, and a full
    /// budget takes on no more relays. `u64::MAX`: no limit; 0: no relays.
    pub carry_budget_bytes: u64,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            election: ElectionParams::default(),
            fatsoen: FatsoenParams::default(),
            t_gossip_ms: 300_000,
            t_want_min_ms: 600_000,
            want_ttl_ms: 3_600_000,
            nack_threshold_permille: 800,
            nack_rounds: 2,
            dwell_ms: 20_000,
            t_score_ms: 60_000,
            neighbor_ttl_ms: 3_600_000,
            max_neighbours: 256,
            max_offered_ids: 8192,
            max_conflicts: 64,
            max_askers_per_object: 32,
            max_relay_asks: 1024,
            control_reserve: 100,
            max_passes: 1,
            t_nack_stall_ms: 60_000,
            t_gossip_min_ms: 30_000,
            tx_jitter_ms: 500,
            meet_every: 5,
            upload_suppress_ms: 20_000,
            t_slot_ms: 10_000,
            t_upload_phase_ms: 1_000,
            t_render_ahead_ms: 1_800_000,
            conflict_ttl_ms: 1_800_000,
            t_report_min_ms: 60_000,
            t_grant_ms: 600_000,
            t_offer_ms: 3_000,
            t_excursion_ms: 2_400_000,
            t_ctrl_period_ms: 60_000,
            t_ctrl_window_ms: 4_000,
            t_acquire_ms: 124_000,
            tells_time: true,
            t_guard_ms: 50,
            ctrl_window_wanders: true,
            cell_menu: true,
            cell_keep_ms: 86_400_000,
            announce_recent: true,
            leave_lacking: true,
            leave_backoff: true,
            proactive: false,
            relay_unfollowed: true,
            t_relay_wait_ms: 600_000,
            relay_learn: true,
            relay_risk_permille: 50,
            relay_withdraw: true,
            evict_by_evidence: true,
            carry_budget_bytes: u64::MAX,
        }
    }
}
