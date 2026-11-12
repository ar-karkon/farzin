# Farzin 0.5.2 OwnNet

Farzin is a UCI chess engine. This maintenance release keeps the search,
evaluation, time management and FZN2-b1 weights from 0.5.1 unchanged.

## Install in a chess GUI

1. Extract the ZIP into a folder of your choice.
2. In your UCI chess GUI (Arena, Cute Chess, Banksia, Fritz/ChessBase, etc.),
   choose the option to add/install a new UCI engine.
3. Select `farzin-0.5.2-windows-x64-avx2-bmi2.exe`.
4. Set Hash (default 16 MB), Threads (default 1), Ponder (default off) and
   Move Overhead (default 50 ms) as appropriate. Keep EvalFile at `<default>`
   to use the included network. No separate network file is needed.

Windows 10/11 x64 is required. No Rust, Python or development tools are
needed to run the executable.

## CPU requirements

This ZIP contains the primary x86-64-v3 build: AVX2, BMI1/BMI2 (PEXT), FMA,
LZCNT, MOVBE, POPCNT and the remaining x86-64-v3 prerequisites. Use Intel
Haswell or newer / AMD Zen 3 or newer. The release page also offers an AVX2
build without BMI2 for Zen 1/2 and an x86-64-v2/POPCNT build for older CPUs.
Unsupported CPUs receive an explanatory message and exit status 1.

Run the executable with `--version` for edition, CPU and network details.
`bench` should report 992681 nodes at its default depth 12. Verify the
executable against its entry in SHA256SUMS (e.g. PowerShell `Get-FileHash`).

## Network and license

Only Farzin's independently trained FZN2-b1 weights are embedded:
`3ec5aa7e6c16b1ea6bd366ad873f4b649da310c91204915000bfd5f1e90ebfc0`.
Training labels came from a CC0 Stockfish-project network; no pretrained
Stockfish weights are included in this edition. Full details are in
NETWORK-PROVENANCE.md.

Farzin is GPL-3.0-only. See LICENSE and THIRD_PARTY_NOTICES.md. Corresponding
source (including weights and build scripts), alternatives and release notes:
https://github.com/ar-karkon/farzin/releases/tag/v0.5.2
