# station

Software for Linux-based nodes: the upcycled Helium miner (Raspberry Pi/CM4 + RAK2287, SX1302
concentrator), or any Linux box with an SX1262 or SX1302 attached.

Not started yet. A station runs the same `core` protocol as every other node; it simply tends to
win the announcer election because it has mains power, storage, an 8-channel LoRa receiver and
often an internet uplink. SX1302 access via `libloragw` bindings (as ChirpStack Concentratord does).
The station is dedicated: it owns the radio; no other mesh firmware runs beside it. See
[docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md).
