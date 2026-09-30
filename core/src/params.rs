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
        ElectionParams { t_beacon_ms: 60_000, n_miss: 3, t_base_ms: 120_000, t_jitter_ms: 20_000, hysteresis: SCORE_MAX / 10, rssi_hysteresis_db: 6, challenge_beacons: 3 }
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
    /// Share of the regulatory budget (permille) kept free for control frames when the control
    /// and bulk carriers share a band.
    pub control_reserve: u16,
    /// An object leaves the carousel after this many full passes unless re-wanted.
    pub max_passes: u16,
    /// Manifests ("always" objects) are carouseled at most this often when nothing else is wanted.
    pub t_always_ms: Millis,
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
    /// A reported conflict between announcers is forgotten after this long.
    pub conflict_ttl_ms: Millis,
    /// Minimum interval between two conflict reports from the same follower.
    pub t_report_min_ms: Millis,
    /// A granted uploader that has not delivered a symbol within this time loses the grant.
    pub t_grant_ms: Millis,
    /// Holders answer an open ask with an offer after a random delay up to this.
    pub t_offer_ms: Millis,
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
            control_reserve: 100,
            max_passes: 3,
            t_always_ms: 300_000,
            t_nack_stall_ms: 60_000,
            t_gossip_min_ms: 30_000,
            tx_jitter_ms: 500,
            meet_every: 5,
            upload_suppress_ms: 20_000,
            t_slot_ms: 10_000,
            t_upload_phase_ms: 1_000,
            conflict_ttl_ms: 1_800_000,
            t_report_min_ms: 60_000,
            t_grant_ms: 600_000,
            t_offer_ms: 3_000,
        }
    }
}
