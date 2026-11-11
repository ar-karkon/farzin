# Farzin 0.5 — OwnNet competition edition (release candidate)

Farzin is a UCI chess engine written in Rust. **Farzin 0.5** is the edition
for rating lists such as CCRL. It embeds only Farzin's own network,
`fzn2-a1.nnue`. It contains no Stockfish network weights and no Stockfish
source code.

## Files

| file | target CPU | use |
|---|---|---|
| `farzin-0.5.0-windows-x64-avx2-bmi2.exe` | x86-64-v3: AVX2, BMI1, **BMI2 (PEXT)**, FMA, LZCNT, POPCNT | **Main build.** Intel Haswell (2013) or newer, AMD Zen 3 or newer |
| `farzin-0.5.0-windows-x64-avx2.exe` | x86-64-v3 without BMI2 (magic bitboards) | AMD Zen 1/Zen 2, where PEXT is microcoded and slow |
| `farzin-0.5.0-windows-x64-popcnt.exe` | x86-64-v2: SSE4.2, POPCNT | older CPUs without AVX2 (slower network code) |
| `farzin-0.5.0-linux-x64-avx2-bmi2` | as the main build | Linux |
| `SHA256SUMS` | | checksums |

All builds play identical moves at a fixed depth (same `bench` node count,
1325774 at depth 12). Only their speed differs. Each build checks the CPU at
start-up. On an unsupported CPU it prints a clear message and exits instead of
crashing.

The Windows executables are self-contained. They link the C runtime
statically and import only Windows system DLLs (`KERNEL32`, `ntdll`,
`msvcrt`, `api-ms-win-core-synch`). No Rust, Python or other runtime is
needed. Windows 10 or 11 (x64) is required.

## Usage

Add the `.exe` to any UCI GUI (Cute Chess, Arena, Banksia, Fritz/ChessBase,
Shredder, and others). Options:

| option | default | notes |
|---|---|---|
| `Hash` | 16 | MB, 1–65536 |
| `Threads` | 1 | 1–256, Lazy SMP |
| `Move Overhead` | 50 | ms kept in reserve per move for GUI/OS latency |
| `Ponder` | false | `go ponder` / `ponderhit` are supported |
| `Clear Hash` | button | |
| `EvalFile` | `<default>` | `<default>` = embedded FZN2 net |

Command-line checks:

```
farzin-0.5.0-windows-x64-avx2-bmi2.exe --version
farzin-0.5.0-windows-x64-avx2-bmi2.exe bench
```

`--version` prints the edition, the compiled CPU features, the slider method
and the embedded network's name, architecture and SHA-256. For example:

```
Farzin 0.5.0 edition OwnNet target x86_64 features [popcnt,avx2,bmi2] sliders pext net fzn2-a1.nnue FZN2 4x768x768 buckets 8 mirrored sha256 f7bb0b7ef9857c3a4f24b39f5152e381e978d76d0c24817b0bebbefb1b6e220c
```

## Network provenance

`fzn2-a1.nnue` (sha256
`f7bb0b7ef9857c3a4f24b39f5152e381e978d76d0c24817b0bebbefb1b6e220c`):

* Architecture: FZN2, Farzin's own format: (4 mirrored king buckets × 768 →
  768) × 2 → 1 × 8 material output buckets, SCReLU (`src/fzn2.rs`).
* Trainer: Farzin's own `farzin-train2` (Rust, CPU). It was trained from
  random initialisation: 4 epochs, Adam, 50 minutes on 3 CPU threads.
* Positions: 105.4M positions sampled from Farzin's own search trees during
  Farzin self-play (`farzin datagen tree_rate=48`).
* **Labels:** each position's target is the static evaluation of
  `nn-ad9b42354671.nnue`. That network was published under CC0 by the Stockfish
  project, and Farzin's independent inference code computed the labels. So the
  network is **distilled**: its weights are new and were trained by Farzin, but
  its training targets came from a CC0 Stockfish-project network. No Stockfish
  weights are copied into, embedded in, or used at run time by this edition.
  We state this openly so that testers can apply their own policy.
* Verification: `build.rs` refuses to build the `ownnet` edition unless the
  embedded file is FZN2 with exactly the hash above. `tools/build_release.sh`
  scans every release binary at every byte offset. It finds 0 of 1,390,584
  indexed 32-byte chunks of the Stockfish net and 147,936 of 147,936 chunks
  of `fzn2-a1.nnue` (the positive control).

The stronger **Research edition** (`cargo build --release` without
`--features ownnet`) embeds the CC0 Stockfish network `nets/default.nnue`.
It reports itself as "Farzin 0.5.0 Research". It is kept for research and is
**not** the submission.

## Reproducible build

The release was built on Linux by cross-compiling with rustc **1.97.0** and
mingw-w64 (Ubuntu 24.04 packages):

```bash
rustup toolchain install 1.97.0 --target x86_64-pc-windows-gnu
sudo apt-get install mingw-w64
git checkout <release commit>
cargo +1.97.0 build --version   # sanity check
tools/build_release.sh dist
cat dist/SHA256SUMS
```

Paths are remapped, timestamps are suppressed and symbols are stripped. Two
builds from different directories produced bit-identical files. The CI job
`release-build` repeats the build on a fresh GitHub runner and uploads
`dist/` as the artifact `farzin-release-candidate`.

SHA256SUMS for this candidate: see the `SHA256SUMS` file attached to the
release. The values at the time of writing are in `docs/v8-report.md`.

## Testing summary

See `docs/v8-report.md` for full details and raw data in `benchmarks/v8/`.
