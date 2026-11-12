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

Release validation is in progress. This document will be updated with actual
results and executable hashes before publication. Native Windows results must
come from the current PR's CI, not historical reports or Linux emulation.

The CI release-build job builds all four requested targets with rustc 1.97.0,
checks for foreign weights with a positive control, repeats all builds from a
different directory, and compares depth-12 scores, nodes and principal
variations against the v0.5.1 source at 11 positions. Both versions must
repeat the 992681-node benchmark three times. The Windows job runs the three
executable variants natively plus full primary-build conformance and games.

Historical measurements in docs/release-0.5.1-report.md are preserved and
are not presented as results of this release.
