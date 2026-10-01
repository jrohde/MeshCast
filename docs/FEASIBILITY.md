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
