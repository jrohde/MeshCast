//! Physical-layer presets and propagation. Numbers from docs/FEASIBILITY.md.

use clap::ValueEnum;
use meshcast_core::frame::CarrierKind;
use meshcast_core::node::CarrierParams;
use meshcast_core::profile::{RegionProfile, EU868, ISM2400, US915};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, ValueEnum, Serialize)]
pub enum Region {
    Eu868,
    Us915,
}

impl Region {
    pub fn profile(&self) -> &'static RegionProfile {
        match self {
            Region::Eu868 => &EU868,
            Region::Us915 => &US915,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, ValueEnum, Serialize)]
pub enum BulkPreset {
    /// GFSK 100 kbit/s, EU band O, 500 mW, 10 % duty cycle, one 250 kHz channel.
    GfskO,
    /// GFSK 100 kbit/s, EU band L, 25 mW, polite access hopping over 15 × 200 kHz.
    GfskL,
    /// ESP-NOW long-range mode, 2.4 GHz, 100 mW, no duty cycle.
    EspNow,
    /// GFSK 300 kbit/s, US 902–928 MHz, 1 W, FCC 15.247 digital modulation, no duty cycle.
    GfskUs,
    /// LoRa SF7/125 in EU band O as the one and only carrier: 10 km reach, ~0.5 kbit/s average.
    LoraBulk,
}

/// Everything the engine needs about a carrier beyond what the core needs.
#[derive(Clone, Debug, Serialize)]
pub struct Phy {
    pub name: String,
    #[serde(skip)]
    pub kind: CarrierKind,
    pub bitrate_bps: u32,
    pub overhead_ms: u32,
    pub band: Option<usize>,
    pub rule_choice: Option<(usize, usize)>,
    pub channels: Vec<u32>,
    pub bw_hz: u32,
    pub tx_dbm: f64,
    pub sensitivity_dbm: f64,
    pub cca_threshold_dbm: f64,
    pub capture_db: f64,
    /// Free-space loss at 1 m for this frequency.
    pub pl0_db: f64,
}

impl Phy {
    pub fn to_core(&self) -> CarrierParams {
        CarrierParams {
            kind: self.kind,
            bitrate_bps: self.bitrate_bps,
            overhead_ms: self.overhead_ms,
            band: self.band,
            channels: self.channels.clone(),
            bw_hz: self.bw_hz,
            tx_dbm: self.tx_dbm.round() as i8,
            near_rssi_dbm: (self.sensitivity_dbm + 17.0).round() as i16,
        }
    }
}

pub fn pl0(freq_hz: f64) -> f64 {
    // 20 log10(4 π d / λ) at d = 1 m.
    let lambda = 299_792_458.0 / freq_hz;
    20.0 * (4.0 * core::f64::consts::PI / lambda).log10()
}

/// LoRa effective bit rate and sensitivity (BW 125 kHz, CR 4/5). SF7 and SF12 from the SX1262
/// datasheet, the rest interpolated.
fn lora(sf: u8, bw_hz: u32) -> (u32, u32, f64) {
    let bw = bw_hz as f64;
    let bitrate = sf as f64 * 0.8 * bw / (1u32 << sf) as f64;
    let sym_ms = (1u32 << sf) as f64 / bw * 1000.0;
    let overhead_ms = 12.25 * sym_ms + 8.0 * sym_ms; // preamble + explicit header
    let sens125 = match sf {
        7 => -124.0,
        8 => -127.0,
        9 => -129.0,
        10 => -132.0,
        11 => -134.5,
        _ => -137.0,
    };
    let sens = sens125 + 10.0 * (bw / 125_000.0).log10();
    (bitrate as u32, overhead_ms.ceil() as u32, sens)
}

pub fn control_phy(region: Region, sf: u8) -> Phy {
    match region {
        Region::Eu868 => {
            let (bitrate, overhead, sens) = lora(sf, 125_000);
            let (band, _) = EU868.band("O").unwrap();
            Phy {
                name: format!("LoRa SF{sf}/125 band O"),
                kind: CarrierKind::LoraControl,
                bitrate_bps: bitrate,
                overhead_ms: overhead,
                band: Some(band),
                rule_choice: Some((band, 0)),
                channels: vec![869_525_000],
                bw_hz: 125_000,
                tx_dbm: 27.0,
                sensitivity_dbm: sens,
                cca_threshold_dbm: sens + 2.0,
                capture_db: 6.0,
                pl0_db: pl0(869.525e6),
            }
        }
        Region::Us915 => {
            let (bitrate, overhead, sens) = lora(sf, 500_000);
            Phy {
                name: format!("LoRa SF{sf}/500 US915"),
                kind: CarrierKind::LoraControl,
                bitrate_bps: bitrate,
                overhead_ms: overhead,
                band: Some(0),
                rule_choice: Some((0, 0)),
                channels: vec![903_000_000],
                bw_hz: 500_000,
                tx_dbm: 30.0,
                sensitivity_dbm: sens,
                cca_threshold_dbm: sens + 2.0,
                capture_db: 6.0,
                pl0_db: pl0(903e6),
            }
        }
    }
}

pub fn bulk_phy(preset: BulkPreset) -> Phy {
    match preset {
        BulkPreset::GfskO => {
            let (band, _) = EU868.band("O").unwrap();
            Phy {
                name: "GFSK 100k band O 500 mW 10 %".into(),
                kind: CarrierKind::GfskBulk,
                bitrate_bps: 100_000,
                overhead_ms: 2,
                band: Some(band),
                rule_choice: Some((band, 0)),
                channels: vec![869_525_000],
                bw_hz: 200_000,
                tx_dbm: 27.0,
                sensitivity_dbm: -107.0,
                cca_threshold_dbm: -104.0,
                capture_db: 6.0,
                pl0_db: pl0(869.525e6),
            }
        }
        BulkPreset::GfskL => {
            let (band, _) = EU868.band("L").unwrap();
            Phy {
                name: "GFSK 100k band L 25 mW polite AFA×15".into(),
                kind: CarrierKind::GfskBulk,
                bitrate_bps: 100_000,
                overhead_ms: 2,
                band: Some(band),
                rule_choice: Some((band, 1)),
                channels: (0..15).map(|i| 865_100_000 + i * 200_000).collect(),
                bw_hz: 200_000,
                tx_dbm: 14.0,
                sensitivity_dbm: -107.0,
                cca_threshold_dbm: -104.0,
                capture_db: 6.0,
                pl0_db: pl0(866.5e6),
            }
        }
        BulkPreset::EspNow => Phy {
            name: "ESP-NOW LR 2.4 GHz 100 mW".into(),
            kind: CarrierKind::EspNow,
            bitrate_bps: 250_000,
            overhead_ms: 2,
            band: None,
            rule_choice: None,
            channels: vec![2_412_000_000],
            bw_hz: 20_000_000,
            tx_dbm: 20.0,
            sensitivity_dbm: -100.0,
            cca_threshold_dbm: -97.0,
            capture_db: 6.0,
            pl0_db: pl0(2.412e9),
        },
        BulkPreset::LoraBulk => {
            let mut p = control_phy(Region::Eu868, 7);
            p.name = "LoRa SF7/125 band O 500 mW 10 % (bulk)".into();
            p.kind = CarrierKind::LoraBulk;
            p
        }
        BulkPreset::GfskUs => Phy {
            name: "GFSK 300k US915 1 W".into(),
            kind: CarrierKind::GfskBulk,
            bitrate_bps: 300_000,
            overhead_ms: 2,
            band: Some(0),
            rule_choice: Some((0, 0)),
            channels: vec![915_000_000],
            bw_hz: 500_000,
            tx_dbm: 30.0,
            sensitivity_dbm: -104.0,
            cca_threshold_dbm: -101.0,
            capture_db: 6.0,
            pl0_db: pl0(915e6),
        },
    }
}

pub fn region_for(preset: BulkPreset) -> Region {
    match preset {
        BulkPreset::GfskUs => Region::Us915,
        _ => Region::Eu868,
    }
}

pub fn profile_for(preset: BulkPreset) -> &'static RegionProfile {
    match preset {
        BulkPreset::EspNow => &EU868, // control carrier still in EU band O
        BulkPreset::GfskUs => &US915,
        _ => &EU868,
    }
}

#[allow(dead_code)]
pub fn ism2400() -> &'static RegionProfile {
    &ISM2400
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Propagation {
    pub exponent: f64,
    pub shadow_sigma_db: f64,
}

pub fn distance_loss(prop: &Propagation, d_m: f64) -> f64 {
    10.0 * prop.exponent * d_m.max(1.0).log10()
}

/// Distance at which the link budget is exhausted (no shadowing): for the budget subcommand.
pub fn range_m(phy: &Phy, prop: &Propagation) -> f64 {
    let budget = phy.tx_dbm - phy.sensitivity_dbm - phy.pl0_db;
    10f64.powf(budget / (10.0 * prop.exponent))
}
