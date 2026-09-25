//! EtherDiscipline: regulatory airtime accounting per band (ETHERDISCIPLINE.md §1).
//!
//! Sliding-window log of transmissions; `may_transmit` is the hard legal gate. The etiquette
//! layer (`fatsoen`) paces well below it.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::profile::{Access, Band, RegionProfile};
use crate::Millis;

const SLICE_HZ: u32 = 200_000;
const HOUR_MS: Millis = 3_600_000;

#[derive(Clone, Copy, Debug)]
struct TxRecord {
    start: Millis,
    dur_ms: u32,
    center_hz: u32,
    lo_slice: u16,
    hi_slice: u16,
}

#[derive(Clone, Debug)]
pub struct BandAccount {
    pub band: &'static Band,
    pub rule: Access,
    log: VecDeque<TxRecord>,
}

#[derive(Clone, Debug)]
pub struct Accounting {
    pub profile: &'static RegionProfile,
    bands: Vec<BandAccount>,
    /// Sliding window used for pruning; the longest any rule looks back.
    window_ms: Millis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    /// Would exceed a cumulative limit; retry after roughly this many ms.
    Wait(Millis),
    /// Single transmission too long, or frequency outside the band: never allowed.
    Never,
}

impl Accounting {
    /// `rule_choice[i]` selects which access rule of band `i` this node uses (index into `band.access`).
    pub fn new(profile: &'static RegionProfile, rule_choice: &[usize]) -> Self {
        let bands = profile
            .bands
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let idx = rule_choice.get(i).copied().unwrap_or(0).min(b.access.len().saturating_sub(1));
                BandAccount { band: b, rule: b.access.get(idx).copied().unwrap_or(Access::None), log: VecDeque::new() }
            })
            .collect();
        Accounting { profile, bands, window_ms: HOUR_MS }
    }

    pub fn rule(&self, band: usize) -> Access {
        self.bands.get(band).map(|b| b.rule).unwrap_or(Access::None)
    }

    fn slices(band: &Band, center_hz: u32, bw_hz: u32) -> (u16, u16) {
        let lo = center_hz.saturating_sub(bw_hz / 2).saturating_sub(band.low_hz);
        let hi = (center_hz + bw_hz / 2).saturating_sub(band.low_hz);
        ((lo / SLICE_HZ) as u16, (hi / SLICE_HZ) as u16)
    }

    fn prune(&mut self, now: Millis) {
        let w = self.window_ms;
        for b in self.bands.iter_mut() {
            while let Some(r) = b.log.front() {
                if r.start + r.dur_ms as Millis + w < now {
                    b.log.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    /// Cumulative on-time in the window ending at `now`, for the whole band.
    pub fn used_ms(&self, band: usize, now: Millis, window_ms: Millis) -> Millis {
        let Some(b) = self.bands.get(band) else { return 0 };
        let from = now.saturating_sub(window_ms);
        b.log.iter().filter(|r| r.start + r.dur_ms as Millis > from).map(|r| r.dur_ms as Millis).sum()
    }

    fn used_in_slice(b: &BandAccount, slice: u16, now: Millis) -> Millis {
        let from = now.saturating_sub(HOUR_MS);
        b.log.iter().filter(|r| r.start + r.dur_ms as Millis > from && r.lo_slice <= slice && slice <= r.hi_slice).map(|r| r.dur_ms as Millis).sum()
    }

    /// Fraction of time (permille) this band's rule allows on average, for pacing.
    /// `n_channels` is how many distinct 200 kHz slices the node hops over (polite rules).
    pub fn budget_permille(&self, band: usize, n_channels: u16) -> u16 {
        match self.rule(band) {
            Access::DutyCycle { permille, .. } => permille,
            Access::Polite { cum_on_s_per_hour_per_200khz, .. } => {
                let total = cum_on_s_per_hour_per_200khz as u64 * n_channels.max(1) as u64;
                (total * 1000 / 3600).min(1000) as u16
            }
            Access::Digital { .. } | Access::None => 1000,
        }
    }

    pub fn may_transmit(&mut self, band: usize, now: Millis, dur_ms: u32, center_hz: u32, bw_hz: u32) -> Verdict {
        self.prune(now);
        let Some(b) = self.bands.get(band) else { return Verdict::Never };
        if center_hz < b.band.low_hz || center_hz > b.band.high_hz || bw_hz > b.band.max_bw_hz {
            return Verdict::Never;
        }
        match b.rule {
            Access::None => Verdict::Ok,
            Access::Digital { min_bw_hz } => {
                if bw_hz < min_bw_hz {
                    Verdict::Never
                } else {
                    Verdict::Ok
                }
            }
            Access::DutyCycle { permille, window_s } => {
                let window = window_s as Millis * 1000;
                let limit = window * permille as Millis / 1000;
                let used = self.used_ms(band, now, window);
                if used + dur_ms as Millis <= limit {
                    Verdict::Ok
                } else {
                    // Oldest record in window expires first; wait until enough has aged out.
                    let from = now.saturating_sub(window);
                    let mut need = used + dur_ms as Millis - limit;
                    let mut wait = window;
                    for r in b.log.iter().filter(|r| r.start + r.dur_ms as Millis > from) {
                        let expires_in = (r.start + r.dur_ms as Millis + window).saturating_sub(now);
                        if r.dur_ms as Millis >= need {
                            wait = expires_in;
                            break;
                        }
                        need -= r.dur_ms as Millis;
                    }
                    Verdict::Wait(wait.max(1))
                }
            }
            Access::Polite { ton_max_ms, toff_min_ms, cum_on_s_per_hour_per_200khz, .. } => {
                if dur_ms > ton_max_ms {
                    return Verdict::Never;
                }
                // Minimum off time on the same nominal frequency.
                if let Some(last) = b.log.iter().rev().find(|r| r.center_hz == center_hz) {
                    let end = last.start + last.dur_ms as Millis;
                    if now < end + toff_min_ms as Millis {
                        return Verdict::Wait(end + toff_min_ms as Millis - now);
                    }
                }
                let (lo, hi) = Self::slices(b.band, center_hz, bw_hz);
                let limit = cum_on_s_per_hour_per_200khz as Millis * 1000;
                let mut worst_wait: Millis = 0;
                for s in lo..=hi {
                    let used = Self::used_in_slice(b, s, now);
                    if used + dur_ms as Millis > limit {
                        // Wait until the oldest record in this slice ages out.
                        let from = now.saturating_sub(HOUR_MS);
                        let oldest = b.log.iter().find(|r| r.start + r.dur_ms as Millis > from && r.lo_slice <= s && s <= r.hi_slice);
                        let w = oldest.map(|r| (r.start + r.dur_ms as Millis + HOUR_MS).saturating_sub(now)).unwrap_or(1000);
                        worst_wait = worst_wait.max(w.max(1));
                    }
                }
                if worst_wait > 0 {
                    Verdict::Wait(worst_wait)
                } else {
                    Verdict::Ok
                }
            }
        }
    }

    pub fn record(&mut self, band: usize, start: Millis, dur_ms: u32, center_hz: u32, bw_hz: u32) {
        let Some(b) = self.bands.get_mut(band) else { return };
        let (lo, hi) = Self::slices(b.band, center_hz, bw_hz);
        b.log.push_back(TxRecord { start, dur_ms, center_hz, lo_slice: lo, hi_slice: hi });
    }

    /// Slice with the least cumulative on-time in the last hour, among `centers`. For AFA.
    pub fn quietest_channel(&self, band: usize, centers: &[u32], bw_hz: u32, now: Millis) -> usize {
        let Some(b) = self.bands.get(band) else { return 0 };
        let mut best = 0;
        let mut best_used = Millis::MAX;
        for (i, &c) in centers.iter().enumerate() {
            let (lo, hi) = Self::slices(b.band, c, bw_hz);
            let used: Millis = (lo..=hi).map(|s| Self::used_in_slice(b, s, now)).max().unwrap_or(0);
            if used < best_used {
                best_used = used;
                best = i;
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::EU868;

    #[test]
    fn duty_cycle_band_o() {
        let mut a = Accounting::new(&EU868, &[0, 0, 0, 0, 0, 0, 0]);
        let (o, _) = EU868.band("O").unwrap();
        let mut now = 0;
        let mut sent = 0u64;
        // 10 % of an hour is 360 s.
        loop {
            match a.may_transmit(o, now, 1000, 869_525_000, 200_000) {
                Verdict::Ok => {
                    a.record(o, now, 1000, 869_525_000, 200_000);
                    sent += 1000;
                    now += 1000;
                }
                Verdict::Wait(_) => break,
                Verdict::Never => panic!(),
            }
        }
        assert_eq!(sent, 360_000);
        // An hour later the window has cleared.
        assert_eq!(a.may_transmit(o, now + HOUR_MS + 1, 1000, 869_525_000, 200_000), Verdict::Ok);
    }

    #[test]
    fn polite_band_l_per_slice() {
        let mut a = Accounting::new(&EU868, &[0, 1, 0, 0, 0, 0, 0]);
        let (l, _) = EU868.band("L").unwrap();
        let c = 865_100_000;
        assert_eq!(a.may_transmit(l, 0, 2000, c, 200_000), Verdict::Never); // > Ton_max
        assert_eq!(a.may_transmit(l, 0, 500, c, 200_000), Verdict::Ok);
        a.record(l, 0, 500, c, 200_000);
        assert!(matches!(a.may_transmit(l, 550, 500, c, 200_000), Verdict::Wait(_))); // Toff_min
        // Another channel is free immediately.
        assert_eq!(a.may_transmit(l, 550, 500, c + 400_000, 200_000), Verdict::Ok);
        // Fill the first slice to 100 s.
        let mut now = 1000;
        let mut total = 500;
        while let Verdict::Ok = a.may_transmit(l, now, 1000, c, 200_000) {
            a.record(l, now, 1000, c, 200_000);
            total += 1000;
            now += 1100;
        }
        // Frames are 1 s, the first was 0.5 s: the hard cap of 100 s is never exceeded.
        assert!(total <= 100_000 && total > 99_000, "{total}");
        assert_eq!(a.budget_permille(l, 15), 416);
    }
}
