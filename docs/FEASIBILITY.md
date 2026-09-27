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

## 6. Corrections to the original brainstorm, summarised (before simulation)

1. Band O is 250 kHz wide: 100–150 kbit/s GFSK, not 300; 10–15 kbit/s average, not 30.
2. The SX1302 has one FSK demodulator, not eight.
3. Polite spectrum access with frequency agility in band L is a real alternative worth simulating.
4. Sub-GHz FSK is not automatically the best bulk carrier; ESP-NOW LR beats it inside a
   neighbourhood and internet beats everything. The protocol must be carrier-agnostic.
5. RaptorQ inactivation decoding is heavy on a microcontroller; feasible on the XIAO ESP32S3
   (8 MB PSRAM) for 50–200 kB blocks, but carousel plus a compact NACK bitmap is the simpler first step.
6. Regulation is per region, not Dutch. See [ETHERDISCIPLINE.md](ETHERDISCIPLINE.md).


## 7. Phase 0 simulation results

The simulator in `sim/` runs the real `meshcast-core` protocol on modelled radios (log-distance
path loss, exponent 3, 6 dB shadowing, capture, half-duplex, CCA, per-band accounting). Tracks are
540 kB (3 minutes of Opus at 24 kbit/s). Every number below is reproducible with the command
shown; seeds are fixed. These are simulation results, not measurements: Phase 1 checks them
against real radios.

### 7.1 What the simulator changed in the design

Five protocol defects were found and fixed before any hardware existed:

1. **The carousel never went idle**, looping manifests forever at the full duty cycle and
   causing half-duplex losses during uploads. Now: `max_passes` per object, manifests at most every
   5 minutes, silence when nothing is wanted.
2. **Control and bulk shared one duty-cycle budget** (EU band O), so the carousel starved the
   beacons and followers declared the announcer dead. Now: 10 % of the budget reserved for control.
3. **A missed upload symbol caused a full re-upload** of 2700 symbols. Now: NACKs are answered
   by the uploading source too, with exactly the missing symbols.
4. **Electing over a long-range control carrier elects announcers that followers cannot hear
   on the bulk carrier.** With LoRa control (10 km) and GFSK bulk (2.7 km), 200 nodes on 30 km²
   ended up with two announcers, WANT/NACK storms and 100 million collisions; with ESP-NOW
   (460 m) nothing was delivered at all. Now: beacons, election, gossip and NACK travel on the bulk
   carrier; a cell is what hears each other there; LoRa carries only manifest discovery.
5. **Overlapping cells in one channel caused election churn** (50 000 role changes per day):
   announcers hearing each other weakly kept yielding on the id tie-break, orphaning their
   followers. Now: followers follow the strongest signal, announcers yield on a near-tie only to a
   strong (same-cell) or lonely counterpart, better nodes challenge after three beacons. Churn fell
   to 6 600 events per day, most of them normal switches between overlapping cells.

Two more found while testing frequency agility: hop sequences derived from a fixed per-announcer
offset never coincide, so nodes could not find each other (now pseudo-random sequences with
dwell-start beacons, scanning, and a common control-plane sequence every fifth dwell); and timers
aligned to dwell boundaries made edge-of-range nodes, invisible to CCA, collide every time (now
0–500 ms jitter before control frames).

And one found by asking where the last undelivered tracks went: all four came from one source
whose uploads to its announcer crawled at the throttle floor because five carousels shared the
channel. The fix became two principles rather than an exception: fresh content has right of way
over repetition, and occupancy by other MeshCast nodes is judged loosely ("fair among
ourselves") while foreign energy is judged strictly ("polite to strangers"). Together with
suppression-based uploads to any announcer, this also solved the cross-cell exchange question:
two clusters 1.8 km apart with separate announcers now exchange an album in 1.4 hours in band L.

### 7.2 Two nodes

`meshcast-sim two-nodes --distance-m D --tracks 3 --bulk B`

| Carrier | Distance | Track 1 / 2 / 3 complete after | Note |
|---|---|---|---|
| GFSK band O, 500 mW, 10 % | 1 km and 2 km | 15 / 25 / 36 min | 5 min of that is the initial election |
| GFSK band O | 5 km | never | out of GFSK range (2.7 km); LoRa control alone cannot carry content |
| LoRa SF7 as bulk (`lora-bulk`) | 5 km | 3.0 / 5.9 / 8.8 h | the sparse-cell fallback: slow but works |
| GFSK band L, 25 mW, polite, 15 channels | 800 m | 16 / 21 / 27 min | includes finding the hop sequence |
| ESP-NOW LR, 100 mW | 300 m | 6 / 7 / 8 min | no duty cycle |

### 7.3 One cell, density sweep

`meshcast-sim cell --nodes N --area-km2 1 --stations 1 --sources 1 --tracks 3 --hours 8`

| Nodes per km² | Followers complete (3 tracks) | p50 completion | max | Collisions (per receiver) |
|---|---|---|---|---|
| 10 | 9 / 9 | 15 / 25 / 36 min | same | 18 |
| 100 | 99 / 99 | 15 / 25 / 36 min | same | 1 141 |
| 1000 | 999 / 999 | 15 / 25 / 36 min | 25 / 46 / 76 min | 147 431 |

Receivers cost nothing: a thousand followers complete at the same median time as ten. The tail
at 1000 nodes is NACK repair after collisions between the few nodes that do transmit (WANTs).
Delivered volume scales with followers: 1.9, 20.5 and 207 MB per hour for the same announcer
airtime.

### 7.4 Failover

`meshcast-sim failover --nodes 20 --kill-at-h 2 --revive-at-h 6 --hours 10`

The station (announcer) is switched off at 2.00 h. A new announcer is running at 2.07 h
(260 s: three missed 60 s beacons plus the score-weighted wait). At no time are there two
announcers, except for one beacon interval when the revived station, rebooted as a follower,
challenges and takes the role back. Listeners keep playing their local copies throughout.

### 7.5 A town: 200 nodes on 30 km², 3 stations, 5 sources × 10 tracks, 24 h

`meshcast-sim cell --nodes 200 --area-km2 30 --stations 3 --sources 5 --tracks 10 --hours 24 --bulk B`

| Bulk carrier | Announcers at end | Role events / day | Tracks fully delivered (of 50) | Follower-completions | Mean of per-track median completion | Bulk occupancy p50 / max |
|---|---|---|---|---|---|---|
| GFSK band O, before fresh-content priority | 5 | 6 645 | 45 | 94 % | 9.4 h | 20 % / 82 % |
| GFSK band O, fair share, before colouring and grants | 5 | 14 637 | 50 | 100 % | 10.5 h | 32 % / 92 % |
| GFSK band L, fair share, before colouring and grants | 26 | 4 791 | 50 | 100 % | 11.4 h | 17 % / 74 % |
| GFSK band O, final (colouring, slots, granted uploads) | 5 | 9 949 | 49 | **100.0 %** (9 949 of 9 950) | **7.3 h** | 4 % / 100 % |
| GFSK band L, final | 19 | 8 557 | 4 | **66 %** | 14.4 h | 17 % / 67 % |
| ESP-NOW LR (460 m cells) | not connected at this density: see §7.5.1 | | | | | |

Band O: with the final code the five announcers take turns in time slots on the one channel,
which brought the median from 10.5 to 7.3 hours and the typical occupancy from 32 % to 4 %,
with the same complete delivery. Most role events are followers moving between overlapping
cells, not announcer changes.

Band L is the honest disappointment of this round: before colouring and granted uploads,
about 25 cells on 15 channels with random hopping and any-holder uploads delivered everything in
11.4 hours; with them, 66 % in 24 hours. On a carrier with many channels, random hopping already
kept coinciding announcers to one dwell in fifteen, while the grant round-trips at the
100-second meeting-dwell cadence, one object per holder at a time, and uploads to different
announcers landing on coinciding channels (10 million upload-against-upload collisions) slow the
cross-cell path down. The mechanisms that rescued the scarce-channel cases (band O, ESP-NOW)
cost the rich-channel case. This is the first item of open question 10: either the grant path
must become as cheap as the old any-holder path where channels are plentiful, or the rules must
be chosen per carrier by channel count, which is not yet a principle. Content crosses cells through bridge nodes that hear two
carousels and through holders answering neighbouring announcers' WANTs; a track published at one
edge reaches the far edge after several hours.

The fresh-content rule turned 45 of 50 into 50 of 50, at the cost of more airtime and role
churn in band O. Whether that churn matters on real hardware is a Phase 1 question.

#### 7.5.0 A neighbourhood: 50 nodes on 1 km², 1 station, 2 sources × 10 tracks, 3 h

`meshcast-sim cell --nodes 50 --area-km2 1 --stations 1 --sources 2 --tracks 10 --hours 3 --bulk B`

| Bulk carrier | Announcers | Tracks fully delivered (of 20) | Follower-completions | Mean p50 | Occupancy p50 / max | Collisions |
|---|---|---|---|---|---|---|
| GFSK band O | 1 | 20 | 100 % | 1.10 h | 18 % / 27 % | 0 |
| ESP-NOW, one channel, fixed 50 % share | 8 | 15 | 85 % | 1.09 h | 50 % / 100 % | 37 M |
| ESP-NOW, one channel, derived fair share | 5 | 15 | 84 % | 1.43 h | 27 % / 100 % | 12.6 M |
| ESP-NOW, channels 1/6/11, fair share | 6 | 12 | 78 % | 1.82 h | 24 % / 74 % | 7.3 M |

The surprise: on 1 km² ESP-NOW is not faster than band O, despite 25 times the raw bit rate.
Band O reaches the whole square from one announcer at 10 % duty cycle with no collisions at all.
ESP-NOW's 460 m reach splits the square into five to eight cells whose announcers cannot hear
each other but whose followers hear several of them: the hidden-node case in its purest form.
Deriving each announcer's share from the number of announcers it hears (instead of a fixed 50 %)
cut collisions by two thirds and occupancy in half but did not raise delivery; spreading cells
over the three non-overlapping WiFi channels cut collisions again but slowed cross-cell
propagation, because a follower is deaf to other cells except during the common dwell.

Following the neighbourhood down to its causes took five more rounds, each one measured before
the next change (per-node collision counters, then collisions by sender role, then by object):

| Round | Finding in the data | Change | Result at 3 h |
|---|---|---|---|
| 1 | every node loses 10–55 % of bulk frames; random hop sequences put conflicting announcers on the same channel a third of the time | conflict colouring: followers report the announcers they hear, announcers take a colour, colour = channel offset or time slot; meeting dwell is control-only | 88 % |
| 2 | colouring collapses after 30 min: followers reported a conflict once | periodic re-reports from bridge followers | 78 % |
| 3 | stale colours: reports carried the colour a follower saw long ago | reports carry current colours, changes reported at once, gossip carries the followed announcer's colour | 80 %, colouring valid |
| 4 | 4.6 M of 6.6 M bulk collisions are upload against upload; 1 383 uploads started for 100 needed | offer-and-grant replaces any-holder uploads; ask only for what is not flowing | 65 % (grants lapsed while uploaders were queued) |
| 5 | granted uploaders switched objects on every re-ask | grants queue per holder, one object per holder at a time, NACKs answered by the granted uploader only, grants live 10 min | 52 % at 3 h, **88 % at 6 h**, upload redundancy 6× |
| 6 | (from the dynamics scenario) nodes collected and kept objects of channels they do not follow | you carry what you listen to; orphans evicted | unchanged |
| 7 | nearly complete objects were re-asked in full; grants forgotten after 10 min left NACKs unanswered; cross-cell NACKs never reached the other sequence; uploaders used stale colours | objects ≥ 80 % repaired by NACK only; grants stay answerable while the announcer asks; announcer NACKs in the meeting dwell; gossip refreshes colours | 80 % at 6 h, uploads 252 (from 661) |
| 8 | uploaders that had never heard the asking announcer's beacon defaulted to colour 0 and collided | an announcer's gossip states its colouring and counts as a sighting; beacons carry the colour count | **91 % at 6 h** (best), band L neighbourhood 70 % |
| 9 | uploaders on opposite sides of one cell cannot hear each other (hidden uploaders) | tried: one inbound upload per announcer at a time | collisions gone (0.7 %) but throughput halved: 69 %, and single-cell band O fell to 75 %; **rejected** |
| 10 | same, keeping parallel budgets | tried: uploaders to one announcer take turns from their position in its WANT list, nested inside announcer slots | 74 %, clusters in band L 67 %; positions shift as objects complete and turns starve; **rejected** |

The reverted state (round 8) is what ships: parallel granted uploads. Hidden uploaders inside one
cell remain the known residual of the 2.4 GHz and 25 mW cases.

The sub-GHz cases were re-run after every round and stayed at 100 %; the grant mechanism also
cut their uploads to the minimum (a 20-node cell: 4 uploads for 3 tracks and a manifest).

ESP-NOW between overlapping cells is therefore *better but not solved*: 86–91 % of
follower-completions in 6 hours against 100 % in 1.8 hours on band O, with a bulk collision
rate still near 17 % and about four uploads per object per cell instead of one. The remaining cause is
pull-based fetching among hidden nodes: a grant lapses when the uploader's frames are lost, the
re-ask brings in another holder, and the duplicates collide with each other. Within one cell
ESP-NOW is the fastest carrier we have (two nodes at 300 m: three tracks in eight minutes).
Recorded, with candidate remedies, as open question 10 in PROTOCOL.md. In practice the sub-GHz
carrier carries content between cells and ESP-NOW distributes it within a street; the simulator
models one bulk carrier per node, so that combination is untested.

#### 7.5.1 ESP-NOW at town scale

200 nodes on 30 km² is 6.7 nodes per km²; at ESP-NOW's 460 m reach a node hears about four
others, and the graph of who hears whom falls apart into islands. The 24-hour run did not finish
in the time allowed (tens of announcers each at a 50 % airtime share generate far more frames
than a duty-cycled carrier), and the model says the outcome anyway: ESP-NOW cannot bridge the
gaps between islands. It is a neighbourhood carrier, not a town carrier; the sub-GHz carrier
carries content across the gaps and ESP-NOW distributes it within a street. The dense-town run
below shows it in its element.

### 7.6 A living network: many channels, changing subscriptions, daily bulletins

`meshcast-sim dynamics --nodes 50 --area-km2 1 --stations 1 --channels 8 --follows 3 --publish-h 24 --churn-h 6 --hours 72`

Eight channels, each node follows three of them, every six hours a tenth of the nodes swap one
subscription, and every channel publishes a new 300 kB bulletin (five minutes of Opus speech)
every `publish_h` hours while dropping its oldest, keeping three in its manifest.

| Bulk carrier | Publish every | Delivered to followers within one period | Latency per bulletin (p50 / p90) | Uploads | Orphans per node at end |
|---|---|---|---|---|---|
| GFSK band O | 24 h (72 h run, 47 publications) | 99.1 % | ~7 min / ~7 min | 78 | 0.5 |
| GFSK band O | 6 h (48 h run, 87 publications) | 99.8 % | ~7 min / ~7 min | 158 | 1.3 |
| GFSK band L | 24 h (72 h run) | 88.8 % | | 491 | 0.5 |

In band O one announcer serves the square; a fresh bulletin reaches its followers seven minutes
after publication, whatever the subscription pattern, and subscription changes cost nothing but
a WANT. Eviction keeps storage bounded (half an orphaned object per node, waiting for the next
sweep). In band L the same square splits into two 25 mW cells and the cross-cell path through
offers, grants and meeting dwells is slower and still leaky; this is the same weakness as
ESP-NOW's, on a smaller scale.

What this scenario does not model yet: nodes that follow nothing (pure relays), storage limits
below the working set, and a source that publishes faster than its cell can carry.

### 7.7 A fresh measurement pass

After the design had settled, a pass with one new instrument, a counter that records *why* a
ready content frame was not sent, found nine faults. None of them was where we had been looking:
time slots, which we suspected, cost nothing at all. Each fix replaces a rule that had drifted
from its principle.

| # | What the data said | The principle it restored |
|---|---|---|
| 1 | Announcers in band L were blocked by the airtime accounting 42 % of the time while using a quarter of their legal budget | A signal that ends exactly on a 200 kHz boundary does not occupy the slice above it. With inclusive edges every channel cost two slices and every slice was shared by two channels |
| 2 | A blocked node sat still for up to 60 s, three dwells, on channels that had budget | Waiting is for a radio with one channel; a hopping radio hops |
| 3 | Well-separated announcers were throttled to a fraction of their entitlement | Share the channel only with those colouring could not separate from you |
| 4 | One 20 ms frame per mandatory 100 ms pause spent five sixths of a polite band on silence | A transmission lasts as long as it needs, not as long as it may: pause × p/(1 − p) |
| 5 | An announcer sat at 95 % of an object and sent 1 223 unanswered NACKs | A node can reach 95 % by overhearing, so a NACK is an ask: any holder may answer it |
| 6 | Repairs were queued and never sent | An upload queued behind nothing must start. One helper decides where an upload goes |
| 7 | 93 000 repair answers in one cell, channel at 72 % occupancy | Cancelling a duplicate must also cancel the answer that has not begun, which was the common case |
| 8 | An object picked up by overhearing never got an uploader assigned | The want list is where responsibility is assigned; an ownerless object belongs on it however complete it is |
| 9 | Whoever answered first was whoever happened to draw the shortest delay | Whoever hears the asker best answers first, ranked against the holder's own neighbours so no absolute signal level is needed |

Results, against the state before the pass:

| Scenario | Before | After |
|---|---|---|
| ESP-NOW neighbourhood, 50 nodes on 1 km², 6 h | 86 % | **100 %** |
| Band L, 100 nodes on 15 km², 12 h | 69 % | **100 %** |
| Band O, 100 nodes on 15 km², 12 h | 45 %, occupancy 73 % | **100 %**, occupancy 24 % |
| Band L neighbourhood, 50 nodes on 1 km² | 79 % | 90 % |
| Band L dynamics, 8 channels with churn | 83 % | **99.1 %** |
| Band O dynamics | 99.1 % | 99.1 % |
| Clusters, density, failover, two-node cases | 100 % | 100 %, and faster |

The remaining weak case is a dense neighbourhood in band L: two or three announcers in one square
kilometre at 25 mW, each picking up most of the other's content by overhearing and repairing the
last few per cent. It delivers 90 % in six hours at 37 % occupancy.

### 7.8 What Phase 0 could not answer

- Real GFSK sensitivity at 100 kbit/s (interpolated), real CCA behaviour, and real building
  loss: Phase 1.
- Whether 0–500 ms jitter and a 30 % occupancy target are the right values in a live band with
  LoRaWAN and other users: Phase 2 measurements.
- Announcer-to-announcer exchange without bridge followers (PROTOCOL.md open question 7).
