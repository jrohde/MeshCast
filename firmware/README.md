# firmware

Edge-node firmware for ESP32-S3 boards with an SX1262 (Seeed XIAO ESP32S3 + Wio-SX1262, Heltec V3,
LilyGo) and, later, nRF52-based boards.

Not started yet. Planned stack: Rust with `esp-hal`/`embassy`, the `core` protocol crate, an SX126x
driver with GFSK support, ESP-NOW and BLE via ESP-IDF bindings where the Rust crates fall short.
Fallback if the Xtensa Rust toolchain disappoints in Phase 1: C++ with PlatformIO + RadioLib and a
port of `core`. See [docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md).
