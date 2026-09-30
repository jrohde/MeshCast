# MeshCast

Peer-to-peer content delivery over LoRa/FSK, 2.4 GHz, internet and sneakernet. Music and voice
programs first. Store-and-forward, content-addressed, spectrum-polite, zero-config, offline-first.

*In vredestijd vermaak, in oorlogstijd verzet.*
*Entertainment in peacetime, resistance in wartime.*

## What it is

MeshCast turns cheap LoRa hardware (an SX1262 dongle, or an upcycled Helium miner with an SX1302)
into a radio station that nobody has to run. Anyone can start a **channel** and publish tracks or
spoken bulletins. Nodes that follow the channel collect the content over hours or days, chunk by
chunk, over whatever carrier happens to be available. Then every node plays the same programme
from its local copy, in sync. **The listening is live; the distribution is not.**

That inversion is the whole trick. Sub-GHz spectrum is slow and legally rationed, but music has
no deadline. So MeshCast never streams. It trickles files through the mesh and lets the
regulatory duty cycle limit only how fast new content spreads, never the listening experience.

## Principles

- **Content-addressed objects.** Every track, bulletin or file is an object identified by its hash.
  It does not matter how a chunk arrives: FSK burst, LoRa frame, ESP-NOW packet, HTTPS, SD card.
- **Channels are public keys.** A channel owner signs a manifest (playlist, schedule, object list).
  Subscribing is following a key. Encrypted channels are the same thing with a shared secret.
- **Store-and-forward.** All content traffic is delay-tolerant. That property makes decentralised
  spectrum sharing easy, where realtime chat makes it hard.
- **One protocol, many carriers.** The node discovers at boot what it has: sub-GHz radio always,
  ESP-NOW if the chip has WiFi silicon, internet only if it is already there. Nothing to configure.
  A network of only SX1262/SX1302 nodes with no internet is the baseline, not a fallback.
- **Automatic announcer.** In every neighbourhood exactly one node ends up transmitting a carousel
  of the content the cell needs; every other node stays silent. Nobody assigns the role: nodes
  score themselves and the best one speaks first. If it disappears, the next one takes over.
- **Receivers never transmit.** No presence beacons, no telemetry, no per-packet ACKs. A hundred
  thousand listeners cost exactly as much airtime as ten.
- **EtherFatsoen** (Dutch: "ether decency"): the spectrum-etiquette layer. Listen before talk,
  content always yields to control, self-throttling on measured channel occupancy, rarest-first.
- **EtherDiscipline**: legal limits per region, enforced in firmware. No region profile, no
  transmission.

## What it is not

- **Not a chat network.** Meshtastic and MeshCore exist for that and do it well. MeshCast has no
  routing tables, no connections, no acknowledgements: less than TCP/IP, not a replacement for it.
- **Not a live stream.** Even in the best case, sub-GHz spectrum moves a few tens of kilobits per
  second on average. See [docs/FEASIBILITY.md](docs/FEASIBILITY.md) for the honest numbers.
- **Not compatible with Meshtastic or MeshCore, and not running beside them.** Different goal,
  different protocol. A MeshCast node is dedicated hardware; those meshes fill the same bands with
  realtime traffic that MeshCast yields to, so sharing a radio with them would only import their
  congestion.

## Why this can work

Numbers verified against the current ETSI standard and the chip datasheets, details in
[docs/FEASIBILITY.md](docs/FEASIBILITY.md):

| Carrier | Regime (EU) | Average throughput per transmitter | Rough range |
|---|---|---|---|
| GFSK, 869.4–869.65 MHz | 500 mW, 10 % duty cycle, 250 kHz max | ~10–15 kbit/s | ~3 km |
| GFSK, 865–868 MHz | 25 mW, polite access with frequency agility | ~40 kbit/s | ~1 km |
| ESP-NOW LR, 2.4 GHz | 100 mW, no duty cycle | 50–100 kbit/s, continuous | 150–450 m |
| LoRa SF7, 869.4–869.65 MHz | metadata only | hundreds of bit/s | ~10 km |
| Internet | if present | unlimited | anywhere |

Audio travels as the codes of a neural codec (SNAC): an hour of music is 0.84 MB, an hour of
speech half that, and a 3-minute track 42 kB. One transmitter in the 10 % band moves five to eight
hours of music per hour; in the polite band or over ESP-NOW, about a day's worth. LoRa alone, at
roughly 0.5 kbit/s, still carries a track every ten to fifteen minutes (an estimate before protocol
overhead). The codes are decoded only by the phone or station that plays them, ahead of time.
Because the carousel is broadcast, adding listeners costs nothing. In the US (FCC 15.247, no duty
cycle, 1 W) the same hardware runs roughly ten times faster.

## Architecture

Every node runs the same protocol; the difference is capacity, not role.

- **Edge node**: ESP32-S3 + SX1262 (XIAO, Heltec, LilyGo). GFSK bulk transfer, LoRa control
  frames, ESP-NOW, BLE to a phone, optional SD card and audio output.
- **Station**: a Linux box with an SX1302 or SX1262, typically an upcycled Helium miner. Storage,
  transcoding, an 8-channel LoRa receiver, often an internet uplink. It tends to win the announcer
  election; nothing else is special about it.
- **Phone app**: library, subscriptions, playback, recording bulletins, and the microphone.

Implementation: Rust, one `no_std` protocol crate shared by firmware, station and simulator, so
the simulator tests the code that ships. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Documents

| Document | Contents |
|---|---|
| [docs/FEASIBILITY.md](docs/FEASIBILITY.md) | Verified numbers, corrections to the original brainstorm, why it scales |
| [docs/PROTOCOL.md](docs/PROTOCOL.md) | Objects, manifests, carousel, gossip, announcer election and healing, wire formats |
| [docs/TRANSPORTS.md](docs/TRANSPORTS.md) | Carrier comparison and zero-config discovery |
| [docs/ETHERFATSOEN.md](docs/ETHERFATSOEN.md) | Spectrum etiquette: the six mechanisms and the throttling algorithm |
| [docs/ETHERDISCIPLINE.md](docs/ETHERDISCIPLINE.md) | Regulatory profiles worldwide, with sources, and firmware enforcement |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Node types, hardware, data flow, implementation stack |
| [docs/ABUSE.md](docs/ABUSE.md) | Spam, flooding and other abuse: what holds, what does not, and the rule it asks for |
| [docs/PRIOR-ART.md](docs/PRIOR-ART.md) | What we borrow from FLUTE, DTN, DVB carousels, Othernet, Bitswap, Nostr, and the mesh projects |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Phases 0–3 with definitions of done |
| [docs/GLOSSARY.md](docs/GLOSSARY.md) | Terms, including the two Dutch loanwords |

## Status

Day 0, design phase. Phase 0 is a simulator that runs the real protocol core, not hardware. See
[docs/ROADMAP.md](docs/ROADMAP.md).

## Related projects

[Meshtastic](https://meshtastic.org), [MeshCore](https://meshcore.co.uk),
[Meshpoint](https://github.com/KMX415/meshpoint), [Reticulum](https://reticulum.network),
[Othernet](https://github.com/Othernet-Project), [LoRa-APRS](https://github.com/lora-aprs),
[Codec2](https://github.com/drowe67/codec2), [Opus](https://opus-codec.org),
[RaptorQ (RFC 6330)](https://www.rfc-editor.org/rfc/rfc6330).

## License

AGPL-3.0. See [LICENSE](LICENSE).
