# Farzin 0.5.2 OwnNet — release verification

This maintenance release starts at main `2cd281a47b14a0fe423c4a994be7bffa749e8502`.
Search, evaluation, time management, CPU guard and network files are unchanged
from the published v0.5.1 (`73d4596fd22de660c6124e412391865d69f8a64a`).
Only version metadata, documentation and release verification/packaging change.

## Licensing and provenance

The complete GNU GPL v3 text in LICENSE matches GNU's published text after
normalizing whitespace, typographic quotes, the copyright symbol and the
final why-not-lgpl URL. Cargo.toml declares GPL-3.0-only; README agrees.
LICENSE and THIRD_PARTY_NOTICES.md are preserved unchanged. Caissa's MIT
notice and the Stockfish-project network's CC0 attribution remain intact.
FZN2-b1 is independently trained with Farzin's Rust trainer, warm-starting
from Farzin's own a1 weights. Targets were generated using the CC0
Stockfish-project network; this is distillation, not training without a teacher.
See nets/README.md for the data, teacher, architecture and training history.

Embedded network SHA-256:
`3ec5aa7e6c16b1ea6bd366ad873f4b649da310c91204915000bfd5f1e90ebfc0`.
The OwnNet build pins this checksum and does not embed default.nnue.

## Validation status

Local rustc 1.97.0 results:

* Formatting and Clippy with warnings denied: default, ownnet, legacy pass.
* Default and OwnNet unit/integration tests and legacy library tests pass.
* Python tooling: 29 tests pass.
* Full Linux release conformance: 10/10 checks pass, including 16,046,250
  perft leaf nodes, 30 random games / 2,155 legal moves, 72 legal PV lines,
  limits, ponder, clocks, four-thread stress and quit during search.
* Both 0.5.1 and 0.5.2: three depth-12 benchmarks each, all 992681 nodes;
  all 11 fixed-depth position scores, node counts and PVs agree exactly.
* Eight local 3+0.05 self-play games: no crashes, illegal moves or time
  forfeits; four completed pairs, zero scheduler failures.

Raw local evidence is in benchmarks/release-0.5.2/. Native Windows, canonical
cross-build hashes and repeat-build verification remain pending current CI.
No historical Windows result is claimed as a result of this release.

The CI release-build job builds all four requested targets with rustc 1.97.0,
checks for foreign weights with a positive control, repeats all builds from a
different directory, and compares depth-12 scores, nodes and principal
variations against the v0.5.1 source at 11 positions. Both versions must
repeat the 992681-node benchmark three times. The Windows job runs the three
executable variants natively plus full primary-build conformance and games.

Historical measurements in docs/release-0.5.1-report.md are preserved and
are not presented as results of this release.
