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

| Regime | Raw rate | Airtime allowed | Average | Per hour | Opus music (24 kbit/s) per hour | SNAC music (1.88 kbit/s) per hour |
|---|---|---|---|---|---|---|
| EU band O, 500 mW, 10 % DC | 100–150 kbit/s | 10 % | 10–15 kbit/s | 4.5–6.8 MB | 25–37 min | 5.3–8 h |
| EU band O, LoRa SF7/125, 10 % DC | 5.47 kbit/s | 10 % | ~0.55 kbit/s | ~0.25 MB | ~1.4 min | ~17 min |
| EU band L, 25 mW, polite + AFA | 100 kbit/s | up to ~42 % | ~40 kbit/s | ~19 MB | ~1.7 h | ~22 h |
| ESP-NOW LR, 100 mW | 50–100 kbit/s (distance-dependent) | 100 % | 50–100 kbit/s | 22–45 MB | 2–4 h | 26–53 h |
| US 902–928 MHz, FCC 15.247 digital modulation, 1 W | 300 kbit/s (≥ 500 kHz 6 dB bandwidth) | 100 % | 300 kbit/s | 135 MB | ~12 h | ~160 h |
| Internet | n/a | n/a | n/a | unlimited | unlimited | unlimited |

The LoRa row is computed, not measured: SF7 at 125 kHz and coding rate 4/5 is
7 × 125 000 / 2⁷ × 4/5 = 5.47 kbit/s on air, before headers. The last column is the codec chosen
in §8: an hour of music is 0.84 MB, an hour of speech (SNAC 24 kHz, 0.98 kbit/s) 0.44 MB. All
averages are per transmitter and before protocol overhead.

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
540 kB (3 minutes of Opus at 24 kbit/s). §8 later replaced Opus by SNAC, which makes a track
42 kB, so these runs load the network about 13 times harder than the chosen codec will; they
have not been re-run at the new size yet. Every number below is reproducible with the command
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
| Band L neighbourhood, second pass | 90 % | **100 %** |
| Band L town, 200 nodes on 30 km², 24 h | 66 % | **91.9 %** |
| Band O town, 200 nodes on 30 km², 24 h | 100 %, median 7.3 h | **100 %**, median 9.9 h |
| Clusters, density, failover, two-node cases | 100 % | 100 %, and faster |

Two of the nine were worth the whole pass on their own. Only announcers' NACKs may be answered by
arbitrary holders, because a follower's missing symbols are exactly what its own announcer's
carousel puts at the front of the next round; that one line took the band O town from 960 000
queued repair answers and 70 % occupancy to 13 %. And announcers must agree on the length of
their slot cycle: they had been computing it from their own view of the conflict graph and
arriving at 4, 5 and 6 slots side by side, so their turns overlapped anyway. The count now
travels with the conflict reports until they agree.

Band O at town scale was the one number that did not come back at first: 93.9 % against the
100 % it reached before the pass. Measuring rather than guessing found it in one look. Of the
time its announcers spent not transmitting, **1 325 hours were spent waiting for a time slot**
against 369 on listen-before-talk and 13 on the occupancy gate; the five announcers had agreed
on a cycle of nine slots, so each idled eight ninths of the time. Three tracks never entered the
mesh at all.

Taking turns is for carriers where nothing else bounds what everyone adds up to. In band O the
duty cycle already caps every transmitter at 10 %, so five announcers cannot exceed half the
channel however they are arranged, and listen-before-talk handles the rest. With slots used only
where no regulatory cap exists, the band O town returned to **100 % at a 9.9 hour median**, and
the three orphaned tracks were delivered as well. The channel is busier for it (33 % occupancy
against 13 %), a little above EtherFatsoen's 30 % target, which is the next thing to look at.

### 7.7.1 Fountain coding: tried, measured, not merged

Looking at where the airtime went turned up a larger waste than anything the pass had fixed:
**62 % of the symbols a node receives it already had**, and 75 % at town scale. A carousel makes
several passes of the same symbols, and every node catches an overlapping subset of each.
Fountain coding (RaptorQ, RFC 6330) is the standard answer: send a different encoding symbol
every time, and let a receiver finish as soon as it holds *enough* of them rather than the right
ones. It was in the roadmap as v1; the number made it the first thing worth building.

It is implemented on the branch `experiment/fountain-coding`: fresh symbol ids per pass, a block
that completes on `K + 2` distinct symbols (or on all `K` source symbols, since the code is
systematic), a NACK that asks "three more" instead of naming symbols, and a `store::Decoder` hook
where the real codec goes in firmware.

| Scenario | On main | With fountain coding |
|---|---|---|
| One announcer, band O neighbourhood | 100 %, 62 % duplicates | **100 %, 0 % duplicates** |
| Four announcers on one channel, 100 nodes | 100 %, 62 % duplicates | 100 %, 39 % duplicates, median 1.6 → 1.3 h |
| Two announcers, band L neighbourhood | 100 % | **26 %** |
| Five announcers, ESP-NOW neighbourhood | 98 % | **33 %** |

Where one announcer serves a cell it is everything one could ask for: the duplicate traffic goes
away completely. Where several announcers serve the same objects it collapses, and the reason is
worth stating because it is not obvious:

> A fountain needs a source that can produce unlimited fresh symbols, and only a node that holds
> the whole object can do that. An announcer that is still collecting can generate nothing, so it
> serves nothing and its cell starves. Letting it pass on the symbols it does hold was tried and
> is worse: it repeats the same handful and duplicates rise to 80–94 %.

The explanation first written here, that announcers fill up by overhearing each other and so
never complete, was then measured and does not hold. On main an announcer is served: in a dense
band L cell 57 and 97 % of the symbols its two announcers receive arrive for objects they asked
for and had an uploader assigned to, on ESP-NOW 92 to 100 %, and they hold every object complete.
On the branch the same announcers completed 4 and 8 of 22 objects.

Measuring the two changes apart found the real cause: the upload path still let through only
symbol ids below K, a leftover from when a repair always named source symbols. Every repair
answer on the branch uses ids beyond the carousel passes, so all of them were silently dropped
(9 000 queued, 50 sent). With that fixed, on the variant `experiment/fountain-only`, the full
reference set against main:

| Scenario | main | fountain only |
|---|---|---|
| Band O, one announcer, 50 nodes | 100 %, 1.09 h, 0 % dup, 94 k frames | 100 %, 1.04 h, 0 % dup, 100 k frames |
| Band O, four announcers, 100 nodes | 100 %, 1.60 h, 62 % dup, 0.65 M frames | 100 %, 1.98 h, 67 % dup, 2.6 M frames |
| Band L neighbourhood | 100 %, 1.46 h, 27 % dup | 97.2 %, 2.10 h, 29 % dup |
| Band L, 100 nodes on 15 km² | 100 %, 3.91 h, 36 % dup | 72.7 %, 3.62 h, 65 % dup |
| ESP-NOW neighbourhood | 99.1 %, 0.80 h, 30 % dup | 94.5 %, 0.70 h, 27 % dup |
| Clusters, band L | 100 %, 0.66 h, 7 % dup | 86.0 %, 0.47 h, 24 % dup |

**Verdict: not merged.** The branch is equal where one announcer serves a cell, which is where
main already has no duplicates, and worse everywhere else. The comparison also corrects the
premise: main's duplicates do not come from a carousel repeating its passes (a single announcer
has none) but from a node hearing several announcers send the *same* symbols of the same object.
A fountain in which every announcer starts from the same ids does nothing about that. The sharper
hypothesis for a later attempt is a symbol range per announcer, so that two carousels a node
hears at once are never redundant.

### 7.7.2 Nodes that come and go

The dynamics scenario originally kept every node switched on for the whole run. It now also
models nodes going away and returning (batteries, pockets, switches) and newcomers: a node
replaced by one that has learned nothing, which then follows a few channels and must fetch
everything from scratch.

`meshcast-sim dynamics --nodes 50 --channels 8 --follows 3 --publish-h 24 --churn-h 6
--node-churn-h 3 --newcomer-h 8 --hours 72`

| Regime | Delivered within one publication period | Of the nodes on at the end, holding the full current window | Newcomers that fetched the catalogue that existed when they joined |
|---|---|---|---|
| Band O, no node churn | 99.1 % | 42 of 42 (100 %) | n/a |
| Band O, a tenth toggling every 3 h | 86.5 % | 21 of 23 (91 %) | 7 of 8, mean 4.8 h, worst 22 h |
| Band L, a tenth toggling every 3 h | 87.6 % | 20 of 23 (87 %) | 8 of 8, mean 0.4 h, worst 0.6 h |
| Band O, a tenth toggling every hour, publishing every 12 h | 83.9 % | 17 of 21 (81 %) | 11 of 11, mean 2.6 h, worst 7.8 h |

The first column counts a node as having missed a bulletin even when it was switched off for the
whole period, which measures the batteries rather than the protocol; the second column is the
steady-state question and is the one to read. A cold start costs well under an hour in band L,
where a node has four times the airtime, and a few hours in band O. Nothing in the protocol had
to change for any of this: a returning node reboots as a follower, a lapsed grant is reassigned,
and a newcomer's first WANT is answered like any other.

### 7.8 What Phase 0 could not answer

- Real GFSK sensitivity at 100 kbit/s (interpolated), real CCA behaviour, and real building
  loss: Phase 1.
- Whether 0–500 ms jitter and a 30 % occupancy target are the right values in a live band with
  LoRaWAN and other users: Phase 2 measurements.
- Announcer-to-announcer exchange without bridge followers (PROTOCOL.md open question 7).

## 8. The audio codec: measured by ear and by clock

Until Phase 0 ended, every number here assumed Opus at 24 kbit/s: 540 kB per 3-minute track.
Neural audio codecs turn audio into a short stream of integer codes and back, and some of them
now reach listenable music at about 2 kbit/s. This section records how the codec was chosen.
The listening tests and timings were run once, on the maintainer's own AI-generated tracks and
hardware; the scripts are not in the repository (they drive Python reference implementations,
and the project has no Python), so the method is described in enough detail to repeat it.

### 8.1 Listening tests

Method: 30-second excerpts from 1:00 of five of the maintainer's tracks (brass, reggae, hip-hop,
a quiet song, dance), each encoded and decoded only from the codes that would be transmitted,
stored losslessly, and presented blind in a fixed random order with the original hidden among
them. One listener, headphones, absolute category rating from 1 (bad) to 5 (excellent).

| Variant | kbit/s | kB per 3 min | Mean rating, 5 tracks |
|---|---|---|---|
| Original (hidden) | 1536 | 34 560 | 4.2 |
| **SNAC 32 kHz** | **1.88** | **42** | **3.8** |
| Opus 24, mono | 26.8 (measured, incl. Ogg) | 603 | 3.6 |
| SNAC 44 kHz | 2.6 | 58 | 3.4 |
| EnCodec 48 kHz stereo, 6 kbit/s | 6.1 | 136 | 3.4 |
| EnCodec 24 kHz, 6 / 3 / 1.5 kbit/s | 6 / 3 / 1.5 | 135 / 68 / 34 | 3.2 / 2.6 / 1.8 |
| Opus 12 / 6, mono | 12.4 / 7.0 | 280 / 159 | 2.6 / 1.6 |
| WavTokenizer medium (music), 75 tokens/s | 0.9 | 20 | 2.6 |
| WavTokenizer large, 40 tokens/s | 0.48 | 11 | 1.8 |
| WavTokenizer large, 75 tokens/s ("speech" checkpoint) | 0.9 | 20 | 1.0 |

A second round on three of the tracks added Vocos, a light decoder for EnCodec codes, at 12, 6,
3 and 1.5 kbit/s: 2.7, 2.3, 1.3 and 1.0, below EnCodec's own decoder at the same bit rate
(Vocos's published EnCodec model is trained on speech). The anchors repeated round one exactly
(SNAC 32 kHz 3.7, Opus 24 3.7, WavTokenizer 2.3 on the same three tracks), which says the
ratings are consistent, not that they generalise: one listener, five tracks.

SNAC's 24 kHz model, trained on speech, was heard on the same music on a phone and judged not
good enough for music.

Sources: SNAC ([code and model cards](https://github.com/hubertsiuzdak/snac), MIT),
WavTokenizer ([code](https://github.com/jishengpeng/WavTokenizer), MIT), EnCodec
([code](https://github.com/facebookresearch/encodec), MIT), Vocos
([code](https://github.com/gemelo-ai/vocos), MIT).

### 8.2 What decoding costs

Decoding runs on every device that plays, so its cost decides where playback can happen.
Measured on an AMD Ryzen 7 255 (one core unless stated), 30 s of the same excerpt:

| Decoder | Parameters | Multiply-adds per second of audio | PyTorch 2.5, 1 core | onnxruntime 1.30 native | onnxruntime-web 1.23 (WebAssembly), 1 core |
|---|---|---|---|---|---|
| SNAC 32 kHz | 38.5 M | 18.3 G | 0.9× real time | 1.8× (1 core), 3.3× (4 cores) | 0.73× |
| SNAC 24 kHz | 13.1 M | 4.9 G | 3.5× | | 2.4× |
| SNAC 44 kHz | 38.5 M | not counted (same network at 44.1 kHz) | | | 0.56× |
| WavTokenizer (40 / 75 tokens/s) | 63 M | 2.6 / 4.8 G | 24× / 13× | | |
| EnCodec 24 kHz | 7.4 M | 1.2 G | 20× | | |
| Vocos on EnCodec codes | 8.0 M | 0.6 G | 90× | | |

Multiply-adds are PyTorch's `FlopCounterMode` count halved. The ONNX models are the SNAC decoders
exported with the noise injection, local attention and activation rewritten as plain operators;
their output matches PyTorch to 118–124 dB signal-to-noise ratio.

On a phone, the same page ran in the Claude app on a Pixel 4a (2020, Snapdragon 730G,
Android 13), onnxruntime-web on one core: **SNAC 24 kHz 0.80×, SNAC 32 kHz 0.27×, SNAC 44 kHz
0.19× real time**, each matching the desktop output to 123–124 dB. The browser's WebGPU path on
the phone's Adreno 618 returned wrong audio (0–1 dB against the desktop) and then hung, so the
graphics chip is untested rather than slow. On the desktop, native onnxruntime was 2.5× faster
than WebAssembly on one core; applying that ratio, a native app on the Pixel 4a would decode
music at roughly 0.7× real time per big core. That is an estimate, not a measurement.

Repackagings of SNAC for other runtimes were checked and do not help yet: Vokra (a Rust runtime,
pre-1.0) decoded 30 s of SNAC 44 kHz in 112 s using 12 GB; CrispASR's GGUF of SNAC 24 kHz is
full precision because, per its model card, 8-bit quantisation damaged the codec output.

### 8.3 Decision

- **Music: SNAC 32 kHz, 1.88 kbit/s.** Rated above Opus 24 at 14 times fewer bytes. A 3-minute
  track is 42 kB, one source block.
- **Speech: SNAC 24 kHz, 0.98 kbit/s.** Half the bytes and a quarter of the decoding work of the
  music model. Kept separate from music by choice: two models are more to maintain, but speech and
  music are different kinds of programme and each gets the model made for it.
- **Decode ahead, never in real time.** Tracks arrive minutes to hours before they play, so a
  player decodes each object when it completes. The slowest path measured, one phone core in a
  browser, needs under four hours of background work per hour of music.
- **Only players decode.** Dongles carry codes and hand them to the phone; they have neither the
  memory nor the arithmetic for a 38.5 M-parameter decoder.

PROTOCOL.md §1.1 pins both models to exact weights and defines the payload layout.

### 8.4 Open

- SNAC 24 kHz has not been rated blind on the maintainer's own spoken programmes; the choice rests
  on the model card and the demo samples.
- Energy: how much battery a phone spends per hour of music, on the processor and on the graphics
  chip, is unmeasured. The browser test has a battery mode, but it needs a working WebGPU path.
- A lighter decoder for the same SNAC codes, trained on music, would keep the objects valid and cut
  the decoding cost. Vocos shows the size (8 M parameters, 0.6 G multiply-adds per second) is
  possible; whether the quality is, nobody has measured.
- Opus as a fallback for players without a neural decoder: resolved in §9.5, local only.
- The simulations of §7 used 540 kB tracks and should be re-run at 42 kB.

## 9. After the codec change: ensembles, mixed traffic and four faults

With 42 kB tracks (§8) the Phase 0 scenarios were run again, and with them a question §7 never
asked: how much does one run say? This section records what that turned up. Every number in
this section is an ensemble of eight seeds (`meshcast-sim ... --seeds 8`), given as the mean and
the worst seed; the scenarios are those of §7 with 8 objects per source where §7 used fewer, so
that a mix of four kinds has at least two of each. `--mix` sets what each source publishes, for
example `snac-music:42,snac-speech:22`.

### 9.1 One run is one throw of the dice

The band L neighbourhood of §7.7 (50 nodes, 1 km², 540 kB tracks, 6 h) delivered 100 % on seed 1.
On eight seeds the same code delivers 100, 88.6, 100, 95.2, 99.6, 100, 92.7 and 87.4 %: a mean
of 95.4 %. Nothing in the protocol changed between those runs, only the random draws, and the
spread is wide enough to hide a fault or invent an improvement. From here on designs are
compared on ensembles; the single-seed tables of §7 stand as they were measured.

### 9.2 Four faults

The ensembles exposed a band L neighbourhood that stalled at 66 % on one seed while its
neighbours delivered everything. A trace of each announcer's want list (`MESHCAST_DEBUG_WANTS=1`)
showed two announcers asking every few minutes, for hours, for objects the other cell held, and
never getting them.

1. **Offers went where the asker does not listen.** An announcer's ask reaches the other cell in
   the rendezvous, when every node is on the meeting channel. The holders there answered with an
   offer a few seconds later, on their own cell's hop sequence, which the asking announcer never
   visits. Rule now: a frame goes where its addressee listens; an offer to another cell's
   announcer waits for the rendezvous.
2. **A grant that had delivered once never lapsed.** The rule kept a grant alive if any symbol had
   arrived after it was given, so an uploader that sent a few symbols and fell silent stayed
   responsible for hours. PROTOCOL.md already said the right thing (a grant lapses after
   `T_grant` without a symbol); the code now does it, counted from the last symbol.
3. **Symbols that arrived before their object's metadata were counted but not kept.** A node
   that collected an object before it knew what the object was stored a tally of symbols and no
   payload; when the metadata arrived the object was complete, with a buffer of zeros, and
   nothing announced the completion. For a manifest that meant a node could hold a "complete"
   manifest it could not read. Rules now: whoever counts a symbol keeps it, and an object
   completes once, with the same consequences, however it got there. (Found because the
   simulator reported objects as undelivered that the nodes held.)
4. **Small objects waited behind large ones** (§9.4).

Faults 1 and 2, SNAC objects, before and after (8 seeds each):

| Scenario | Before: delivered, mean (worst) | After | Bulk frames |
|---|---|---|---|
| Band O neighbourhood | 100 % (100) | 100 % (100) | unchanged |
| Band L neighbourhood | 99.0 % (95.0) | **100 % (100)** | −31 % |
| Band O, 100 nodes on 15 km² | 99.9 % (99.6) | 99.9 % (99.6) | −7 % |
| Band L, 100 nodes on 15 km² | 98.9 % (95.8) | **99.9 % (99.3)** | −15 % |
| ESP-NOW neighbourhood | 98.2 % (91.4) | **99.8 % (98.5)** | −31 % |
| Two clusters, band L | 98.7 % (89.5) | **100 % (100)** | −32 % |
| LoRa only, 5 km | 100 % (100) | 100 % (100) | unchanged |
| Town, band O (200 nodes, 30 km²) | 100 % (100) | 100 % (100) | +5 % |
| Town, band L | 99.9 % (99.7) | **100 % (100)** | −15 % |

Single-channel carriers are untouched, as they should be: there is no other channel to be on.
The two-cluster world that stalled at 89.5 % is now a smoke test (`sim/tests/smoke.rs`) that
fails without the two fixes. The remaining 0.1 % in band O at 15 km² was fault 3, a measurement
that missed objects the nodes held.

### 9.3 SNAC-sized objects, all fixes

All four fixes, SNAC music (42 kB) and speech (22 kB) alternating, 8 seeds, next to the same
scenario with 540 kB tracks on one seed before these fixes:

| Scenario | 540 kB Opus, one seed (§7 code) | SNAC 42 + 22 kB, 8 seeds: delivered, mean (worst) | median | bulk frames |
|---|---|---|---|---|
| Band O neighbourhood (50 nodes, 1 km², 3 h) | 100.0 %, median 66 min | 100.0 % (100.0) | 9 min | 13,483 |
| Band L neighbourhood (50 nodes, 1 km², 6 h) | 91.4 %, median 90 min | 100.0 % (100.0) | 22 min | 55,357 |
| Band O, 100 nodes on 15 km² (12 h) | 100.0 %, median 156 min | 100.0 % (99.9) | 10 min | 88,813 |
| Band L, 100 nodes on 15 km² (12 h) | 98.7 %, median 355 min | 100.0 % (100.0) | 47 min | 380,782 |
| ESP-NOW neighbourhood (30 nodes, 6 h) | 99.4 %, median 57 min | 100.0 % (100.0) | 16 min | 96,988 |
| Two clusters 1.8 km apart, band L (12 h) | 100.0 %, median 68 min | 100.0 % (100.0) | 23 min | 20,048 |
| LoRa only, two nodes 5 km apart (24 h) | 100.0 %, median 793 min | 100.0 % (100.0) | 48 min | 2,509 |
| Town, band O (200 nodes, 30 km², 24 h) | 87.4 %, median 572 min | 100.0 % (100.0) | 18 min | 213,033 |
| Town, band L (200 nodes, 30 km², 24 h) | 100.0 %, median 633 min | 100.0 % (99.9) | 79 min | 1,293,710 |

Delivery is complete everywhere, and a track that took hours now takes minutes: over LoRa alone,
48 minutes instead of 13 hours. In the dynamics scenario of §7.6 (8 channels, daily bulletins,
subscription churn) a new bulletin reaches its followers in 1.8 minutes in band O and 6 in band L
at 22 kB, against 6.6 and 20 at 300 kB; every follower that is on holds the current window of
every channel it follows, in both bands.

### 9.4 Mixed traffic and Smith's rule

A network may carry SNAC objects next to much larger ones: Opus renditions (PROTOCOL.md open
question 12), web bundles, firmware. With four kinds mixed (SNAC music 42 kB, SNAC speech 22 kB,
Opus music 540 kB, Opus speech 180 kB), SNAC music in the band O neighbourhood took 27 minutes
instead of 9. Two places decided the order in which objects travel, and neither looked at size:
the carousel sorted by how many followers wanted an object, and the announcer asked holders for
objects in id order, which is hash order, which is random. A holder uploads one object at a time,
so a 540 kB object asked for first held up every small object of its source.

The rule for one transmitter serving many waiting listeners is old: to minimise their total
wait, send in order of weight over length (Smith's rule). Here the weight is the number of
listeners, so **everywhere one sender chooses among objects, it takes the most listeners per
byte first**: the carousel's order, and the order in which an announcer asks. Among objects of
one size it is most-wanted first, as before; every wanted object is still sent every round.

SNAC and Opus mixed, 8 seeds, the carousel already ordered by listeners per byte, and the ask
order changed from object id to the same rule:

| Scenario | SNAC music, median: ask in id order | Smith's rule | Opus music, median: id order | Smith's rule |
|---|---|---|---|---|
| Band O neighbourhood | 27 min | **9 min** | 31 min | 37 min |
| Band L neighbourhood | 37 min | **23 min** | 47 min | 53 min |
| Band O, 15 km² | 27 min | **10 min** | 60 min | 69 min |
| Band L, 15 km² | 99 min | **43 min** | 158 min | 183 min |
| ESP-NOW neighbourhood | 23 min | **17 min** | 35 min | 41 min |
| Two clusters, band L | 33 min | **28 min** | 61 min | 73 min |
| LoRa only, 5 km | 169 min | **41 min** | 370 min | 410 min |
| Town, band O | 69 min | **19 min** | 188 min | 188 min |
| Town, band L | 180 min | **64 min** | 267 min | 338 min |

Small objects stop waiting behind large ones, most of all where the channel is slowest (LoRa
alone: 169 to 41 minutes); the large ones pay a little, as Smith's rule says they must. Delivery
stays at 100 % in every scenario.

### 9.5 What an Opus rendition on the air costs

PROTOCOL.md open question 12 asks whether Opus should travel as a second rendition of a
programme. The simulator's closest case is a source whose objects are half SNAC and half Opus;
with every fix in place:

| Scenario | Bulk frames, SNAC only | SNAC and Opus mixed | Ratio |
|---|---|---|---|
| Band O neighbourhood | 13,483 | 42,441 | 3.1× |
| Band L neighbourhood | 55,357 | 241,301 | 4.4× |
| Band O, 15 km² | 88,813 | 300,144 | 3.4× |
| Band L, 15 km² | 380,782 | 2,275,480 | 6.0× |
| ESP-NOW neighbourhood | 96,988 | 468,667 | 4.8× |
| Two clusters, band L | 20,048 | 116,712 | 5.8× |
| LoRa only, 5 km | 2,509 | 9,234 | 3.7× |
| Town, band O | 213,033 | 1,280,239 | 6.0× |
| Town, band L | 1,293,710 | 7,606,850 | 5.9× |

Half the programmes as Opus cost three to six times the airtime of all of them as SNAC. A real
second rendition, of every programme and alongside its SNAC codes, costs more still. That is the
measured price of the on-air answer to question 12; the local answer (a player re-encodes what it
has decoded, for a speaker next to it) costs nothing on the air, and is the one adopted.

### 9.6 Hidden uploaders, and what dividing the listening time uncovered

In the band L dynamics scenario of §7.6 (50 nodes on 1 km², 8 channels, daily 22 kB bulletins,
subscription churn, 72 hours) uploads varied from 169 to 2,085 across eight seeds. Classifying
every upload frame at the announcer it was meant for (`upload_outcome` in the simulator) showed
why: in the bad worlds a third to half of them collided, nearly always with another uploader
sending to the same announcer, a holder it could not hear. Capping concurrent grants traded this
for delay: at one grant per announcer collisions fell from 35 % to 1 %, but the median bulletin
took 27 minutes instead of 6 and the worst p90 911 minutes instead of 64. Fixed phases (three of
one second each, chosen by a hash of object and holder) cut upload frames by 46 % and collisions
to 2 %, at a median of 9.5 minutes. An oracle in which uploads to the same announcer never
collide showed what perfect scheduling would give: the same saving at an unchanged 6.3 minutes.

The design that followed is in PROTOCOL.md §4: each grant names its phase and the beacon says
how many are in use. Getting from there to the oracle took nine steps. Each row adds one to the
row above; every row is eight seeds, band L, with the mean over seeds and the worst world:

| Step | Uploads per 72 h (worst) | Upload frames collided | Grants lapsed | Bulletin median | Worst p90 | Worst world holds the current window |
|---|---|---|---|---|---|---|
| No phases (main) | 1,013 (2,085) | 35 % | 184 | 6.2 min | 64 min | 100 % |
| 1. Each grant names its phase | 1,081 (3,757) | 4 % | 154 | 6.8 min | 186 min | 92.9 % |
| 2. A grant ends with the announcer's role | 850 (1,789) | 4 % | 144 | 7.1 min | 191 min | 100 % |
| 3. A symbol carries its object's length | 632 (981) | 6 % | 22 | 7.6 min | 184 min | 100 % |
| 4. An uploader uses its whole phase | 728 (1,270) | 5 % | 11 | 6.0 min | 21 min | 92.9 % |
| 5. Content waits on its own clock | 796 (1,755) | 7 % | 5 | 5.9 min | 35 min | 100 % |
| 6. A NACK names the phase of its answers | 898 (1,847) | 8 % | 6 | 5.9 min | 14 min | 100 % |
| 7. A NACK names who answers | 352 (691) | 0 % | 5 | 5.9 min | 20 min | 100 % |
| 8–9. Phases only under polite access; an announcer's HAVE is not an offer | **330 (524)** | **0 %** | **2** | **6.0 min** | **21 min** | **100 %** |

Over the eight worlds the upload frames sent fell from 349,089 to 192,793 (−45 %), and 97 % of
them now arrive, against 61 %. In band O nothing changed that mattered (1.8 minutes, 79 uploads
against 80, collisions 2 % to 1 %): one uploader at a time is the usual case there. The worst
p90 is a single publication in a single world and moves by tens of minutes with it; the medians
and upload counts are the steadier measure. Steps 8 and 9 came from the cell scenarios below.

1. **Phases.** A hash of object and holder would have cost no byte on the air, but with a dozen
   uploads running the announcer needed K = 16 far too often: the birthday problem. Explicit
   phases in the grant do not collide. Collisions fell from 35 % to 4 %, but one world ended with
   only 92.9 % of its followers up to date, and in it 43 % of all upload frames went to nodes
   that were no longer announcing.
2. **A grant outlived its announcer.** A node that stopped announcing kept its grants, and kept
   naming the holders in the WANT it sent as a follower; the holders kept uploading to a node that
   listened to someone else. Grants now end with the role, and a holder stops uploading to a node
   as soon as that node says it follows someone else. Frames to former announcers fell from
   16,143 to 12 in that world.
3. **An object named without its length could never complete.** A grant trace
   (`MESHCAST_TRACE_GRANTS=1`) showed one announcer granting the same two-symbol manifest to the
   same holder 80 times in 17 hours, and receiving both symbols every time. It had collected the
   symbols before it knew the manifest, took on the want when a follower asked, and nothing told
   it the length; without the length, symbols are counted but an object never completes. BULK frames now carry the object's length instead of
   the block's K (PROTOCOL.md §3.2), 2 bytes more per frame: an object completes from its symbols
   alone. In the worst world lapsed grants fell from 697 to 11 and uploads from 1,789 to 522.
4. **A phase was used by less than half.** Inside its one-second phase an uploader still sent
   the short bursts that keep a node listening (ETHERFATSOEN.md §7), and used about 0.4 s of it (an estimate from the burst rule: bursts of about 60 ms, each
   followed by the 100 ms pause).
   Nobody else speaks to the announcer in that phase, so the uploader now uses it as one
   transmission up to `Ton_max`. The median bulletin went from 7.6 to 6.0 minutes, below the
   no-phase 6.2.
5. **Content held back control.** Every pause content takes (the rendezvous, a time slot, a
   phase, the token bucket) was one timer for the whole radio. An announcer whose carousel had
   paused for the meeting dwell sent its WANT after it, on its own channel, where no other cell
   listens; in one world a cell never got a manifest published three hours before the end. Content
   now waits on its own clock (ETHERFATSOEN.md §2). In that world 311 of the 1,150 asks and grants
   announcers sent had missed the rendezvous; afterwards none of 1,104 did.
6. **Repair answers had no phase.** Holders answering an announcer's NACK without a grant sent
   whenever they liked: in the worst world 5,344 of 5,369 same-announcer collisions involved one.
   A NACK now names the phase for its answers, the grant's or a reserved one. The worst p90 fell
   to 14 minutes, but the answers still collided with each other.
7. **Repair answers came from everyone.** Every holder that heard an announcer's NACK answered
   after a wait scaled by how well it heard the asker, and holders that cannot hear each other do
   not suppress each other: the same failure as uploads before grants (ETHERFATSOEN.md §6). The
   NACK now names who answers, the granted uploader or the holder the announcer hears best among
   those that said they have the object. Collisions went to 0 % and uploads fell by 61 %.

The cell scenarios of §9.3 then showed what the dynamics scenario could not. Against main, band
L got faster and cheaper, but band O, ESP-NOW and the two band L clusters got 14 to 37 % slower.
Ablations, one step removed at a time on eight seeds, found two more causes:

8. **Phases where uploads are short.** Under band O's duty cycle, budgeted per hour, an uploader
   sends its object in a burst of seconds at the full rate; held to one phase in K, the median
   upload took 39 s instead of 4 s. Hidden uploads that short rarely meet, so there was nothing
   to save: band O had 2 % collisions without phases. An announcer now divides its listening time
   only under polite access (PROTOCOL.md §4). In the band O scenario on 15 km² the median went
   from 13.4 back to 9.5 minutes.
9. **An announcer's HAVE counted as an offer.** In the two clusters, removing step 5 made things
   faster again, which pointed at the rendezvous. A trace of the world that slowed most showed
   the manifest crossing to the other cluster in 27 minutes instead of 12: the asking announcer
   granted it three times in a row to the source, which was itself an announcer and never
   uploads, and each grant waited out `T_grant`. The source's HAVE had counted as an offer, and
   the followers that would have offered had cancelled theirs on hearing it. With content and
   control on one timer the announcers' gossip had mostly missed the rendezvous, which hid the
   fault; step 5 exposed it. An announcer's HAVE is no longer an offer (PROTOCOL.md §4), and the
   smoke test `announcers_are_never_granted_uploads` fails without the rule.

All nine steps, the scenarios of §9.3 (SNAC music and speech alternating, eight seeds each):

| Scenario | Main: delivered (worst), median, bulk frames | This design | Frames |
|---|---|---|---|
| Band O neighbourhood | 100 % (100), 8.7 min, 13,483 | 100 % (100), 8.8 min | unchanged |
| Band L neighbourhood | 100 % (100), 21.5 min, 55,357 | 100 % (100), 21.2 min | −41 % |
| Band O, 100 nodes on 15 km² | 100 % (99.9), 9.8 min, 88,813 | 100 % (100), 9.6 min | −22 % |
| Band L, 100 nodes on 15 km² | 100 % (100), 46.9 min, 380,782 | 100 % (100), **34.3 min** | −28 % |
| ESP-NOW neighbourhood | 100 % (100), 16.4 min, 96,988 | 100 % (100), 15.1 min | −17 % |
| Two clusters, band L | 100 % (100), 22.6 min, 20,048 | 100 % (100), 20.3 min | −13 % |
| LoRa only, 5 km | 100 % (100), 47.7 min, 2,509 | 100 % (100), 48.0 min | unchanged |
| Town, band O | 100 % (100), 18.0 min, 213,033 | 100 % (100), 17.7 min | +3 % |
| Town, band L | 100 % (99.9), 79.4 min, 1,293,710 | 100 % (100), **49.3 min** | −28 % |

Every world of every scenario now delivers everything, and none is slower beyond the spread of
its seeds. The band L town, the hardest case, gets its programmes half an hour sooner with a
quarter less airtime.

The smoke test `hidden_uploaders_take_turns_at_their_announcer` (six sources in a 900 m ring
around a station, band L) fails without phases, where 63 % of its upload frames collided, and
now sees 0.4 %.

### 9.7 The announcer keeps quiet in the phases it gave away

With collisions gone, the largest loss left was the announcer's own carousel: 1 to 5 % of upload
frames in the dynamics worlds, and 26 % in the ring smoke test, arrived while the announcer was
transmitting. Carrier sensing does not prevent it: an uploader the announcer can decode may still
be below the clear-channel threshold, 15 dB above sensitivity (ETSI EN 300 220-2 Table 18), and
the other way round. The announcer knows the phases it gave out, so it now holds its carousel in
a phase whose uploader it heard in the last two cycles (PROTOCOL.md §4). Eight seeds, band L:

| Scenario | Before | Quiet announcer |
|---|---|---|
| Dynamics: upload frames that arrive | 97 % | 100 % |
| Dynamics: bulletin median, worst p90 | 6.0 min, 21 min | 5.9 min, 13 min |
| Neighbourhood (50 nodes, 1 km²) | 21.2 min, 32,530 frames | 21.0 min, 32,151 frames |
| 100 nodes on 15 km² | 34.3 min, 273,649 frames | 29.8 min, 239,541 frames |
| Two clusters | 20.3 min, 17,364 frames | 17.7 min, 16,524 frames |
| Town (200 nodes, 30 km²) | 49.3 min, 937,091 frames | **43.2 min**, 845,799 frames |

In the ring smoke test the frames arriving while the station transmits fell from 26 % to 1.7 %,
and the uploads needed 43 % fewer frames, because fewer had to be repaired. Band O and ESP-NOW do
not divide the listening time and are unchanged.

### 9.8 One pass, served by the announcer it names

Every object used to get three full carousel passes, because followers never say when they are
done. Measuring renditions (large objects for a few listeners) showed what that costs: three times
the object for nobody in particular. So the pass count was measured on its own, eight seeds each:
one pass was as fast as three everywhere and cheaper by a third to a half, because the NACK repair
does the work of the repeated passes, aimed. One world of the band O 15 km² scenario, though,
ended with two followers at 12 to 63 % of four objects after twelve hours, although they asked
every ten minutes.

A trace of one of them showed why: two announcers answered each of its WANTs, the one it follows
and a neighbouring one that overheard it, and both started the same pass at the same instant, on
the same channel, frame for frame. Hidden from each other, every frame of both collided at the
follower, which caught 27 of one object's 216 symbols in twelve hours; with three passes the two
had drifted apart and the fault stayed hidden. A want is now served only by the announcer it
names (PROTOCOL.md §4). Both changes, against the current design, eight seeds each:

| Scenario | Three passes: median, bulk frames | One pass, named announcer | Frames |
|---|---|---|---|
| Band O neighbourhood | 8.8 min, 13,483 | 9.1 min, 6,936 | −49 % |
| Band L neighbourhood | 21.0 min, 32,151 | **17.6 min**, 17,541 | −45 % |
| Band O, 15 km² | 9.6 min, 68,865 | 8.9 min, 44,647 | −35 % |
| Band L, 15 km² | 29.8 min, 239,541 | 28.6 min, 131,452 | −45 % |
| ESP-NOW neighbourhood | 15.1 min, 80,955 | 15.6 min, 44,863 | −45 % |
| Two clusters, band L | 17.7 min, 16,524 | 18.3 min, 8,533 | −48 % |
| Town, band O | 17.7 min, 219,499 | **15.4 min**, 169,354 | −23 % |
| Town, band L | 43.2 min, 845,799 | **41.2 min**, 489,293 | −42 % |

Every world delivers everything. In the dynamics scenario a bulletin arrives in 5.8 minutes in band L
(5.9 before) and 1.8 in band O, and every follower that is on holds the current window at the end.

### 9.9 Open

- All 50 nodes of the dynamics scenario start at the same instant, and in band L almost every
  node became announcer within the first quarter hour before they found each other. Resolved in
  §12: candidates on a hopping carrier step up in the meeting dwell.

## 10. Renditions: sound for devices that cannot decode

A board with a speaker but no neural decoder, such as a LilyGo T-Deck Pro, gets each programme as
an Opus rendition made from the codes in its cell (PROTOCOL.md §1.2). The simulator models it:
sources name one rendition per audio object in a rendition table, a number of followers cannot
decode and want renditions instead of codes, and nodes that decode make renditions when granted.
Two draft profiles: Opus at 16 kbit/s for music and 8 for speech, and a leaner 12 and 6. A
3-minute song is then 367 or 275 kB against 43 kB of codes, a 3-minute talk 183 or 137 kB
against 22 kB: six to eight times the codes (arithmetic: the codes' duration at the profile's bit
rate). The schedule plays one object per source per hour; a device asks for a rendition
`T_render_ahead` (30 min) before its slot. Eight seeds each, 12 hours.

**Naming renditions costs nothing.** A manifest that names a rendition table and no device that
asks: 7,795 bulk frames against 7,800 in the band O neighbourhood. Listing the renditions in every
manifest entry instead cost 4 % more frames, because manifests are repeated every five minutes.

**Where a device asks, a cell pays for what it plays, once.** In the band O neighbourhood every
rendition arrives before its slot, and the cost does not depend on how many devices listen:

| Band O neighbourhood | 1 small device | 5 | 20 |
|---|---|---|---|
| Renditions before their slot, 16/8 kbit/s | 100 % | 100 % | 100 % |
| Rendition frames, 16/8 (codes: 7,800 frames in all) | 27,520 | 27,520 | 27,520 |
| Rendition frames, 12/6 | 20,640 | 20,640 | 20,640 |

27,520 frames is the 5.5 MB of twenty renditions sent exactly once. The codes still reach every
other follower in the same time (music median 10.5 to 10.9 minutes, against 11.0 without
renditions).

With several cells, each cell where a device asks carries its own copy, and a cell whose
announcer cannot make it first fetches it from a node that can. Renditions at 16/8 kbit/s, every
node that decodes able to make them:

| Scenario | Small devices | Before their slot | Bulk frames, with (without) renditions | Codes: music median, with (without) |
|---|---|---|---|---|
| Band L neighbourhood | 1 | 100 % | 47,021 (18,837) | 22.6 (22.4) min |
| Band L neighbourhood | 5 | 99.9 % | 73,655 (18,837) | 23.4 (22.4) min |
| Band L neighbourhood | 20 | 99.6 % | 90,548 (18,837) | 22.4 (22.4) min |
| Band O, 15 km² | 5 | 99.0 % | 153,537 (44,647) | 10.9 (10.5) min |
| Band O, 15 km² | 20 | 98.7 % | 289,359 (44,647) | 10.3 (10.5) min |
| Band L, 15 km² | 5 | 96.0 % | 295,528 (131,452) | 41.8 (36.6) min |
| Band L, 15 km² | 20 | 94.6 % | 687,369 (131,452) | 44.7 (36.6) min |

Every follower that decodes still gets every object. Twenty devices scattered over 15 km² make
the cells carry five to six and a half times the airtime, and in band L the codes arrive a fifth
later; the 12/6 profile saves 30 % of the rendition frames (389,493 against 554,417) at the same
punctuality (95.3 % before their slot).

**A rendition costs what listening costs.** A device plays in real time, so a cell carries a
rendition's bit rate for as long as someone listens to a programme nobody else there has asked
for. A band O announcer averages 10 to 15 kbit/s (§1): one 16 kbit/s stream of continuous music
does not fit, and in band L (about 40 kbit/s) two do. The scenarios above schedule three minutes
per source per hour, well within that. Renditions over the sub-GHz cell therefore suit bulletins,
a few programmes an hour and the occasional pick, not continuous music radio on a small board.
For that, the last hop needs more room than the cell has: ESP-NOW from a nearby station, or a
decoder on the device (ROADMAP.md).

**Who makes them matters.** When only stations make renditions and the station announces for
another cell, it cannot upload to the cell that asks: announcers serve their own cells. In the
band L scenarios, 14 to 48 % of renditions then arrived before their slot on 15 km² and 62 to
96 % in the neighbourhood, against 95 to 100 % when every node that decodes can make them. So
renditions need a node that decodes in or near each cell where a device asks, which in practice
is a phone behind a dongle.

**Asking earlier does not help.** In band L on 15 km² with five devices, every late rendition
belonged to the first slot, an hour after the whole catalogue was published at once, while the cell was still carrying
the codes (music median 42 minutes). Asking two hours ahead instead of thirty minutes put more
renditions in flight during that hour and delivered fewer on time (94.1 % against 96.0 %).

**Two faults found on the way.** Each rendition was first made by every node that heard the ask,
before offering; now a node offers what it could make and makes it only when granted (20
renditions made for 20 objects, not 31). And a node that became announcer never fetched the
codes of the channels it had followed as a device that cannot decode, nor, in general, the
objects of channels it had not followed; its cell then went without. A new announcer now adopts
the manifests it holds again, as an announcer: with 20 such devices in a band L neighbourhood
the other followers' deliveries went from 95 % (worst world 65 %) back to 100 %.

## 11. A node that keeps asking

ABUSE.md names the lever: a 50-byte WANT makes an announcer send a whole object. The simulator
now has an attacker (`--attackers`, sim/README.md): a follower that asks its announcer for every
object of the scenario, eight per WANT, about once a minute at random times, under its own node id
or under a fresh made-up one each time. One attacker sends about 700 WANTs in 12 hours.

**What it cost before.** Nothing that listeners noticed, and a great deal that the spectrum did.
Fresh content goes before repetition, so every follower still got every object as fast as before.
But each WANT after a pass brought the object back for another, so the announcer repeated the
whole catalogue for as long as the attacker asked: in the band O neighbourhood 196,090 bulk frames
in 12 hours instead of 7,800, in band L 464,968 instead of 18,837, 25 times the normal traffic. In
the living network of §7.6 the announcer's airtime went from 0.3 % to 8 % in band O, the legal
limit, and to 14 % in band L.

**Repetition that does not help is repeated ever more slowly** (PROTOCOL.md §4). The first
repetition of an object comes at once; each further one waits 10, 20, 40, then at most 80 minutes
after the pass before, until nobody has asked for the object for twice that wait. The rule looks
at the object, not at who asks, so made-up ids do not get around it. Three things were measured
on the way:

- *A fixed wait of 10 minutes before every repetition* changed nothing in band O, where a pass of
  the catalogue takes longer than that anyway, and made honest band L deliveries 7 to 11 % slower.
  Letting the first repetition come at once kept them as fast as before.
- *The ceiling is a trade.* With no ceiling the attacker got 4 times the normal traffic, but a
  follower that started listening during the attack waited up to 11 hours for the repetition it
  needed, and in three of sixteen worlds one follower was still behind at the end. Ceilings of 20,
  40 and 80 minutes left nobody behind and bounded the announcer's airtime under attack at 2.8,
  1.6 and 0.9 %; 80 minutes it is.
- *The attacker made the election churn.* In one world it tripled the role changes, and each new
  announcer starts with an empty carousel history, so the backoff began again. Announcers yielded
  to any announcer named in a report with a higher score, even one they could not hear; the
  attacker's WANTs named its own announcer once a minute. Announcers now yield only to beacons
  they hear themselves (PROTOCOL.md §5.2). In the nine scenarios of §9.3 without an attacker that
  changed nothing.
- *Made-up names inflated the election.* The number of distinct neighbours heard is the largest
  term of a node's score, and every made-up id counted at once: an attacker using a fresh id for
  each WANT raised the score of every node that heard it, and one band L world changed roles
  27,335 times in 72 hours instead of 259. A neighbour now counts only from its second frame
  (PROTOCOL.md §5.1); the role changes went back to those without an attacker.
- *Renditions flooded too.* An attacker asking for the rendition of every programme got each one
  made and sent, eight times the size of the codes, and the devices that needed them were late:
  75.6 % arrived before their slot in band O. An announcer now serves a rendition only from twice
  `T_render_ahead` before its programme's slot until the programme has played (PROTOCOL.md §1.2).

With all of it, against one attacker (a WANT about once a minute, made-up ids unless noted), eight
seeds each. "Before" is the design without these rules, except in the last three rows, where it is
the design with the earlier rules but without the one that row is about:

| Scenario | Normal | Attacked, before | Attacked, now |
|---|---|---|---|
| Band O neighbourhood, 12 h: bulk frames | 7,800 | 196,090 | 45,130 (five attackers: 45,161) |
| Band L neighbourhood, 12 h: bulk frames | 18,975 | 464,968 | 56,127 (five attackers: 85,925) |
| Living network, band O: announcer airtime | 0.3 % | 8.0 % | 0.9 % |
| Living network, band L: announcer airtime | 0.3 % | 13.8 % | 1.0 % |
| Living network, band L: bulletin median | 5.8 min | | 6.0 min |
| Living network, band L: role changes, worst world | 294 | 27,335 | 294 |
| Rendition flood, band O: frames, devices on time | 35,292, 100 % | 197,945, 75.6 % | 149,301, 100 % |
| Rendition flood, band L: frames, devices on time | 76,032, 99.8 % | 332,484, 95.6 % | 209,909, 99.4 % |

Every listener still gets everything, the announcer's airtime under attack is about three times
normal instead of the legal limit, and the made-up names no longer move the election. Without
an attacker, the nine scenarios of §9.3 changed against the design without these rules by
between −5 and +11 % in median delivery time and between −12 and +7 % in bulk frames; the
11 % is the two-cluster world of 20 nodes, the noisiest of the nine, and the rest moved by 5 %
or less.
What remains: with its own id an attacker still raised the role changes by a third to a half in
two of eight band L worlds, probably through the score's term for unused airtime, which an
announcer loses while it repeats; and a rendition flood still costs a cell several times what
its devices need, within the window around each slot.

One measurement was an artefact and is recorded so that it is not repeated: an attacker sending at
a fixed period from time zero sent every WANT on a dwell boundary, where it jammed the first frame
of each upload, and band L bulletins took 45 % longer. With random timing they took 6.0 minutes
against 5.8 without an attacker. Jamming is a radio problem, not a protocol one (ABUSE.md).


## 12. Elections that settle, and content that crosses stable cells

Two items were open after §11: when every node of a band L network switched on at once, almost
every node became announcer before they found each other (§9.9); and an attacker with its own id
still raised the role changes in some band L worlds. Both turned out to be about the election.
Settling the election then exposed paths between cells that the churn had been carrying without
anyone designing them, and each got a rule. All numbers are eight seeds unless one world is named.

### 12.1 The cold start, and a failover, on a hopping carrier

In the living network of §7.6 (band L, 50 nodes on 1 km², all switched on at time zero) between 5
and 22 nodes were announcer five minutes after the start, every node had been announcer once, and
the first hour had 239 to 294 role changes. A failover (20 nodes; the station switched off after
two hours) produced up to 13 new announcers in one world and settled after 165 to 4,000 s; on
ESP-NOW, which hops over Wi-Fi channels 1, 6 and 11, one world needed an hour to elect anyone.

A role trace showed why. On a hopping carrier a node that follows nobody scans slowly, while a
new announcer's first beacon goes out on its own hop sequence. The candidates could hear it only
in the next meeting dwell, every 100 s; their timers were spread over 40 s. So nobody heard
anybody in time.

Four remedies were inventoried: a wider spread of the timers (slower failover everywhere); letting
a node that follows nobody listen on the base sequence, where a new announcer of colour 0 speaks
(tried in §9.9 and judged a trade); a new announcer heralding itself on the scan channel; and
stepping up where every candidate already listens, in the meeting dwell. The second and the last
were measured. In the living network both ended the storm (one to three announcers at five
minutes), with the first hour's role changes at 166 on average for base-sequence listening and
130 for the meeting dwell, against 259; both delivered as fast as before. In the failover the
meeting dwell was better, a single clean election in seven of eight worlds after 212 s, while
base-sequence listening still produced ten announcers in one world: a new announcer is colour 0
only if it has no conflicts, the meeting dwell works whatever the colours. The meeting dwell it is
(PROTOCOL.md §5.2).

### 12.2 Challenges that compared unlike things

The eighth world was a cascade. Three new announcers had stepped up after the failover with
scores of 72 to 76; a follower in the middle of them reached 120 within three minutes, took
over, made two of them yield, and then yielded itself to the third on the tie-break. Each
announcer that yielded left its followers waiting out three missed beacons.

The score adds up what a node *is* (mains power, an uplink) and what it *experiences*: how many
neighbours it hears, how much of its airtime budget is left, how many objects it holds. The
second part depends on the role. An announcer transmits, so it hears less than its followers, and
it spends its budget while they keep theirs; the follower in the middle heard the uploaders of all
three cells. Comparing a follower's score with its announcer's compares unlike things, and every
challenge in that world was such a comparison. Capability is now carried in two spare bits of
the beacon's flags byte, and a follower challenges only an announcer less capable than itself
(PROTOCOL.md §5.1, §5.2). Four more faults were found by measuring that rule:

- *A challenge could never complete on a hopping carrier.* A candidate went back to following
  when it heard any beacon, and a challenger hears its own announcer at every dwell start. A
  station returning from a power cut took 107 to 1,797 s to take over in band L and up to 3,840 s
  on ESP-NOW; with the score-based challenge it had only completed when its random wait happened
  to fall between two beacons. A candidate now stands down only for an announcer at least as
  capable as itself.
- *A challenge cycle.* In a town with three stations one station, following a battery announcer
  it could challenge, also heard another station. It stepped up, yielded to the other station on
  score, followed the battery node again and challenged it again: 127 challenges and 3,100 role
  changes in a day. A follower does not challenge while it hears an announcer at least as capable
  as itself.
- *Battery announcers that persisted next to a station.* At first more capability made an
  announcer yield only when the other was in its own cell; a far battery announcer then kept
  serving beside a station that its followers could also hear. More capability now counts like a
  clearly higher score, near or far.
- *A station that stepped up second.* The meeting dwell orders candidates by score plus a jitter,
  and the jitter let two battery nodes step up three seconds before the station; the station,
  more capable, stepped up anyway, they yielded, and their new followers waited out three missed
  beacons. Candidates now step up in the order announcers yield in: capability first, in four
  bands, then score, then chance (PROTOCOL.md §5.2). With that rule the span on a carrier that
  does not hop was halved to 70 s, because a network without stations otherwise waited out the
  stations' bands for nothing: a band O failover took 272 s with the full span and 199 s with the
  half one.

The election after all of it, against the design of §11:

| | §11 | Now |
|---|---|---|
| Living network, band L: announcers 5 min after a cold start | 5 to 22 | 1 to 3 |
| Living network, band L: role changes in the first hour | 239 to 294 | 100 to 119 |
| Band L cell of 50 nodes, four worlds: role changes in the first hour (the same announcers at the end in three) | 243 to 318 | 107 to 109 |
| Failover band L: first new announcer; settled after | 261 s; 165 to 4,000 s | 216 s; 165 to 217 s |
| Failover ESP-NOW: first new announcer; settled after | 741 s mean, 3,588 s worst; up to 3,789 s | 217 s; 216 to 217 s |
| Failover band O: first new announcer; settled after | 257 s; 248 to 262 s | 199 s; 195 to 218 s |
| Station back from a power cut takes over, band L / ESP-NOW / band O | 107–1,797 / 114–3,840 / 201–222 s | 106–207 / 106–206 / 202–231 s |
| Challenges per failover world | | exactly one (the returning station) |

### 12.3 What the churn had been carrying

With stable cells, three scenarios that had always delivered everything did not, each in one
world, and in each the trace named a path between two cells that only churn had been using.
Content crosses between two cells over any pair of nodes, one in each, that hear each other, and
which pair it is decides how (PROTOCOL.md §4):

- *A follower of one cell hears the other cell's announcer and holds what it asks for.* It offers
  in the rendezvous and uploads when granted. This path existed.
- *A follower hears another cell's announcer that has what it wants.* In one 15 km² band L world a
  source's cell was the source, one neighbour and their announcer, and nobody else in it heard any
  other announcer. Announcers do not upload, so the content stayed in that cell for twelve hours
  and 97 nodes lacked a third of everything; with churn, followers had wandered in and out.
  Followers of the neighbouring cells did hear that announcer. Now a follower whose want has
  brought nothing, and whose own announcer has granted it to nobody, follows an announcer that
  lists the object for as long as the visit brings something: an *excursion*. The smoke test
  `a_follower_fetches_what_its_cell_cannot_get` builds such an island and checks both that it is
  served and that it is not served without excursions.
- *Two followers hear each other and neither announcer hears the other cell.* In the two-cluster
  world of seed 2, two followers of the far cluster overheard the near cluster's source uploading,
  because the two cells were not in conflict and shared a channel, and collected 112 of 113
  symbols of three objects. Their own announcer was asking for the objects itself, so their NACK
  went nowhere; main had delivered this world only because one of them had overheard every symbol.
  A follower whose announcer is itself asking for an object, and has granted it to nobody, now
  names in its NACK the holder it hears best in another cell, and a named holder answers.

Calibrating the two new rules:

- *When to go on an excursion.* At first a follower went after 20 minutes without a symbol. In a
  band L neighbourhood 18 followers left at once after 26 minutes: their cell was busy, not
  unable, and the objects were granted. The rule now also asks that the follower's announcer has
  granted the object to nobody in that time. Then the time itself, in the 15 km² band L scenario:

  | `T_excursion` | Median | Bulk frames | Excursions per world |
  |---|---|---|---|
  | (main, no excursions) | 29.5 min | 133,198 | |
  | 20 min | 25.8 min | 147,251 | 41 to 68 |
  | 40 min | 26.9 min | 133,662 | 0, and 31 in one world |
  | 60 min | 27.7 min | 133,879 | 0, and 40 in one world, whose slowest object took 86 min |

  At 20 minutes excursions were a shortcut that cost the visited carousels a repeated pass each;
  at 40 they happen only where a cell cannot get an object otherwise. 40 minutes it is.
- *Whom to name.* At first a follower named the holder it heard best, wherever it was. In a band O
  world of 15 km² that doubled work: a holder in the follower's own cell answers its announcer's
  ask anyway, and the same object went up twice. Naming only holders in other cells took that
  world from 60,628 bulk frames to 50,187 at the same delivery.

A third rule came from a band L neighbourhood where a source sat in a small cell beside a large
one. It uploaded each object to its own announcer and then to the neighbouring one, one after the
other, and its offers silenced those of nine neighbours that already held the objects. A holder
that is uploading now offers nothing until it is done (PROTOCOL.md §4); that world's music median
went from 22.8 to 18.3 minutes.

### 12.4 False announcers

An excursion follows an announcer that lists an object, so an attacker can list objects it does
not have. The simulator's `--attack-lure` does that: in the rendezvous it beacons as an announcer
and lists every object in HAVE, and it serves nothing. In the island of §12.3, with such a lure
800 m from the one follower that can fetch (more than 6 dB weaker for it than its own announcer,
so it is not followed by signal, but stronger than the island's announcer), that follower went to
the lure every 41 minutes for six hours and its cell got nothing. A visit that brings not one
symbol now makes the follower ignore that announcer for an hour: it went to the lure once, after
46 minutes, came back at 86, fetched from the island's announcer and had everything in its own
cell at 91 (smoke test `a_lure_is_visited_once`, which fails without the rule).

A lure is also a false announcer, and a follower that hears it better than any other follows it
by signal: the election capture of ABUSE.md. In the simulator a lure is muted apart from its lies,
so that its own node never serves anything by accident. In the 15 km² band L scenario, five lures
that claimed nothing (score 0, no capability) cost the design of §11 little: a follower captured
by one outscored it and challenged it within three beacons. The new election, which challenges
only on capability, lost a fifth of all deliveries to them and half in the worst world. But the
protection of §11 was an accident of the attacker's modesty: against five lures claiming the
maximum score it delivered 66 % on average and 60 % in the worst world, because nobody outscored
them.

The rule that holds against both is evidence before belief (PROTOCOL.md §5.2): an announcer that
lists what it does not serve is not followed, and a source stops offering an object only when it
uploaded it or hears someone send it, not because its announcer claims to have it. Getting the
evidence right took four rounds:

- *Nothing for 40 minutes* was the first version: a follower that got not one symbol of anything
  it wanted for `T_excursion` while its announcer listed it left that announcer. Against lures it
  worked; in the living band L network with one WANT-flooding attacker it made followers leave
  honest announcers 56 to 320 times in the 71 hours after the first, against none without the
  rule.
  The flood slows an honest announcer's repetitions to its ceiling of 80 minutes (§11), and a
  follower that missed a pass waits that long.
- *Never one symbol, for longer than the ceiling*: an object the announcer lists and the follower
  wants of which it has never had one symbol, or a source's own object the announcer lists and
  nobody has been heard sending, for the ceiling plus one ask, 90 minutes. No false alarms under
  the flood, but slower against lures: five lures claiming nothing left 93.8 % delivered, 84.1 %
  in the worst world. "Never one symbol" is needed because a follower on a channel shared with a
  neighbouring cell overhears that cell's symbols; "nobody sending" because a source between two
  lures wanted nothing itself, and only its own objects could show the lie.
- *A silent channel, sooner*: a listed want that has stalled for `T_excursion` while not one
  `BULK` frame of any object arrived. An announcer busy repeating other objects under a flood is
  not silent; one that serves nothing is.
- *No excursion for what our announcer lists*: under the flood the remaining role changes were
  excursions, followers fetching elsewhere what their own announcer had but was repeating
  slowly. An object our announcer lists, it either serves or is found out; it is not a reason to
  leave. With that, no role changes at all after the first hour in any of the eight worlds.

A smoke test, `a_false_announcer_is_left`, puts a lure claiming the maximum beside a band L cell
switched on at once; it fails without the rule.

| 15 km² band L, eight seeds: delivered on average, in the worst world | §11 | New election without the rule | Now |
|---|---|---|---|
| No attacker | 100 %, 100 % | 100 %, 100 % | 100 %, 100 % |
| One lure claiming nothing | 99.4 %, 97.7 % | 98.3 %, 96.5 % | 99.4 %, 98.7 % |
| Five lures claiming nothing | 96.6 %, 94.1 % | 80.6 %, 53.2 % | 97.8 %, 94.7 % |
| Five lures claiming the maximum | 65.8 %, 59.5 % | 76.3 %, 49.2 % | 95.2 %, 86.9 % |

| Living network, band L, one WANT-flooding attacker | §11 | Now |
|---|---|---|
| Role changes after the first hour, per world | 0 in six, 72 and 159 in two | 0 in all eight |
| Uploads, bulletin median, worst p90 | 314, 6.0 min, 15.6 min | 289, 5.9 min, 12.6 min |

The two worlds with role changes under §11 are the item §11 left open: an attacker with its own id
raised the churn there, which §11 suspected came through the score's term for unused airtime, an
announcer losing it while it repeats. Followers now challenge on capability only (§12.2), and the
churn is gone.

What a false announcer still costs: each follower it captures stays with it until it has seen
that announcer's channel silent for 40 minutes, or 90 minutes on a shared channel; every
announcer that hears its claims yields to it first; with five of them, 2 to 5 % of deliveries
were still outstanding after twelve hours; and a follower that hears it better than its own
announcer follows it again by signal once the hour is over (the living network below). Making
the time ignored double with each offence, remembered for 32 hours, cut those role changes in
band O but cost band L dearly: an honest announcer ignored by mistake was then ignored for hours,
and one world held only half of its windows at the end. It was left out.

### 12.5 The nine scenarios, and the living network

The nine scenarios of §9.3 (SNAC music and speech alternating, eight seeds each), against main
before this round; "delivered" is the mean over the seeds, with the worst in brackets:

| Scenario | Main: delivered, median, bulk frames | Now | Frames |
|---|---|---|---|
| Band O neighbourhood, 50 nodes, 1 km² | 100 % (100), 9.1 min, 6,936 | 100 % (100), **7.8 min**, 6,930 | 0 % |
| Band L neighbourhood, 50 nodes, 1 km² | 100 % (100), 16.8 min, 16,774 | 100 % (100), 17.0 min, 16,101 | −4 % |
| Band O, 100 nodes, 15 km² | 100 % (100), 8.9 min, 39,480 | 100 % (100), **8.6 min**, 45,726 | +16 % |
| Band L, 100 nodes, 15 km² | 100 % (100), 29.5 min, 133,198 | 100 % (100), **26.9 min**, 134,672 | +1 % |
| ESP-NOW, 30 nodes, 1 km² | 100 % (100), 14.9 min, 45,585 | 100 % (100), **14.4 min**, 42,956 | −6 % |
| Two clusters 1.8 km apart, band L | 100 % (100), 20.3 min, 9,167 | 100 % (100), 20.1 min, 7,263 | −21 % |
| LoRa only, two nodes 5 km apart | 100 % (100), 47.5 min, 2,483 | 100 % (100), 47.4 min, 2,441 | −2 % |
| Town, band O, 200 nodes, 30 km² | 100 % (100), 14.7 min, 171,656 | 100 % (100), 14.6 min, 176,430 | +3 % |
| Town, band L, 200 nodes, 30 km² | 100 % (100), 41.5 min, 481,424 | 100 % (100), **36.0 min**, 507,517 | +5 % |

The living network of §7.6 (72 hours, 50 nodes on 1 km², a daily bulletin per channel, eight
seeds), without an attacker and against one attacker of each kind: a WANT flood under its own id,
the same under a made-up id per WANT, and a lure claiming nothing. Bulletin median and worst p90
in minutes, the worst world's share of followers holding every current window at the end, and
role changes after the first hour per world:

| Band L | §11 | Now |
|---|---|---|
| No attacker | 5.8, 15.6, 100 %, 0 | 5.8, 15.6, 100 %, 0 |
| WANT flood | 6.0, 15.6, 100 %, 0 to 159 | 5.9, 12.6, 100 %, 0 |
| WANT flood, made-up ids | 6.0, 16.8, 100 %, 0 | 5.9, 12.6, 100 %, 0 |
| One lure | 17.8, 2,078, 61.9 %, 7,165 to 67,134 | 23.8, 52.2, 92.9 %, 12 to 289 |

| Band O | §11 | Now |
|---|---|---|
| No attacker | 1.8, 1.8, 100 %, 0 | 1.8, 1.8, 100 %, 0 |
| WANT flood | 1.8, 2.4, 100 %, 0 | 1.8, 3.0, 100 %, 0 |
| WANT flood, made-up ids | 1.8, 2.4, 100 %, 0 | 1.8, 3.0, 100 %, 0 |
| One lure | 2.2, 7.8, 95.2 %, 65,059 to 154,564 | 1.8, 3.0, 100 %, 24 to 487 |

Under §11 a single lure turned the election into a perpetual challenge: followers outscored it,
stepped up, yielded to its next beacon and outscored it again, tens of thousands of times. Now a
follower near it leaves it when it is found out and comes back when it is no longer ignored. In
band L that is still the largest cost in this section: the median bulletin took 24 minutes
instead of 6, because each new bulletin found the followers near the lure back with it.

What it cost: in the band O scenario of 15 km² the network used 16 % more airtime than main at
the same delivery. There the election now forms its final cells, three to five of them, within
five minutes of a cold start, and every cell carries every object once; main reached about the
same number of announcers only after up to two hours of churn, during which fewer, half-formed
cells had carried the first objects to more nodes at once. The band L town used 5 % more for a
median 13 % faster. Everywhere else frames were within 4 % or fewer, and no scenario was slower by
more than a fifth of a minute.

## 13. How long a piece should be, and manifests that reach everyone

Should MeshCast limit how large an object may be, and so how long an audio file? Only if it has
to: a limit is a rule every publisher must know, and abuse is better bounded where it happens
(ABUSE.md). So the question was measured instead: if a source has an hour of music, how should it
cut it? The sweep found two faults that only many small objects showed, and a repetition that grew
with the catalogue. Removing that repetition made manifests depend on being asked for, and a living
network with nodes coming and going then showed four ways a manifest could fail to reach a node,
one of them an attack. All numbers are eight seeds.

### 13.1 The measure and the sweep

A listener wants a programme whole, however its source cut it. The ensemble report now gives
**whole content**: per follower and source, the time until the follower held the first of that
source's objects and until it held all of them; the median of each over the pairs, the 90th
percentile of the second, averaged over the seeds (sim/README.md).

Each source publishes one hour of SNAC music (1.88 kbit/s, PROTOCOL.md §1.1: about 846 kB) as
120 pieces of 30 seconds, 60 of 1 minute, 20 of 3, 12 of 5, 6 of 10, 4 of 15, 2 of 30 or one of
60 minutes (7 to 846 KiB in the simulator, which counts a kB as 1,024 bytes; 42 KiB is 3 minutes
and 3 seconds). Four scenarios of §9.3: the neighbourhood (50 nodes on 1 km², one station, two
sources, 6 hours) and the 15 km² network (100 nodes, two stations, three sources, 12 hours), each
in band O and band L.

### 13.2 Two faults that only many objects showed

On main, 30-second pieces delivered nothing at all, in all four scenarios. The simulator's store
keeps the bytes only of objects up to 4 kB, because it counts audio symbols instead of storing
them, and a manifest is the one large object a node must read. A manifest listing more than 62
objects outgrew that, was collected and never read, and nobody wanted its objects. A real node
keeps what it carries, so the fault was the simulator's; but the core now keeps the bytes of every
object a node reads (manifests and rendition tables) whatever the store's setting, and the store's
unit test `a_manifest_is_kept_whatever_its_size` holds it to that.

The second fault was the protocol's. In the band L neighbourhood an hour arrived whole after 21
minutes as one piece and after 70 as twenty 3-minute pieces. An announcer divides its listening
time into phases and gave each *grant* its own (PROTOCOL.md §4). A source holding twenty objects
was granted several of them at once, each with a phase of its own; it uploads one object at a
time, so it used one of its phases, and in every cycle the announcer spent the others listening
to nobody. Phases now go to holders: a holder with running grants gets their phase again.

| Band L neighbourhood, one hour per source | Main: first piece, all of it (median, p90) | Phases per holder |
|---|---|---|
| 1-minute pieces | 10.1, 68.1, 73.7 min | 9.1, 38.5, 53.2 min |
| 3-minute pieces | 13.4, 69.7, 73.5 min | 9.6, 27.4, 32.6 min |
| 5-minute pieces | 15.2, 61.2, 63.3 min | 10.6, 24.9, 33.1 min |
| 10-minute pieces | 18.3, 46.3, 49.0 min | 12.3, 23.2, 31.1 min |
| 15-minute pieces | 19.2, 34.5, 43.6 min | 13.0, 19.3, 31.6 min |
| 30-minute pieces | 20.5, 25.8, 35.8 min | 16.8, 20.7, 30.0 min |
| One piece | 21.0, 21.0, 29.6 min | 21.0, 21.0, 29.6 min |

In the 15 km² band L network 3-minute pieces went from 114 to 81 minutes. In band O, where an
announcer does not divide its listening time (PROTOCOL.md §4), no size became slower and none
faster by more than 3.7 minutes. The fault was in every band L scenario with more than one object
per source: the standard mix of §9.3 has twenty, and the band L neighbourhood's median went from
17.0 to 10.9 minutes with this alone (§13.8).

### 13.3 What repeating manifests cost

With small pieces the carousels repeated a lot that nobody had asked for. Nearly all of it was
manifests: a carousel passed every manifest it served every `T_always` (5 minutes) and, in
addition, at the start of every round in which anything at all was wanted. A manifest grows with
the catalogue, 120 entries for an hour in 30-second pieces, and so did the repetitions. Neither
helps anyone: a node that lacks a manifest learns its id from `MANIFEST_ANNOUNCE` and asks for it.
A carousel now passes a manifest once when it is new and otherwise only when it is wanted, still
before anything else (PROTOCOL.md §4).

| Band L neighbourhood | Passes nobody asked for again | All frames | All of it, median |
|---|---|---|---|
| 1-minute pieces, repeated on the timer and every round | 10,367 | 50,741 | 38.5 min |
| without the timer | 5,069 | 45,042 | 38.0 min |
| only when new or asked for | 1,458 | 41,868 | 37.2 min |
| 3-minute pieces, repeated on the timer and every round | 2,551 | 44,105 | 27.4 min |
| only when new or asked for | 101 | 41,969 | 26.3 min |

(The second, third and fifth rows include the rule of §13.4.)

In the nine scenarios of §9.3 this saved up to 29 % of all frames at the same delivery and speed
(§13.8). It also made every node depend on asking for what it lacks, which §13.6 tested.

### 13.4 A busy holder offers last

§12.3 stopped a holder that is uploading from offering at all, because its offer silenced holders
that were free. With many objects per source that left an object only the busy holder had unasked
until a later round. A busy holder now offers, but a whole `T_offer` later than a free one: a free
holder's offer comes first and silences it, and an object nobody else holds is granted to the busy
one and queued behind its current upload. In the band L neighbourhood an hour in 3-minute pieces
went from 27.4 to 24.6 minutes; in the nine scenarios it was neutral to slightly better (the band
L town 32.5 to 31.5 minutes, ESP-NOW 14.4 to 14.1, everything else within 0.2 minutes, frames
within 1 %).

### 13.5 What small pieces still cost: asking

After these fixes, pieces shorter than about three minutes are still slower in band L, and the
reason is the asking. A follower lists at most 8 objects in a WANT and sends one per `T_want_min`
(10 minutes); its announcer's asks are bounded the same way. Twenty pieces per source take more
rounds of asking than six, and in a network of several cells each round is slow. An experiment
let followers and announcers ask for up to 24 objects per `T_want_min`, in up to three frames: in
the 15 km² band L network an hour in 3-minute pieces arrived after 64 minutes instead of 79 (p90 75
instead of 91), in 10-minute pieces after 51 instead of 54, for 6 % more frames. That is a cost per
object, not per byte, and more frames are the wrong way to pay it. The way to pay it belongs to the
design of collections (PROTOCOL.md §9, question 13): a follower that asks for a collection by its
manifest and a bitmap of what it lacks asks for twenty pieces in one entry. The experiment was not
merged. (§14 measured that proposal and found it half right: a follower's asks alone gain nothing;
the cost sits in the announcer's rounds of asking and granting.)

### 13.6 Manifests that reach everyone

The living network of §7.6 with nodes coming and going (every 3 hours a tenth of the nodes switch
off or on, every 6 hours one is replaced by a newcomer that knows nothing; 72 hours) tests what
depends on asking: a node that returns after hours or a newcomer must find the current manifests.
The measure is the share of followers on at the end that hold the current window of every channel
they follow, in the worst of the eight worlds. With manifests repeated (and §13.2's fixes) that was
100 % in band L and 58.8 % in band O; without the repetition band L fell to 83.3 %. Traces of every
follower that lacked something found four faults.

- *A channel followed again was never fetched again.* A node that stops following a channel evicts
  its manifest and objects but remembers the manifest's sequence number, so announcers that list
  that number tell it nothing new. When it followed the channel again it waited for a number that
  would not come; the repetition had been covering for it. It now asks for a manifest it follows and
  does not hold (smoke test `a_channel_followed_again_is_fetched_again`, which fails without it).
- *An announcer behind its cell.* The worst band O world failed with manifests repeated too: the
  station came back from a power cut with the manifests it had held before, took over, and never
  learned the newer ones, because a source announces its own manifest only until its announcer has
  it, and the announcer it had told had stepped down. In another world an announcer had never
  heard of one channel at all. Its followers held the newer manifests and had no way to say so.
  Now a follower that hears its own announcer announce an older manifest than the one it holds,
  or leave a channel out, announces its own (PROTOCOL.md §2): 45 to 109 such announcements per
  world in 72 hours.
- *An announcement cost a follower its window.* A node replaced the manifest it held by a newer one
  as soon as it was announced, and evicted every object the old one listed while it waited for
  the new one. In two worlds of the band L living network of §7.6, without nodes coming and
  going, 879 and 907 objects were evicted and fetched again, and the announcers granted 264 and
  261 uploads instead of 164 and 168; in band O it did not occur. A node now keeps the manifest it
  adopted, and its objects, until the newer one is held.
- *One frame could freeze a channel.* Reading that code showed a worse fault: since an announced
  sequence number was taken as the channel's, one unsigned `MANIFEST_ANNOUNCE` with the highest
  number and a made-up id made a node ignore every real announcement of the channel and refuse its
  real manifests, and announcers passed the claim on. Now a node believes only manifests it has
  checked; the latest announcement replaces one still pending unless that one is arriving; and
  announcers announce only what they hold (ABUSE.md; smoke test
  `a_false_announcement_blocks_nothing`, which fails on the earlier code both at the eviction and
  at the freeze).

With those four, every world held every current window, but counting what was fetched twice
showed two more things. A node that restarts is not announcing yet, so it evicted at once the
library of every channel it did not follow itself; a station back from a power cut took over a
minute later and fetched it all again. A node that restarts or stops announcing now evicts
nothing for `want_ttl`. In the eight band L worlds with nodes coming and going, objects fetched
twice went from 881 to 63 and uploads from 375 to 273 per world; in band O from 240 to 82 and
from 160 to 138.

And with far fewer passes, the evidence rule of §12.4 lost an accidental crutch. A source holds
its own objects pending until it hears someone send them or an announcer confirms its upload,
and an announcer that lists a pending object, or one the follower wants and never gets a symbol
of, for 90 minutes is taken for a liar. In one band L world of the ordinary living network a
source left an honest announcer every hour, 218 role changes in three days where §12.5 had none.
Three faults lined up: a source that announced passed its own objects without that counting as
sent; the clocks for its wants and for a silent channel ran from before it followed the announcer
it then accused, from when it first wanted an object or last heard a frame, as announcer itself;
and an upload to a neighbouring cell's announcer was confirmed only if that announcer was the
source's own at the time. Fixing them in that order took the role changes from 218 to 82, 10
and 0 (PROTOCOL.md §5.2). Before, followers fetched objects twice so often (the eviction above)
that sources heard their objects sent again, which hid all three.

| Living network, nodes coming and going, eight worlds | Repeated (§13.2 fixed) | Not repeated | + followed again | + corrections | Now |
|---|---|---|---|---|---|
| Band L: followers holding the current window, worst world | 100 % | 83.3 % | 83.3 % | 100 % | 100 % |
| Band L: worlds below 100 % | 0 | 2 | 1 | 0 | 0 |
| Band L: newcomers caught up, mean per world; slowest (h) | 0.13–0.46; 3.19 | 0.14–0.19; 0.39 | 0.20–0.28; 0.42 | 0.21–0.30; 0.49 | 0.20–0.28; 0.47 |
| Band L: uploads per world, mean | 405 | 390 | 397 | 408 | 270 |
| Band L: corrections per world | – | – | – | 16–48 | 46–78 |
| Band O: followers holding the current window, worst world | 58.8 % | 58.8 % | 58.8 % | 100 % | 100 % |
| Band O: worlds below 100 % | 3 | 3 | 3 | 0 | 0 |
| Band O: newcomers caught up, mean per world; slowest (h) | 0.14–3.88; 18.0 | 0.13–3.89; 18.0 | 0.24–3.82; 18.0 | 0.17–0.24; 0.45 | 0.16–0.33; 1.06 |
| Band O: uploads per world, mean | 129 | 132 | 134 | 158 | 145 |
| Band O: corrections per world | – | – | – | 10–51 | 45–95 |

"Repeated" has the fixes of §13.2 and the timer and every-round repetition; "not repeated" has
the timer switched off; the next two add a new manifest passed only once (§13.3) with the rule of
§13.4 and the first fix above, and then the corrections; "now" is everything in this section,
including §13.7. All 24 worlds of "now" (eight more in band O) held every current window at the
end. The median bulletin reached its followers in 6.0 minutes in band L before and 5.4 now, and in
1.8 minutes in band O throughout; in every column 6 to 9 of the 184 bulletins took their median
follower more than an hour. Band O carries more uploads now because it now delivers what it did
not before.

Two costs remain. Corrections rose when announcers stopped announcing what they do not yet hold:
a follower then corrected an announcer again while it was still fetching, so a follower does not
correct an announcer that is asking for that very manifest (in eight further band O worlds, 124
to 278 per world before that rule and 35 to 138 with it, at the same delivery). Most of the rest
are a new announcer being taught, while the station is switched off, the channels it did not
follow itself. And a newcomer, which knows nothing, used to overhear every manifest in the next
round of its cell's carousel; now it asks for the manifests and then for their objects, one
`T_want_min` apart, and in band L catches up a few minutes later on average (0.20 to 0.28 hours
per world against 0.14 to 0.19 while manifests went out in every round). Letting a node ask at
once for the first manifest of a channel would save that round but make every node of a cold
start ask in the same seconds; it was not done.

### 13.7 What the repetition had been doing for the defence

The attacks of §12.4 and §12.5, run again on all of the above, found the rules of §12.4 weaker
than they had been, and two reasons that the repetition had been hiding.

- *A false announcer that lists only what a follower cannot want yet.* The rule ignored an
  announcer that *lists* an object its follower wants and never sends one symbol of it. In one
  world with five false announcers ten honest followers ended with nothing at all. The one traced
  had followed a false announcer from the first minute to the last: it wanted only the three
  manifests, which it had heard announced elsewhere, and the false announcer listed the tracks.
  Without a manifest a follower cannot want a track, so there was no evidence; before, a manifest
  repeated by any announcer within earshot reached such a follower within minutes, after which it
  wanted the tracks and caught the lie. Now the rule asks what an honest announcer does with an
  object its follower wants: it serves it, or, lacking it, asks for it or grants it. One that does
  none of these for 90 minutes is not followed, whether it lists the object or not.
- *Silence that is not a lie.* The quicker test, a want stalled for `T_excursion` on a channel that
  carried not one `BULK` frame in that time, assumed an honest announcer is never silent that long.
  Under a WANT flood it is: every repetition waits up to the 80-minute ceiling (§11), and only the
  manifest repetition had kept the channel busy. In the living network with one flooding attacker,
  followers left honest announcers up to 40 times per world in band L and 97 in band O, where §12.5
  had none. Making the silent test as long as the other (90 minutes) stopped that but let false
  announcers keep their followers longer. Instead a follower now asks for proof: after
  `T_excursion` of silence it sends its announcer a `NACK` for one symbol of what it waits for,
  naming that announcer, every `T_nack_stall`. An honest announcer answers from the front of its
  next round whatever the backoff; one that has sent nothing `T_want_min` later serves nothing
  (PROTOCOL.md §3.5, §5.2). Honest announcers under the flood were asked 0 to 8 times per world in
  72 hours, and never without an attacker; a false announcer up to 72 times per world in band L,
  by all the followers it held together.

The clocks of §13.6 run from when a follower began to follow, except the one for a source's own
objects: with that clock moved too, false announcers that listed a source's objects held the
source for 90 minutes every time it was captured, and five of them left 89 % delivered, 58 % in
the worst world (third column below). Once its own carousel passes and its uploads to any cell
count, a source's pending objects are evidence again.

| 15 km² band L, eight worlds, delivered after 12 hours: mean, worst world | §12 | Not repeated, §12's evidence | Not repeated, clocks from following | Now |
|---|---|---|---|---|
| One false announcer claiming nothing | 99.4 %, 98.7 % | | | 99.4 %, 98.9 % |
| Five claiming nothing | 97.8 %, 94.7 % | 94.6 %, 86.8 % | 89.4 %, 57.9 % | 96.7 %, 92.6 % |
| Five claiming the maximum | 95.2 %, 86.9 % | 93.4 %, 85.8 % | 84.9 %, 54.5 % | 96.3 %, 94.0 % |

Runs to 24 hours, of §12 and of a step between, gave the same numbers to the decimal as at 12
hours: what is missing then is missing for good. In the world traced above, 312 follower-object
pairs were missing: 240 belonged to the ten followers without a manifest and 72 to three of the
false announcers themselves, which the measure counts as followers in every column.

| Living network, 72 hours, eight worlds: bulletin median, worst p90, worst world held, role changes after the first hour | §12.5 | Now |
|---|---|---|
| Band L, no attacker | 6.0 min, 15.6 min, 100 %, 0 | 5.4 min, 15.6 min, 100 %, 0 |
| Band L, WANT flood | 6.0 min, 12.6 min, 100 %, 0 | 5.4 min, 21.0 min, 100 %, 0 |
| Band L, WANT flood, made-up ids | 6.0 min, 12.6 min, 100 %, 0 | 5.4 min, 13.8 min, 100 %, 0 |
| Band L, one false announcer | 6.0 min, 52.2 min, 92.9 %, 12 to 289 | 5.4 min, 52.8 min, 100 %, 25 to 179 |
| Band O, no attacker | 1.8 min, 1.8 min, 100 %, 0 | 1.8 min, 1.8 min, 100 %, 0 |
| Band O, WANT flood | 1.8 min, 3.0 min, 100 %, 0 | 1.8 min, 3.0 min, 100 %, 0 |
| Band O, WANT flood, made-up ids | 1.8 min, 3.0 min, 100 %, 0 | 1.8 min, 3.0 min, 100 %, 0 |
| Band O, one false announcer | 1.8 min, 3.0 min, 100 %, 24 to 487 | 1.8 min, 7.8 min, 100 %, 18 to 34 |

(§12.5's table gives the bulletin median as a mean of medians; here it is the median, 6.0 minutes
in band L for the same runs.) What it costs: the slowest bulletins took longer in two rows, under
the WANT flood in band L (worst p90 21.0 minutes against 12.6) and with a false announcer in band O
(7.8 against 3.0); the medians are the same or better everywhere, and no world loses a window.

Open: an honest announcer that has never heard of an object its follower wants (a manifest the
follower heard announced in another cell, say) neither serves it nor asks for it, and now counts as
one that does not serve. No measured world showed it, without an attacker or with one; an
announcer that took on such a want itself, by asking for it, would close it.

### 13.8 The optimum, and the nine scenarios

The sweep of §13.1 with everything above, eight seeds each: the first piece and all of the hour
(median over follower and source, in minutes), every pair complete in every world:

| Piece (KiB × pieces per source) | Band O, 1 km²: first, all | Band L, 1 km² | Band O, 15 km² | Band L, 15 km² |
|---|---|---|---|---|
| 30 seconds (7 × 120) | 4.8, 30.8 | 9.0, 69.8 | 4.9, 53.4 | 15.5, 263.6 |
| 1 minute (14 × 60) | 4.8, 24.2 | 9.1, 39.1 | 4.8, 50.1 | 14.8, 151.5 |
| 3 minutes (42 × 20) | 5.4, 27.5 | 9.7, 25.8 | 5.7, 47.3 | 16.2, 80.4 |
| 5 minutes (70 × 12) | 6.1, 24.5 | 10.3, 24.6 | 5.9, 46.0 | 19.2, 65.5 |
| 10 minutes (141 × 6) | 7.8, 25.5 | 11.6, 21.1 | 8.8, 41.7 | 23.2, 54.9 |
| 15 minutes (211 × 4) | 9.6, 25.2 | 11.7, 18.9 | 11.5, 39.9 | 30.3, 55.8 |
| 30 minutes (423 × 2) | 14.3, 25.2 | 16.6, 20.9 | 19.6, 39.3 | 40.3, 56.6 |
| One piece (846 × 1) | 20.5, 20.5 | 19.7, 19.7 | 36.9, 36.9 | 59.7, 59.7 |

Two waits pull apart. The first piece comes about as soon as a piece can: within about 6 minutes
in band O for pieces up to 5 minutes long, against 20 to 37 minutes for the hour in one piece. All
of it comes soonest in one piece. In band O pieces cost a fifth to a half more in the
neighbourhood (24 to 31 minutes against 20.5) and, across 15 km², a seventh or less from 10-minute
pieces up. In band L pieces shorter than 3 minutes cost dearly: across 15 km² an hour in 30-second
pieces took 4.4 hours and in 1-minute pieces 2.5, the asking of §13.5. Frames do not grow as pieces
shrink, except in band L across 15 km², 562,000 for 30-second pieces against 448,000 to 456,000
from 3 to 15 minutes.

So the optimum is a range, not a size: **pieces of 5 to 15 minutes** start playback in a sixth to
three fifths of the time the hour in one piece takes, and bring all of it within a third of the
best in every scenario (10 and 15 minutes within a quarter). 10 minutes is a good default for long
programmes. A song as it is, 3 to 5 minutes, is fine in band O; in band L across 15 km² an hour of
such songs takes a fifth to a half longer than in 10-minute pieces. Below 3 minutes the answer is
the asking by collection of §13.5, not a rule for publishers.

No limit is needed. A large object is not slower to arrive whole than the same content in
pieces; it only starts later, because the carousel and the asks serve the most listeners per
byte first (§9.4), so small objects pass large ones and publishers are rewarded for cutting long
programmes. What a large object could be abused for, a catalogue no node can carry, is the
channel flood and store exhaustion of ABUSE.md, and its answer is ABUSE.md's bounded generosity,
not a size field.

The nine scenarios of §9.3 (SNAC music and speech alternating, twenty objects per source, eight
seeds each), against main after §12; "delivered" is the mean over the seeds, with the worst in
brackets, and the last column the change in bulk frames:

| Scenario | Main: delivered, median, bulk frames | Now | Frames |
|---|---|---|---|
| Band O neighbourhood, 50 nodes, 1 km² | 100 % (100), 7.8 min, 6,930 | 100 % (100), 7.3 min, 6,596 | −5 % |
| Band L neighbourhood, 50 nodes, 1 km² | 100 % (100), 17.0 min, 16,101 | 100 % (100), **10.8 min**, 13,872 | −14 % |
| Band O, 100 nodes, 15 km² | 100 % (100), 8.6 min, 45,726 | 100 % (100), 8.4 min, 39,106 | −14 % |
| Band L, 100 nodes, 15 km² | 100 % (100), 26.9 min, 134,672 | 100 % (100), **24.0 min**, 112,507 | −16 % |
| ESP-NOW, 30 nodes, 1 km² | 100 % (100), 14.4 min, 42,956 | 100 % (100), 14.9 min, 42,934 | 0 % |
| Two clusters 1.8 km apart, band L | 100 % (100), 20.1 min, 7,263 | 100 % (100), **17.3 min**, 5,855 | −19 % |
| LoRa only, two nodes 5 km apart | 100 % (100), 47.4 min, 2,441 | 100 % (100), 47.1 min, 1,322 | −46 % |
| Town, band O, 200 nodes, 30 km² | 100 % (100), 14.6 min, 176,430 | 100 % (100), 13.8 min, 142,274 | −19 % |
| Town, band L, 200 nodes, 30 km² | 100 % (100), 36.0 min, 507,517 | 100 % (100), **33.6 min**, 365,429 | −28 % |

Most of the speed is §13.2's phases per holder, which every band L scenario with more than one
object per source needed; most of the frames are §13.3's manifests. ESP-NOW is the exception in
both, and within its own spread: its eight worlds range from 21,000 to 88,000 bulk frames, so a
difference of a few thousand in the mean says nothing either way.

## 14. What a piece costs, and asking per collection

§13 found that pieces shorter than about three minutes still cost band L dearly and laid that at
the follower's door: it asks for at most eight objects per `T_want_min`. The proposed answer was
asking per collection, a manifest and a bitmap of what is missing in one entry. This section
measured that proposal, found it half right, and found what a piece really costs. All numbers are
eight seeds unless one world is named.

### 14.1 Where the last piece waits

The whole-content measure now also says where the last piece of each follower and source waited:
when the follower's announcer held it, and how much later the follower did.

| One hour of music per source | Last piece at the announcer (median) | Follower after that (median, p90) |
|---|---|---|
| Band L, 15 km², 1-minute pieces | 140 min | 11, 51 min |
| Band L, 15 km², 10-minute pieces | 50 min | 2, 16 min |
| Band L neighbourhood, 1-minute pieces | 39 min | 0, 3 min |
| Band L neighbourhood, 10-minute pieces | 19 min | 0, 4 min |

Small pieces are late *at the announcer*: a supply problem, not one of asking or of the carousel.
That is why the first two attempts gained nothing. Letting followers ask in sets left the 15 km²
network with 1-minute pieces at 151 minutes and cost 28 % more frames, because every follower now
re-asked everything it missed each round and the carousels repeated more; letting an announcer
grant a busy holder several objects at once left it at 151 too.

The upload timeline at the station of one neighbourhood world showed the cost. Uploads were quick,
a median 14 seconds from first symbol to complete, but the announcer asked for at most eight objects
per round, and each ask needed an offer and a grant, 1.6 minutes, before the upload began: 120
pieces in rounds of eight at about 2.5 minutes a round is the 39 minutes measured. On a hopping
carrier an announcer asks only in the meeting dwell, so a round there is at least a hop cycle.
Letting announcers ask for 24 objects per round, in up to three frames, took the 15 km² network with
1-minute pieces to 137 minutes; adding follower sets to that, to 89 (−41 %, for 10 % more frames).
Both sides of the asking mattered, which is what the experiment of §13.5 had measured without
being able to say why.

### 14.2 Sets in all three steps

The principled form asks, offers and grants in sets (PROTOCOL.md §3.3): a follower asks its
announcer for the pieces of a collection in one entry; an announcer asks holders the same way,
open; a holder offers what it holds of it the same way; and the announcer grants each holder its
pieces in one entry, which the holder uploads one after the other in its phase. A frame stays
within its old 234 bytes.

| One hour of music, all of it (median) | Before | Sets in all three steps | Frames |
|---|---|---|---|
| Band L, 15 km², 1-minute pieces | 151.5 min | 72.1 min | +11 % |
| Band L, 15 km², 3-minute pieces | 80.4 min | 59.9 min | +11 % |
| Band L, 15 km², 10-minute pieces | 54.9 min | 53.6 min | +2 % |
| Band L neighbourhood, 1-minute pieces | 39.1 min | 24.2 min | +14 % |
| Band L neighbourhood, 3-minute pieces | 25.8 min | 19.2 min | +5 % |

In the nine scenarios of §9.3, however, band L and ESP-NOW gained and four others lost: the band O
neighbourhood 7.3 to 9.1 minutes, the band O town 13.8 to 15.7, the two clusters 17.3 to 26.4 and
LoRa across 5 km 47.1 to 51.3. Ablating each part of the change one at a time traced four causes.

- *The order of a holder's uploads.* Asked by name, objects were asked most listeners per byte
  first; a granted set arrived in the collection's order, so a 42 kB track went before every 22 kB
  bulletin of the same source. A holder now uploads what one grant brought smallest first, and
  among equal sizes in the collection's order (PROTOCOL.md §4). LoRa went back to 47.2 minutes.
- *A carousel that chained its frames.* In one band O world a source took 2.5 minutes for a piece
  that takes 0.8. Counting who made its channel busy each time it wanted to send found the
  station, 3,298 times, once for every frame the station sent; with names, 65 times. Content frames
  get no jitter, so a carousel with work sends them back to back, and a holder with grants queued
  finds the channel busy whenever it wakes. Leaving a frame's airtime free after each carousel
  frame while uploads are owed did not change that count. Dividing the announcer's listening time
  in phases under a duty cycle too, as under polite access, did: the neighbourhood went to
  7.2 minutes. (Phases were kept to polite access in §9.6 because they made a lone upload K times
  slower; with phases per holder (§13.2) a lone holder has K = 1, so that objection no longer held.)
- *The order across holders.* The band O town stayed at 15.7 minutes with phases: speech had gone
  from 8.5 to 11.4 minutes, music from 19.1 to 20.1, and all of a source's content arrived about as
  late as before. Asked by name, an
  announcer asked for the most listeners per byte first, so a cell's speech, small, came in before
  its music, at 5.0 and 12.8 minutes in one world; asked in sets, everything was asked at once, at
  4.6 and 5.1, and the phases gave every holder an equal share of the announcer's time, so speech
  took 1.3 minutes an upload instead of 0.5 and was complete at 14 minutes instead of 9. Keeping
  each holder's whole queue in that order made it worse (speech 13 minutes): the order that
  mattered was between holders, which only the announcer decides.
- *Sets a node could not read.* The two-cluster loss was one world, 55 minutes before and 115 after.
  A follower of the far cluster went on an excursion (§12.3) to the near cluster's announcer, whose
  HAVE now came as sets of a manifest the follower did not yet hold. It could not read them,
  fetched the manifest, saw nothing more listed and came home; only a second excursion an hour
  later brought the rest. A node now keeps have sets it cannot read per neighbour and reads them
  when it adopts the manifest (PROTOCOL.md §3.3): 55 minutes again.

### 14.3 The rule: what a round costs decides how much it asks for

The order across holders and the rounds pull in opposite directions. Asking in rounds of eight,
most listeners per byte first, is Smith's rule at the announcer and gives small objects the cell's
inbound time first; it costs a round per eight objects. Where a round is cheap, that cost is small
and the order is worth more; where a round costs a hop cycle, the rounds dominate. So where rounds
are dear, on a carrier that hops, followers, announcers and holders use sets; where asking is cheap
a round asks for the eight most valuable objects by name, a busy holder is not asked for a second
object until its first is done, and the next round comes as soon as they have arrived
(PROTOCOL.md §4). Band O and LoRa then behave exactly as before; band L and ESP-NOW keep the gain.
Phases under a duty cycle were then no longer needed and were left out.

The size sweep of §13 with this rule (first piece and all of the hour, medians in minutes; frames
against §13.8):

| Piece | Band L, 1 km²: before | Now | Frames | Band L, 15 km²: before | Now | Frames |
|---|---|---|---|---|---|---|
| 30 seconds | 9.0, 69.8 | 10.4, **30.9** | +27 % | 15.5, 263.6 | 29.6, **81.9** | +4 % |
| 1 minute | 9.1, 39.1 | 9.7, **23.9** | +15 % | 14.8, 151.5 | 16.5, **72.8** | +10 % |
| 3 minutes | 9.7, 25.8 | 10.2, **19.4** | +5 % | 16.2, 80.4 | 19.1, **62.0** | +12 % |
| 5 minutes | 10.3, 24.6 | 10.8, **18.8** | +5 % | 19.2, 65.5 | 19.8, **54.3** | +7 % |
| 10 minutes | 11.6, 21.1 | 12.3, 20.9 | +1 % | 23.2, 54.9 | 25.0, 52.9 | +2 % |
| One piece | 19.7, 19.7 | 19.7, 19.7 | 0 % | 59.7, 59.7 | 58.8, 58.8 | +1 % |

In band O every size stayed within 2.2 minutes of §13.8, and its frames between 8 % fewer and 5 %
more, as it should: there the rule asks as before. In band L the penalty for small pieces has
mostly gone: an hour in 1-minute pieces now arrives in 73 minutes across 15 km², against 53 in
10-minute pieces, where it was 152 against 55; so §13.8's advice of 5 to 15 minutes still holds,
but a song as it is costs little now.
The price is the first piece: with everything granted at once, many holders upload side by side in
phases, each in a share of the announcer's time, and the first piece of 30-second pieces across
15 km² came after 30 minutes instead of 16. Granting the earlier pieces of a collection to the
holders first would buy it back; that is open.

The nine scenarios of §9.3, against §13.8:

| Scenario | §13.8: median, bulk frames | Now | Frames |
|---|---|---|---|
| Band O neighbourhood | 7.3 min, 6,596 | 7.3 min, 6,596 | 0 % |
| Band L neighbourhood | 10.8 min, 13,872 | **10.0 min**, 13,935 | 0 % |
| Band O, 15 km² | 8.4 min, 39,106 | 8.4 min, 39,644 | +1 % |
| Band L, 15 km² | 24.0 min, 112,507 | **22.6 min**, 115,254 | +2 % |
| ESP-NOW | 14.9 min, 42,934 | **13.8 min**, 36,862 | −14 % |
| Two clusters, band L | 17.3 min, 5,855 | 17.5 min, 5,839 | 0 % |
| LoRa only, 5 km | 47.1 min, 1,322 | 47.1 min, 1,322 | 0 % |
| Town, band O | 13.8 min, 142,274 | 13.8 min, 144,093 | +1 % |
| Town, band L | 33.6 min, 365,429 | **32.4 min**, 358,860 | −2 % |

Every scenario delivered 100 % in every world. The attacks of §13.7 and the living network with
nodes coming and going were run again: no role changes without an attacker or under a WANT flood,
with or without made-up ids; every world held every current window; against one false announcer
99.4 % (98.8 % in the worst world), five 97.0 % (92.9 %), five claiming the maximum 96.0 % (91.0 %,
one world three points lower than §13.7, the others within a point). Under the flood the slowest
bulletin of one band O world took 6.6 minutes instead of 3.0; all other worlds were the same.

Also in this round: a store that keeps an object's bytes reserved a whole block of 1,024 symbols for
it, so a 43 kB track took 205 kB; it now keeps exactly the object's symbols.

## 15. Faults that collections exposed

Collections (PROTOCOL.md §9, question 13) add a level of manifest, and measuring what that level
costs found three faults that were there before it; fixing one of them exposed a fourth. Each is
fixed and measured here against main, so that §16 can measure collections against a main without
them. Eight seeds per scenario, as before.

### 15.1 Uploads that start together

In the ESP-NOW neighbourhood 45 % of the upload frames collided at their own announcer: 88,295 of
194,306 over the eight worlds, between 23 % and 61 % per world. ESP-NOW hops, and an upload to an
announcer waits for the end of the meeting dwell, when the announcer's own channel comes back:
every holder with something for it starts at that moment, and holders that cannot hear each other
cannot take turns by sensing the carrier. Dividing the announcer's listening time into phases
stops exactly that (§9.6), but it had been kept to polite access, on the argument that elsewhere
an upload is a burst of seconds that rarely meets another. That holds under band O's duty cycle,
where each holder's own budget limits it and phases made uploads K times slower. It does not hold
where no regulator caps the sender: there the announcer's one receiver is the only limit, so
dividing its time costs nothing in total. With phases on every radio carrier without a cap, on
top of the first fix of §15.2 (PROTOCOL.md §4):

| ESP-NOW neighbourhood, 8 worlds | Before | Phases |
|---|---|---|
| Upload frames | 194,306 | 95,282 |
| Of those, collided at their announcer | 45.4 % | 0.03 % |
| Median | 13.7 min | **11.8 min** |
| Bulk frames per world | 36,383 | 22,919 (−37 %) |

Every other scenario ran byte for byte as before: they all have a regulator.

### 15.2 A source that dropped its own uploads, and its own cell first

A node drops what no manifest of interest names any more: wants, and with them its offers and
the uploads it has queued. A source need not follow its own channel, and in the simulator it does
not, so its own objects were not of interest to it: whenever it adopted another channel's
manifest it dropped every upload of its own it had queued, until its announcer granted the object
again a round later. A grant trace in a band L world showed it: a source granted the same
manifest by two announcers in the same minute uploaded it to the first and dropped the second.
Own objects are now offered and uploaded whatever the source follows (PROTOCOL.md §4, "You carry
what you listen to").

That alone made small pieces slower in the band L neighbourhood (an hour in 42 kB pieces 19.4 →
21.5 minutes, the first piece later at every size), and the trace of the worst world showed why.
Kept, a source's queue now served grants in the order they came, and a source granted by its own
announcer and by another cell's uploaded to the other cell for ten minutes while its own waited:
its own cell had everything after 44 minutes instead of 16, the other after 36 instead of 50. The
dropped uploads had, by accident, put the own cell first. Now a holder uploads what its own
announcer granted before what another cell's granted (PROTOCOL.md §4): its own cell is where it is
heard best, and every follower there that gets an object becomes a holder for the neighbouring
cells. With both rules, against main:

| One hour of music, band L neighbourhood | First piece | All of it | Frames |
|---|---|---|---|
| 7 kB pieces | 10.4 → 9.5 min | 30.9 → **19.9** min | −1 % |
| 14 kB | 9.7 → 9.5 | 23.9 → **18.1** | +1 % |
| 42 kB | 10.2 → 9.6 | 19.4 → 19.1 | +3 % |
| 141 kB | 12.3 → 10.9 | 20.9 → **18.0** | +2 % |
| 211 kB | 12.1 → 11.4 | 17.5 → 18.5 | +2 % |
| One piece | 19.7 → 19.4 | 19.7 → 19.4 | −1 % |

The band O neighbourhood, one cell, ran byte for byte as before at every size. Across 15 km² band L
moved by at most 3.1 minutes either way, and band O within the spread of its worlds, which is
eight minutes per world and nearly a factor of three in frames (70 kB pieces 45.0 → 42.6 minutes,
211 kB 39.8 → 42.1).

### 15.3 Asking on an excursion

A follower asks at most every `T_want_min`. In the two-cluster world where a follower goes on an
excursion (§12.3) it asked the visited announcer for a manifest, had it a few seconds later, and
then waited ten minutes to ask for the pieces the manifest named. At home that wait costs little,
because an announcer passes a new manifest unasked and a follower asks on its cadence; on a visit
only the visitor asks. A follower on an excursion now asks for what a manifest fetched there names
soon: after a random wait of up to `T_offer`, at most every `T_gossip_min`, and only for what it
never asked for (PROTOCOL.md §4). That world went from 54.9 to 45.9 minutes; every other world of
every scenario ran as before. Asking soon at home too was tried and rejected: the band O
neighbourhood went from 7.3 to 7.9 minutes and the town from 14.1 to 14.7, with up to 7 % more
frames, because a cell full of followers asking at once after every new manifest cost the duty
cycle more than it saved.

### 15.4 The nine scenarios, the living network and the attacks

| Scenario | Main: median, bulk frames | All four rules | Frames |
|---|---|---|---|
| Band O neighbourhood | 7.3 min, 6,596 | 7.3 min, 6,596 | 0 % |
| Band L neighbourhood | 10.0 min, 13,935 | **9.6 min**, 13,991 | 0 % |
| Band O, 15 km² | 8.4 min, 39,644 | 8.6 min, 38,366 | −3 % |
| Band L, 15 km² | 22.6 min, 115,254 | **21.7 min**, 114,942 | 0 % |
| ESP-NOW | 13.8 min, 36,862 | **11.9 min**, 23,057 | **−37 %** |
| Two clusters, band L | 17.5 min, 5,839 | **16.4 min**, 5,811 | 0 % |
| LoRa only, 5 km | 47.1 min, 1,322 | 47.1 min, 1,322 | 0 % |
| Town, band O | 13.8 min, 144,093 | 14.1 min, 147,381 | +2 % |
| Town, band L | 32.4 min, 358,860 | **30.5 min**, 360,637 | 0 % |

Every scenario delivered 100 % in every world; the two band O networks of many cells moved within
the spread of their worlds. The living network of §13.6, with nodes coming and going, held every
current window in every world in both bands; band L newcomers caught up in 13.7 minutes on average
(14.4 before), the slowest in 26.4 (29.4). Without an attacker and under a WANT flood, with or
without made-up ids, no role changed after the first hour, as before. Against one false announcer
99.4 % was delivered (99.0 % in the worst world), against five 97.6 % (93.9 %), against five
claiming the maximum 96.5 % (92.8 %), each as good as §14 or a little better. The worst p90 moved
by a few minutes either way, as a single publication in a single world does (band L without an
attacker 12.0 → 15.6 minutes, band O under the flood 6.6 → 4.8).

## 16. Collections

A provider now publishes collections (PROTOCOL.md §2): a signed root manifest per channel names
its albums, series and singles, each with a collection manifest that lists its pieces and is as
authentic as the root that names it by its full hash, and with a cover, a JPEG object. A node
follows a whole channel or single collections. In the simulator a source publishes its pieces as
one series by default; `--collections`, `--follow-collections` and `--cover-kb` vary that
(sim/README.md). Eight seeds per scenario, as before.

### 16.1 What a second level costs to discover

The first measurement, against main as #11 left it, put every collection manifest one round of
asking behind its root: the band L neighbourhood took 13.8 minutes instead of 10.0, ESP-NOW 20.9
instead of 13.8, the two clusters 21.3 instead of 17.5, the band L town 35.1 instead of 32.4. A
trace of the neighbourhood's station showed the rounds: the roots at 5.3 minutes, their
collection manifests at 8.7, the first piece at 12.3, where main had had the first piece at 8.8.
On a hopping carrier an announcer asks only in the meeting dwell, every 1.7 minutes there, and
each level took an open ask in one dwell and a grant in the next.

### 16.2 A root brings what changed in it

The root now flags the collection manifests that are new in it (`changed`), and a grant of the
root covers those (PROTOCOL.md §4): a holder lists them with the root in the HAVE that offers it,
an announcer that grants the root on that HAVE takes them as granted to the same holder in the
same phase, and the holder uploads them right after the root, which the announcer must hold
first to read them. In a smoke test the collection manifest otherwise reached the station 200
seconds after its root. The first version let the holder upload what it held and the announcer
count what the holder had listed at any time; in the ring of hidden uploaders that pushed the
collisions at the station above the test's 2 % (2.1 %). Holder and announcer now go by the same
frame, so they agree on whose phase an upload uses. That took the band L
neighbourhood to 11.1 minutes, ESP-NOW to 14.9, the two clusters to 18.8 and the band L town to
33.1, and tracing what was still left found the four rules of §15.

### 16.3 Manifests first at the holder

On top of §15, the 15 km² band L network was still 0.9 minutes slower than main. Per announcer, a
collection manifest mostly arrived with its root, but in one world a holder granted a collection
manifest at 15.1 minutes uploaded it at 20.9, after the pieces it had lined up before. Nothing of
a collection can be read without its manifest, and a manifest is a few symbols, so a holder now
uploads a manifest before the pieces it already lined up, after any repair (PROTOCOL.md §4). The
band L town went from 31.3 to 30.4 minutes, the 15 km² network from 22.6 to 22.2.

### 16.4 An ask answered in full

In the living network with nodes coming and going, newcomers took longer: 21.5 minutes on average
in band L instead of 13.7 on main, the slowest 50.4, and in band O 20.8 instead of 13.3, the
slowest 64.8. A newcomer has to ask for everything, and each of its asks waited `T_want_min`: the
root, then the collection manifest, then the pieces. What a manifest a follower asked for names
is now the rest of that ask, asked for soon, after a random wait of up to `T_offer`, at most
every `T_gossip_min` and only for what was never asked for (PROTOCOL.md §4). A manifest that came
unasked, from a pass, changes nothing; asking soon after every manifest had cost the band O
networks more than it saved (§15.3). Newcomers then caught up in 9.1 minutes in band L, the
slowest in 22.2, and in 8.0 in band O, the slowest in 19.8: faster than on main. In the matrix it
moved the band O networks by a few tenths either way and took LoRa from 47.2 to 46.0 minutes. It
also changed one smoke test's world: a follower in the source's cell that asked for the root now
asked for the pieces at once, its station passed them, and the edge of the next cell overheard
them, so the excursion the test is about was no longer needed; in the test that neighbour no
longer follows the channel.

### 16.5 What an announcer asked for is about that announcer

One band O world still had a newcomer that waited 65 minutes for one piece. Its announcer learned
the channel's new root only an hour after it was published, though the newcomer held it from the
first minute. A follower tells its announcer of a newer manifest (PROTOCOL.md §2), but not while
the announcer is asking for that manifest itself, and the newcomer took its announcer to be
asking: it had first followed another announcer, which had asked for the root, and what an
announcer asks for was remembered for `want_ttl` whoever announced. Counted, 61 of the 69 times
the newcomer saw its announcer behind, it held back for that reason. What a follower's announcer
asked for and granted is now forgotten when it follows another one. That world's slowest newcomer
then took 16 minutes. On main the rule changed nothing measurable: every cell of one announcer
ran byte for byte as before, the 15 km² band O network went from 8.6 to 8.2 minutes, the band L
town from 30.5 to 30.2, and newcomers stayed as they were.

### 16.6 Following one collection, and covers

Twenty pieces per source, an hour of mixed SNAC: one series, or four albums of five followed
whole, or four albums of which each follower follows one at random, with and without a cover of
16 kB for each album (an assumed size). Medians in minutes over the eight worlds: the first piece
and all of it per follower and source, of what that follower follows; what a follower holds at
the end; bulk frames.

| Band O neighbourhood | First | All | Held | Frames |
|---|---|---|---|---|
| One series | 4.8 | 19.4 | 1,283 kB | 13,003 |
| Four albums, followed whole | 4.9 | 22.4 | 1,283 kB | 13,092 |
| One album each | 6.0 | **18.3** | **322 kB** | 13,200 |
| Four albums with covers | 6.6 | 23.1 | 1,412 kB | 14,380 |
| One album each, with its cover | 7.1 | 19.4 | 355 kB | 14,516 |

| 15 km² | Band O: all, frames | Band L: all, frames, worst world delivered |
|---|---|---|
| One series | 34.1, 113,479 | 49.7, 326,415, 100 % |
| Four albums, followed whole | 35.9, 117,262 | 57.9, 306,534, 100 % |
| One album each | **23.1**, **90,992** | 52.0, 245,597, **86 %** |
| Four albums with covers | 36.4, 128,053 | 64.2, 340,829, 100 % |
| One album each, with its cover | 27.6, 98,386 | 67.8, 280,261, 86 % |

A follower of one album of four holds a quarter, and where its cell is all it needs it has its
album sooner, since the carousel passes fewer pieces for fewer listeners: 22 % fewer frames
across 15 km² of band O. Its first piece comes later because it is the first of its own album,
which may be the third the source uploads. Splitting a source into collections costs a little in
itself, 1.3 to 3 minutes in the neighbourhoods and across 15 km² of band O, 8 across 15 km² of
band L (the median piece 33.9 against 38.6 minutes there). A cover arrived after 5 to 9 minutes,
after 18 to 26 across 15 km² of band L, and cost about 10 % more frames at 16 kB.

The 86 % is the open question this leaves. In two band L worlds of 15 km², the listeners of one
album in a far part of the network never got its pieces: their announcers and they held the root
and wanted the pieces, and no announcer or holder within reach had them. Content crosses cells
only through nodes that carry it (PROTOCOL.md §4), and with each listener carrying one album of
four, the chain of carriers for an album broke where a whole channel's never did. Carrying every
collection manifest of a followed channel, so that a cell can at least read what its neighbours
list, changed nothing (86 to 87 %): what was missing was carriers of the pieces. Whether a node
should relay collections of a channel it follows but does not listen to, and how much, is the
other side of ABUSE.md's bounded generosity, and the next question.

### 16.7 The nine scenarios, the living network, the attacks, the size sweep

| Scenario | Main: median, bulk frames | Collections | Frames |
|---|---|---|---|
| Band O neighbourhood | 7.3 min, 6,596 | 7.5 min, 6,600 | 0 % |
| Band L neighbourhood | 9.6 min, 13,991 | 9.6 min, 14,168 | +1 % |
| Band O, 15 km² | 8.6 min, 38,366 | 8.7 min, 39,495 | +3 % |
| Band L, 15 km² | 21.7 min, 114,942 | 22.2 min, 116,455 | +1 % |
| ESP-NOW | 11.9 min, 23,057 | 12.0 min, 23,899 | +4 % |
| Two clusters, band L | 16.4 min, 5,811 | 16.5 min, 5,910 | +2 % |
| LoRa only, 5 km | 47.1 min, 1,322 | **46.0 min**, 1,322 | 0 % |
| Town, band O | 14.1 min, 147,381 | 14.8 min, 151,252 | +3 % |
| Town, band L | 30.5 min, 360,637 | 30.4 min, 364,398 | +1 % |

Every scenario delivered 100 % in every world. A second level of manifest costs a few tenths of a
minute and up to 4 % more frames where there are many cells, and nothing in a cell of its own;
across 15 km² of band L five of eight worlds were slower, by up to 2.2 minutes, two faster.

In the living network every world held every current window in both bands. A bulletin is now a
root, a collection manifest and a piece, so uploads rose by 38 % (band L 181 to 251 per world,
band O 79 to 109), each extra one a few symbols. Newcomers caught up faster than on main (§16.4).
With nodes coming and going, more announcers stepped up where nodes went off and on, and
followers changed announcer more often: role changes after the first hour 216 on average in band
L against 193, the most in one world 380 against 276; without nodes coming and going none, as
before. Against one false announcer 99.4 % was delivered (98.7 % in the worst world), against
five 97.1 % (93.1 %), against five claiming the maximum 96.3 % (93.0 %), within a point of main.

The size sweep of §13 moved by a few minutes either way at most sizes. In the band O
neighbourhood it showed a pattern that main has too: a world either has all of an hour's music
after about 20 minutes or 10 to 25 minutes later, and collections changed which worlds fall
which way (at 70 kB, five of eight late instead of one). In a late world one of the two sources
uploaded at a third of its rate for 25 minutes, deferring to a busy channel 4,576 times (41 times on
main): the other source's frames, sent back to back, left it no gap. That is the chaining of §14.2
between two holders rather than an announcer and a holder, and it is open.

## 17. Relaying for another cell's listeners

§16.6 left one question open: across 15 km² of band L, with every listener following one album of
four, two of eight worlds left 9 to 14 % of an album's listeners without it. The trace of the worst
world: the album's source was the announcer of its own cell, which does not upload; its carousel
passed the album once, for the few listeners there, only three followers caught it, and it never
left that cell. Twelve announcers elsewhere asked for it 1,873 times in twelve hours, and no node
in their reach held it. Content crosses cells only through nodes that carry it
(PROTOCOL.md §4), and with each listener carrying one album of four the chain of carriers broke.

Five variants were measured, on single-album follows across 15 km² of band L and band O. Medians
of all of what each follower follows, minutes; frames; what a follower holds; the worst world's
share of listener-piece pairs that completed in twelve hours:

| Variant | Band L | Band O |
|---|---|---|
| None (§16) | 52.0, 245,597, 473 kB, 86.1 % | 23.1, 90,992, 482 kB, 100 % |
| A follower keeps what it overhears of a channel it follows | 42.6, 237,768, 1,585 kB, 86.9 % | |
| ... and asks for every collection manifest of it | 42.4, 239,034, 1,610 kB, 86.9 % | |
| ... and an announcer passes what another asks for and holds | 42.3, 240,130, 1,611 kB, 86.9 % | |
| The first two, and a follower takes on every ask it hears from another announcer | 42.7, 292,452, 1,895 kB, 100 % | 27.1, 107,867, 1,914 kB, 100 % |
| Instead of keeping what it overhears, it fetches only what another announcer asks for | 44.6, 304,638, 1,578 kB, 100 % | 27.9, 125,246, 1,528 kB, 100 % |
| ... only what it asks for its own listeners | 44.3, 302,497, 1,480 kB, 100 % | 26.8, 111,935, 1,304 kB, 100 % |
| ... and only once nobody has met that ask for `T_want_min` | **47.5, 292,287, 1,000 kB, 99.6 %** | **24.9, 96,225, 664 kB, 100 %** |

Keeping what a follower overhears did not help: the followers in reach of other cells were at the
edge of the source's cell and had caught the one pass only in part, and what a node does not want
it does not repair. Nor did an announcer passing what a neighbour asked for: the source's
announcer heard none of the asks. What worked was a follower taking on the want itself: it asks
its own announcer, completes the piece and offers it to the cell that asked. Taking on every ask
cost what following the whole channel costs, in storage and in band O frames, because an
announcer asks for everything it serves, listened to or not. So an announcer now marks in its asks
what its own followers asked for (bit 7 of the phase byte, PROTOCOL.md §3.3), and a follower
relays only those, and only once nobody has met the ask for `T_want_min`, since most are answered
by a holder within a round. The remaining 0.4 % of band L were two nodes that were the announcers
of cells of their own and heard no other announcer at all: between announcers alone there is no
way across (PROTOCOL.md §9, question 11).

What relaying costs where nothing was missing, single-album follows again, all of it in minutes,
frames, what a follower holds:

| | Without | With |
|---|---|---|
| Band O neighbourhood | 18.3, 13,200, 322 kB | 18.3, 13,200, 324 kB |
| Band L neighbourhood | 14.5, 28,666, 322 kB | 14.1, 29,282, 358 kB |
| Band O, 15 km² | 23.1, 90,992, 482 kB | 24.9, 96,225 (+6 %), 664 kB |
| Band L, 15 km² | 52.0, 245,597, 473 kB | 47.5, 292,287 (+19 %), 1,000 kB |

With covers the band L network went from 86.3 to 99.2 % in its worst world. Where every listener
follows whole channels nothing changes: six of the nine scenarios ran byte for byte as before and
the other three within a tenth of a minute (band L 15 km² 22.2 to 22.1 minutes, with ESP-NOW and
the band L town 0.1 to 1.2 % fewer frames); the living network held every window, newcomers
caught up as before (band L 9.1 minutes, the slowest 24.0 instead of 22.2), no role changed
without nodes coming and going, and the false announcers were within a point of §16. The size
sweep of §13 ran byte for byte as before except across 15 km² of band L, where every size took
as long or up to 3 minutes less.

## 18. Two uploaders under a duty cycle

In the band O neighbourhood of the size sweep a world either had all of an hour's music after
about 20 minutes or 10 to 25 minutes later, on main as in §16.7. In a late world one of the two
sources uploaded at a third of its rate for 25 minutes and deferred to a busy channel 4,576 times,
where in the same world before collections it had deferred 41 times. A holder that finds the channel
busy waits a random time from a window that doubles with each attempt, up to 12.8 seconds; the
other holder, sending back to back, always finds it clear and never waits. That is the classic
capture of carrier sensing with exponential backoff, between two holders that hear each other.

The announcer already divides its listening time into phases under polite access and where no
regulator caps the sender (§15.1). Under a duty cycle phases had been left out: with a phase per
grant a lone upload was held to one phase in K and took K times as long (§9.6), and after phases
went to holders (§13.2), which gives a lone holder the whole cycle, they were left out as no
longer needed (§14.3). They are needed: with phases under the duty cycle too (PROTOCOL.md §4),
every world of the band O neighbourhood had the hour in 20.4 to 22.7 minutes at every size from
14 kB up, the first piece came sooner at large sizes (423 kB pieces: 15.7 to 12.5 minutes), and the
15 km² band O network gained at every size:

| One hour of music | Band O neighbourhood: all of it | Band O, 15 km²: all of it, frames |
|---|---|---|
| 7 kB pieces | 31.8 → 31.5 min | 55.6 → **45.4** min, −24 % |
| 14 kB | 26.8 → **20.9** | 49.3 → 46.3, −14 % |
| 70 kB | 28.1 → **20.6** | 44.1 → 40.8, −1 % |
| 141 kB | 29.1 → **20.8** | 43.4 → 40.2, −1 % |
| 423 kB | 24.8 → **20.5** | 38.9 → 32.6, −3 % |
| One piece | 20.6 → 20.4 | 40.2 → **30.0**, −2 % |

In the nine scenarios the band O neighbourhood went from 7.5 to 7.3 minutes, the 15 km² band O
network from 8.7 to 8.4 with 4 % fewer frames, and the band O town held at 14.9 minutes instead
of 14.8 with 11 % fewer frames; every other scenario, band L, ESP-NOW and LoRa, ran byte for byte
as before, since there phases were already used. With collections (§16.6), the band O neighbourhood
had one album each in 15.1 minutes instead of 18.3 and four albums in 16.9 instead of 22.4; across
15 km² of band O four albums took 32.5 minutes instead of 35.9 with 9 % fewer frames, and one album
with its cover 28.4 instead of 26.9 with 10 % fewer. The living network held every window in both
bands with the same newcomers, band O with 5 % fewer uploads, no role changed without nodes coming
and going, and the false announcers delivered as before.

## 19. Earlier pieces first

A listener plays a collection from its first piece. In the 15 km² band L network with an hour of
music per source in 14 kB pieces (one minute each), the first piece of a source's programme
took the median listener 31 minutes to get (the mean over eight worlds); in one world half the
listeners of one source got it only after 78 minutes, while that source's own cell had it after 17.
Following that piece through the world showed two causes, neither of them radio.

- *The order of asking.* In band L an announcer asks in sets, four to a frame (PROTOCOL.md §3.3),
  and the sets went in the order of manifest ids. One announcer had heard the source offer the
  piece at 10 minutes and wanted it, yet between 17 and 43 minutes its frames named that source's
  pieces in 3 open asks, against 41 and 130 for the other two sources: the sets of the lower ids
  and the grants made on them filled every frame.
- *The order of uploading.* A holder granted the piece at 42 minutes uploaded it at 58, after 33
  later pieces of the same programme that earlier grants had brought: a holder sorted what one
  grant brought, smallest first, and lined it up behind everything before it.

Offers lost in the rendezvous, where many hidden holders answer one announcer at once, were the
first suspect; counted at the announcers that wanted what was offered, 1,113 of 15,204 offers
collided in the meeting dwell (7 %), too few to matter.

To measure what a listener waits for, the ensemble report now also gives, per listener and
collection, when the first piece arrived and the **playback start**: the earliest moment it could
start the collection at its first piece and play it through without waiting, given how long each
piece plays (sim/README.md).

### 19.1 The earlier place first

A piece's place is its position in the collection manifest, 0 for every piece of singles and for
anything that is not a piece, and the earlier place goes first wherever a node chooses
(PROTOCOL.md §2, §4): asking by name, the earlier place first and among equal places the most
listeners per byte; a holder uploads the earlier place first, whatever grant brought it, and
among equal places the smallest first; asking in sets, the sets went by the earliest place each
names, and among equals the one asked for longest ago. In the 15 km² band L network with 14 kB
pieces playback could start after 31.9 minutes instead of 48.8 (37.5 with the order of asking
alone), and in the nine scenarios of §9.3 sooner in every one.

### 19.2 Side by side or one after another

With three sources publishing four albums each (20 pieces of 42 kB music and 22 kB speech per
source, §16.6), the same rule made band L worse: listeners following every album could play one
through after 49.4 minutes instead of 38.9, those following one album per source after 50.1
instead of 43.8, with up to 11 % more frames. Twelve albums asked for side by side shared the
network's uploads, and none arrived fast enough to be played as it came; asked for in the order
of manifest ids, the first ones were complete early and could be played while the others came.
Which is better depends on whether what is in flight keeps up with playback, and no node knows
the capacity of its network.

It does not have to. What is flowing is not asked for (PROTOCOL.md §4), so a collection whose
uploads run gives up its place among the sets by itself. The rule became: every collection gets a
set before any gets a second; collections in progress, a piece of which arrived in the last
`T_want_min` or is granted, go before the others; within that the earliest place first, then the
set asked for longest ago. As many collections are in flight as arrive, and one that stopped
arriving, because nobody in reach holds the rest, takes turns with the others again. (Counting a
collection as begun as soon as a piece of it was held, without the time limit, gave about the
same numbers where it was measured, four albums after 39.1 minutes, but let four begun
collections that nobody could serve fill every frame for good.)

| Band L, 15 km², 3 sources × 20 pieces | Playback start, median: main → side by side → in progress first | First piece | Frames |
|---|---|---|---|
| One programme per source | 36.9 → 24.1 → **24.4** min | 33.5 → 21.4 | 0 % |
| Four albums, all followed | 38.9 → 49.4 → **38.5** | 36.7 → 26.0 | +6 % |
| Four albums, one followed per source | 43.8 → 50.1 → **38.3** | 42.3 → 31.6 | +6 % |
| Four albums with covers | 45.6 → 52.7 → **44.8** | 43.8 → 34.6 | +6 % |
| Four albums with covers, one followed per source | 53.0 → 59.0 → **47.5** | 51.1 → 41.9 | +5 % |

All of what a listener follows arrived about as before: one programme per source 49.4 → 51.4
minutes at the median, four albums 57.4 → 56.5, one album per source 47.5 → 45.0. The frames
are the cost. In band O and in the neighbourhoods every one of the five could start sooner,
or within 0.3 minutes: in the 15 km² band O network one programme per source after 8.3 minutes
instead of 21.9 with 10 % fewer frames, four albums after 18.0 instead of 22.7; in the band O
neighbourhood after 5.2 instead of 11.2 and 6.2 instead of 10.4; in the band L neighbourhood
10.2 instead of 12.2 and 10.4 instead of 12.0.

### 19.3 The nine scenarios

In the nine scenarios of §9.3 (eight worlds each, 42 kB music and 22 kB speech alternating in
one programme per source, 8 pieces), playback could start sooner in every one:

| Scenario | Playback start, median | 90th percentile | First piece | All of it, median | Frames |
|---|---|---|---|---|---|
| Band O neighbourhood | 7.7 → **5.2** min | 7.7 → 5.2 | 7.7 → 5.2 | 10.6 → 10.5 | 0 % |
| Band L neighbourhood | 10.2 → 9.5 | 16.7 → 14.2 | 10.2 → 9.5 | 12.0 → 12.1 | +1 % |
| Band O, 15 km² | 9.4 → **6.1** | 16.1 → 13.5 | 9.3 → 5.7 | 13.4 → 13.3 | 0 % |
| Band L, 15 km² | 22.6 → 19.6 | 34.0 → 31.6 | 21.8 → 18.7 | 26.5 → 27.6 | +2 % |
| ESP-NOW | 13.0 → 12.5 | 17.8 → 15.3 | 12.8 → 11.5 | 14.7 → 15.6 | −1 % |
| Two clusters, band L | 18.0 → 16.6 | 20.5 → 19.8 | 18.0 → 16.6 | 18.9 → 18.7 | 0 % |
| LoRa across 5 km | 70.8 → 69.3 | 70.8 → 69.3 | 48.0 → **33.6** | 89.2 → 89.2 | 0 % |
| Band O town | 23.8 → **15.9** | 40.1 → 33.4 | 21.1 → **8.0** | 29.3 → 30.2 | −2 % |
| Band L town | 32.0 → **26.7** | 45.0 → 36.4 | 30.6 → 22.2 | 37.4 → 39.7 | +1 % |

What it costs. Most listeners per byte first had put a source's speech before the music it plays
between; now the programme comes in its order, so speech on its own arrives later (band O town:
music 21.7 → 16.6 minutes, speech 8.0 → 17.2, the median of each). And in band L the median
listener held all of a programme up to 2.3 minutes later (town 37.4 → 39.7); at the 90th
percentile within a minute of before (15 km² 37.1 → 37.8, town 48.9 → 47.6).

### 19.4 Piece sizes

With an hour of music per source in pieces of every size of §13, playback could start sooner
wherever there was an order to keep, by far most with small pieces under a duty cycle, where
most listeners per byte first had put the pieces of a programme in any order:

| Playback start, median (min) | Band O neighbourhood | Band L neighbourhood | Band O, 15 km² | Band L, 15 km² |
|---|---|---|---|---|
| 7 kB pieces (30 s) | 28.0 → **4.7** | 11.8 → 11.9 | 37.4 → **8.4** | 69.3 → **46.4** |
| 14 kB (1 min) | 17.6 → **4.8** | 11.4 → 10.6 | 36.7 → **8.8** | 48.8 → **30.5** |
| 42 kB (3 min) | 14.9 → **5.2** | 10.7 → 10.4 | 34.2 → **12.5** | 35.2 → 27.7 |
| 141 kB (10 min) | 11.1 → 7.1 | 11.2 → 11.5 | 23.3 → 12.9 | 33.9 → 28.8 |
| 423 kB (30 min) | 12.5 → 12.5 | 15.1 → 15.6 | 22.4 → 21.4 | 44.7 → 43.8 |

In the band L neighbourhood the median moved by at most 0.8 minutes either way and the 90th
percentile came sooner at every size up to 423 kB (7 kB pieces 46.8 → 22.3 minutes). The last
piece arrived about as before, within 4.1 minutes either way, and the frames changed by −7 to
+8 % (band L neighbourhood with 7 kB pieces −7 %, band O neighbourhood with 7 kB pieces +8 %).

### 19.5 The living network and the attacks

The living network of §7.6 (daily 22 kB bulletins, with nodes that come and go as in §7.7.2)
held every window in both bands with the same medians (band L 5.4 minutes, band O 1.8),
newcomers caught up in 9.4 minutes in band L instead of 9.1 and in 8.1 instead of 7.9 in band O,
and the false announcers changed nothing beyond noise (midL with one lure 99.6 % delivered
instead of 99.5, with five maximum-claiming lures 96.2 % instead of 96.4). One bulletin in one
band O world under a WANT flood reached its listeners after 7.2 minutes instead of 1.8: the
collection manifest that named it, two symbols uploaded with its root, lost one symbol in a
collision with another node's gossip frame at the station, and with one symbol of two a node is
below the 80 % at which it repairs by NACK (PROTOCOL.md §4), so it waited for the station's
next regular round of asking, five minutes later. That was not the order of pieces; §20 traces
it and fixes it. (This paragraph first blamed a grant that lapsed; no grant in these runs did.)

### 19.6 Tried and not taken

- *Smallest first across a holder's whole queue* instead of the earlier place: the same where all
  pieces are of one size; in the nine scenarios the 15 km² band L network started after 21.8
  minutes instead of 20.2 and the band L town after 30.1 instead of 27.3 (both against the
  earlier place first with sets side by side). §14.2 had found a whole queue smallest first worse
  for speech in the band O town; the earlier place first is not that rule.
- *The earlier place only between objects that serve as many listeners per byte*, when asking by
  name: about half the gain (band O town 20.1 minutes, neighbourhood 6.9).
- *The carousel by the earlier place first too*, whatever the size: band L up to a minute sooner
  (two clusters 15.6 instead of 16.6 minutes, LoRa's first piece after 19 instead of 34), the band
  O town 7 % more frames than with the carousel as it is, from twice the repeated passes in two of
  eight worlds. The carousel keeps the most listeners per byte first, the earlier place between
  equals (PROTOCOL.md §4).

## 20. A small object short of one symbol, and an announcer left behind

### 20.1 Repairing what is short of one symbol

The slowest bulletin of §19.5 (one band O world under a WANT flood, 7.2 minutes instead of 1.8)
was traced with the simulator's upload traces (sim/README.md). Its collection manifest, two
symbols, was uploaded to the station with the root that named it, and the second symbol collided
at the station with another node's gossip frame. With one symbol of two the station was below the
80 % at which a node repairs by NACK, so the manifest had to be asked for again; a station leaves
out of its ask whatever brought a symbol in the last `T_nack_stall`, and its next round of asking
came at its regular cadence, `T_gossip`, five minutes later. No grant lapsed: in the 80 runs of the
living network not one grant lapsed after its upload had begun.

Asking as soon as an upload stops, `T_nack_stall` after its last symbol and once per stop, on
carriers that do not hop, brought that bulletin after 4.2 minutes, but cost the band O town 12.6 %
more frames, and listeners there could start playing 1.1 minutes later: under a duty cycle an
upload that pauses for a minute has seldom stopped, its holder is waiting for its budget, and
asking again brought in a second holder. Not taken.

The rule taken is narrower. A node repairs by NACK what it holds at least 80 % of, **or all of but
one symbol** (PROTOCOL.md §4). Under the fraction alone an object of two to four symbols, a
collection manifest or a root, could not be repaired at all: four symbols of five are already
80 %, but one of two is 50 %. A NACK for one symbol is the smallest repair there is, and it reopens
no ask. The slow bulletin came after 2.4 minutes, and under the WANT flood the slowest bulletin
of any world came after 3.0 minutes instead of 7.2. A follower that fetches its channel again and
loses one of the collection manifest's two symbols holds the manifest after 2.3 minutes instead
of 10.1, the `T_want_min` it waited before asking again (smoke test
`a_small_object_short_of_one_symbol_is_repaired`). In the nine scenarios, the collection variants
and the size sweep nothing moved beyond the spread of the worlds, the band O town over sixteen
worlds included (playback start 17.4 → 17.7 minutes, frames +2 %); the false announcers delivered
as before (99.5, 97.1 and 96.3 % against 99.6, 97.4 and 96.2).

### 20.2 An announcer that falls behind, and stays behind

Validating that, one newcomer in a band O world took 67 minutes to hold the channels it followed,
where every other newcomer took at most 20. The traces showed a chain. Churn took the cell's
announcer, and the node elected in its place had never followed one of the newcomer's channels.
On hearing its followers' corrections (PROTOCOL.md §2) it asked for the channel's current root;
then it heard another cell's announcer, itself behind, announce an older root, and since an
announcement is a hint and the latest heard replaces one still pending, it fetched that one and
adopted it. Its followers had heard it ask for the current root, took it to be fetching it, and
held back their corrections for `want_ttl`, an hour. The newcomer knew nothing better.

What an announcer asked for now counts for `T_want_min` when a follower decides whether to correct
it, since an announcer that is fetching asks again every round; the same world's newcomers caught
up within 20 minutes. The rule that the latest announcement replaces a pending one stays: keeping
the higher one would let a false announcement of the highest seq hold a channel for good
(ABUSE.md), and with corrections renewed every `T_want_min` an announcer that took an old root is
put right within that.

Over sixteen worlds per band, a newcomer held everything it followed after 8.0 minutes on average
in band O (7.9 on main) and 9.1 in band L (9.1). One band L world had a newcomer at 62 minutes,
where main had none: its cell's announcer asked for a new root every five minutes for an hour
and no holder of it could hear the ask; the newcomer had it only by an excursion (§12.3). Main
elected another announcer in that world, and the limit is the known one of a cell that no holder
can reach. The report of the living network now also gives the newcomers' time counting only the
time they were switched on, which ruled out the first suspicion, a newcomer switched off for an
hour.

## 21. Sparse interests across small cells: an open problem

Every scenario so far had each cell follow nearly every channel. The living network of §7.6 was
spread over 15 km² with 100 nodes and 2 stations, 24 channels, each node following 2 (four worlds,
48 hours, daily 22 kB bulletins), to see what a cell does with content nobody in it follows.

| Spread over 15 km², 24 channels, 2 followed | Band O | Band L |
|---|---|---|
| Cells | 3 to 4 | 13 to 16 |
| Bulletins delivered within one publication period | 99.6 to 99.8 % | 69.7 to 85.8 % |
| Uploads | 812 to 1,064 | 4,121 to 6,635 |
| Excursions | 0 to 9 | 300 to 328 |
| Role changes after the first hour | 0 to 8 | 688 to 1,865 |

Band O, with its few large cells, delivers. Band L, with many small ones, delivers late or not at
all: in one world the median listener had a bulletin after 1 to 11 hours, and one bulletin reached
nobody in its period. Traced in one world, the chain is this. A channel has about eight followers
in the whole network, so most cells between its source and a listener have none. Every announcer
fetches every channel it hears of, but announcers do not upload (PROTOCOL.md §4) and followers
relay only for channels they follow (§17), so content crosses a cell without followers of its
channel only when a listener beyond it goes on an excursion to an announcer that has it, after
`T_excursion` (40 minutes) and one cell at a time. A follower that waits that long without one
symbol of what it wants also takes its honest announcer for one that serves nothing (§12.4),
ignores it for `want_ttl` and leads its own cell for that hour: one node cycled follower, candidate,
announcer, follower every two hours for a whole day, and most of the role changes above are
excursions and such cycles.

Tried so far:

- *An announcer carries only what its cell asked for* (ABUSE.md item 4, bounded generosity): band L
  delivered less (66.2 to 79.9 %) and band O needed 37 % more uploads. The announcer's generosity is
  what lets an excursion find content in a cell that does not follow it; without it there was
  nothing to visit. Not taken. (Taken in §27, with relaying for any channel, which brings content
  where it is asked for without it.)
- *Shorter excursions*: with `T_excursion` at 20 minutes band L delivered 79.5 to 92.5 %, at 10
  minutes 83.7 to 88.5 %, with up to 36 % more uploads. Better, and still far from band O.

Not tried yet, in the order they look worth trying:

1. **Announcers that upload to neighbouring announcers**: a station carries every channel it
   hears of already; answering another announcer's ask for its listeners (the marked asks of
   §17), in the rendezvous where both listen, would make the stations a backbone, one hop per
   ask instead of one excursion per listener per hop. Announcers were stopped from uploading in
   §9.6 because a granted announcer never sent and its carousel HAVE silenced the followers that
   would have offered; an explicit offer, made last, after any follower's, would avoid both.
2. **Relaying for channels a node does not follow**: a follower near a cell border that hears
   another cell's marked ask could fetch the collection manifest the ask's set names from its
   own announcer, which carries it, and relay as for a followed channel (§17), at the cost of
   battery-powered nodes carrying what they do not listen to. (Done in §27: band L delivered 99.7 %
   instead of 92.0 %, with a carry budget for what nodes hold for others.)
3. **No shun for an announcer that is asking**: an announcer that lacks what its follower wants
   but asks for it, or for the manifest that names it, is honest and unable, not false; the
   follower should go on an excursion, not leave its cell leaderless for an hour. This needs care,
   because a false announcer could ask forever (ABUSE.md, election capture). (Measured in §25, the
   other way round: a follower that leaves an announcer that asks and cannot get delivers more.)

An upper bound for option 1, measured with an oracle (branch `experiment/backbone-oracle`, not
merged): every minute each announcer is handed, for free and at once, what an announcer it can hear
holds. Handed only what its own followers asked for, band L delivered 69.0 to 88.5 % (against 69.7
to 85.8): content cannot cross a cell whose announcer nobody asks. Handed everything it wants, which
by its generosity is every channel it hears of, 85.4 to 90.2 %, with 1,073 to 2,910 transfers per
world; excursions fell only from about 300 to 228 to 289. So even a perfect backbone leaves a tenth
of the bulletins late. At the end of such a world 14 listener-bulletin pairs were still missing,
and in them the listener did not know the bulletin existed: its announcer had neither the
channel's newest root nor any announcer in reach that had it. Roots travel only through nodes
that follow the channel or announcers that fetched them, and in a sparse band L network neither
reaches every cell. Whatever comes next has to carry the roots, not only the pieces.

## 22. Roots on the control carrier, and what they exposed

§21 ended on roots: a listener whose cell never had a channel's newest root does not know the
bulletin exists. The long-range control carrier already reaches every cell with
`MANIFEST_ANNOUNCE`, and a root manifest is a symbol or two. So the first time a node announces a
root it holds there, the root's symbols follow on the control carrier, with those of the
collection manifests the root flags as changed, unless someone sent them there in the last
`T_want_min` (PROTOCOL.md §2). Every node in range that follows the channel keeps them, and every
announcer.

The simulator lets a node hear its control carrier and its bulk carrier at once; an SX1262 has one
radio and has to divide its listening between LoRa and GFSK (sim/README.md). The numbers below
assume it hears both.

### 22.1 Sparse interests

The network of §21 (100 nodes over 15 km², 2 stations, 24 channels of which each node follows 2,
48 hours), now on eight worlds:

| 15 km², 24 channels, 2 followed, eight worlds | Main, band L | Now, band L | Main, band O | Now, band O |
|---|---|---|---|---|
| Bulletins delivered within one publication period | 69.7 to 85.8 % (mean 79.2) | 82.2 to 92.6 % (mean 88.0) | 99.6 to 99.9 % | 99.6 to 99.9 % |
| Uploads | 4,121 to 6,635 | 2,862 to 4,359 | 804 to 1,064 | 529 to 703 |
| Excursions | 300 to 329 | 151 to 191 | 0 to 9 | 0 to 6 |
| Role changes after the first hour | 688 to 2,069 | 362 to 1,040 | 0 to 23 | 0 to 10 |

Every world delivers more, by 2.7 to 15.4 points; the oracle of §21, a perfect backbone handing
every announcer whatever a neighbour held, reached 85.4 to 90.2 % on four worlds. The control
carrier carries 1,142 to 1,687 root symbols per band L world in 48 hours and is no busier for
it: in two worlds its mean airtime share fell from 0.028 and 0.031 % to 0.019 and 0.022 %, and the
busiest node's from 0.27 and 0.26 % to 0.13 %, because fewer role changes mean fewer
announcements; in band O it stayed at 0.15 % for the busiest node. Band L
still loses a bulletin in eight; the rest of §21 (a backbone for pieces, relaying channels not
followed, no shun for an announcer that is asking) remains to try.

### 22.2 What every cell asking at once exposed

With roots everywhere at once, every cell asked the sources for their pieces at once, and a band O
network over 15 km² took longer for large pieces: the first piece 0.8 to 6.8 minutes later from
70 kB up. Traced world by world, the cause was not the roots but faults on main that the
simultaneous asking made bigger. Each is fixed as a rule (PROTOCOL.md §4, §5.4), each also helps
without the roots:

- **An upload ends when its announcer holds the object.** In one world with 211 kB pieces, 13.5 %
  of all upload frames on main went to announcers that already held the object, 24 % once roots
  travelled: an announcer completes an object from what it overhears of uploads to another cell's
  announcer, and the holder uploads it whole anyway. The holder now ends the upload on the
  announcer's HAVE, and the announcer lists what it completed in the last `T_grant` first in every
  HAVE, at most half of it: in the rotation alone, a holder that missed the one HAVE after the
  completion uploaded 1,081 frames to an announcer that had held the object for four minutes.
  With this rule and the next, 3.0 % of the upload frames went to an announcer that held the
  object.
- **A repair answer leaves out what its holder sent since.** A granted uploader answers NACKs at
  once, but behind the upload they came during. In that world the upload brought thirteen
  followers everything they had listed before the answers began, and each answer still sent its
  150 symbols.
- **An upload keeps out of the phases other announcers gave away.** In a world with 846 kB pieces,
  7.5 % of the upload frames on main were lost at their own announcer to an upload to another
  cell's announcer, by a holder they could not hear; 15.4 % once all three sources were granted at
  once. A holder that hears another announcer's grants now keeps its own uploads out of those
  phases: 6.9 %, and the first piece of that world came after 25.2 minutes instead of 32.4 on main
  and 44.3 before the rule.
- **A holder's own change of announcer ends nothing.** A holder that began uploading the first
  piece of a programme to the next cell and then followed a new announcer of its own, during the
  first election, dropped the upload and never said so; the grant ran idle for `T_grant`, the new
  announcer did not ask for the piece it had overheard part of, and the first piece reached
  listeners last, after 29.0 minutes. Now the upload goes on, and in that world playback starts
  after 14.9 minutes (17.1 on main).

Tried and not taken: *no offer to another cell while our own announcer still asks for the object*,
on the reasoning that every cell asking a source at once made the source serve the others first.
It did not make the band O network faster at 141 or 211 kB, cost 13 % more frames at 423 kB, and
made band L over 15 km² start up to 5.7 minutes later (141 kB pieces: 32.1 minutes with the rule,
26.4 without).

Removing it exposed two older faults:

- **A follower asks again when one want has stalled.** It asked again only when nothing it wanted
  had moved for `T_want_min`. In one band O world a follower held 79.6 % of one object, just short
  of a repair, while another trickled in from a neighbouring cell a few symbols every ten
  minutes, and it stayed silent for hours; its announcer had long stopped passing the object, and
  three announcers' carousels ran at the full duty cycle for twelve hours, 540,000 frames instead
  of 110,000, with the 90th percentile of playback at 571 minutes. A follower now asks again when
  nothing has moved for `T_want_min`, or one want has not for twice that: 60 minutes and 150,000
  frames in that world. Asking whenever one want had waited `T_want_min` was about as fast and cost up
  to 8 % more frames (band O, 423 kB; band O neighbourhood, 14 kB).
- **Content is paced with jitter.** Two announcers that cannot hear each other sent every frame
  together, 222 ms apart: in one band O world a follower between them lost 2,533 of the 2,615
  frames of one object its announcer sent after the first hour, and two followers still lacked
  objects after twelve hours (99.93 % delivered). The pacing wait now gets a random part of itself
  added, up to half; the budget accrues meanwhile, so the rate stays. That world delivered 100 %.

### 22.3 The false announcer, again

Against five false announcers (lures that claim everything and serve nothing; ABUSE.md), the roots
made the band L network worse: playback started after 48.7 minutes instead of 38.9. On main a
captured follower lacked the roots, which the lure did not list, and an excursion for them took
it to an honest announcer after 40 to 61 minutes (traced in one world: at 61 minutes it wanted
three roots and nothing else); with the roots it holds the manifests and
wants only pieces, which the lure claims, so only the evidence of PROTOCOL.md §5.2 frees it, and
that evidence waited for a channel on which nobody at all had sent for `T_excursion`. In a busy
network any frame of anyone else restarted that wait, and followers stayed 88 minutes. A follower
now asks its announcer for one symbol of a claimed want as soon as it has stalled for
`T_excursion`, and judges it by whether a symbol of that object arrives within `T_want_min`; only
the named announcer answers, so the answer is attributable. Captures now end mostly after 40 to
59 minutes. But a follower that leaves one lure follows the best announcer it hears next, which is
often another lure, and the attack still costs more than on main:

| 15 km² band L, eight worlds | Main | Roots, before the probe rule | Now |
|---|---|---|---|
| One lure: delivered, playback start, 90th percentile, frames | 99.5 %, 20.4, 41.7 min, 126,262 | 99.5 %, 18.0, 51.2 min, 124,395 | 99.2 %, 18.0, 41.3 min, 120,989 |
| Five lures | 97.1 %, 38.9, 89.0 min, 131,044 | 97.1 %, 48.7, 133.7 min, 162,739 | 97.1 %, 49.4, 126.6 min, 149,189 |
| Five lures claiming the maximum | 96.4 %, 42.8, 103.7 min, 130,873 | 96.5 %, 51.6, 204.0 min, 168,829 | 96.2 %, 50.1, 161.9 min, 149,326 |
| Five spoofers flooding WANTs | 100 %, 20.3, 33.1 min, 279,664 | 100 %, 18.0, 26.0 min, 262,150 | 100 %, 18.0, 26.0 min, 262,150 |

Under a WANT flood the probe rule took no honest announcer for a false one: no role changes after
the first hour in any world of the living network, as before (§12).

### 22.4 Everything else

The nine scenarios, eight worlds each (minutes; frames are bulk frames per world):

| Scenario | Playback start | 90th percentile | Frames |
|---|---|---|---|
| Neighbourhood, band O | 5.2 → 4.2 | 5.2 → 4.2 | −4.9 % |
| Neighbourhood, band L | 9.5 → 6.9 | 14.0 → 10.8 | −0.4 % |
| 15 km², band O | 5.9 → 5.9 | 13.7 → 12.3 | −13.2 % |
| 15 km², band L | 19.7 → 16.5 | 32.0 → 23.2 | −6.4 % |
| ESP-NOW neighbourhood | 12.1 → 7.6 | 15.4 → 11.9 | −0.7 % |
| Two clusters, band L | 16.6 → 14.5 | 19.8 → 17.1 | 0.0 % |
| LoRa only, 5 km | 69.3 → 69.4 | 69.3 → 69.4 | +0.2 % |
| Town, band O | 16.2 → 14.2 | 33.0 → 27.1 | −4.6 % |
| Town, band L | 26.6 → 23.6 | 36.5 → 35.4 | +0.9 % |

The size sweep of §13 (an hour of music per source in pieces of 7 to 846 kB, four scenarios):
playback starts sooner at every size in every scenario, by up to 7.2 minutes (band L over 15 km²,
7 kB), and the 90th percentile is lower everywhere but one point (band L neighbourhood, 141 kB:
16.8 → 17.2). The band O network over 15 km² needs 12 to 14 % fewer frames from 42 kB up. The
costs: with 7 and 14 kB pieces there, holding all of a programme takes 3 to 4 minutes longer
(48.3 → 52.5, 43.6 → 47.0) and 8 and 3 % more frames, though playback starts sooner and the 90th
percentile halves at 7 kB (42.1 → 20.7); band L with pieces of 211 kB and more needs up to 8 % more
frames.

The living network of §7.6 (eight worlds, 72 hours): the median bulletin after 4.2 minutes instead
of 5.4 in band L and 1.2 instead of 1.8 in band O with nodes switched off and on, 26 to 48 % fewer
uploads, and the slowest newcomer of eight worlds caught up after 18.6 minutes instead of 62.4 in
band L. With one WANT flooder, one spoofer or one lure, the same or better on every line.

Collections (§16): playback starts sooner in sixteen of twenty variants and as soon in one. Over
15 km² in band O, a programme in four collections starts 1.2 minutes later (18.8 → 20.0), with
covers 0.4 minutes later, and one collection followed of four, with covers, 2.2 minutes later
(17.7 → 19.9), each in fewer frames (−15, −20 and −9 %).

### 22.5 Open

- A follower that shuns a lure follows the next best announcer it hears, often another lure:
  against five lures, playback still starts ten minutes later than on main.
- The SX1262's one radio: what the root push costs in missed GFSK frames while a node listens to
  LoRa has to be measured on hardware (Phase 1).
- Band L with sparse interests still delivers 82 to 93 % within a period.

## 23. One radio for two carriers

Until now the simulator let a node hear the LoRa control carrier and the GFSK bulk carrier at
once. An SX1262 cannot: it receives LoRa or (G)FSK, the packet type changes only in standby, and
the other mode's settings are lost on the way (SX1261/2 datasheet rev 1.2, §13.4.2); the change
is quick, standby to receive taking 83 µs (Table 8-2), and LoRa channel activity detection lasts
1 to 16 symbols plus about half a symbol (§6.1.5). The baseline network is made of such nodes, so
the two-receiver numbers of §21 and §22 were an upper bound. The lower bound: nobody hears the
control carrier at all.

| Eight worlds each | Two receivers (§22) | Control carrier never heard |
|---|---|---|
| Band L neighbourhood: playback start, 90th percentile | 6.9, 10.8 min | 9.4, 23.8 min |
| Band L over 15 km² | 16.5, 23.2 min | 22.5, 56.8 min |
| ESP-NOW neighbourhood | 7.6, 11.9 min | 12.5, 44.4 min |
| Two band L clusters | 14.5, 17.1 min | 28.0, 36.6 min |
| Band L town | 23.6, 35.4 min | 34.7, 72.0 min |
| Sparse interests, band L: delivered within a period (mean) | 88.0 % | 78.8 % |

So the control carrier matters even within one cell, where it brings every node the roots at
once, and a one-radio node has to divide its time between the two.

### 23.1 The control window

Listening for LoRa briefly and often would cut into GFSK frames of 20 ms, so the window is common
instead: every node sends control-carrier frames only in a window of `T_ctrl_window` every
`T_ctrl_period`, a one-radio node listens on the control carrier only then and on the bulk carrier
otherwise, and a bulk carrier on the same radio is silent in the window (PROTOCOL.md §3). ESP-NOW
and IP run on another radio and are not held. Four settings, eight worlds each, measured before the
changes of §23.2:

| Window | Time held | Sparse band L, mean delivered | Band L over 15 km²: start, p90 | Band O town: start, p90 |
|---|---|---|---|---|
| 1 s every 30 s | 3.3 % | 88.0 % | 17.6, 25.1 min | 15.8, 30.5 min |
| 2 s every 30 s | 6.7 % | 86.8 % | 16.9, 24.3 min | 15.5, 32.0 min |
| 2 s every 60 s | 3.3 % | 88.4 % | 17.2, 24.0 min | 15.5, 29.6 min |
| 4 s every 60 s | 6.7 % | 88.8 % | 17.8, 24.4 min | 15.5, 31.6 min |

All four keep nearly everything the control carrier brings. The draft takes 4 s every 60 s: the
most room for clocks that disagree and for a window shared by many nodes. With it the busiest
node of a sparse band L world spent 0.13 % of its time on the control carrier, as with two
receivers; with 1 s every 30 s, 0.16 %. Where the window falls did not matter either: windows every
50 or 100 s placed so that they never meet the meeting dwell of band L were no better than the
draft, whose window meets one in five.

### 23.2 What the window exposed

- **A root's push fits one window.** The push of §22 sent a root's collection manifests whole.
  With 7 kB pieces a programme of 120 pieces has a collection manifest of many symbols, and
  pushed at 4 s a minute it reached a band L neighbourhood after 12 to 14 minutes instead of 4.2;
  its source counted it delivered when it pushed it and no longer offered it in its own cell.
  Now a push carries the root and only as many of the collection manifests as fit one window (about
  ten symbols at SF7); the rest go the usual way: 5.4 to 5.6 minutes in that world, and playback
  in band L neighbourhoods with 7 kB pieces starts after 10.8 minutes instead of 16.9 (8.3 with
  two receivers).
- **The phase count an announcer gives out counts only grants it has named.** In the ring of
  six hidden uploaders (smoke test `hidden_uploaders_take_turns_at_their_announcer`), the shifted
  timing exposed a fault latent on main: an announcer whose asks were full held a grant it had
  not yet sent, counted its phase in the beacon's `upload_phases`, and the uploaders that had
  heard that beacon and one that had not divided the time into six and five phases and collided,
  frame for frame: 5.3 % of the upload frames, against 2 % allowed. Counting only grants named
  in a WANT, and letting an uploader take the count also from the grants and repair phases it
  hears its announcer give out, 1.6 %.

### 23.3 Against main

Main before §22 assumed two receivers too; it is the row a user of main would have seen.

| Eight worlds each: playback start, 90th percentile (min) | Before §22 | §22, two receivers | Now, one radio |
|---|---|---|---|
| Neighbourhood, band O | 5.2, 5.2 | 4.2, 4.2 | 4.1, 4.3 |
| Neighbourhood, band L | 9.5, 14.0 | 6.9, 10.8 | 7.5, 11.3 |
| 15 km², band O | 5.9, 13.7 | 5.9, 12.3 | 5.1, 12.2 |
| 15 km², band L | 19.7, 32.0 | 16.5, 23.2 | 17.4, 24.3 |
| ESP-NOW neighbourhood | 12.1, 15.4 | 7.6, 11.9 | 7.8, 12.9 |
| Two band L clusters | 16.6, 19.8 | 14.5, 17.1 | 14.8, 16.8 |
| LoRa only, 5 km | 69.3, 69.3 | 69.4, 69.4 | 69.4, 69.4 |
| Town, band O | 16.2, 33.0 | 14.2, 27.1 | 14.8, 29.3 |
| Town, band L | 26.6, 36.5 | 23.6, 35.4 | 23.7, 32.8 |
| Sparse interests, band L: delivered within a period (mean) | 79.2 % | 88.0 % | 88.5 % |

The one radio costs at most a minute in the matrix, and the sparse band L network keeps all of
§22's gain. The size sweep shows the cost more clearly: playback in band L over 15 km² starts 1
to 5 minutes later at every size (7 kB: 37.8 → 42.9 minutes), in band L neighbourhoods up to 2.8
minutes later with pieces of 14 kB and less, and band O over 15 km² needs 10 to 14 % more frames
with pieces of 211 kB and more. Collections cost up to 2.8 minutes in band L. In the living
network with nodes switched off and on, band L is unchanged (median 4.2 minutes, 263 uploads)
and band O back where it was before §22 (1.8 minutes instead of 1.2), since a root now waits up
to a minute for its window.

Under five lures 96.0 % was delivered instead of 97.1 %. The simulated lure ignores the window,
so followers miss some of its beacons, leave it after three, and are taken back by the next
one: in the living band L network under one lure that made 136 to 499 role changes after the
first hour per world instead of 4 to 88, with the slowest bulletin earlier (42.6 minutes against
51.0). Under WANT floods and spoofed names, no role changes, as before.

### 23.4 Open

- **Clocks.** The window needs the time base the hop sequences already use (PROTOCOL.md §5.3,
  §6), to well within its 4 s; the simulator's clocks are perfect. How far apart offline cells
  drift is a Phase 1 measurement.
- **Window jamming.** A transmitter that fills every window blocks the control carrier at
  6.7 % airtime (ABUSE.md); before, that took all of it.
- **Stations with a concentrator.** An SX1302 receives LoRa and FSK at once and could listen to
  the control carrier always, relaying into its cell what it hears; the window rule keeps it
  silent there outside the window for the sake of the others.

### 23.5 What still limits sparse interests

With roots everywhere and one radio, sparse band L delivers 88.5 % of bulletins within their
period (eight worlds). The oracle of §21 on this code (branch `experiment/backbone-oracle-2`, not
merged), handing every announcer each minute, for free, what an announcer it can hear holds: only
what its own followers asked for, 88.9 %; everything it wants, 91.4 %. So even a perfect
backbone between announcers adds three points.

At the end of three worlds, the bulletins a follower still lacked were 46, 58 and 41; for 44,
49 and 37 of them the follower heard other nodes on the bulk carrier, but none of them held the
bulletin, and for none was the follower out of everyone's reach. The bulletin had stopped
where nobody follows its channel: a node carries what it listens to, and an announcer what its
cell asks for or what it can fetch from a holder in reach. What remains is a choice of principle,
not a tuning: nodes that relay channels they do not follow (option 2 of §21), at the cost of
carrying what nobody near them listens to, or pieces on the long-range control carrier, at four
seconds a minute.

## 24. A false announcer, found sooner

Against five false announcers (lures that claim everything and serve nothing) the band L network
over 15 km² started playback after 38.9 minutes before §22 and after 51.1 minutes with roots on
the control carrier and one radio (§22.3, §23.3). A captured follower leaves a lure only on the
evidence of PROTOCOL.md §5.2, and asked it for proof only once a want it listed had brought
nothing for `T_excursion`, 40 minutes; then it often followed the next lure. An honest announcer
answers a proof NACK at once from the front of its carousel, so asking sooner costs an honest
one a symbol. Measured first with the rule as it stood, before the changes below:

| Five lures, 15 km² band L, eight worlds | Delivered | Playback start | Band L town, start |
|---|---|---|---|
| Proof after 40 minutes (main) | 96.0 % | 51.1 min | 23.7 min |
| after 20 minutes | 97.0 % | 33.7 min | 23.8 min |
| after 10 minutes | 98.5 % | 28.9 min | 24.0 min |
| after 5 minutes | 98.9 % | 27.2 min | 24.6 min, 2.3 % more frames |

The draft takes `T_want_min`, 10 minutes: the interval after which a follower asks for what has
not come anyway. Four things came out on the way:

- **Only what the announcer lists.** Asked sooner for wants it ignores too, honest announcers
  that had not yet asked for what their followers wanted could not answer, and were left: in the
  band O network over 15 km² with 14 kB pieces, 513 role changes per world instead of 299, and
  21 % more frames. What an announcer ignores is still asked after `T_excursion`.
- **Once proven, trusted.** An announcer that has answered a follower is asked by it again only
  after `T_excursion`. Without that, every waiting follower kept asking honest announcers with long
  queues, and a lost answer now and then cost one: 346 role changes instead of 310, 5 % more
  frames.
- **An answer is proof, not progress.** Counted as progress, one symbol per answer kept the
  follower from asking again: in the island of smoke test `a_lure_is_visited_once` a follower held
  24 of 216 symbols of an object its announcer held, after four hours. Now the probe asks for a
  symbol the follower lacks, its answer marks the announcer as serving, and the want goes on
  stalling until the follower asks for it as usual.
- **Listed since the wait began.** A follower keeps the ids its announcer listed; what it listed
  before the follower began to wait it may have dropped since. Only a listing heard after that
  counts for the quick proof. In the living band L network under one lure, every world then held
  its current window at the end (one held 97.6 % without).

| 15 km² band L, eight worlds | Before §22 | Main | Now |
|---|---|---|---|
| One lure: delivered, start, 90th percentile | 99.5 %, 20.4, 41.7 min | 99.4 %, 17.8, 43.7 min | 99.5 %, 17.3, 29.3 min |
| Five lures | 97.1 %, 38.9, 89.0 min | 96.0 %, 51.1 min, never all | 98.4 %, 30.3, 61.1 min |
| Five lures claiming the maximum | 96.4 %, 42.8, 103.7 min | 95.0 %, 47.8 min, never all | 98.2 %, 29.2, 63.9 min |
| Five spoofers flooding WANTs | 100 %, 20.3, 33.1 min | 100 %, 18.0, 25.4 min | 100 %, 17.8, 24.9 min |

("Never all": in some world a follower never had all of a programme, so there is no 90th
percentile.) Under WANT floods, still no role changes after the first hour in the living network.
The matrix, the living network and collections are unchanged within about a minute;
in the size sweep band O over 15 km² needs up to 7 % more frames at 42 to 141 kB and 4 % fewer
from 211 kB up. Sparse interests in band L delivered 87.6 % instead of 88.5 %: two worlds of eight
lost 2.6 and 4.7 points, and with the quick proof switched off they did not. How it costs them is
not found yet; no follower there left an announcer for not answering about something it listed.

What the new counters show instead is older: in four of those sparse worlds, 126 to 389 times a
follower left an honest announcer because the announcer ignored, for `T_excursion`, something
the follower wanted, and could not answer for it. That is the case §21 already named: an
announcer that cannot get what its follower wants is unable, not false.

## 25. Asking is not getting

§24 left honest announcers that their followers had left for ignoring a want: 21 to 389 times
per world in the sparse band L network of §21 (100 nodes over 15 km², 24 channels, each node
following 2; eight worlds of 48 hours). This section finds why, and what those leaves were doing.
All figures are the eight worlds unless said otherwise.

### 25.1 A wrong first guess

What an announcer lacks it cannot answer a proof `NACK` for, honest or not. So the first attempt
asked such an announcer for a symbol of something it did list, to let an honest one prove that it
serves. The leaves moved, they did not go: leaves for ignored wants fell to 0 to 18 per world, and
leaves under the 90-minute rule (not one symbol for the ceiling) rose from 0 to 37 to 16 to 590.
Bulletins delivered within their period fell from 87.6 % (82.0 to 92.9 % per world) to 84.6 %
(70.9 to 90.3 %), and proof `NACK`s went up four times. Not taken.

### 25.2 Wants that were never asked for

A trace at each such leave in one world (389 leaves) showed something else. The announcer held
the object in none of them and wanted it itself in 380: it was not ignoring the object, it had
taken on its follower's want. But in 351 it had never asked for it, 50 minutes after the follower
began to wait. It wanted a median of 24 objects at the time (25th percentile 21, 75th 31, at most
55), and a frame names eight objects or four sets (PROTOCOL.md §3.3). In the order of asking of
§19, the earlier place first and then the most listeners per byte, the same objects led every
round, and they were ones nobody in reach held: they stayed wanted, and filled the frame for good.
(The follower notes only asks for objects it knows; widening that to everything it wants changed
nothing, so that was not it.)

The fix is a turn (PROTOCOL.md §4, "Asking in turns"). The first form put whatever was asked for
in the last `T_want_min` after whatever was not. It asked for everything, and it undid §19 wherever
more was wanted than a frame holds and all of it could be had: in the size sweep, band O over
15 km² with 7 kB pieces started playback after 22.4 minutes instead of 8.8, and the band O town of
the matrix 2.9 minutes later. The form taken turns only what is **stuck**: asked for, and neither
granted nor arriving for `T_excursion`. That goes after everything else, the one asked for longest
ago first, and everything else keeps the order of §19. A smoke test with 40 wanted pieces that
nobody holds any more (`what_nobody_holds_does_not_keep_the_rest_from_being_asked_for`) had 32
of them never asked for in five rounds before, and none now.

### 25.3 The leaves were the transport

Asking for everything ended the leaves for ignored wants (0 to 10 per world) and with them most
role changes after the first hour (90 to 157 per world instead of 185 to 996), and delivery fell:

| Sparse band L, eight worlds | Delivered, mean | Worst world | Uploads | Role changes after 1 h |
|---|---|---|---|---|
| Main (§24) | 87.6 % | 82.0 % | 2,438 to 4,359 | 185 to 996 |
| Turning everything | 81.1 % | 63.5 % | 1,188 to 2,045 | 90 to 157 |
| Turning what is stuck | 81.8 % | 66.6 % | 1,201 to 1,905 | 86 to 155 |
| No turn, and no leave for an ignored want | 72.7 % | 54.7 % | 981 to 1,852 | 80 to 184 |

So the leaves for ignored wants carried most of what reached a cell without followers of its own
channel. A follower that leaves its announcer follows the next one it hears or, hearing none,
leads a cell of its own; as an announcer its own asks reach the holders around it, which the old
announcer could not reach. Traced in three worlds with the rule below, 60 to 85 % of the
follower-object pairs a follower had left for got the object later, a median of 17 to 24 minutes
after the leave, and each such pair took four to six leaves.

That makes the rule: an announcer that has neither served nor named an uploader for what its
follower wants, for `T_excursion`, loses that follower, whether it asked for the object or not
(PROTOCOL.md §5.2, "Asking is not getting"). Other ways tried to carry the want:

| Sparse band L, eight worlds | Delivered, mean | Range |
|---|---|---|
| Leave an announcer that names no uploader, without turns | 92.8 % | 89.0 to 97.6 % |
| The same, and turns for what is stuck | 92.5 % | 89.8 to 95.5 % |
| Taken: that, and proof `NACK`s only for what is listed (§25.5) | 92.2 % | 88.5 to 96.7 % |
| The same, but only for another announcer it hears, never leading | 83.7 % | 74.1 to 90.8 % |
| Turning everything, and visiting the strongest other announcer it hears instead of leaving | 81.7 % | 67.4 to 90.6 % |
| Leave only if no other announcer was heard asking for it | 89.2 % | 80.1 to 95.9 % |
| Leave only under the 90-minute rule | 88.3 % | 81.6 to 94.8 % |

Leading a cell is what carries the want, and a visit brought no more than turning alone. Followers
in the sparse network heard other announcers ask for what they waited for as well (with that
condition 751 leaves in eight worlds instead of 4,600), so hearing others ask does not tell when
staying helps. How soon matters, in steps: leaving after 35, 40, 45, 50 and 65 minutes without an
uploader (each plus the `T_want_min` of the proof wait) delivered 92.7, 92.5, 91.2, 88.7 and
88.3 %; 25 minutes, 93.3 %.

### 25.4 What it costs

The whole validation against main (§24), eight worlds each:

| | Main | Now |
|---|---|---|
| Sparse band L: delivered, mean (worst world) | 87.6 % (82.0 %) | 92.2 % (88.5 %) |
| Sparse band L: uploads per world | 2,438 to 4,359 | 4,244 to 7,178 |
| Sparse band L: role changes after the first hour | 185 to 996 | 1,286 to 2,672 |
| Sparse band O: delivered; role changes after the first hour | 99.6 to 99.9 %; 0 to 14 | 99.6 to 99.9 %; 0 to 183 |
| Band L over 15 km², 846 kB objects: playback start, frames | 60.4 min | 62.3 min, 19.1 % more |
| the same with 423 kB: start, 90th percentile, frames | 45.7, 67.6 min | 45.7, 70.6 min, 7.5 % more |
| the same with 7 kB: start, 90th percentile, frames | 42.1, 60.5 min | 44.8, 65.2 min, 7.3 % more |
| Band L over 15 km², listeners pick collections: 90th percentile, frames | 67.1 min | 72.9 min, 13.2 % more |
| Matrix, five lures, WANT floods, collections in band O | | within half a minute |
| Living networks, with and without attackers | | the same or better, except that under a WANT flood one bulletin of one world had its 90th percentile after 15.6 minutes instead of 9.0, while the mean of the 90th percentiles fell in six worlds of eight |

(The share of nodes that hold their current window counted the lure itself as a listener: in one
world it held nothing of a channel it named, and the world looked 97.6 % up to date. It no
longer counts attackers.)

The cost is where everybody wants the same large objects and they take longer than `T_excursion`
to cross the network: a follower leaves an announcer that would have had them soon. Leaving after
50 instead of 40 minutes without an uploader cut that cost to 5.0 % more frames at 846 kB and
1.5 % at 423 kB, and the sparse network then delivered 88.7 % instead of 92.5 %: the same step
both ways. `T_excursion` stays where it is, the time after which a cell that cannot get an object
sends its followers out for it anyway.

### 25.5 Proof, and asking again

A follower sent proof `NACK`s for wants its announcer did not list too, after `T_excursion`: an
announcer cannot answer for what it lacks, so they were airtime for nothing, and under the new
rule every want without an uploader brings them. Followers of the sparse band L network sent
14,748 `NACK`s in eight worlds under main, 54,687 under the new rule, and 73 once they asked only
for what was listed, waiting as long for the rest (`T_want_min`, PROTOCOL.md
§5.2). Delivery was the same, 92.2 %.

And an announcer that has answered a proof `NACK` is to be asked again only after `T_excursion`
(§24). The code counted that from the start of the want, not from the answer, so once a want had
waited that long the next `NACK` could follow every answer at once. Counted from the answer now,
it made no measurable difference here: 73 `NACK`s in the sparse band L worlds either way, 190 and
181 in band O.

## 26. A shared time

Until now every simulated node read the simulator's own clock, so every node agreed on every
schedule by construction: hop dwells, the rendezvous, slots, upload phases, the control window.
Real nodes count from wherever they were switched on, and their crystals drift (PROTOCOL.md §6.1
has the tolerances). The simulator now gives each node a clock that starts at a random time up to
`--clock-epoch-s` and runs up to `--clock-ppm` fast or slow, and restarts from zero when the node is
switched on again; `--clock-same` gives all nodes one random start instead. The figures below use
clocks up to an hour apart and ±20 ppm ("real clocks") unless they say otherwise.

### 26.1 Without a shared time

In the neighbourhood of the matrix (50 nodes on 1 km², four worlds), band L, whose carrier hops,
delivered 4 to 20 % of a programme in six hours instead of all of it, with over 6,000 role changes
per world instead of about 110: followers hopped out of step with their announcers. Band O, which
does not hop, delivered everything, but playback started after 7.6 minutes instead of 4.1 and the
cell sent twice the frames: the control window and the slots no longer agreed.

### 26.2 Building it

Each rule of PROTOCOL.md §6.2 answers a step of this; band L and band O neighbourhoods unless said.

- **The latest time, everywhere.** Every node took any later time it heard and never went back.
  Band L delivered everything again, but playback started after 19.9 minutes instead of 6.8. In
  band O it took 40 minutes before the role changes stopped, and playback started after 7.7: a
  follower whose clock ran ahead kept it, since followers do not beacon and nobody learned it.
- **Time follows the announcer.** Followers take their announcer's time either way, announcers the
  latest among themselves, at their own next beacon. Band O started playback after 6.0 minutes,
  and three worlds of four were as calm as with one clock. Band L got worse, 26.4 minutes: a node
  that knows no time finds nobody on a hopping carrier, and announcers met only by chance.
- **The control carrier.** Every announcer tells its time there, and a node that knows none listens
  there until it hears it; one that has heard none steps up only after `T_acquire`, and then tells
  its time at once, outside the window. Band L agreed sooner, but two groups of 26 and 24 nodes
  kept times nine minutes apart for over an hour, until they met by chance: their windows and hop
  sequences never coincided. **The watch**, a whole period on the control carrier every `T_watch`,
  merged them, and after that the nodes kept within 4 ms of each other.
- **Following nobody, the announcer's time either way.** A node whose clock ran ahead of the first
  announcer it heard kept its own, as the latest, and looked for that announcer on the wrong
  channel. Taking it either way, all 50 nodes of the band L neighbourhood agreed within four
  minutes.
- **The guard.** A band O announcer held its beacon to the end of its window and sent it at that
  very millisecond; followers a millisecond behind it still listened on the control carrier, missed
  three in a row and stood for election: 408 role changes in that world. With frames kept
  `T_guard` (50 ms) from the edges of dwells and windows, 128. (The simulator also lets a receiver
  lose a frame that runs past its dwell, as a radio that retunes does; with one clock that had been
  13 frames of 504,000.)
- **Upload phases on the shared time.** Phases were a grid on each node's own clock, so uploaders
  and announcers disagreed about whose turn it was. On the shared time, a band L network over
  15 km² started playback after 19.3 minutes instead of 25.3, and a band L town after 33.6
  instead of 44.6.
- **A candidate whose time moved re-plans its step-up** into the next rendezvous of its new time:
  in the band L town, 30.9 minutes instead of 31.6, and fewer role changes.
- **What was dropped.** A node that heard an announcer on the control carrier listened on its
  schedule for two dwells, and a candidate waited as long. With one clock that cost 4 minutes in the
  band L network over 15 km² (21.1 instead of 17.4), and with real clocks it was worse there too.
  Telling the time every 10 minutes instead of every minute changed nothing in the town; it stays
  at a minute, because `T_acquire` has to cover one telling, and a station back from a power cut
  otherwise stepped up without knowing the announcer that had taken over.

Two findings on the way were older than this section:

- **A node that polled every millisecond.** A frame queued for the control window was held on the
  content hold, which the node's next deadline did not count while the queue was not empty: the
  node woke every millisecond until the window came, most of a minute before every push of a root.
  It came to light when, with a lure that told the time 0, one simulation ran for half an hour before
  it was stopped instead of twelve seconds, and the node found waking every millisecond was waiting
  so. A queued frame now waits on the pace, which the deadline counts.
- **The cold start always began at a rendezvous.** With one clock that started at zero, every node
  was switched on at the first instant of a meeting dwell. In one band L town world, started with
  clocks that agreed at 40 to 200 seconds, the first hour had 29 to 51 excursions; started at 0 and
  20 seconds, 2 and 12. The matrix's cold starts were a little luckier than any real one.
- **A trickle kept a follower silent.** Two followers that still listened for the time when a
  station first passed four objects then took one or two symbols from each repetition; each
  counted as having arrived, so they never asked again, and after six hours they held 64 of 113.
  What has not completed `T_excursion` after the last ask is now asked for again (PROTOCOL.md §4);
  smoke test `a_follower_that_missed_the_first_pass_asks_again`.

### 26.3 Results

The whole validation, eight worlds each, against main (§25). "One clock": every node reads the
simulator's time, as before, so only what the rules cost shows. "Real clocks": up to an hour apart
and ±20 ppm, restarting from zero when a node is switched on again. Playback start in minutes
unless said.

| | Main | One clock | Real clocks |
|---|---|---|---|
| Neighbourhood, band O / band L | 4.1 / 7.5 | 4.1 / 8.0 | 5.8 / 10.0 |
| 15 km², band O / band L | 5.1 / 17.0 | 5.2 / 17.4 | 7.5 / 18.2 |
| Town, band O / band L | 14.7 / 23.9 | 16.0 / 24.7 | 18.9 / 28.6 |
| ESP-NOW / two band L clusters | 7.8 / 14.8 | 7.8 / 9.4 | 10.4 / 10.3 |
| Five lures: delivered, start | 98.5 %, 30.5 | 98.6 %, 22.6 | 98.6 %, 24.6 |
| Five WANT spoofers | 17.8 | 18.4 | 18.5 |
| Living band L: bulletin median, newcomers caught up | 4.2, 8.5 | 4.2, 9.5 | 4.8, 9.6 |
| Sparse band L: delivered, mean | 92.2 % | 92.0 % | 92.9 % |
| Size sweep, band L neighbourhood, 7 kB / 211 kB | 10.5 / 8.5 | 17.1 / 12.3 | 15.9 / 13.8 |
| Size sweep, band O over 15 km², 42 kB | 9.1 | 10.4 | 14.6 |

Every scenario delivered within a point of before; five lures that claim the maximum score, the
worst case, 97.4 % with real clocks against 98.4 %. With one clock the rules cost little, except in the
size sweep's band L neighbourhood, a single cell whose station started the time itself and spent
its first watches deaf to its uploaders. With real clocks a cold start, where every node first has
to learn the time, starts playback 1 to 5 minutes later; once the nodes agree, they keep within a
few milliseconds of each other. In the living networks, which run for days, the median bulletin
arrives within a minute of before; under a lure in band L the worst 90th percentile came after 54
minutes instead of 41.


### 26.4 Open

- A false announcer can pull everyone to a later time (ABUSE.md, "Time pulled ahead"). Not
  measured.
- How a GPS or phone time takes precedence over the mesh's own is not specified; the simulator has
  no node with one.
- The shared time rests on knowing when a frame was received. Semtech documents no latency or
  jitter for the SX1262's receive interrupts (SX1261/2 datasheet rev 1.2, §8.5 and §13.3.1 list
  them without timing); LongShoT reached under 2 µs over LoRaWAN with hardware timestamps (Ramirez,
  Sergeyev, Dyussenova, Iannucci, IPSN 2019). Agreement to within `T_guard` needs far less, but it
  is measured only on hardware (ROADMAP Phase 1).

## 27. Carrying on demand, and relaying for any channel

§21 left sparse interests open: across 15 km² of band L, with each node following 2 channels
of 24, 92.0 % of the bulletins arrived in their period (§26.3). Two things decide who carries
what across a cell nobody in which follows a channel. An announcer fetched every piece of
every channel it heard of, asked for or not (its *generosity*), and a follower relayed only
for channels it followed (§17). The world here is §21's: 100 nodes and 2 stations on 15 km²,
24 channels, each node following 2, a daily 22 kB bulletin per channel, nodes changing what they
follow every 6 hours, 48 hours, eight worlds per band, one clock.

### 27.1 Who fetches what

Delivered within one publication period, mean (worst world); followers that hold the current
window of every channel they follow at the end; frames sent; what a node holds at the end, mean;
role changes after the first hour; and the median, over the hours, of the median latency of the
bulletins published in that hour. In band O the median bulletin arrived within two minutes in
every variant.

| Variant | Band L | Band O |
|---|---|---|
| Main (§26): generous announcers, relaying for followed channels | 92.0 % (88.6), 78.4 %, 840,892, 396 kB, 2,172, 43 min | 99.8 % (99.6), 100 %, 152,630, 221 kB, 14 |
| Announcers fetch only what is asked for | 88.4 % (83.7), 73.3 %, 532,612, 241 kB, 2,464, 49 min | 99.8 % (99.6), 100 %, 147,277, 218 kB, 22 |
| Generous announcers, relaying for any channel | 99.3 % (98.4), 99.7 %, 708,482, 1,050 kB, 436, 31 min | 99.8 % (99.6), 100 %, 150,767, 268 kB, 4 |
| Both, relaying by name what it cannot name | 99.5 % (98.6), 99.4 %, 683,979, 1,218 kB, 918, 29 min | 99.8 % (99.6), 100 %, 150,270, 313 kB, 3 |
| Both, relaying what it can name, while it is of use | 99.4 % (98.4), 99.0 %, 681,494, 328 kB, 222, 29 min | 99.8 % (99.6), 100 %, 147,277, 218 kB, 22 |
| ... and every node keeps the menu it hears | 99.6 % (99.3), 98.9 %, 660,342, 340 kB, 89, 19 min | 99.8 % (99.6), 100 %, 149,560, 229 kB, 1 |
| **... and followers tell their announcer what a new manifest names (27.4)** | **99.7 % (99.4), 99.5 %, 658,432, 343 kB, 90, 19 min** | **99.8 % (99.6), 100 %, 153,172, 230 kB, 0** |

With all of it, band L delivered 99.7 % of its bulletins in their period instead of 92.0 %, a
median 19 minutes after publication instead of 43, with 22 % fewer frames and 90 role changes
instead of 2,172; band O delivered as before, with no role changes.

- **Fetching only what is asked for, alone, is worse**, as §21 found: the generous announcers
  were how content crossed a cell without followers of its channel, for an excursion to find.
- **Relaying for any channel is what fixes sparse interests.** A node that hears another cell's
  announcer ask for its listeners, and that cannot read the collection the ask names, fetches its
  collection manifest from its own announcer first, which holds the manifests of every channel it
  hears of, and then the pieces (PROTOCOL.md §4). With generous announcers it delivered 99.3 %.
- **With both, nothing is fetched that nobody asks for.** Announcers that fetch on request send
  3.5 % fewer frames than generous ones once nodes relay for any channel, at the same delivery.
- **Only what it can name, and only while it is of use.** The first version relayed by name
  what it could not name, a piece whose collection manifest it did not hold, and kept what it
  relayed for good: nodes held 1.2 MB each after two days, and followers left their announcers
  for want of an uploader (§25) 398 times per band L world. Relaying by name also let anyone
  make every node in reach want made-up names (ABUSE.md, "Relay ask"). A node now relays only
  a piece or cover a manifest it holds names, or fetches first the collection manifest a set
  names, and keeps what it relayed while it is asked for or used, and `want_ttl` longer: the
  same delivery and frames, a quarter of the storage, a quarter of the role changes, and 41
  such departures per world.
- **What a node relays still counts against its own announcer.** Leaving it out of the evidence
  that sends a follower on an excursion or away from its announcer (§25) was tried: excursions
  for what a node relays carry content across too, they fell from 77 to 45 per band L world, and
  delivery fell to 98.8 % (worst world 95.0).
- **Every node keeps the menu it hears.** In band O nothing was relayed for channels a node did
  not follow: where rounds of asking are cheap an announcer asks for pieces by name, not in sets
  (§14), and a name does not say which collection manifest to fetch to read it. Followers there
  waited for what nobody brought and left their announcers instead: with real clocks, over
  sixteen band O worlds, 10.6 times per world against main's 1.9, and 157 role changes against
  108, at the same delivery. A node now collects and keeps the roots and collection manifests of
  channels it does not follow when they come by, without asking for them and within its carry
  budget (27.3), and can name what is asked. Band O relays, and its followers left 3.3 times per
  world with 81 role changes; band L delivered more, sooner (a median of 19 minutes instead of
  29), with 3 % fewer frames and 89 role changes instead of 222. What a node holds rose by 12 kB.

### 27.2 How long a relay waits

A node relays an ask only once it has gone unmet for `T_relay_wait`, since most asks are met by a
holder within a round (§17). Delivered, frames and the median latency of 27.1:

| `T_relay_wait` | Band L | Band O |
|---|---|---|
| 10 min | 99.7 % (99.4), 658,432 frames, 19 min | 99.8 % (99.6), 153,172 frames |
| 2 min | 99.8 % (99.6), 668,136 (+1 %), 14 min | 99.8 % (99.7), 183,418 (+20 %) |
| 0 | 99.9 % (99.6), 661,048 (+0.4 %), 9 min | 99.9 % (99.9), 195,938 (+28 %) |

In band L relaying at once halved the median latency for almost no frames. In band O it cost a
fifth to over a quarter more frames, for asks that a holder would have met within a round, as in
§17. `T_relay_wait` stays 10 minutes; a wait that depends on the carrier is left open (27.6).

### 27.3 A budget for what a node carries for others

A node that relays holds what it does not listen to, and so does an announcer that fetched what
its followers asked for, and every node the menu of channels it does not follow. On a device that
is bounded by its storage, so each node has a carry budget (PROTOCOL.md §4). The first version
let the least recently used give way whenever the budget was full. A small budget then evicted
what had just been fetched for another cell before it was handed on, and fetched it again:

| Budget per node | Band L | Band O |
|---|---|---|
| None (first version of 27.1) | 99.5 % (98.6), 683,979 frames | 99.8 %, 150,270 |
| 1 MB, least recently used gives way | 99.4 % (98.9), 692,927 (+1 %) | 99.8 %, 155,735 |
| 256 kB, least recently used gives way | 99.6 % (99.1), 1,564,157 (2.3 times) | 99.8 %, 169,207 |
| 64 kB, least recently used gives way | 77.8 % (73.7), 5,281,872 (7.7 times) | 99.8 %, 191,282 |

At 64 kB band L delivered less than without relaying at all (88.4 %, 27.1). Now only what has not
been of use for `want_ttl` gives way, and a full budget takes on no more relays; with the final
rules of 27.1:

| Budget per node | Band L | Band O |
|---|---|---|
| None | 99.7 % (99.4), 658,432 frames, 343 kB | 99.8 % (99.6), 153,172, 230 kB |
| 256 kB | 98.9 % (98.3), 680,434 (+3 %), 210 kB | 99.8 % (99.6), 167,878 (+10 %), 182 kB |
| 64 kB | 98.0 % (97.0), 640,846 (−3 %), 184 kB | 99.8 % (99.6), 170,692 (+11 %), 173 kB |

A small budget now costs a point or two of delivery in band L, where a node that is full declines
to relay and a listener waits for what nobody brings, and no flood of frames. Protecting what was
of use for `T_want_min` (10 minutes) instead of `want_ttl`, measured before nodes kept the menu,
delivered 99.3 % at 256 kB with 19 % more frames than without a budget, and 98.6 % at 64 kB with
4 % more: what gave way after ten minutes was asked for again later. In band O a budget costs frames
even where little is relayed: it bounds what an announcer holds of the channels its followers asked
for, and what gave way is fetched again for the next follower who asks.

### 27.4 Telling the announcer what a new manifest names

The rest of the validation (27.5) found one cost of fetching on request. In the living band O
network (§13) with real clocks, three of the five scenarios had one hour each in which a bulletin
reached its listeners after 11 minutes instead of 2. Traced in one world: at the hour the bulletin
was published, its listeners had just switched to channels they now followed and asked for those;
the bulletin's root and collection manifest reached them on the control carrier a minute later;
a follower asks at most every `T_want_min` except for what a manifest it asked for names, and
nobody had asked for this one. Its station, which fetches on request, learned that anyone wanted
the bulletin ten minutes later. Generous, it had fetched the bulletin itself, and its listeners
had overheard the upload. Newcomers to the band O cell, too, caught up 1.7 minutes later than on
main.

| A follower asks for what a new manifest names | Living band O, real clocks: slowest hour of three scenarios | Newcomers, band O / band L | ESP-NOW neighbourhood: start, slowest tenth, frames | Two band L clusters: start, frames |
|---|---|---|---|---|
| Main (generous announcers) | 1.8, 2.4, 3.0 min | 8.2 / 9.5 min | 7.8, 11.7 min, 23,265 | 9.4 min, 6,161 |
| On its usual cadence | 10.8, 10.8, 12.0 | 9.9 / 10.1 | 7.8, 11.7, 23,265 | 9.4, 6,161 |
| At once, after up to `T_offer` | 1.8, 3.0, 3.0 | 8.2 / 8.6 | 8.8, 16.7, 26,801 (+15 %) | 10.1, 6,910 (+12 %) |
| After `T_gossip_min`, what the announcer neither has nor asks for | | | 9.1, 16.8, 26,607 | 10.1, 6,939 |
| **After the announcer's next GOSSIP, what it neither has nor asks for** | **1.8, 3.0, 3.0** | **8.6 / 9.4** | **7.7, 12.8, 23,620** | **9.4, 6,175** |

Asking at once brought the living network back, and cost the neighbourhoods where cells overlap:
every cell's announcer passed what its followers would otherwise have overheard from a
neighbour's pass, 17 % more first passes in ESP-NOW. That was §15's finding while announcers were
generous. Asking only for what the announcer neither has nor asks for itself is the rule, but
after `T_gossip_min` the announcer's own ask, which it sends at most every `T_gossip_min` and on
a hopping carrier only in the rendezvous, had often not yet been heard. Waiting for its next
GOSSIP, the follower knows: an announcer that fetches the object anyway, because it follows the
channel or another follower asked, asks for it there, and its followers ask for nothing
(PROTOCOL.md §4).

### 27.5 The whole validation

Against main (§26), eight worlds each; "one clock" and "real clocks" as in §26.3.

- **Scenario matrix and false announcers**: within the spread of main. With one clock ESP-NOW
  started playback after 7.7 minutes against 7.8, two band L clusters after 9.4 against 9.4, the
  band O town after 15.3 against 16.0 with 3 % fewer frames, and the rest within 0.5 minutes
  and 2 % of frames; with real clocks within 0.6 minutes, the band L town and the two band L
  clusters with 3 % more frames. Five lures that claim the maximum score: 97.8 % delivered,
  start 28.6 minutes, against 97.8 % and 29.2; with real clocks 97.3 % and 32.8 against
  97.4 % and 31.5.
- **Collections**: whole channels within 2 minutes and 4 % of frames. One collection per
  listener across 15 km² is faster and cheaper (band L 35.8 minutes to start against 39.7, 7 %
  fewer frames; with covers 46.9 against 50.9), and within one cell about a minute slower to
  start (band L 11.5 against 10.3, band O 5.6 against 5.1): the station fetches what is asked
  for, and is asked once it has shown, in its GOSSIP, that it does not have it.
- **Living network**: bulletins as fast (band L median 4.2 minutes, band O 1.8), newcomers
  8.6 / 9.4 minutes against 8.2 / 9.5, a fifth to a quarter fewer uploads; under a lure in band L
  the slowest tenth after 42.6 minutes as on main.
- **Sparse**: with real clocks band L delivered 99.8 % (worst world 99.4) against 92.9 % (86.9),
  with 82 role changes after the first hour against 1,848, and band O over sixteen worlds
  99.6 % as main, with 58 role changes against 108.
- **Size sweep**: playback started within 2 minutes of main at every size and in every
  scenario but one, band L over 15 km² in 14 kB pieces, 3.1 minutes later with one clock and 2.5
  minutes sooner with real clocks; frames between 11 % fewer and 9 % more.

### 27.6 Open

- **The menu at scale.** An announcer holds the manifests of every channel it hears of, and
  every node what comes by of them within its budget; with tens of thousands of providers that
  is neither storable on a small node nor cheap to keep current (PROTOCOL.md §9, question 14).
- **What budget a device sets.** The simulator's default is none; a node with storage to spare
  loses nothing by a large one.
- **One collection per listener in one cell** starts about a minute later than when the station
  fetched everything (27.5).
- **A relay wait that depends on the carrier.** In band L relaying at once halved the sparse
  median for 0.4 % more frames; in band O it cost 28 % more (27.2). No wait where a round of asking
  is dear (§14) and `T_relay_wait` where it is cheap is a candidate, to be measured across the
  whole validation. (Done in §28, by a wait each node learns from the asks it hears: band L relays
  at once, band O waits longer than before.)

## 28. How long a relay waits, when it ends, and what gives way

§27 left two constants that were chosen, not derived: a relay waited a fixed `T_relay_wait` (10
minutes), and a full carry budget let the least recently used give way. Two questions followed:
can the wait be worked out from what the network does, and is there evidence of what will not be
asked for again? Both were measured in §27's sparse world (100 nodes, 2 stations, 15 km², 24
channels, 2 followed each, a daily 22 kB bulletin, 48 hours), eight worlds per band unless said,
sixteen where the differences are small. Columns as in 27.1: delivered within one publication
period, mean (worst world); frames sent; role changes after the first hour; the median, over the
hours, of the median latency.

### 28.1 How soon others meet an ask

The simulator now records every ask an announcer makes for its listeners: when it was first sent,
how often it went out still open, when and to whom it was granted, and when the announcer held the
object. With relaying for other cells switched off, this is how soon holders alone met a cell's
asks:

| | Band L | Band O |
|---|---|---|
| Met within 2 / 5 / 10 / 20 / 60 minutes | 3.8 / 23 / 32 / 38 / 54 % | 41 / 55 / 72 / 77 / 80 % |
| Of the asks still open, met within 0–2 / 2–5 / 5–10 / 10–20 / 20–60 minutes | 4 / 20 / 11 / 9 / 25 % | 41 / 24 / 38 / 18 / 11 % |
| Of the asks still open, granted at the next hearing after 1 / 2 / 3 | 24 / 6 / 3 % | 49 / 34 / 11 % |
| Never granted | 48 % | 9 % |

In band O holders, in the cell or a neighbouring one, met most asks within half an hour, and an
ask still open after ten minutes had a fair chance yet. In sparse band L almost nothing was met in
the first minutes, and little more after the first round of asking: half of the asks were never
met by holders at all. A fixed wait is too long for the one and about right for the other, so the
wait has to follow from what a node sees.

### 28.2 A wait learned from the neighbourhood

Counting rounds of asking instead of minutes (relay once an ask was heard open k times) made band O
send 28, 21, 17 and 10 % more frames for k = 1 to 4: where rounds of asking are cheap an announcer
repeats an open ask every half minute, and k rounds are no measure of how soon holders answer.

So each node keeps a small life table of the other cells' asks it hears (PROTOCOL.md §4): per age,
doubling from one minute, how many asks reached it and how many of those someone else met before
they were twice as old. It relays an ask once that share is below `relay_risk`. What the nodes
learned, averaged over nodes by how many asks reached each age:

| Age of the ask (minutes) | < 1 | 1–2 | 2–4 | 4–8 | 8–16 | 16–32 | 32–64 | > 64 |
|---|---|---|---|---|---|---|---|---|
| Band L, met by others before twice that age | 2 % | 20 % | 10 % | 13 % | 19 % | 37 % | 42 % | 21 % |
| Band O | 46 % | 26 % | 32 % | 64 % | 43 % | 21 % | 17 % | 0 % |

The age is counted from when the node first heard the ask; the asking announcer does not say how
old it is. In band L others met fewer than one in twenty of the asks a node heard in their first
minute before the second, so it relays an ask as soon as it hears it; an ask it first hears later
was met by others more often at every age, and waits until `want_ttl`. In band O the share stays
above one in twenty up to an hour, and a node waits until `want_ttl`.

Sixteen worlds per band, against the fixed wait, with no budget:

| Relay when the share is below | Band L | Band O |
|---|---|---|
| Fixed `T_relay_wait` (§27) | 99.7 % (99.2), 671,050, 93, 19 min; 4,679 relays | 99.7 % (99.4), 152,718, 1; 426 relays |
| **5 %** | **99.7 % (98.9), 672,458, 88, 11 min; 5,199** | **99.7 % (99.4), 152,955, 1; 314** |
| 10 % | 99.8 % (99.5), 690,383, 91, 11 min; 6,411 | 99.7 % (99.4), 153,208, 2; 324 |
| 20 % | 99.9 % (99.6), 694,570, 68, 10 min; 6,876 | 99.7 % (99.4), 159,344, 6; 840 |

A share of 5 % costs 0.2 % more frames and nearly halves band L's median latency, as relaying at
once did in §27.2. Band O relays a quarter less, at the same delivery and frames. Two choices that
look like detail decided the outcome. Before a node has heard any ask it needs a prior: one that
expects nothing (half the asks met, at every age) made nodes wait for evidence while their asks
aged, and band L sent 10 % more frames; one that says what `T_relay_wait` said (half met within
it, none after) costs nothing once evidence arrives. And a ceiling: relaying at the latest after
`T_relay_wait`, whatever was learned, cost band L 4 % more frames; after `want_ttl`, nothing. The
ceiling bounds what a false announcer can make a node learn (ABUSE.md, "Relay deterrence").

### 28.3 Relays that passed the ask on

With a carry budget the learned wait cost much more. Eight band L worlds, the stations exempt
from the budget as a device with room to spare would be, and the eviction order of 28.4:

| Budget per node | Fixed `T_relay_wait` | Learned, 5 % |
|---|---|---|
| None (sixteen worlds) | 99.7 % (99.2), 671,050 | 99.7 % (98.9), 672,458 (+0.2 %) |
| 1 MB | 99.6 % (99.3), 661,252 | 99.8 % (99.1), 730,892 (+11 %) |
| 512 kB | 99.5 % (99.0), 695,836 | 99.8 % (99.6), 840,383 (+21 %) |
| 256 kB | 99.2 % (98.1), 704,635 | 99.4 % (98.8), 813,642 (+15 %) |
| 64 kB | 98.8 % (97.8), 665,757 | 98.9 % (98.3), 698,548 (+5 %) |

The extra frames were repeated carousel passes and uploads: at 1 MB 32 % more repeats and 15 % more
uploads, at 512 kB 86 % and 33 %. The nodes evicted half again as much of what they held for their
own cells (510 objects per world at 1 MB against 328) and needed it again a third of the time
instead of an eighth. Two remedies that went for the storage were measured and rejected: letting a
relay taken on before `T_relay_wait` use free room only (850,000 frames at 256 kB instead of
814,000), and letting what a node holds for its own cell outrank relays (822,000).

What was wrong was elsewhere. Some forty-five nodes relayed every relayed object, and only one
relay in four was ever handed on to the cell that asked. An oracle that let one node per
neighbourhood relay each object, and abandoned the rest, did not help: 1.5 % more frames, a median
of 14 minutes instead of 10, and two and a half times the role changes. Several relayers in several
places are how an object finds a way when one of them cannot get it. But a relayer asks its own
announcer for the object; that announcer counts the ask as its follower's and marks its own ask as
for listeners; the next cell relays that, and so on. More than half of the asks marked for
listeners (51 to 55 %) came from cells where nobody listened to the object, and a relay was kept
until `want_ttl` after the last ask, so each relayer's announcer went on passing the object long
after the listeners had it.

The remedy was to end that: **a relay ends when the cell that asked for it holds the object.**
Sixteen band L worlds without a budget, and eight at 1 MB and 256 kB:

| A relay ends | No budget | 1 MB | 256 kB |
|---|---|---|---|
| `want_ttl` after the last ask (as before) | 99.7 % (98.9), 671,038, 94 | 99.8 % (99.1), 730,892 | 99.4 % (98.8), 813,642 |
| When the asking announcer grants it or lists it | 99.5 % (98.8), 623,206, 88 | 99.5 % (98.8), 638,324 | |
| When any announcer lists it | 99.6 % (99.0), 629,389, 183 | 99.6 % (99.3), 642,944 | 99.5 % (99.0), 733,095 |
| The same, only relays not yet started | 99.6 % (99.2), 648,442, 102 | 99.7 % (99.4), 665,599 | 99.4 % (98.5), 741,988 |
| **When the announcer that asked lists it** | **99.6 % (99.1), 641,028, 66** | **99.6 % (98.8), 663,212** | **99.4 % (98.6), 776,814** |

Ending relays did not make fewer nodes relay (47 per object against 43) but made them stop asking:
carousel repeats fell by 29 % without a budget and by 36 % at 1 MB. Ending a relay also when the
asking announcer granted the object to someone else cost delivery: a grant is not yet delivery.
The first version ended a relay whenever any announcer listed the object, not the one that asked:
in one world relays for a cell that still lacked a bulletin were dropped, taken on again and
dropped, its followers left their announcers 881 times, and roles changed 1,883 times after the
first hour instead of 224. A relay now ends only on the HAVE of an announcer that asked for it
(smoke test `a_relay_ends_when_the_cell_that_asked_holds_it`).

### 28.4 What gives way

Every eviction now records what was known of the object, and the simulator notes whether the node
held it again later. Eight band L worlds, fixed wait, stations exempt, the least recently used
giving way and a menu manifest counted as used when it arrives (§27); per world:

| What gave way | 256 kB | held again | 64 kB | held again |
|---|---|---|---|---|
| The menu of channels the node does not follow | 2,753 | 97 % | 5,529 | 92 % |
| Content held for its own cell | 1,003 | 15 % | 1,209 | 14 % |
| A relay an announcer listed as held | 299 | 4.7 % | 247 | 8.3 % |
| A relay no announcer listed | 15 | 28 % | 9 | 19 % |
| Anything out of its collection's window | 8 | 1.6 % | 35 | 2.8 % |

The menu was most of what gave way and came back when passed again, for free; being counted as of
use when it arrived, it filled small budgets and kept relays out (19,117 relays declined per world
at 256 kB). A relay that an announcer listed was rarely needed again, one that none listed several
times as often. Content out of its window was rarely needed again, but almost nothing that gave
way was out of its window, and idle time told little (own-cell content idle six hours or more came
back 12 % of the time, against 16 % within two hours). So the menu is of use when it names an ask,
not when it arrives, and gives way first; then relays another cell's announcer lists as held;
then the least recently used (PROTOCOL.md §4). With the fixed wait this delivered 99.2 % (98.1) instead of 98.9 %
(98.3) at 256 kB, with 5,222 relays declined per world and 4.6 % more frames, and 98.8 % (97.8)
instead of 98.3 % (97.3) at 64 kB with 4.1 % more.

### 28.5 Two holes the new rules exposed

The final rules delivered as main on the whole, but a few worlds fell short where main had not,
and the two traced were faults that main had as well.

**An announcer that listens alone.** In one band L world 98.9 % of the bulletins arrived in their
period, against 99.4 % on main and 100 % without ending relays (28.3). Nine of the missing
follower-bulletin pairs were three announcers' own, of channels none of their followers had asked
for; one of them asked for its bulletin every few minutes for ten hours without an offer. An
announcer marked as for listeners only what its followers asked for, so its own asks went out
unmarked, and the nodes that heard them, holding nothing and following nothing, did not relay
them. Kept for an hour after the last ask, relays for other cells had brought such objects within
its reach; ended sooner, they no longer did. An announcer now marks what it listens to itself as
well (PROTOCOL.md §4; smoke test
`an_announcer_that_listens_alone_is_relayed_to`). The four weakest band L worlds then delivered
99.8, 100, 99.8 and 99.9 % instead of 98.9, 99.4, 99.4 and 99.3.

**A root nobody announced.** Over sixteen band O worlds with real clocks, in two worlds 4 and 5
followers of one announcer lacked the current window at the end, against at most 2 on main.
Traced: the announcer had overheard a neighbouring cell's upload of its channel's new root, a
single symbol, before it heard any announcement of it. Symbols carry no kind, so it held the root
as content; holding it, it never fetched it; and with no announcement reaching it in the hour that
was left, it never read it, and neither did its cell. It happens on main too: in the worlds
traced, 22 to 56 roots per world were completed unnamed, and the next announcement usually mended
it. An announcement that came while such an object was still arriving did not: the kind it named
was not taken, and the announcement, pending, was not taken up again. A root needs no announcement
to be read, since its bytes name its channel and carry the channel's signature. A node that
completes an object nobody has named now reads it as a root, and an announcement names the kind of
an object already being collected (PROTOCOL.md §1; smoke test
`an_announcer_reads_a_root_it_overheard_unannounced`). The two worlds then held 76 and 74 of 76.
The rest of the shortfall in the second is a time island: its announcer ran 196 seconds off the
shared time and heard the announcer of the channel's source only on the control carrier (−108 dBm,
below the bulk carrier's sensitivity), whose windows it never shared (28.7).

### 28.6 The whole validation

Against main (§27), with all of 28.2 to 28.5; "one clock" and "real clocks" as in §26.3.

| Sparse world | Main | Now |
|---|---|---|
| Band L, one clock, 16 worlds | 99.7 % (99.2), 99.4 %, 671,050, 93, 19 min | 99.8 % (99.5), 100 %, 645,863, 85, 10 min |
| Band L, real clocks | 99.8 % (99.4), 99.5 %, 665,379, 82, 20 min | 99.9 % (99.6), 100 %, 652,001, 73, 11 min |
| Band O, one clock, 16 worlds | 99.7 % (99.4), 100 %, 152,718, 1 | 99.7 % (99.4), 100 %, 151,100, 7 |
| Band O, real clocks, 16 worlds | 99.6 % (99.2), 99.7 %, 156,796, 58 | 99.6 % (99.2), 99.7 %, 154,686, 67 |

The second column of each cell is the share of followers that hold the current window of every
channel they follow at the end. Band L delivered as much or more with 2 to 4 % fewer frames, and
its median bulletin arrived in half the time; every band L world now holds every follower's
window at the end. In band O one world had a station take over its neighbours at the twelfth
hour (102 role changes after the first hour; the other fifteen 0 to 3), at the same delivery; with
real clocks role changes ranged from 3 to 300 per world in both.

With a budget (eight worlds, stations exempt):

| Budget | Band L, main | Band L, now | Band O |
|---|---|---|---|
| 1 MB | 99.6 % (99.1), 672,790, 19 min | 99.8 % (99.6), 680,521 (+1 %), 9 min | 99.8 %, −2 % frames |
| 256 kB | 98.9 % (98.3), 673,376, 19 min | 99.4 % (98.7), 826,345 (+23 %), 10 min | 99.8 %, −2 % |
| 64 kB | 98.3 % (97.3), 639,796, 19 min | 99.1 % (97.7), 735,488 (+15 %), 10 min | 99.8 %, −0.2 % |

At 1 MB the rules deliver more, sooner, for 1 % more frames. Below that a node that relays as
soon as it hears an ask holds more than it can keep: at 256 kB a third of the relays and own-cell
content that gave way was needed again, against an eighth on main, where the menu filled the
budget and most relays were declined (19,117 per world against 1,985). There the fixed wait did
better on everything but speed: with the rest of 28.3 to 28.5 it delivered 99.8 % (99.5) at
256 kB with 658,372 frames, 2 % fewer than main, and a median of 19 minutes. Relaying before
`T_relay_wait` only into free room, worse before relays ended (28.3), now took the learned wait
from 826,345 frames to 783,561 at 256 kB (99.6 %) and changed nothing at 64 kB; it was not taken
(28.7).

- **Scenario matrix and false announcers**: as main but for the band L town (start 24.9 minutes
  against 24.2 with 1.4 % more frames with one clock; with real clocks 29.6 against 29.2 with
  1.3 % fewer) and tenths of a minute elsewhere. Lures and spoofers: the same delivery, start
  within 0.1 minutes; the slowest tenth under five silent lures 69.5 minutes against 64.5 with one
  clock and 68.6 against 69.2 with real clocks.
- **Collections**: whole channels within 0.7 minutes and 1 % of frames. One collection per
  listener starts within 0.3 minutes of main or sooner (band L across 15 km² with covers 45.7
  minutes against 46.9), with 2 to 12 % fewer frames across 15 km² and up to 3.5 % more within
  one cell.
- **Living network**: bulletins as fast (band L median 4.2 minutes, band O 1.8), newcomers 9.2 /
  9.7 minutes against 9.4 / 9.3. In one of the eight band L worlds of the daily network one hour's
  slowest tenth arrived after 22 minutes instead of 10, an upload granted twelve minutes before it
  began, and under an attacker or a spoofer 14 instead of 10 and 11; over sixteen more worlds the
  slowest tenth averaged 5.5 minutes against 5.4, with 6 hours of 368 above 15 minutes against 4.
- **Size sweep**: playback started within 1.4 minutes of main at every size, frames within 1.1 %.

### 28.7 Open

- **Asks that travel.** The chain of asks marked for listeners is also how an ask reaches a source
  several cells away; it now ends when its listeners are served, but it still spreads in every
  direction until then. Scoping it by distance (an ask marked with how many relays away its
  listeners are, relayed later the farther, as in an expanding ring search) is a candidate.
- **One table for every cell.** A node's life table pools the asks of every cell it hears. A cell
  whose asks no holder reaches waits as long as the neighbourhood's asks are met: in band O up to
  `want_ttl`. A table per asking announcer, starting from the pooled one, is a candidate.
- **Time islands.** An announcer whose shared time differs from its neighbours' by more than a
  control window hears them on the control carrier only while it watches (§26), and missed every
  announcement of a channel's root for 21 hours (28.5). (Done in §29: the windows wander, the time
  is told in every one of them, and nobody watches.)
- **A wait that weighs what it pushes out.** Below 1 MB relaying as soon as an ask is heard cost
  15 to 23 % more frames than main, and the fixed wait none but twice the time (28.6). The life
  table weighs only how likely others are to meet an ask; a relay that has to push out something
  the node may need again should wait longer. Relaying early only into free room went part of
  the way. Which budget a device sets is still open (§27).

## 29. Windows that wander: the time across cells that meet only on the control carrier

§26 gave the mesh a shared time and carried it on the control carrier where the cell's carrier
hops; on a carrier that does not hop, it said, every node hears every announcer's beacon on the
cell's channel. That holds within reach of the bulk carrier. The control carrier reaches further
(LoRa SF7 down to −124 dBm against about −107 dBm for GFSK at 100 kbit/s, §2), and joins cells
whose bulk carriers do not reach each other, but only in the control windows they share (§23),
and the shared time places the windows. §28.5 traced a band O announcer that ran 196
seconds off its neighbours' time and missed a channel's new root for 21 hours.

### 29.1 How often

The simulator now reports, every hour, the pairs of announcers that hear each other on the
control carrier and not on the bulk carrier, and how many of their control windows in the coming
hour they share (sim/README.md). In §27's sparse world, sixteen worlds per band:

| | Band L | Band O |
|---|---|---|
| Announcer pairs that hear each other only on the control carrier | 80 to 86 | 6 |
| ... sharing no window, one clock | 0 | 0 |
| ... sharing no window, real clocks | 0 | 4.6, in every world, 750 of 752 hours |

Band L tells the time on the control carrier and watches for others (§26), and its pairs share
their windows. Band O did neither, and four in five of its pairs never shared a window: the
control carrier did not join those cells at all. Delivery hid it, since band O's 500 mW bulk
carrier joins most cells anyway.

### 29.2 Telling and watching everywhere

Telling the time on the control carrier and watching for others wherever nodes listen there only
in the window, whether the carrier hops or not, joined them: in band O with real clocks pairs that
shared no window fell from 4.6 per world to 0.01, delivery rose from 99.6 % (worst world 99.2) to
99.7 % (99.4), every follower held its window at the end instead of 99.7 %, role changes fell from
67 to 40 and uploads from 856 to 773. Telling alone did not: 4.4 pairs per world still shared no
window. The watch did it, and the watch had a price. An announcer listened on the control carrier
through a whole period every 30 minutes, deaf on its bulk carrier for that minute, also where it
had no one to find: in the living band O neighbourhood (§13) with real clocks newcomers caught up
after 10.2 minutes instead of 9.2 over sixteen worlds (worst 28.8 instead of 24.6), and under a
lure the slowest tenth of the bulletins took 7.2 minutes instead of 3.0.

### 29.3 Windows that wander

The window had sat in the middle of every period. Placed instead where the period's number puts
it, pseudo-randomly as a hop sequence is (PROTOCOL.md §3), it is the same for everyone who shares a
time, and two groups whose times differ share one now and then: about one period in ten, an
estimate, since two windows of 4 seconds placed at random in 60 overlap by a second or more about
that often. An announcer that tells its time in every window then meets the
other group's within minutes, and the later time spreads as any later time does (§26). No one
listens longer than before, and the watch goes. Over the same worlds:

| | Before (§28) | Telling and watching | Windows that wander |
|---|---|---|---|
| Band O, real clocks, 16 worlds: announcer pairs sharing no window | 4.6 | 0.01 | 0 |
| ... delivered (worst world), windows held at the end | 99.6 % (99.2), 99.7 % | 99.7 % (99.4), 100 % | 99.7 % (99.4), 100 % |
| ... role changes after the first hour, frames | 67, 154,686 | 40, 167,385 | 50, 161,203 |
| Living band O neighbourhood, real clocks, 16 worlds: newcomers caught up | 9.2 min (worst 24.6) | 10.2 (28.8) | 8.8 (20.4) |
| Band L at 256 kB (§28.6): delivered, frames, role changes | 99.4 % (98.7), 826,345, 307 | as before | 99.9 % (99.4), 775,724, 140 |

Band L told and watched before, so telling and watching everywhere changed nothing there. Without
the watch, band L under a small budget delivered more with fewer frames, and role changes halved.

Placed anywhere in the period, a window now and then took the start of a meeting dwell, where
candidates step up and announcers beacon: over 24 band L neighbourhoods with one clock, playback
started after 8.3 minutes instead of 7.9, and several worlds a whole meeting cycle (100 seconds)
later. Kept inside one dwell and clear of its first tenth, it started after 7.9.

Two rules had assumed windows a period apart. An announcer told its time once every 60 seconds,
which with windows apart by up to two periods skipped a window now and then; it now tells in every
window. And a node that knows no time listened for `T_acquire`, a period and its window, before
starting a time of its own: a station back from a power cut sometimes heard none in that span and
started one, instead of challenging the node that had taken over (smoke test
`a_station_back_from_a_power_cut_takes_over_once`). `T_acquire` is now two periods and a window,
the longest a node can wait for a whole window. At a network's birth, where nobody has a time yet,
that is a minute more before the first announcer: over 24 band L neighbourhoods with real clocks
the median start was the same (10.4 minutes against 10.3), the slowest tenth 1.2 minutes later.

### 29.4 The whole validation

Against §28, with all of 29.2 and 29.3; "one clock" and "real clocks" as in §26.3.

- **Sparse**: band L with one clock over sixteen worlds delivered 99.8 % (worst world 99.2)
  against 99.8 % (99.5), with 3 % fewer frames and 69 role changes after the first hour against
  85; over 24 more worlds 99.84 % against 99.83 %, worst 99.2 against 99.4, with 5 % fewer frames.
  With real clocks 99.9 % (99.7) against 99.9 % (99.6), with 6 % fewer frames and 54 role changes
  against 73. Band O delivered as much, with real clocks more (99.7 % against 99.6 %, every
  follower holding its window at the end), and sent 4 % (real clocks) to 10 % (one clock) more
  frames: the time told in every window, one LoRa beacon a minute per announcer. Under a budget
  band L delivered as much or more (1 MB 99.8 %, 256 kB 99.9 % against 99.4 %, 64 kB 99.3 %
  against 99.1 %) with up to 6 % fewer frames.
- **Scenario matrix and false announcers**: with one clock within 1.2 minutes of §28 (the band O
  town 16.3 minutes against 15.3, the band L clusters 8.2 against 9.4). With real
  clocks the band L town started after 27.7 minutes against 29.6, and band O sooner everywhere;
  the two band L clusters after 11.7 minutes against 9.9 and the band L neighbourhood's slowest
  tenth after 15.4 against 13.7, over the matrix's eight worlds; over 24 others the clusters
  started after 21.3 minutes against 22.1, mid band L after 18.3 against 19.1, and the
  neighbourhood as before (10.4 against 10.3) with its slowest tenth 1.2 minutes later. Lures and
  spoofers: delivery within 0.4 points, start within 1.7 minutes.
- **Collections**: band L across 15 km² within a minute or sooner; band O across 15 km² up to 4
  minutes sooner with real clocks, and with one clock within a minute but for four collections with
  covers, 2.2 minutes later; band O in one cell within a minute. In one band L cell, within a minute
  either way with one clock, and with real clocks 1.6 to 2.7 minutes later in four of five cases:
  every node starts there without a time, and waits two periods for one (29.3).
- **Living network**: newcomers in band L caught up after 8.5 / 9.1 minutes (one clock / real
  clocks) against 9.2 / 9.7; in band O 8.5 / 8.6 against 8.6 / 8.3, and over sixteen more worlds
  with real clocks 8.8 against 9.2. The slowest tenth of the daily band L bulletins arrived after
  10.8 / 13.2 minutes against 22.2 / 18.0.
- **Size sweep**: with one clock band L across 15 km² started as soon or sooner at every size (up
  to 5 minutes sooner), the band L neighbourhood within 1.6 minutes either way, and band O within
  a minute except across 15 km² in 7, 423 and 846 kB pieces (1.3 to 2.4 minutes later). With real
  clocks band O started as soon or sooner at every size; band L 1.1 to 2.8 minutes later at three
  sizes of eight in one cell and five of eight across 15 km², where every node starts without a
  time (29.3). Frames from 14 % fewer to 8 % more.

### 29.5 Open

- **A first time sooner.** Where every node starts without a time, one waits two control periods
  before it starts one itself, a period more than before, and band L with real clocks started 1 to
  3 minutes later (29.4). An announcer that also told its time once a period outside the window,
  where only nodes that know none listen, would let `T_acquire` be a period again, for a frame a
  minute per announcer where the carrier hops.

## 30. A menu that scales: serving what the cell listens to

Every announcer kept the root and every collection manifest of every channel it heard of current,
and passed each new one once unasked (§27): that is how another cell's ask could be named and
relayed. With 24 channels that cost a few thousand frames over two days. PROTOCOL.md §9 Q14 asked
what it costs when there are many more channels than anyone nearby follows: how does everyone
learn what is on the mesh without the menu eating the airtime it is a menu of?

### 30.1 Many channels nobody follows

§27's sparse world (100 nodes over 15 km², two stations, 24 channels of which each node follows
two, a 22 kB bulletin a day per channel, subscriptions changing every 6 hours, 48 hours), with
100, 400 and 1000 more channels that nobody follows, each publishing once a week, published by the
same nodes (sim/README.md, `--quiet-channels`). Eight worlds per band, 24 for band L at 400:

| Quiet channels | 0 | 100 | 400 | 1000 |
|---|---|---|---|---|
| Band L: delivered (worst world) | 99.8 % (99.2) | 98.9 % (96.0) | 88.8 % (8.5) | 59.3 % (5.0) |
| ... frames | 621,704 | 842,033 | 1,297,405 | 1,594,810 |
| ... role changes after the first hour | 74 | 1,032 | 3,260 | 4,195 |
| ... asks met at the announcer, slowest tenth after | 31.7 min | 50.5 | 77.8 | 170.2 |
| Band O: delivered (worst world) | 99.8 % (99.6) | 99.7 % (99.0) | 99.6 % (98.5) | 98.9 % (98.7) |
| ... frames | 165,967 | 234,497 | 253,970 | 442,245 |
| ... role changes after the first hour | 3 | 3 | 16 | 541 |

Band L, with 13 to 16 announcers, broke down: at 400 one world in 24 delivered 8.5 % and another
64 %. Band O, with 3 to 6 over larger cells, held until a thousand. More channels that are all
followed cost less: with 48 and 96 channels and nothing quiet, band L delivered 99.6 % (99.3) and
98.3 % (97.3), with 499 and 1,381 role changes after the first hour.

### 30.2 Where the airtime went

In the first band L world with 400 quiet channels, announcers sent 241,200 bulk frames of roots
and 276,663 of collection manifests, against 1,132 and 2,291 without them: 64 % of their bulk
frames, where pieces had been 98 %. Each announcer kept every channel current, and each new
announcer passed every manifest it held once more. The menu took the airtime the followers' uploads
needed, and followers left an announcer that named no uploader for what they lacked (§25): 1,206
times per world with 400 quiet channels, 333 with 100, 21 without. Every role change made a new
announcer that passed the menu again. Band O has fewer, larger cells, so fewer announcers passed
it.

### 30.3 Serving what the cell listens to

An announcer now serves a channel (keeps it current, passes its new manifests unasked) while a
follower of its cell has asked for anything of it within `cell_keep`, 24 hours, or announced it,
and the channels it follows itself (PROTOCOL.md §2). Of every other channel it keeps what comes by,
as every node does, and answers an ask for what it holds. A follower's announcement counts because a
source announces its own publication to its announcer: without it, a channel published where
nobody followed it, which in the sparse world is most of them, was not fetched by the source's own
announcer, and left the cell only on the control carrier. Step by step, over the same eight worlds:

| Band L, 400 quiet channels | Every channel heard of | Asked for | ... or announced by a follower | ... and newest first (30.4) |
|---|---|---|---|---|
| Delivered (worst world) | 94.6 % (90.9) | 97.2 % (92.8) | 96.8 % (95.8) | 99.1 % (95.5) |
| Frames | 1,342,958 | 728,943 | 758,742 | 746,896 |
| Role changes after the first hour | 3,298 | 1,264 | 1,559 | 884 |
| Asks met at the announcer, slowest tenth after | 50.6 min | 19.6 | 22.9 | 21.9 |
| Nodes holding a channel's newest root, hourly mean | 95.8 % | 87.4 % | 88.7 % | 91.3 % |

In the first world, announcers sent 1,253 root frames and 2,532 of collection manifests when
serving what was asked for, 6,517 and 9,048 when also serving what a follower announced, against
241,200 and 276,663 before.

### 30.4 Newest first

Serving its cell's channels, an announcer still announced every root it holds in channel order,
seven new entries a round (PROTOCOL.md §2). With about 370 roots that is 53 rounds of 5 minutes,
4.4 hours, before a given root comes round again. Traced in one world: a channel published in a
cell where nobody followed it was first asked for 19.5 hours after publication, because its new
root waited its turn in each cell on the way to its followers. An announcer whose list does not
fit one frame now announces, in the round after it adopted a root, the eight roots it adopted
last, newest first, flagged so that nobody concludes from it that a channel left out is gone
(PROTOCOL.md §3.4); never two rounds running, so that the rotation goes on.

Every other round regardless was tried first. Over the first eight worlds at 400 it delivered
99.6 % (99.1), over 24 worlds 99.0 % (94.9), as much as the rule above (98.9 %, worst 95.5;
difference −0.04 points, standard error 0.27). But it costs where nothing is new: followers tell
that their announcer lacks a root from the stretches in channel order (PROTOCOL.md §2), which now
came half as often. In the sparse world with 24 channels and real clocks, over 24 more worlds and
against neither rule: every other round alone delivered 0.042 points less (standard error 0.019),
with 41 role changes after the first hour against 35; with the cell rule, 0.042 less (0.015) and
55. The cell rule alone, 0.067 less (0.046) and 48. The cell rule with newest first in the round
after a new root only: as much (+0.000, standard error 0.017), and 39.

### 30.5 Across the number of channels

| | 0 quiet: before | now | 100: before | now | 400: before | now | 1000: before | now |
|---|---|---|---|---|---|---|---|---|
| Band L: delivered (worst world) | 99.8 % (99.2) | 99.9 % (99.2) | 98.9 % (96.0) | 99.9 % (99.5) | 88.8 % (8.5) | 98.9 % (95.5) | 59.3 % (5.0) | 97.5 % (90.3) |
| ... frames | 627,219 | 616,282 | 842,033 | 677,125 | 1,297,405 | 726,896 | 1,594,810 | 842,519 |
| ... role changes after the first hour | 69 | 43 | 1,032 | 118 | 3,260 | 613 | 4,195 | 870 |
| Band O: delivered (worst world) | 99.7 % (99.5) | 99.7 % (99.4) | 99.7 % (99.0) | 99.8 % (99.6) | 99.6 % (98.5) | 99.8 % (99.6) | 98.9 % (98.7) | 99.8 % (99.6) |
| ... frames | 166,945 | 167,225 | 234,497 | 231,920 | 253,970 | 229,486 | 442,245 | 253,175 |
| ... role changes after the first hour | 3 | 6 | 3 | 7 | 16 | 8 | 541 | 28 |

Without quiet channels sixteen worlds per band (30.6), at 400 in band L 24, otherwise eight. With
48 and 96 channels all followed, band L delivered 99.7 % (99.5) and 99.0 % (97.9), with 227 and 683
role changes after the first hour against 499 and 1,381, and band O 99.9 % and 99.8 % as before.

What it costs: a node holds a quiet channel's newest root less often (90.8 % of channels in band L
at 400, against 95.5 %), since only the cells that listen to a channel keep it current. A listener
who opens such a channel from the menu asks for it, and its announcer then serves it (smoke test
`an_announcer_serves_what_its_cell_listens_to`).

What it does not solve: band L still delivers less with many channels than without (98.9 % at 400,
97.5 % at 1000, and 9 of 24 worlds at 400 below 99 %). Followers still left an announcer that
named no uploader 210 times per world at 400, against 10 without quiet channels. And every node
still keeps every root and collection manifest that comes by, within its carry budget
(PROTOCOL.md §2): at 400 a band L node held 817 kB at the end against 355 kB without them.

Two remedies did not help, and were left out:

- *Passing only what is new.* A node that steps up passed every manifest of the channels it serves
  once unasked, as every carousel has passed what is new to it since collections came in (§16).
  Passing unasked only a root that changed, over the first eight worlds at 400: 99.1 % (97.5)
  against 99.6 % (99.1), 593 role changes against 349; at 1000, 97.3 % against 96.9 %. Fewer root
  frames, more collection manifests asked for, and the same breakdown.
- *A warm start.* Over 24 living band L neighbourhoods (§13) newcomers caught up after 8.4 minutes
  against 8.2 without the cell rule, a difference within the spread between worlds (standard error
  about 0.23 minutes); band O 8.7 against 8.8. A node that steps up starts with no record of what
  its cell asked for; every node noting what its cell asks of its announcer, so that a new one
  knows at once, gave 8.5 and 8.9.

### 30.6 The whole validation

Against §29, with the cell rule and newest first; "one clock" and "real clocks" as in §26.3.

- **Sparse**: band L with one clock over sixteen worlds delivered 99.9 % (worst world 99.2)
  against 99.8 % (99.2), with 2 % fewer frames and 43 role changes after the first hour against
  69; with real clocks over eight 100.0 % (99.9) against 99.9 % (99.7), with 3 % fewer frames, and
  over 24 more as much (30.4). Band O delivered as much, with up to 1 % more frames.
- **Under a budget** (§28.6): band L at 1 MB 99.8 % (99.2) against 99.8 % (99.1) with 2 % fewer
  frames, at 256 kB 99.9 % (99.9) against 99.9 % (99.4), at 64 kB 99.4 % (98.6) against 99.3 %
  (98.2) with every follower holding its window at the end instead of 99.3 %, 3 % fewer frames
  and 481 role changes against 686. Band O as much, with up to 2 % more frames.
- **Scenario matrix, false announcers, collections and the size sweep**: frame for frame the
  same. There every channel is followed in every cell, and every list fits one frame.
- **Living network**: newcomers as in 30.5. The slowest tenth of the daily bulletins arrived
  within 2 minutes of before over eight worlds per band and clock, under attack, lures and
  spoofers too, and up to 4.8 minutes sooner in four cases; two exceptions, band O with one clock
  under attack (6.0 against 3.0 minutes) and band L with real clocks under a spoofer (16.2 against
  13.2).

### 30.7 What other systems do

Real catalogues are large. The Podcast Index lists 4,734,281 podcast feeds, of which 338,833
published in the last 30 days (stats.podcastindex.org/daily_counts.json, read 2026-10-04), and
radio-browser.info 60,114 stations. Should a small share of either reach a mesh, the menu runs to
tens of thousands of channels. Announced in rotation as now (24 bytes an entry, eight a frame), ten
thousand channels are about 1,250 frames, 250 kB, a round of the menu; the control carrier carries
about 42 bytes a second in its windows (an estimate: LoRa at about 5 kbit/s for 4 seconds a
minute), so one rotation would take at least an hour and a half of all its airtime. Rotation does
not scale to that; the question is what replaces it. PRIOR-ART.md lists what was studied; the
lessons:

- **Three questions, three answers.** What a node follows stays on the device, as here (receivers
  do not transmit). What is new is told in small heads repeated in tiers under a fixed budget, as
  SAP (RFC 2974) and DVB service information (ETSI TS 101 211) do: what is near and new often, the
  rest seldom. What exists is a catalogue a node merges from what its neighbours carry.
- **Leaving the catalogue outside the protocol centralises it.** Arweave and IPFS keep content
  permanent and addressed by hash, and leave "what exists" to indexers; in both, a few central
  indexers and gateways became where people look.
- **Reconciling sets beats rotating lists, when the lists are long.** Rateless IBLT (SIGCOMM 2024)
  sends one stream from which each receiver decodes its own difference at 1.35 to 1.72 symbols per
  differing item; PinSketch/Minisketch costs about 8 bytes per difference. Neither is needed while
  newest first keeps a cell current; both are candidates between announcers at scale.
- **Spam is bounded by demand, not by work.** Proof of work stops a small node and not a flooder
  with a graphics card (PRIOR-ART.md). What an announcer serves is already bounded by what its
  cell asks for (30.3); a catalogue can rank by how widely a channel is followed.

PROTOCOL.md §9 Q14 now holds the direction chosen: no catalogue held by the project and no key of
its own, channels that describe themselves when published in a fixed taxonomy, and a guide merged
from what neighbours carry.

### 30.8 Open

- **What still breaks down in band L at many channels** (30.5): not the passes of a node that
  steps up, nor its cold start. Followers leaving announcers that name no uploader remain the
  sign; why they find none is the next thing to trace.
- **Heads instead of whole roots** for channels a node only keeps, and their budget, before tens
  of thousands of channels (30.5, 30.7).
- **`cell_keep`** was measured at 24 hours only.
