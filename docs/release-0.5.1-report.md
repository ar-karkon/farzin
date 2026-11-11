# Farzin 0.5.1 (OwnNet) — release test report

Release candidate for CCRL / Graham Banks. Source: `main` at `e2d1e49`
(all PRs through #7 merged) plus this release branch. The only engine-level
changes are the embedded network and a start-up CPU check; **the search is
identical to 0.5.0** (verified below). All local measurements ran on one
4-core cloud VM (Intel Xeon 2.1 GHz, AVX-512 capable, 15 GB RAM). **Nothing
here is a CCRL rating.** Raw data: `benchmarks/release-0.5.1/`.

## 1. What changed from 0.5.0

| area | 0.5.0 (candidate, never published) | 0.5.1 |
|---|---|---|
| embedded net | `fzn2-a1.nnue` `f7bb0b7e…220c` | **`fzn2-b1.nnue` `3ec5aa7e6c16b1ea6bd366ad873f4b649da310c91204915000bfd5f1e90ebfc0`** |
| `bench` (depth 12) | 943892 | **992681** |
| unsupported CPU | crashes with *illegal instruction* (see §4) | clear message, exit status 1 |
| search, time management, UCI | — | unchanged |

`build.rs` pins the new hash and refuses any other file for `--features
ownnet`; `tools/build_release.sh` uses `fzn2-b1` as the positive control of
the foreign-weights scan; CI and the tag workflow check the new hash on
Linux and native Windows. The version is 0.5.1 so the new binaries cannot
be confused with the `fzn2-a1` 0.5.0 candidate binaries that already exist
as CI artifacts. The Research edition (plain `cargo build --release`) is
unchanged apart from the version number: it embeds `default.nnue`, reports
"Farzin 0.5.1 Research", and has the same `bench` (844744) as at `e2d1e49`.

## 2. Release binaries

rustc 1.97.0, mingw-w64 13 (Ubuntu 24.04), `tools/build_release.sh dist`:

```
f004e302dacf50ff7a28986879f2983eb2520407d8b757cb202ba21724a6c928  farzin-0.5.1-windows-x64-avx2-bmi2.exe
c54c7b71c8ee203680c8aca51f9eaa626245856fc828319566a8df278ef7ea31  farzin-0.5.1-windows-x64-avx2.exe
23b29cd2dd05c09a00996d88b7babfd793c3838f56c45ef24b6a2934c7440bcc  farzin-0.5.1-windows-x64-popcnt.exe
734405d79e4e316f377368eabc7403ccef69e35ee65f777b3df67f0e76707427  farzin-0.5.1-linux-x64-avx2-bmi2
```

* **Reproducible:** two builds from different directories were
  bit-identical, and so was a build with a different `SOURCE_DATE_EPOCH`
  (the hashes do not depend on the commit time). The baseline 0.5.0 build
  from `e2d1e49` reproduced CI's artifact bit for bit (`73f1cff1…` for the
  main exe), so the toolchain matches CI.
* **No Stockfish weights:** each binary was scanned at every byte offset
  for the 1,390,584 indexed 32-byte chunks of `nets/default.nnue`: **0
  found**. The positive control found 148,017 of 148,017 chunks of
  `fzn2-b1.nnue`. The network file itself also contains 0 such chunks.
  17,818 chunks of `fzn2-a1` occur in the binaries; exactly the same number
  occur in `fzn2-b1.nnue` itself (b1 was warm-started from a1, and weights
  for inputs the new data never activated are unchanged), so this is
  expected and is not a stale a1 copy.
* `--version`:
  `Farzin 0.5.1 edition OwnNet target x86_64 features [popcnt,avx2,bmi2] sliders pext net fzn2-b1.nnue FZN2 4x768x768 buckets 8 mirrored sha256 3ec5aa7e…0ebfc0`.
* Negative test: replacing `nets/fzn2-b1.nnue` by `fzn2-a1.nnue` makes the
  `ownnet` build fail in `build.rs` (hash mismatch).

## 3. Search: merged improvements active, identical to 0.5.0

* Benchmark reproducibility: `bench` = 992681 nodes in 3 repeated runs and
  in all four builds (Linux; the three Windows exes under Wine).
* The 0.5.1 binary with `EvalFile=nets/fzn2-a1.nnue` gives 943892, exactly
  the 0.5.0 OwnNet signature: only the network differs.
* All ten PR #7 search features (`x_prior`, `x_pawnhist`, `x_evalhist`,
  `x_malus`, `x_frac`, `x_postlmr`, `x_minorcorr`, `x_hindsight`,
  `x_smallpc`, `x_threats`) and the SMP vote `x_vote` default to on in the
  OwnNet binary (`FARZIN_TUNE=1` exposes them). Turning each off changes the
  `bench` node count (959922, 1073142, 992702, 1087243, 1169757, 1143012,
  1085961, 970868, 868440, 989059), so each is live. The tuning options are
  not shown to GUIs.

## 4. CPU compatibility

Emulated with `qemu-x86_64 -cpu …` on Linux builds compiled with the same
flags as the three Windows variants (`bench 9`, 189657 nodes everywhere
it runs):

| emulated CPU | avx2-bmi2 | avx2 | popcnt |
|---|---|---|---|
| Core 2 Duo (no SSE4.2/POPCNT) | message, exit 1 | message, exit 1 | message, exit 1 |
| Nehalem (SSE4.2, no AVX) | message, exit 1 | message, exit 1 | runs |
| Sandy Bridge (AVX, no AVX2) | message, exit 1 | message, exit 1 | runs |
| Haswell without BMI2 | message, exit 1 | runs | runs |
| Haswell | runs | runs | runs |
| AMD EPYC Rome (Zen 2) | runs | runs | runs |

**Bug found and fixed:** before this release every build, including the
0.5.0 candidate, died with SIGILL on an unsupported CPU instead of printing
the documented message. With fat LTO and `target-cpu=x86-64-v3` the
compiler places AVX instructions in `main` itself (a `vmovaps` at
`main+103`), before `uci::run` reaches its feature check. The engine
binary now registers a static initializer (ELF `.init_array.00090`,
Windows CRT `.CRT$XCB`, i.e. before the Rust runtime and `main`) that
checks `cpuid`/`xgetbv` and exits with

```
info string Farzin: this build requires a CPU with x86-64-v3: AVX2, BMI1/BMI2, FMA, LZCNT, MOVBE, POPCNT. This processor lacks some of them; use another Farzin build (see README: Hardware requirements).
```

The disassembly of the guard in all six Linux/Windows variants contains no
vector or post-baseline instruction (0 xmm/ymm uses). On Windows the guard
sits between `__xc_a` and `__xc_z` and calls `_write`/`_exit` from
`msvcrt.dll`. A negative run on Windows could not be emulated here (Wine
cannot hide CPUID features), so the Windows path is verified statically
plus by the identical Linux code; native Windows CI covers the supported
path.

## 5. Protocol, correctness and stability (`tools/release_check.py`, full)

| where | binary | result |
|---|---|---|
| Linux (local) | `linux-x64-avx2-bmi2` (final) | **10/10**. 30 random games / 2155 moves legal; 4 threads 20 s: depth 28, 4.56M nps, hashfull 999; quit 0.03 s |
| Wine 9.0 (local) | `windows-x64-avx2-bmi2.exe` (final) | **10/10**. 4 threads 20 s: depth 28, 4.51M nps, hashfull 999 |
| native Windows (GitHub `windows-latest`) | `windows-x64-avx2-bmi2.exe` (final, `7392c4f`) | **10/10** (CI job `windows`) |

All four final binaries give `bench` 992681 (Linux natively, the three
exes under Wine), and the foreign-weights scan of the final files again
finds 0 chunks of `default.nnue` (control 148,017/148,017). JSON:
`release-check-linux.json`, `release-check-wine.json`.

`cargo fmt`, `clippy -D warnings` (default, `ownnet`, `legacy`,
`nnue-stack`, x86-64-v3, Windows gnu/msvc) and all tests pass with rustc
1.97.0 and current stable.

## 6. Games: 0.5.1 vs 0.5.0 (the shipped binaries)

Opponent: the 0.5.0 OwnNet candidate built from `e2d1e49` (`fzn2-a1`, the
same search). Book `tools/books/farzin-openings.epd`, every opening played
with both colours, conservative adjudication, no time margin, 1 thread and
1 core per engine unless noted. Elo with 95 % interval from pentanomial
pair statistics (`tools/arena.py`).

| time control | binaries | pairs (games) | pentanomial LL…WW | Elo [95 %] | LOS |
|---|---|---|---|---|---|
| 10+0.1, hash 16 | Linux | 150 (300) | 2 / 21 / 76 / 44 / 7 | **+38.4** [+16.3, +60.8] | 99.97 % |
| 40/20 repeating, hash 64 | Windows exes (Wine) | 60 (120) | 0 / 3 / 38 / 18 / 1 | **+49.6** [+23.8, +75.9] | 99.99 % |
| 60+0.6, hash 64 | Linux | 50 (100) | 0 / 0 / 35 / 15 / 0 | **+52.5** [+30.1, +75.3] | >99.99 % |
| 10+0.1, 2 threads each, hash 64 | Linux | 50 (100) | 0 / 8 / 26 / 14 / 2 | +34.9 [−1.3, +71.8] | 97.1 % |
| 10+0.1, hash 64 (CI, `4a864fa`) | native Windows | 12 (24) | — | +43.7 | — |
| 10+0.1, hash 64 (CI, `7392c4f`) | native Windows | 12 (24) | — | −14.5 [−78.7, +48.8] | — |

620 local games plus 48 on native Windows: **0 time forfeits, 0 crashes,
0 illegal moves, 0 disconnects**. Both engines reach the same depth at
each time control (20.6 vs 20.7 at 10+0.1, 25.3 vs 25.2 at 40/20, 28.5
vs 28.4 at 60+0.6) at the same speed (within 1 % nps), so the gain is
evaluation quality, not speed.

**Reading the numbers.** The `+63.1` in `nets/README.md` came from the net
selection run (intermediate `farzin-s3` search, 5+0.05, SPRT stopped
early, which biases upwards). With the final search the gain is
smaller, about +35 to +50 here, but it does **not** shrink with more
time: the longest control (60+0.6, the closest to CCRL 40/15 that this
VM allows) gave the largest and most significant result, with no lost
game pair at all. The two 12-pair native-Windows runs are too short to
mean anything individually (they disagree within their noise); they are
there to show that the exe plays full games on real Windows without
forfeits. The 2-thread run is consistent with the others but on its own
not significant at 95 %.

What this does **not** tell us: Elo against other engines (self-play
between two nets of the same engine usually overstates the gain), and
behaviour on CCRL hardware. Expect the CCRL difference between 0.5.1 and
0.5.0 to be positive but smaller than these numbers.

## 7. Observations and remaining items

* **Time use near the end of the clock (pre-existing, unchanged).** At
  short increments the time manager sometimes spends a large share of the
  remaining time on one move (max share 0.62 at 10+0.1, 0.69 at 40/20 and
  60+0.6), and the lowest clock seen was 0.05 s (0.5.0 at 40/20) and
  0.09 s (0.5.1 at 10+0.1). It happens equally with 0.5.0, never caused a
  forfeit in 668 games, and is the documented hard limit min(3 × soft,
  70 % of remaining time). It is unchanged on purpose (no unrelated engine
  changes in this release) and worth watching in the first CCRL games.
* **Unsupported CPUs on Windows:** verified statically (disassembly,
  CRT initializer order) and by the identical Linux code under qemu; no
  real pre-AVX2 Windows machine was available.
* **No CCRL hardware:** all games ran on one 4-core cloud VM, 1–2 threads
  per engine; CCRL testers usually run 1 thread at 40/15.
* **Owner steps:** tag `v0.5.1` on the merge commit; the tag workflow
  builds the binaries, checks the network hash on Linux and native
  Windows, and creates a **draft** release; compare its `SHA256SUMS`
  with §2 before publishing and sending it to CCRL.
