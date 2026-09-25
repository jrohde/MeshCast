# MeshCast: agent instructions

## Language and scope
- Everything in this repository is in English. The conversation with the maintainer may be in Dutch;
  repository artifacts never are. Exceptions: the bilingual motto in the README and the two loanwords
  `EtherFatsoen` and `EtherDiscipline`, each with a one-line English gloss where first used.
- The docs in `docs/` are the specification. Protocol-affecting changes go into the docs first,
  code second. Do not let code and docs drift.

## Facts and numbers
- Regulatory limits, datasheet figures and measurements are quoted only with a source (URL or
  document/section). Never invent a number. If a value is an estimate, say so and show the assumption.
- The protocol is region-agnostic. Per-region limits live only in `docs/ETHERDISCIPLINE.md` and the
  region profiles derived from it.
- Verified references already collected: ETSI EN 300 220-2 V3.3.1 Table 4 and Table 18, SX1262
  datasheet rev 1.2 (sensitivity table), SX1302 datasheet (demodulator inventory), FCC 47 CFR 15.247,
  LoRaWAN RP002-1.0.3 regional summary, Meshtastic firmware region table. Prefer these over memory.

## Design invariants (do not break without a docs change and simulator evidence)
- Receivers never transmit. No presence beacons, no telemetry, no per-packet ACKs.
- All content traffic is delay-tolerant; backoff may be arbitrarily large.
- One announcer per cell, elected automatically; every node runs the same protocol.
- Transport-agnostic, zero-config, offline-first: a network of only SX1262/SX1302 nodes with no
  internet is the baseline case. Internet, ESP-NOW and SD are accelerators, never requirements.
- Chat is out of scope. MeshCast is content delivery, not messaging.

## Implementation stack
- Rust everywhere: Cargo workspace with `core` (`no_std`, the protocol), `sim`, `station`, `firmware`.
  The simulator runs the real `core`. C via FFI only where a Rust crate is missing (ESP-NOW, BLE,
  libloragw for the SX1302). No Python in the project.

## Git
- Feature branches, PRs into `main`, never force-push `main`. Commit messages in English, imperative.
- End commit messages with the attribution line the harness provides, when present.
