//! Region profiles (ETHERDISCIPLINE.md). Only the budget differs per region; nothing else does.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Access {
    /// Cumulative on-time ≤ `permille`/1000 of any `window_s` window.
    DutyCycle { permille: u16, window_s: u32 },
    /// ETSI EN 300 220-2 §4.6 polite spectrum access.
    Polite { cca_us: u32, cca_threshold_dbm: i16, ton_max_ms: u32, toff_min_ms: u32, cum_on_s_per_hour_per_200khz: u32 },
    /// FCC 15.247 digital modulation: no duty cycle, minimum 6 dB bandwidth.
    Digital { min_bw_hz: u32 },
    /// No requirement.
    None,
}

#[derive(Clone, Copy, Debug)]
pub struct Band {
    pub id: &'static str,
    pub low_hz: u32,
    pub high_hz: u32,
    pub max_erp_dbm: i8,
    pub max_bw_hz: u32,
    pub access: &'static [Access],
    pub source: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct RegionProfile {
    pub id: &'static str,
    pub bands: &'static [Band],
}

const ETSI: &str = "ETSI EN 300 220-2 V3.3.1 Table 4 / Table 18";
const FCC: &str = "47 CFR 15.247";

pub const EU_POLITE: Access = Access::Polite { cca_us: 160, cca_threshold_dbm: -83, ton_max_ms: 1000, toff_min_ms: 100, cum_on_s_per_hour_per_200khz: 100 };

pub static EU868: RegionProfile = RegionProfile {
    id: "EU868",
    bands: &[
        Band { id: "K", low_hz: 863_000_000, high_hz: 865_000_000, max_erp_dbm: 14, max_bw_hz: 2_000_000, access: &[Access::DutyCycle { permille: 1, window_s: 3600 }, EU_POLITE], source: ETSI },
        Band { id: "L", low_hz: 865_000_000, high_hz: 868_000_000, max_erp_dbm: 14, max_bw_hz: 3_000_000, access: &[Access::DutyCycle { permille: 10, window_s: 3600 }, EU_POLITE], source: ETSI },
        Band { id: "M", low_hz: 868_000_000, high_hz: 868_600_000, max_erp_dbm: 14, max_bw_hz: 600_000, access: &[Access::DutyCycle { permille: 10, window_s: 3600 }, EU_POLITE], source: ETSI },
        Band { id: "N", low_hz: 868_700_000, high_hz: 869_200_000, max_erp_dbm: 14, max_bw_hz: 500_000, access: &[Access::DutyCycle { permille: 1, window_s: 3600 }, EU_POLITE], source: ETSI },
        Band { id: "O", low_hz: 869_400_000, high_hz: 869_650_000, max_erp_dbm: 27, max_bw_hz: 250_000, access: &[Access::DutyCycle { permille: 100, window_s: 3600 }, EU_POLITE], source: ETSI },
        Band { id: "P", low_hz: 869_700_000, high_hz: 870_000_000, max_erp_dbm: 7, max_bw_hz: 300_000, access: &[Access::None], source: ETSI },
        Band { id: "Q", low_hz: 869_700_000, high_hz: 870_000_000, max_erp_dbm: 14, max_bw_hz: 300_000, access: &[Access::DutyCycle { permille: 10, window_s: 3600 }, EU_POLITE], source: ETSI },
    ],
};

pub static US915: RegionProfile = RegionProfile {
    id: "US915",
    bands: &[
        Band { id: "ISM", low_hz: 902_000_000, high_hz: 928_000_000, max_erp_dbm: 30, max_bw_hz: 26_000_000, access: &[Access::Digital { min_bw_hz: 500_000 }], source: FCC },
    ],
};

/// 2.4 GHz for ESP-NOW / SX128x class radios. Regulatory detail (EN 300 328, 15.247) is out of
/// scope for the accounting: no duty cycle applies; EtherFatsoen limits own share instead.
pub static ISM2400: RegionProfile = RegionProfile {
    id: "ISM2400",
    bands: &[Band { id: "2G4", low_hz: 2_400_000_000, high_hz: 2_483_500_000, max_erp_dbm: 20, max_bw_hz: 20_000_000, access: &[Access::None], source: "ETSI EN 300 328 / 47 CFR 15.247" }],
};

impl RegionProfile {
    pub fn band(&self, id: &str) -> Option<(usize, &'static Band)> {
        self.bands.iter().enumerate().find(|(_, b)| b.id == id).map(|(i, b)| (i, b))
    }
}
