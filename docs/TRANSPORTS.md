# Transports

MeshCast defines one object and chunk format and lets every available carrier move it. This
document compares the carriers, states how a node discovers them without configuration, and fixes
the baseline: **a network of only SX1262/SX1302 nodes, with no internet, must work.** Everything
else is an accelerator.

## 1. Carrier comparison

Figures from [FEASIBILITY.md](FEASIBILITY.md); EU regime unless stated. "Average" is after the
regulatory rule is applied; "range" is the rough suburban estimate, halve it in a city.

| Carrier | Hardware | Band / rule | Raw rate | Average per transmitter | Range | Role in MeshCast |
|---|---|---|---|---|---|---|
| LoRa control | SX1262, SX1302 | EU band O, 500 mW, 10 % | SF7/125: 5.5 kbit/s; SF12: 0.3 kbit/s | ~0.5 kbit/s at SF7 | 10–25 km | manifests, beacons, gossip: the city-wide "programme guide" |
| GFSK bulk, high power | SX1262 | EU band O, 500 mW, 10 %, 250 kHz | 100–150 kbit/s | 10–15 kbit/s | ~3 km | content carousel across a district |
| GFSK bulk, polite | SX1262 | EU band L, 25 mW, polite + AFA over 15 × 200 kHz | 100 kbit/s | ~40 kbit/s | ~1 km | content carousel inside a neighbourhood |
| GFSK bulk, US | SX1262 | 902–928 MHz, FCC 15.247, 1 W, no DC | 300 kbit/s | 300 kbit/s | ~3 km | content carousel |
| ESP-NOW LR | any ESP32 with WiFi | 2.4 GHz, 100 mW, no DC | 50–100 kbit/s (distance-dependent) | 50–100 kbit/s | 150–450 m reliable | fast local carousel between neighbours; zero-config |
| IP | station, or a dongle whose phone/WiFi gives it internet | any | Mbit/s | unlimited | global | seeding between stations, sources uploading to a station |
| SD card | any node with a slot | none | n/a | n/a | wherever you walk | sneakernet import/export of the object store |

The SX1302 receives on 8 LoRa channels at once and has one FSK demodulator; a station uses it as
the control-plane receiver and as one FSK receiver. Its transmitter sends one frame at a time,
like any other node.

## 2. Discovery at boot: nothing to configure

A node determines its carriers by probing, not by settings:

1. **Sub-GHz radio**: always present on a MeshCast node. Tuned per the EtherDiscipline profile.
   Both the LoRa control channel and the GFSK bulk channel(s) live here.
2. **ESP-NOW**: if the SoC has WiFi silicon, the node enables 802.11 LR mode on a fixed channel
   and listens for MeshCast frames. No SSID, no access point, no pairing; frames are broadcast and
   any MeshCast node in range hears them. The user is never asked to "turn on WiFi".
3. **IP**: if the node already has an IP route (station on Ethernet, dongle given internet by the
   phone app or by a WiFi credential the user chose to add), it announces itself to the stations it
   knows and offers/fetches objects over HTTPS. Absence of IP changes nothing else.
4. **SD**: if a card with a MeshCast object store is mounted, its objects join the local library.
5. **BLE**: link to the phone app; not a mesh carrier but the way bulletins are recorded and
   subscriptions are chosen.

The only user-set value is the **region** (EtherDiscipline profile). Firmware images may be built
with a fixed region for a given market so that even this is pre-set.

## 3. One chunk format, per-carrier framing

Objects are split into fixed-size symbols (draft: 200 bytes; see
[PROTOCOL.md](PROTOCOL.md) §3). A bulk frame carries one symbol plus a 16-byte header and a
CRC. The same frame goes into:

- an SX126x GFSK packet (max 255 bytes payload, so one symbol per packet);
- a LoRa packet (only for tiny objects or when no bulk carrier exists; costly);
- an ESP-NOW frame (250-byte payload limit in classic ESP-NOW, larger with v2; one symbol per frame);
- an HTTPS body (thousands of symbols per request; the framing is kept so a station can relay
  what it fetched without re-encoding);
- a file on SD (`objects/<id>/symbols.bin`).

Receivers reassemble by object id and symbol index regardless of origin. A symbol that arrives
twice over two carriers is simply a duplicate.

## 4. Choosing among carriers

A node never "decides" globally. Each carrier runs its own instance of the carousel logic:

- The **announcer** of a cell drives the sub-GHz carousel (and the ESP-NOW carousel if it has WiFi
  silicon; a second ESP-NOW-only announcer may emerge among neighbours the sub-GHz announcer
  cannot reach at 2.4 GHz, since cells are per carrier).
- A node with IP fetches from stations first; what it obtains it advertises in its HAVE gossip and
  the announcer may include it in the carousel. This is how internet content "teleports" between
  regions and then radiates locally.
- SD import is just another way for objects to appear in HAVE.

The rule is always the same: rarest-first among what followers need, cheapest carrier first
among those that can deliver it.

## 5. Baseline scenario

Two dongles, 5 km apart across farmland, no internet, no phones after initial setup:

- LoRa SF9–SF12 control frames carry the two manifests and gossip both ways at 500 mW in band O.
- GFSK at 500 mW in band O (10 %) moves content: about 5 MB per hour in each direction if they
  alternate as announcer, so a 3-minute Opus track every 15–20 minutes.
- If a third node appears between them, it likely becomes announcer for both (hears the most).

Two dongles in the same street with ESP-NOW: the same protocol, 50–100 kbit/s, an album in an hour.

A station on a roof with an SX1302, internet, and 200 dongles across town: the station wins the
election, its LoRa receiver hears every source's gossip on 8 channels, its GFSK carousel serves the
district, ESP-NOW carousels serve individual streets, and the internet feeds it what other towns
publish.
