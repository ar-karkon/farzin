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

Raw evidence is in benchmarks/release-0.5.2/.

* Primary-build CPU guard: qemu-x86_64 10.0.13 accepts Haswell and EPYC Rome;
  Core 2 Duo, Nehalem, Sandy Bridge and Haswell without BMI2 print a clear
  requirements message and exit 1. These are Linux emulation results;
  unsupported-CPU Windows execution was not available locally.
* Foreign-network scan: zero indexed Stockfish chunks in the local primary
  binary and in FZN2-b1; 148017/148017 own-network control chunks found.
* CI release-build at c76d17c4a8427905c555fc39af0133ec8bc27d6b passed:
  all four targets reproduced byte-for-byte from a different source directory;
  0.5.1 comparison passed, including all 11 position scores/nodes/PVs and
  three benchmark repetitions per version. Artifact checksums were verified
  again after download. All four downloaded executables have zero indexed
  foreign-network chunks and all 148017/148017 own-network control chunks.
* The downloaded canonical Linux executable also passed the full 10/10
  conformance suite locally; see release-check-ci-binary.json.
* Windows PE inspection: primary build has 39 PEXT instructions and AVX2;
  the AVX2 alternative has no PEXT; the POPCNT variant has no PEXT or YMM
  operands. All three import only Windows system DLLs (windows-static.json).
* The main ZIP was unpacked and checked: exactly the primary executable,
  full unchanged LICENSE, unchanged THIRD_PARTY_NOTICES.md, concise English
  README.md, NETWORK-PROVENANCE.md and SHA256SUMS; executable checksum matches.

Native Windows validation is a mandatory publication gate. Current results
and raw JSON are available from PR #12's `windows` job and its
`windows-test-results` artifact:
https://github.com/ar-karkon/farzin/pull/12/checks

That job runs all three Windows variants natively (benchmark 992681 and
conformance), full primary conformance and tournament stability. Publication
requires every CI job on the final PR head to succeed. Unsupported-CPU
Windows execution is not emulated by the Linux checks above.

## Canonical executable checksums

rustc 1.97.0; ubuntu-24.04 MinGW-w64; tools/build_release.sh.
See benchmarks/release-0.5.2/SHA256SUMS:

```
85f46bb590e29b15f319c957d9df333cd97080c4bc63390a603f26e247dc0fdd  farzin-0.5.2-windows-x64-avx2-bmi2.exe
c3bc749cd02cd25bbefe9365437165f48b940a16b4fb47a63b37116fb37213ca  farzin-0.5.2-windows-x64-avx2.exe
801f4e2494728719817cefb44c9f67729a3cf65a3ba5d03d37db3da18fc4fdc9  farzin-0.5.2-windows-x64-popcnt.exe
324b236a5a097350d7bf3047f994ef1e3d69907f913aec753e1cb61d69158d1c  farzin-0.5.2-linux-x64-avx2-bmi2
```

No historical Windows result is claimed as a result of this release.

The CI release-build job builds all four requested targets with rustc 1.97.0,
checks for foreign weights with a positive control, repeats all builds from a
different directory, and compares depth-12 scores, nodes and principal
variations against the v0.5.1 source at 11 positions. Both versions must
repeat the 992681-node benchmark three times. The Windows job runs the three
executable variants natively plus full primary-build conformance and games.

Historical measurements in docs/release-0.5.1-report.md are preserved and
are not presented as results of this release.
