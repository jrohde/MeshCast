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
**Voice over LoRa meshes.** Meshtastic's audio module sends Codec2 (700 bit/s by default) from a
push-to-talk button; it is experimental and runs only on 2.4 GHz SX128x radios, because, in its
documentation's words, the sub-GHz bands are not wide enough for continuous audio on the mesh
([docs](https://meshtastic.org/docs/configuration/module/audio/)). QMesh
([GitHub](https://github.com/faydr/QMesh)) floods Codec2 voice through a LoRa mesh in
synchronised TDMA slots, transmitting in every third one, and accepts the latency that brings
over several hops. Both carry speech between people in near real time; MeshCast carries
programmes to listeners ahead of time, which is why it can use the sub-GHz bands at all.

## Discovery: what exists and what is new

How everyone learns what is on the mesh (PROTOCOL.md §9 Q14). These shaped the direction there;
FEASIBILITY.md §30 has the measurements that led to it.

**SAP** (RFC 2974, Session Announcement Protocol). Multicast sessions announced on a shared
channel with a total bandwidth limit (4000 bit/s by default) and an interval that grows with the
number of announcements, `max(300 s, 8 × N × size / limit)`. The model for a fixed metadata budget
in which more channels mean slower repetition, not more airtime.

**DVB service information** (ETSI TS 101 211). Present/following tables of the own transport
stream about every 2 s, of other streams about every 10 s, schedules every 10 to 300 s: tiered
repetition by how near and how soon. The model for announcing what a cell follows and what
changed lately more often than the rest (PROTOCOL.md §3.4).

**mDNS** (RFC 6762). Known-answer and duplicate-answer suppression: a responder stays silent when
the answer was already heard. MeshCast's offers and corrections follow the same rule.

**PodNet** (Lenders, Karlsson, May, IEEE SECON 2007). Podcast distribution between phones that
meet: peers exchange Bloom filters of the channels they offer, and among the caching strategies
studied, uniform caching of channels nobody nearby subscribes to did best overall. Close kin to the
carry budget (PROTOCOL.md §4).

**PSync** (Zhang, Lehman, Wang, IEEE INFOCOM 2017) and **State Vector Sync** (NDN). A producer
publishes an invertible Bloom filter of its latest names and a consumer a Bloom filter of its
subscriptions; SVS spreads a vector of latest sequence numbers, and partial SVS sends the recent
and a random part of it in about a third of the bytes. Close to "what is new", but both have
consumers transmit; MeshCast receivers do not.

**Rateless IBLT** (Yang, Gilad, Alizadeh, ACM SIGCOMM 2024, arXiv 2402.02668). One stream of coded
symbols serves every receiver, and each decodes its own difference after 1.35 to 1.72 symbols per
differing item. Set reconciliation that fits broadcast: the candidate if announcers ever need to
reconcile their menus instead of rotating them.

**PinSketch / Minisketch** (BIP 330). Set reconciliation in about 8 bytes per difference for
64-bit items, decoding quadratic in the difference: fine on a PC, seconds on an ESP32 for a
difference of a hundred (estimate). **Range-based set reconciliation** (Meyer, arXiv 2212.13567;
Negentropy, Nostr NIP-77) needs a few interactive rounds, which a silent receiver cannot take.

**Binary fuse filters** (Graf, Lemire, arXiv 2201.01174). About 9 bits per item at 0.39 % false
positives; a candidate for a compact summary a follower tests its subscriptions against.

**Arweave and the Interplanetary Network Indexer.** Storage that is permanent and
content-addressed, with the index of what exists left outside the protocol: in both, discovery
ended at a few central indexers and gateways. The lesson for MeshCast is to keep "what exists" in
the protocol, as a merge of what neighbours carry, rather than leave it to whoever runs an index.

**Swarm** (postage stamps, neighbourhood responsibility by address prefix). Who keeps what follows
from the content address, with no coordinator. An option for spreading the long tail; proof of
work is not: a 20-bit hashcash takes an ESP32-S3 about 2.5 s and a current graphics card about
50 µs (estimates), so it stops the honest and not the flooder.

## What is genuinely new here

Not much, and that is the point. The combination is: FLUTE-style carousel delivery, Nostr-style
channel identity, DTN-style delay tolerance, an emergent single announcer per cell, and a
spectrum-etiquette layer that treats regulatory politeness as a design pillar rather than a
compliance checkbox, all on hardware that costs less than a pizza. If any of it turns out to
exist already, we would rather adopt it than compete with it.
