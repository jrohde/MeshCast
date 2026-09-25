# EtherDiscipline

**EtherDiscipline** (Dutch loanword, "ether discipline"): the legal limits of each region, encoded as
a profile and enforced in firmware. Where [EtherFatsoen](ETHERFATSOEN.md) is how nodes treat each
other, EtherDiscipline is how nodes obey the law. The two are separate layers on purpose: etiquette
may be tuned by simulation, discipline is not negotiable.

> **Verify before relying on this.** Regulations change and the authors are not lawyers. Every row
> below cites its source; if you find a discrepancy, open an issue with the newer source. Where two
> sources disagree the profile takes the more conservative value and the disagreement is noted.

## 1. Rules of the discipline layer

1. **No profile, no transmission.** A node without a region profile stays in receive-only mode.
   There is no "worldwide" default.
2. **Frequency whitelist.** The radio may only be tuned to frequencies inside the profile's bands,
   with the profile's maximum occupied bandwidth per band.
3. **Power cap.** Transmit power is capped per band at the profile's e.r.p./EIRP limit minus the
   configured antenna gain. Unknown antenna gain is treated as the worst plausible case for the
   board (a 2.15 dBi dipole unless the profile says otherwise).
4. **Airtime accounting per band, sliding window.** Every transmission is logged with its
   duration and centre frequency. Before transmitting, the node checks that the transmission fits:
   - duty-cycle regimes: cumulative on-time in the last 3600 s plus the new frame ≤ limit × 3600 s;
   - polite regimes: cumulative on-time in the last 3600 s in every 200 kHz slice the frame touches
     plus the new frame ≤ 100 s, single frame ≤ Ton_max, and ≥ Toff_min since the last frame on
     that nominal frequency;
   - dwell regimes (FCC hopping): average occupancy per frequency ≤ 400 ms per 20 s (or 10 s);
   - ARIB-style regimes: carrier sense ≥ 5 ms below threshold, frame ≤ 4 s, pause ≥ 50 ms.
5. **Clear channel assessment where required.** In polite regimes the node listens for at least
   the CCA interval and defers if the channel is above the threshold. This is the legal minimum;
   EtherFatsoen adds much more on top.
6. **Fail closed.** If the accounting state is lost (reboot), the node assumes the window is full
   until a full window has elapsed, unless it can prove otherwise from persistent storage.
7. **The protocol never assumes a region.** Frame formats, timers and election rules are
   identical everywhere. Only the budget differs.

## 2. Region profiles

### EU868 (CEPT countries: EU, UK, Norway, Switzerland, and others adopting ERC/REC 70-03)

Source: ETSI EN 300 220-2 V3.3.1 (2025-03), Table 4 and Table 18.
<https://www.etsi.org/deliver/etsi_en/300200_300299/30022002/03.03.01_60/en_30022002v030301p.pdf>

| Band | Frequency | Max e.r.p. | Rule | Max bandwidth | MeshCast use |
|---|---|---|---|---|---|
| K | 863–865 MHz | 25 mW (14 dBm) | 0.1 % DC or polite | 2 MHz | polite bulk (secondary) |
| L | 865–868 MHz | 25 mW | 1 % DC or polite | 3 MHz | **polite bulk, 15 × 200 kHz AFA** |
| M | 868.0–868.6 MHz | 25 mW | 1 % DC or polite | 600 kHz | avoid (LoRaWAN join channels) |
| N | 868.7–869.2 MHz | 25 mW | 0.1 % DC or polite | 500 kHz | avoid |
| O | 869.4–869.65 MHz | 500 mW (27 dBm) | 10 % DC or polite | 250 kHz | **control (LoRa) + high-power bulk** |
| P | 869.7–870.0 MHz | 5 mW | none | 300 kHz | low-power control, optional |
| Q | 869.7–870.0 MHz | 25 mW | 1 % DC or polite | 300 kHz | optional |

Polite access limits (Table 18): CCA ≥ 160 µs; CCA threshold ≤ 15 dB above the Rx sensitivity
limit of §4.5.1.3 (11 dB for 100–500 mW e.r.p.); Ton_max 1 s (4 s dialogue); Toff_min 100 ms on
the same nominal frequency; 100 s per hour per 200 kHz.

Notes: Meshtastic's EU_868 profile uses band O with a 10 % duty cycle at 27 dBm and marks audio as
not permitted; LoRaWAN's EU868 plan uses +16 dBm EIRP by convention, which is not the legal limit.
National implementations (e.g. the Dutch "Regeling gebruik van frequentieruimte zonder vergunning")
follow the CEPT recommendation; check national exceptions.

### US915 (United States; Canada under RSS-247 is similar, verify)

Source: 47 CFR § 15.247. <https://www.law.cornell.edu/cfr/text/47/15.247>

| Regime | Requirement | Max peak output | Extra |
|---|---|---|---|
| Frequency hopping, channel 20 dB BW < 250 kHz | ≥ 50 hopping frequencies; average occupancy per frequency ≤ 0.4 s in any 20 s | 1 W (≥ 50 channels), 0.25 W (25–49 channels) | max hopping-channel 20 dB BW 500 kHz |
| Frequency hopping, channel 20 dB BW ≥ 250 kHz | ≥ 25 hopping frequencies; ≤ 0.4 s in any 10 s | as above | |
| Digital modulation (§15.247(b)(3)) | 6 dB bandwidth ≥ 500 kHz | 1 W | PSD ≤ 8 dBm in any 3 kHz (§15.247(e)) |
| Antenna gain | output reduced by the dB the antenna exceeds 6 dBi (§15.247(b)(4)) | | |

No duty cycle. MeshCast use: GFSK 300 kbit/s or LoRa 500 kHz as digital modulation with ≥ 500 kHz
6 dB bandwidth, or GFSK ≤ 500 kHz channels with ≥ 50-channel hopping and 400 ms dwell. The
"hybrid" clause (§15.247(f)) requires each mode to comply independently. Meshtastic US profile:
902–928 MHz, 30 dBm, no duty cycle.

### AU915 / ANZ (Australia, New Zealand; Meshtastic also maps Argentina, Brazil, Chile, Colombia, Ecuador here)

Sources: ACMA Low Interference Potential Devices class licence (Australia); RSM General User
Radio Licence for Short Range Devices (New Zealand); LoRaWAN RP002-1.0.3 AU915-928 (+30 dBm EIRP,
no duty cycle, 400 ms dwell assumed until told otherwise); Meshtastic ANZ profile 915–928 MHz,
30 dBm, no duty cycle.

| Band | Max EIRP | Rule |
|---|---|---|
| 915–928 MHz | 1 W (30 dBm) | no duty cycle; digital-modulation/hopping conditions per ACMA LIPD |
| NZ 864–868 MHz (NZ_865) | Meshtastic uses 36 dBm; verify with RSM | |

Brazil: ANATEL allows 902–907.5 and 915–928 MHz (Meshtastic BR_902); verify.

### AS923 (South-East Asia, Japan sub-variant)

Source: LoRaWAN RP002-1.0.3 AS923-1..4: 915–928 MHz (variants offset), +16 dBm EIRP default, < 1 %
duty cycle where applicable, 400 ms dwell in some countries; "AS923 end-devices operated in Japan
SHALL perform Listen Before Talk". Country-specific overrides from Meshtastic's table:

| Country | Band | Max power | Rule | Source |
|---|---|---|---|---|
| Japan | 920.5–923.5 MHz | 20 mW (13 dBm) | ARIB STD-T108: carrier sense ≥ 5 ms (threshold −80 dBm), single transmission ≤ 4 s, pause ≥ 50 ms | ARIB STD-T108; Meshtastic PR #11747 |
| Korea | 920–923 MHz (LoRaWAN KR920: 920.9–923.3) | RP002: +14 dBm; Meshtastic: 23 dBm (**conflict, profile uses 14 dBm**) | LBT mandatory | RP002-1.0.3; Meshtastic |
| Taiwan | 920–925 MHz | 0.5 W indoor/coastal, 1 W outdoor | | NCC Low-power RF Devices Technical Regulations §5.8.1 |
| Thailand | 920–925 MHz | 27 dBm | 10 % duty cycle | NBTC; Meshtastic TH |
| Malaysia | 919–923 MHz 500 mW no restriction; 923–924 MHz 500 mW with 1 % DC or hopping | | MCMC SRD specification |
| Singapore | 917–925 MHz | 100 mW | no restriction | IMDA TS SRD |
| Philippines | 915–918 MHz | 250 mW EIRP, no external antenna | | NTC (via Meshtastic) |

### IN865 (India)

Source: RP002-1.0.3 IN865-867: 865–867 MHz, +30 dBm EIRP, no duty cycle or dwell listed.
Meshtastic IN: 865–867 MHz, 30 dBm. Verify against WPC (Department of Telecommunications) GSR
notifications.

### RU864 (Russia)

Sources: RP002-1.0.3 RU864-870: 864–870 MHz, +16 dBm EIRP, < 1 % duty cycle. Meshtastic RU:
868.7–869.2 MHz, 20 dBm, 100 % with LBT (cites GKRCh decision 18-46-03-1). **Conflict**: the
profile uses 864–870 MHz at +16 dBm with 1 % duty cycle unless LBT is implemented, then
868.7–869.2 MHz per the GKRCh annex. Verify.

### CN470 (China)

Source: RP002-1.0.3 CN470-510: 470–510 MHz, +19 dBm EIRP. Meshtastic CN: 470–510 MHz, 19 dBm.
Verify against MIIT regulations. The SX1262 tunes to 470 MHz; note the datasheet's remark that
some LoRa bandwidths scale below 400 MHz (not relevant at 470).

### Other profiles carried over from Meshtastic's table (verify each)

| Code | Band | Power | Rule | Meshtastic source |
|---|---|---|---|---|
| UA_868 | 868.0–868.6 MHz | 25 mW (14 dBm) | 1 % DC | NKRZI 2016 |
| UA_433 | 433.0–434.7 MHz | 10 mW | 10 % DC | NKRZI 2016 |
| EU_433 | 433.05–434.79 MHz | 10 mW | 10 % DC | EN 300 220-2 band H |
| KZ_863 | 863–868 MHz | 25 mW EIRP, 500 kHz channels, not at airfields | | Meshtastic issue #7204 |
| MY_433 | 433–435 MHz | 100 mW | none | MCMC |
| PH_868 | 868–869.4 MHz | 25 mW e.r.p. | | NTC |

### 2.4 GHz (worldwide ISM, for ESP-NOW and SX1280-class radios)

| Region | Rule | Source |
|---|---|---|
| EU | EN 300 328: wideband data (WiFi, FHSS) 100 mW EIRP; non-specific SRD 10 mW EIRP; no duty cycle | ETSI EN 300 328 |
| US | 47 CFR 15.247: 1 W, digital modulation ≥ 500 kHz or hopping; antenna-gain rule with point-to-point exception (1 dB per 3 dB above 6 dBi) | eCFR |

ESP-NOW is ordinary 802.11 framing and inherits the WiFi allowance.

## 3. Profile schema (draft)

```
profile:
  id: "EU868"
  sources: [ "ETSI EN 300 220-2 V3.3.1 Table 4", "... Table 18" ]
  bands:
    - id: O
      low_hz: 869400000
      high_hz: 869650000
      max_erp_dbm: 27
      max_occupied_bw_hz: 250000
      access:
        - kind: duty_cycle
          limit: 0.10
          window_s: 3600
        - kind: polite
          cca_us: 160
          cca_threshold_dbm: -83     # 11 dB above -94 dBm limit at this power class
          ton_max_s: 1
          toff_min_s: 0.1
          cum_on_s_per_hour_per_200khz: 100
  audio_permitted: false            # informational; MeshCast sends data, not analogue audio
```

A band may list several access rules; the node picks one per band at boot (never mixes them
within a window) and the accounting enforces that rule.

## 4. What EtherDiscipline does not cover

- Content law (copyright, broadcasting licences). MeshCast is a transport; channel owners are
  responsible for what they publish. The design intent is copyright-free and self-produced content.
- Amateur radio. Licensed amateurs may operate the same chips at higher power on amateur bands
  (e.g. 70 cm) under their own rules; a future `HAM_*` profile could encode that, with the
  no-encryption constraint most amateur regulations impose.
- Type approval and CE/FCC marking of assembled devices. Hobby use of certified modules is the
  assumed context.
