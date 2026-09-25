# sim

Phase 0 deliverable: a discrete-event simulator that runs the real `core` protocol crate against
modelled radios, so the design is tested before any hardware is soldered.

Not started yet. See [docs/ROADMAP.md](../docs/ROADMAP.md) (Phase 0) for scope, the questions the
simulator must answer, and the definition of done.

Planned models: sub-GHz GFSK and LoRa (SX1262 sensitivity table), ESP-NOW long-range at 2.4 GHz,
log-distance path loss with shadowing, capture effect, hidden nodes, duty-cycle and polite-access
accounting per EtherDiscipline profile, announcer election and failover, EtherFatsoen congestion control.
