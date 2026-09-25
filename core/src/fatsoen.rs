//! EtherFatsoen: spectrum etiquette (ETHERFATSOEN.md). Occupancy-driven AIMD self-throttling,
//! class gating, token-bucket pacing and CCA backoff. One instance per carrier.

use crate::frame::Class;
use crate::params::FatsoenParams;
use crate::rng::Rng;
use crate::Millis;

#[derive(Clone, Debug)]
pub struct Fatsoen {
    p: FatsoenParams,
    occ_ewma: u16,
    foreign_ewma: u16,
    rate: u16,
    attempt: u8,
    pub backoff_until: Millis,
    tokens_us: i64,
    last_refill: Millis,
    last_window: Millis,
    pub windows: u64,
}

impl Fatsoen {
    pub fn new(p: FatsoenParams, now: Millis) -> Self {
        Fatsoen { p, occ_ewma: 0, foreign_ewma: 0, rate: p.rate_max / 2, attempt: 0, backoff_until: 0, tokens_us: p.burst_ms as i64 * 1000, last_refill: now, last_window: now, windows: 0 }
    }

    /// Feed the measured occupancy: `occ_total` is the fraction (permille) of the last window
    /// busy with others' energy, `own_air_ms` the airtime of MeshCast frames we decoded in that
    /// window. Foreign occupancy is the difference. Returns true when a window closed.
    pub fn maybe_window(&mut self, now: Millis, occ_total: u16, own_air_ms: u64) -> bool {
        if now < self.last_window + self.p.window_ms {
            return false;
        }
        self.last_window = now;
        self.windows += 1;
        let a = self.p.alpha as u32;
        let total = occ_total.min(1000);
        let own = ((own_air_ms * 1000) / self.p.window_ms.max(1)).min(total as u64) as u16;
        let foreign = total - own;
        self.occ_ewma = ((a * total as u32 + (256 - a) * self.occ_ewma as u32) / 256) as u16;
        self.foreign_ewma = ((a * foreign as u32 + (256 - a) * self.foreign_ewma as u32) / 256) as u16;
        if self.foreign_ewma > self.p.occ_high {
            self.rate = (self.rate / 2).max(self.p.rate_min);
        } else if self.occ_ewma > self.p.occ_high_own {
            self.rate = (self.rate / 2).max(self.p.rate_min_own);
        } else if self.foreign_ewma < self.p.occ_low && self.occ_ewma < self.p.occ_low_own {
            self.rate = (self.rate + self.p.rate_step).min(self.p.rate_max);
        }
        true
    }

    pub fn occupancy(&self) -> u16 {
        self.occ_ewma
    }

    pub fn foreign_occupancy(&self) -> u16 {
        self.foreign_ewma
    }

    pub fn rate(&self) -> u16 {
        self.rate
    }

    /// Class gate: control always; metadata and uploads unless congested; carousel content only
    /// on a quiet channel. Foreign energy is judged strictly, our own protocol's traffic loosely.
    pub fn allows(&self, class: Class, upload: bool) -> bool {
        match class {
            Class::Control => true,
            Class::Metadata => self.foreign_ewma < self.p.occ_high && self.occ_ewma < self.p.occ_high_own,
            Class::Content if upload => self.foreign_ewma < self.p.occ_high && self.occ_ewma < self.p.occ_high_own,
            Class::Content => self.foreign_ewma < self.p.occ_content && self.occ_ewma < self.p.occ_content_own,
        }
    }

    /// Token-bucket pacing for content. `budget_permille` is the regulatory (or self-imposed)
    /// share of airtime; the effective rate is `budget × rate`. Returns how long to wait if the
    /// bucket cannot cover `airtime_ms` now.
    pub fn take_airtime(&mut self, now: Millis, airtime_ms: u32, budget_permille: u16, exempt: bool) -> Result<(), Millis> {
        let rate = if exempt { self.p.rate_max } else { self.rate };
        let per_mille2 = budget_permille as i64 * rate as i64; // 0..1e6
        let elapsed = now.saturating_sub(self.last_refill) as i64;
        self.last_refill = now;
        // refill in µs: elapsed_ms × 1000 × per_mille2 / 1e6
        self.tokens_us += elapsed * per_mille2 / 1000;
        let cap = self.p.burst_ms as i64 * 1000;
        if self.tokens_us > cap {
            self.tokens_us = cap;
        }
        let need = airtime_ms as i64 * 1000;
        if self.tokens_us >= need {
            self.tokens_us -= need;
            Ok(())
        } else if per_mille2 == 0 {
            Err(1000)
        } else {
            let deficit = need - self.tokens_us;
            // ms = deficit_us × 1000 / per_mille2
            Err(((deficit * 1000) / per_mille2).max(1) as Millis)
        }
    }

    /// Channel was busy at CCA: exponential backoff with jitter.
    pub fn cca_busy(&mut self, now: Millis, rng: &mut Rng) -> Millis {
        let exp = self.attempt.min(self.p.backoff_max_attempt);
        let span = self.p.backoff_base_ms << exp;
        self.backoff_until = now + 1 + rng.below(span.max(1));
        if self.attempt < self.p.backoff_max_attempt {
            self.attempt += 1;
        }
        self.backoff_until
    }

    pub fn cca_clear(&mut self) {
        self.attempt = 0;
    }
}
