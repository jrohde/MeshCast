# Contributing to MeshCast

Thanks for your interest. MeshCast is at day zero: the design documents in `docs/` are the
product right now, and the simulator (Phase 0) is the first code. Contributions to either are welcome.

## Ground rules

- **All project communication and artifacts are in English**: docs, code, comments, commit
  messages, issues, pull requests. Two Dutch loanwords survive on purpose, `EtherFatsoen` and
  `EtherDiscipline`; see [docs/GLOSSARY.md](docs/GLOSSARY.md).
- **The docs are the spec.** A change that alters protocol behaviour starts as a pull request
  against `docs/PROTOCOL.md` (or the relevant doc), not as code.
- **Numbers need sources.** Any regulatory limit, datasheet figure or measured result quoted in
  a doc carries a citation. Estimates are labelled as estimates. Nothing is invented.
- **Region-agnostic by default.** The protocol never assumes a country. Only the
  EtherDiscipline profile changes per region.
- **Spectrum politeness is not optional.** Changes that increase airtime, add beacons, or make
  receivers transmit need a very good argument and simulator evidence.

## Workflow

1. Open an issue or a draft PR describing the change.
2. Branch from `main`; branch names like `feat/announcer-election` or `docs/etherdiscipline-us915`.
3. Keep commits small and messages in the imperative ("Add polite access budget table").
4. PR into `main`. No force pushes to `main`.

## Licensing

By contributing you agree that your contribution is licensed under the AGPL-3.0, like the rest
of the project. See [LICENSE](LICENSE).
