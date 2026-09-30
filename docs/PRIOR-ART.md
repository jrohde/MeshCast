# Prior art

MeshCast is a new protocol, but almost none of its ideas are new. This page lists where each one
comes from, what we borrow, and where we deliberately differ. Read it before proposing a
mechanism: it has probably been tried.

## File delivery over one-way channels

**FLUTE / ALC / LCT** (RFC 6726, RFC 5775, RFC 5651). "File Delivery over Unidirectional
Transport", used in 3GPP MBMS and 5G broadcast, DVB-H and ATSC 3.0 (ROUTE). Objects are announced
in a signed File Delivery Table, split into encoding symbols, and sent in a carousel with
RaptorQ repair symbols; receivers need no return channel. This is MeshCast's carousel and
symbol model almost exactly; our manifest is a small FDT with a schedule. We differ by adding a
sparse return path (GOSSIP, NACK) because our announcers can hear their audience.

**RaptorQ** (RFC 6330). The fountain code FLUTE uses. Systematic, near-optimal, `no_std` Rust
implementation available (`raptorq` crate). MeshCast v1 uses it for repair symbols; v0 uses plain
carousel repetition plus NACK because RaptorQ decoding is heavy on a microcontroller.

**DVB data carousel** (DSM-CC, ISO/IEC 13818-6). The object carousel that carries interactive TV
applications and firmware updates to set-top boxes, endlessly looping so a receiver that tunes in
at any moment eventually has everything. Firmware-over-carousel is a direct inspiration for
MeshCast's OTA plan.

**Outernet / Othernet**. A satellite datacast of Wikipedia pages, weather and news to cheap
L-band (later Ku-band with LoRa modems) receivers, with LDPC-coded file carousels and no uplink.
Its open-source receiver `open-ondd` is the closest existing system to "the listening is live, the
distribution is not". MeshCast is Othernet without the satellite: many small terrestrial
announcers instead of one big one in the sky.

## Delay-tolerant networking

**DTN Bundle Protocol** (RFC 9171; implementations ION, µD3TN for embedded). Store-carry-forward
for networks that are never fully connected: bundles wait at nodes until a contact appears. The
DTN insight MeshCast relies on is that once latency is unconstrained, most hard networking problems
become storage problems. We do not implement BP (its addressing and routing are more than we
need), but the sneakernet carrier and the bridge-node behaviour are DTN by another name.

## Peer-to-peer content distribution

**BitTorrent**. Rarest-first piece selection, HAVE bitfields, content addressing by hash. MeshCast
keeps rarest-first and HAVE gossip and drops everything that assumes a return channel per peer
(choking, tit-for-tat).

**IPFS Bitswap**. WANT/HAVE messages between peers over content-addressed blocks. Our GOSSIP frame
is a radio-sized Bitswap.

**Nostr** (NIP-01). Identity is a key pair; publishing is signing; following is storing a public
key; relays are dumb. MeshCast channels are Nostr identities and announcers are dumb relays.

**Usenet**. Flood-fill of articles between servers, each keeping what its readers want. The
observation that such systems naturally favour popular content, and starve niche content, applies
to MeshCast too.

## LoRa mesh networks

**Meshtastic**. Managed flooding with hop limits, later next-hop routing for direct messages;
every node rebroadcasts; position, telemetry and node-info beacons; a per-node channel-utilisation
meter that refuses to send above a threshold. Lessons taken: the failure mode (utilisation above
65 % at busy sites, project guidance to leave the default preset above about sixty nodes in range),
the value of measuring utilisation, and the cost of chatty beacons. Differences: MeshCast has no
flooding, no rebroadcast, no beacons from followers, no realtime goal, and one transmitter per
cell. Meshtastic's region table (firmware `RadioInterface.cpp`) is the seed for our EtherDiscipline
profiles.

**MeshCore**. Explicit roles: companions do not relay strangers' traffic, only repeaters do,
placed high with good antennas; quieter and more reliable than flooding once infrastructure
exists. MeshCast takes the "few well-placed transmitters" outcome and makes it emergent (the
announcer election) rather than configured, so that a network with no infrastructure still works.

**Reticulum**. A complete network layer (addresses, announces, links, encryption) that runs over
LoRa and many other media. It is what "a new TCP/IP for LoRa" looks like. MeshCast is deliberately
smaller: no addresses, no links, no routing; content, not endpoints.

**LoRaWAN**. Star topology, gateways with SX1302 concentrators, Regional Parameters (RP002) as the
canonical worldwide band table. We borrow the regional table structure and the SX1302 gateway
practice (`libloragw`, ChirpStack Concentratord). LoRaWAN's uplinks at 867.x MHz are the traffic
MeshCast must listen for in EU band L.

**Helium / Meshpoint**. Helium miners (Raspberry Pi + RAK2287/SX1302) are the hardware MeshCast
stations are upcycled from; Meshpoint is a Meshtastic/MeshCore base-station firmware for the
same boxes and proof that the SX1302 can be driven from Python on a Pi. MeshCast nodes are
dedicated and do not run it alongside.

**LoRa-APRS**. Amateur-radio position beacons over LoRa on 70 cm; evidence that these chips are
already used under amateur licences at higher power, a possible future profile.

## Spectrum sharing

**IEEE 802.11 DCF**. CSMA/CA with binary exponential backoff, thirty years of proof that
listen-before-talk plus random backoff works without coordination. EtherFatsoen mechanism 1.

**TCP congestion control** (AIMD). Halve on congestion, grow linearly otherwise; fairness emerges
from local observation. EtherFatsoen mechanism 4, minus the latency target.

**Slotted ALOHA**. Throughput peaks near 37 % offered load and collapses beyond; the origin of
our 30 % occupancy target.

**ETSI EN 300 220 polite spectrum access**. The regulator's own recipe for sharing 863–870 MHz
without a duty cycle: CCA, 1 s max on, 100 ms off, 100 s per hour per 200 kHz, more with frequency
agility. EtherDiscipline implements it literally; EtherFatsoen goes further.

## Audio

**SNAC** (Multi-Scale Neural Audio Codec, [MIT](https://github.com/hubertsiuzdak/snac)).
Residual vector quantisation with coarse levels at lower frame rates: 0.98 kbit/s speech at
24 kHz, 1.9 kbit/s music at 32 kHz. MeshCast's codec for both (FEASIBILITY.md §8), at the cost
of a decoder too large for a microcontroller.
**WavTokenizer, EnCodec, Vocos, DAC, Mimi**. Other open neural codecs. WavTokenizer (one codebook,
0.48–0.9 kbit/s) is excellent for speech and weaker for music; EnCodec is layered (1.5–24 kbit/s
from one model) but weaker per bit; Vocos is a very light decoder for EnCodec codes trained on
speech; DAC is the architecture SNAC extends; Mimi is a speech codec. Rated or measured in
FEASIBILITY.md §8.
**Opus**. 16–24 kbit/s music, 8 kbit/s speech; decoders run on ESP32-class hardware. The codec
MeshCast assumed until §8, and now the format of renditions for devices without a neural decoder
(PROTOCOL.md §1.2).
**Codec2**. 1.2–3.2 kbit/s intelligible speech; the digital-voice codec of the amateur world
(FreeDV, M17).

## What is genuinely new here

Not much, and that is the point. The combination is: FLUTE-style carousel delivery, Nostr-style
channel identity, DTN-style delay tolerance, an emergent single announcer per cell, and a
spectrum-etiquette layer that treats regulatory politeness as a design pillar rather than a
compliance checkbox, all on hardware that costs less than a pizza. If any of it turns out to
exist already, we would rather adopt it than compete with it.
