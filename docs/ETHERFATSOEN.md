# EtherFatsoen

**EtherFatsoen** (Dutch loanword: "ether decency"): the protocol's spectrum-etiquette layer. Where
[EtherDiscipline](ETHERDISCIPLINE.md) is the law, EtherFatsoen is the manners: how MeshCast nodes
share the air with each other and with everyone else on the band, without any central coordinator.

The first rule of EtherFatsoen: **whoever is in no hurry always yields.** MeshCast is never in a
hurry, so it always yields.

## Why this is easier for MeshCast than for chat meshes

Meshtastic reports that its default preset degrades once roughly sixty nodes are in range of each
other, with measured channel utilisation above 65 % at busy sites, mostly from position and
telemetry beacons. Chat needs low latency, so a chat mesh cannot simply wait. MeshCast can: a
music chunk arriving at 14:37 instead of 14:00 harms nobody. Delay tolerance turns spectrum sharing
from a coordination problem into a local-observation problem, the same way TCP congestion control
works without anyone assigning bandwidth on the internet.

## The six mechanisms

### 1. Listen before talk with random exponential backoff, and jitter

Before every frame, on every carrier, the node samples RSSI for at least the CCA interval (draft
1 ms, well above the 160 µs legal minimum) and transmits only if the channel is below the CCA
threshold. On a busy channel it waits `rand(0, B × 2^attempt)` with `B` draft 50 ms, capped at
attempt 8 (12.8 s), then listens again. There is no maximum number of attempts: a MeshCast frame can
wait forever. Control and metadata frames additionally wait a random 0–500 ms even on a clear
channel: CCA cannot see a transmitter at the edge of range, and timers aligned to the same
boundaries (dwell starts, 10-minute WANT intervals) would otherwise collide every time.

The CCA threshold is set as close to the sensitivity as the radio allows (LoRa channel activity
detection reaches the sensitivity; averaged RSSI on GFSK gets within about 3 dB), not at the
−80 dBm of a naive RSSI check: every dB of gap is a ring of hidden nodes.

### 2. Scavenger quality of service: repetition is always the humble guest

Priority classes: **control** (BEACON, MANIFEST_ANNOUNCE) > **metadata** (GOSSIP, NACK) >
**fresh content** (the first copy of an object into the cell: an upload or a carousel's first
pass) > **repeated content** (later carousel passes). Repeated content is sent only when the
measured occupancy is below a stricter threshold than the other classes, and only at the
throttled rate of mechanism 4; fresh content is admitted like metadata and paced at the full
budget. One copy per cell is cheap; the repetitions are where the airtime goes.

**Polite to strangers, fair among ourselves.** The node separates occupancy into energy it could
decode as MeshCast frames and energy it could not. Foreign energy (LoRaWAN, Helium, anything
else) is judged strictly: MeshCast yields to everything it does not understand. Occupancy caused
by other MeshCast nodes is judged loosely, with a higher threshold and a higher rate floor,
because those nodes run the same congestion control and the goal among them is fair sharing, not
retreat. Without this distinction five carousels on one channel throttled each other, and the
sources between them, to the floor.

### 3. Redundancy makes collisions harmless

In a chat network a lost packet is a problem: it must be retransmitted, costing airtime, or the
message is gone. In MeshCast a lost symbol is a symbol that arrives next round, or is covered by a
repair symbol. Because loss is cheap, the protocol can afford to be extremely polite: high
backoff, low duty cycle, and let the redundancy absorb the losses. Politeness is cheap when loss is
acceptable.

### 4. Self-throttling nodes (spectrum congestion control)

Every transmitting node measures local channel occupancy and adjusts how much of its own legal
budget it allows itself to use. No agreement between nodes is needed; fair sharing emerges from
local observation, as with TCP, but simpler because there is no latency target.

```
// runs once per window W on each carrier
occ      = fraction of RSSI samples in the last W above CCA_threshold,
           excluding samples taken during our own transmissions
occ_ewma = ALPHA * occ + (1 - ALPHA) * occ_ewma

if occ_ewma > OCC_HIGH:
    rate = max(RATE_MIN, rate / 2)          // multiplicative decrease
elif occ_ewma < OCC_LOW:
    rate = min(RATE_MAX, rate + RATE_STEP)  // additive increase

// rate is the fraction of the EtherDiscipline budget we allow ourselves to spend.
// The per-frame gate: transmit BULK only if
//   own_airtime_in_window / (budget_in_window * rate) < 1
//   and occ_ewma < OCC_CONTENT
```

Draft parameters:

| Name | Draft | Meaning |
|---|---|---|
| `W` | 10 s | measurement window |
| `ALPHA` | 0.3 | EWMA weight |
| `OCC_HIGH` | 0.30 | above this, halve own rate |
| `OCC_LOW` | 0.15 | below this, increase own rate |
| `OCC_CONTENT` | 0.25 | content frames only below this occupancy |
| `RATE_MIN`, `RATE_MAX` | 0.05, 1.0 | bounds on the budget fraction |
| `RATE_STEP` | 0.05 | additive increase per window |
| CCA sample rate | 1 kHz | RSSI samples per second while idle |

**Two ceilings, the lower wins.** A node paces its content to the smaller of (a) the regulatory
budget of the band minus a reserve for control frames, and (b) a fair share of the channel's
occupancy target among the announcers it can hear, itself included: `occ_target / (heard + 1)`.
On duty-cycled bands (a) usually binds; on carriers without a duty cycle (ESP-NOW, US 915 MHz) (b)
is what stops eight announcers on one channel from each claiming half of it. The share is
derived from what the node hears, never configured; when announcers leave, the survivors' share
grows by itself.

On a band shared by the control and bulk carriers (EU band O carries both the LoRa control
channel and the GFSK bulk channel), the regulatory ceiling uses only `1 − control_reserve`
(draft 90 %) of the legal budget, so that beacons and gossip always have airtime left. The simulator's first
runs let the carousel spend the whole 10 % and starved the beacons, after which followers
concluded the announcer had vanished.

The target aggregate occupancy per cell is **≤ 30 %** including foreign traffic. That number
comes from the Meshtastic experience (trouble above ~40 %, collapse above ~65 %) and from slotted
ALOHA theory (throughput peaks near 37 % offered load and falls beyond it). The simulator will
confirm or move it.

### 5. The announcer as weather station, not as boss

The announcer's BEACON carries measured occupancy per bulk channel: the "spectrum weather report".
Nodes use it to pick the quietest channel (adaptive frequency agility) and the quietest hour for
uploads. It is shared observation, not allocation: nobody is told what to do, everybody sees the
same sky. Coordination without a coordinator.

### 6. Answer once: suppression

When an announcer asks for an object, every holder in range could answer, and on a broadcast
channel that would be as bad as everyone repeating a rumour. So a holder waits a random time
before uploading (shorter for its own announcer, longer for a neighbouring cell's) and gives up
if it hears anyone else sending that object meanwhile. This is the suppression rule of reliable
multicast: the first to speak silences the rest, without anyone coordinating.

### 7. Rarest-first

The carousel orders objects by how few nodes report having them (from GOSSIP). Symbols named in
NACKs go first. The emergent effect is that popular content spreads with the fewest transmissions
and no node ever repeats what everyone already has. This is BitTorrent's piece-selection rule
applied to broadcast.

## The six mechanisms are now seven

Mechanism 6 was added in Phase 0 when the simulator showed that content could not cross between
cells whose followers do not overlap; the title of this document is a name, not a count.

## The one remaining hard problem: hidden nodes

A and C cannot hear each other but both hear B. Both pass their own CCA and collide at B. The
classic fix, RTS/CTS, does not exist for broadcast. MeshCast's answer is mechanism 3: the collision
costs one symbol that arrives next round, and mechanisms 4 and 5 keep occupancy low enough that
the collision probability stays small. The single-announcer rule (only one node transmits bulk in
a cell) removes most hidden-node pairs by construction: the only remaining hidden pairs are
announcers of adjacent cells and uploading sources. The simulator's job is to measure how this
degrades with density and where adjacent-cell channel separation (weather report plus AFA) is
needed.

## What EtherFatsoen forbids

- Presence beacons from followers. A follower that has nothing new to offer sends nothing.
- Per-frame acknowledgements.
- Retransmission on a fixed schedule; every retransmission goes through the gate.
- Any frame that skips CCA, on any carrier, including ESP-NOW.
- Using the "spectrum weather" to claim a channel; it is advice, never a reservation.

## Measurability

A node keeps counters per carrier: frames sent by class, airtime used versus budget, CCA
deferrals, occupancy histogram, NACKs received, symbols delivered per round. Stations export them
(Prometheus-style text, or into the phone app). Without measurement the etiquette cannot be shown
to work, and "it seems fine" is how chat meshes ended up at 65 %.
