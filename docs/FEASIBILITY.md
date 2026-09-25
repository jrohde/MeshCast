# Feasibility

This document records the numbers the design rests on, where they come from, and what the
original brainstorm got wrong. Everything here was checked against primary sources in
September 2026. Where a figure is an estimate, the assumption is shown so it can be redone.

## 1. Regulatory budget in Europe (863–870 MHz)

Source: ETSI EN 300 220-2 V3.3.1 (2025-03), Table 4 (bands) and Table 18 (polite spectrum access).
<https://www.etsi.org/deliver/etsi_en/300200_300299/30022002/03.03.01_60/en_30022002v030301p.pdf>

| Band | Frequency | Max e.r.p. | Access rule | Max occupied bandwidth |
|---|---|---|---|---|
| K | 863–865 MHz | 25 mW | ≤ 0.1 % duty cycle **or polite** | 2 MHz |
| L | 865–868 MHz | 25 mW | ≤ 1 % duty cycle **or polite** | 3 MHz |
| M | 868.0–868.6 MHz | 25 mW | ≤ 1 % duty cycle or polite | 600 kHz |
| N | 868.7–869.2 MHz | 25 mW | ≤ 0.1 % duty cycle or polite | 500 kHz |
| O | 869.4–869.65 MHz | **500 mW** | ≤ **10 %** duty cycle or polite | **250 kHz** |
| P | 869.7–870.0 MHz | 5 mW | no requirement | 300 kHz |
| Q | 869.7–870.0 MHz | 25 mW | ≤ 1 % duty cycle or polite | 300 kHz |

Polite spectrum access (Table 18), the alternative to duty cycle in bands K, L, M, N, O, Q:

| Parameter | Limit |
|---|---|
| Minimum clear-channel-assessment (CCA) listen | 160 µs |
| CCA threshold | 15 dB above the Rx sensitivity limit (11 dB for 100–500 mW); example in §4.5.1.3: ≤ −94 dBm for 200 kHz, so a threshold of about −79 dBm |
| Max single transmission (Ton_max) | 1 s (4 s for a dialogue) |
| Min off time on the same frequency (Toff_min) | 100 ms |
| Max cumulative on-time | **100 s per hour per 200 kHz of spectrum** |
| Note in the standard | "Longer accumulated transmission time is possible by implementing more AFA channels." |

### What this means for MeshCast

**Correction 1: band O is only 250 kHz wide.** The brainstorm assumed 300 kbit/s GFSK at 10 % duty
cycle. A 300 kbit/s GFSK signal occupies far more than 250 kHz (occupied bandwidth ≈ bit rate +
2 × deviation). Within 250 kHz the realistic GFSK rate is 100–150 kbit/s (for example 100 kbit/s
with 25–50 kHz deviation, Carson bandwidth 150–200 kHz). Average throughput per transmitter is
therefore **10–15 kbit/s**, not 30.

**Opportunity: polite access in band L.** Band L is 3 MHz wide, so 15 slices of 200 kHz, each
allowing 100 s per hour. A node hopping across all of them (adaptive frequency agility) may
accumulate up to 1500 s per hour, about **42 % airtime**, at 25 mW. With 100 kbit/s GFSK in
200 kHz channels that is roughly **40 kbit/s average**, four times band O, at 13 dB less power.
Note that LoRaWAN and Helium uplinks live at 867.x MHz, inside band L, so listen-before-talk there
is a necessity, not a courtesy. In band O, polite access gives only about 100–125 s per hour
(one and a quarter 200 kHz slices), less than the 10 % duty cycle; use the duty cycle there.

**Open legal question:** 863–865 MHz is also allocated for wireless audio applications (10 mW,
no duty cycle). That annex targets audio equipment such as wireless headphones; whether a
self-built digital bulk transfer of audio files qualifies is doubtful. Recorded as a question, not
a plan.

## 2. Radio hardware

### SX1262 (edge nodes)

Source: Semtech SX1261/2 datasheet rev 1.2, Table 3-7/3-8 (Rx sensitivity), §6.
<https://cdn.sparkfun.com/assets/6/b/5/1/4/SX1262_datasheet.pdf>

| Mode | Sensitivity |
|---|---|
| GFSK 38.4 kbit/s (dev 40 kHz, BW 160 kHz) | −109 dBm |
| GFSK 250 kbit/s (dev 125 kHz, BW 500 kHz) | −104 dBm |
| GFSK 100 kbit/s | ≈ −107 dBm (interpolated, not in datasheet) |
| LoRa SF7 / 125 kHz | −124 dBm |
| LoRa SF7 / 250 kHz | −121 dBm |
| LoRa SF7 / 500 kHz | −117 dBm |
| LoRa SF12 / 125 kHz | −137 dBm |

FSK bit rate 0.6–300 kbit/s, deviation 0.6–200 kHz, Rx filter 4.8–467 kHz, frequency range
150–960 MHz (covers every band in this document).

### SX1302 (Helium miner concentrator, RAK2287)

Source: Semtech SX1302 datasheet.
<https://www.elecrow.com/download/product/CRT01266M/SX1302_Datasheet.pdf>

- 8 × SF5–SF12 LoRa demodulators plus 8 × SF5–SF10, one 125/250/500 kHz single-SF LoRa
  demodulator, **one (G)FSK demodulator**. GFSK sensitivity specified at 50 kbit/s: −111 dBm.
- Transmits one packet at a time.

**Correction 2:** the SX1302 is an excellent metadata hub (it hears 8 LoRa channels at once) but it
is not "8 parallel FSK receivers". It receives one FSK stream.

### ESP-NOW long-range mode (2.4 GHz, any ESP32 with WiFi)

Source: Espressif developer blog, field test with ESP32-C6 and PCB antenna, December 2024.
<https://developer.espressif.com/blog/esp-now-for-outdoor-applications/>

| Condition | ESP-NOW-LR throughput |
|---|---|
| Open field, 150 m | 100 kbit/s |
| Open field, 900 m | ~10 kbit/s |
| Open field, packet success | ~100 % to 450 m, 40 % at 900 m |
| Forest, 200 m | 80 kbit/s |

No duty cycle. In the EU, 2.4 GHz wideband under EN 300 328 allows 100 mW EIRP. ESP-NOW needs no
access point, SSID or pairing: it is broadcast on a fixed channel, which makes it zero-config.

## 3. Range estimate

Log-distance path loss with exponent n = 3 (suburban), free-space loss at 1 m of 31 dB at 868 MHz
and 40 dB at 2.4 GHz, dipole antennas, no fade margin. **Halve these in a city.** The simulator
replaces this with a proper model with shadowing.

| Link | TX | Sensitivity | Path-loss budget | Range |
|---|---|---|---|---|
| GFSK 100 kbit/s, band L | 14 dBm (25 mW) | −107 dBm | 121 dB | ~1 km |
| GFSK 100 kbit/s, band O | 27 dBm (500 mW) | −107 dBm | 134 dB | ~2.7 km |
| LoRa SF7/125, band O | 27 dBm | −124 dBm | 151 dB | ~10 km |
| LoRa SF12/125, band O | 27 dBm | −137 dBm | 164 dB | ~27 km (horizon-limited in practice) |
| ESP-NOW LR | ~20 dBm | ~−105 dBm | 125 dB | ~0.7 km (matches the field test) |

The FSK/LoRa gap of about 20 dB is the price of speed: FSK moves roughly nine times more bits per
second of airtime than LoRa SF7/250 but reaches a third as far.

## 4. Throughput per transmitter, by regime

| Regime | Raw rate | Airtime allowed | Average | Per hour | Opus music (24 kbit/s) per hour |
|---|---|---|---|---|---|
| EU band O, 500 mW, 10 % DC | 100–150 kbit/s | 10 % | 10–15 kbit/s | 4.5–6.8 MB | 25–37 min |
| EU band L, 25 mW, polite + AFA | 100 kbit/s | up to ~42 % | ~40 kbit/s | ~19 MB | ~1.7 h |
| ESP-NOW LR, 100 mW | 50–100 kbit/s (distance-dependent) | 100 % | 50–100 kbit/s | 22–45 MB | 2–4 h |
| US 902–928 MHz, FCC 15.247 digital modulation, 1 W | 300 kbit/s (≥ 500 kHz 6 dB bandwidth) | 100 % | 300 kbit/s | 135 MB | ~12 h |
| Internet | n/a | n/a | n/a | unlimited | unlimited |

Opus at 16 kbit/s mono (speech, or lo-fi music) halves the file sizes; Codec2 speech at
1.2 kbit/s makes a 5-minute bulletin a 45 kB object.

## 5. Why it scales

- **Broadcast is one-to-many.** The carousel is heard by everyone in the cell. 100 000 receivers
  cost the same airtime as 10. Receivers never transmit.
- **Sources with internet cost no airtime.** They upload to a station; the station's carousel
  serves the cell. 5000 sources nationwide need airtime only where they have no uplink.
- **One announcer per cell.** Ten sources each using 10 % is a full channel; one announcer using
  10 % is one tenth of a channel. Spatial reuse between cells, like cellular telephony.
- **Delay tolerance makes polite access cheap.** A node that may wait arbitrarily long can afford
  high backoff and still deliver.

Where it strains: dense areas with many cells overlapping (the simulator must quantify the
hidden-node collision rate versus density), and niche content that must cross many cells for few
listeners (the system naturally favours popular content, like Usenet did).

## 6. Corrections to the original brainstorm, summarised

1. Band O is 250 kHz wide: 100–150 kbit/s GFSK, not 300; 10–15 kbit/s average, not 30.
2. The SX1302 has one FSK demodulator, not eight.
3. Polite spectrum access with frequency agility in band L is a real alternative worth simulating.
4. Sub-GHz FSK is not automatically the best bulk carrier; ESP-NOW LR beats it inside a
   neighbourhood and internet beats everything. The protocol must be carrier-agnostic.
5. RaptorQ inactivation decoding is heavy on a microcontroller; feasible on the XIAO ESP32S3
   (8 MB PSRAM) for 50–200 kB blocks, but carousel plus a compact NACK bitmap is the simpler first step.
6. Regulation is per region, not Dutch. See [ETHERDISCIPLINE.md](ETHERDISCIPLINE.md).
