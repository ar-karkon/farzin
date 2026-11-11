# Farzin 0.5 (OwnNet) — release test report

All local measurements ran on one 4-core cloud VM (Intel Xeon 2.1 GHz, AVX-512
capable, 15 GB RAM). The release binaries do not use AVX-512: they target
x86-64-v3. Matches used `tools/arena.py` with paired openings from
`tools/books/farzin-openings.epd`, conservative adjudication, `--time-margin
0` (any clock overrun counts as a forfeit), 1 thread and 16 MB hash unless
stated otherwise. Intervals are 95% pentanomial. Raw JSON/PGN are in
`benchmarks/v8/`. **Everything below is a local measurement. None of it is a
CCRL rating.**

## 1. What changed for the release

| area | change |
|---|---|
| network | Cargo feature `ownnet` embeds only `nets/fzn2-a1.nnue`. `build.rs` checks the FZN2 magic and SHA-256 and fails otherwise. |
| identity | The OwnNet edition reports `id name Farzin 0.5.0`; the research build reports `Farzin 0.5.0 Research`. `--version` prints the edition, CPU features, slider method and network hash. |
| build | `target-cpu=native` removed. `tools/build_release.sh` builds x86-64-v3 (AVX2+BMI2/PEXT), x86-64-v3 without BMI2, and x86-64-v2 Windows binaries (static CRT, mingw-w64), plus Linux, with path remapping, no timestamps and stripped symbols. Fat LTO and `codegen-units=1` give about +8% nps (best of 6: 1.43–1.45M vs 1.29–1.34M nps). |
| CPU | PEXT slider lookups when built with BMI2 (magic fallback otherwise). They are equivalence-tested against the magic tables. On this VM the speed difference is within noise. A start-up check exits with a clear message if the CPU lacks a compiled-in feature. |
| UCI | `Ponder` option, `go ponder` / `ponderhit` (clock limits are counted from ponderhit, and no answer is sent before ponderhit or stop). Tuning options hidden in the OwnNet edition. `Move Overhead` default raised from 20 to 50 ms. |
| **bug fix** | A search stopped inside iteration 1 (`go nodes 1`, or a near-zero clock) could return the **previous search's** PV move, which can be illegal in the new position. Found by `tools/release_check.py` random-game testing. Fixed by resetting the PV before the stop check, and by validating the best move and the ponder move against the legal-move list. A regression test was added. |
| tooling | `tools/release_check.py` (UCI conformance and stress), `tools/check_no_foreign_weights.py`, 40/N repeating clocks and clock-safety stats in `arena.py`, CI on Linux, x86-64-v3 and native Windows, and a tag-triggered draft-release workflow. |

The search algorithm is unchanged. `farzin 0.4` with `EvalFile=fzn2-a1.nnue`
and the 0.5 OwnNet build give the same `bench` signature (1325774 nodes at
depth 12). The 0.5 research build matches 0.4 (1179018).

## 2. Release binaries

Built with rustc 1.97.0 and mingw-w64 13 (Ubuntu 24.04):

```
1fd1651a7aa74c7604d7258a7699738ad0dd5b80067134b3b871cbbbbb3e3845  farzin-0.5.0-windows-x64-avx2-bmi2.exe
024540ca36ea2039d6cda9ebcd743036a6a123a0a8fdcefedffb680fe078c470  farzin-0.5.0-windows-x64-avx2.exe
578bd795e4f9ea5c38cf7d2edbc3afa52b83ccf817cfbdecb5ddeb3c3f2958a9  farzin-0.5.0-windows-x64-popcnt.exe
c0e2cf7c7096b329e15b0ef37256e291e37fcb60703af68b4739379885baf055  farzin-0.5.0-linux-x64-avx2-bmi2
```

* **Reproducible:** a second build from a different directory, and the CI job
  `release-build` on a fresh GitHub Ubuntu 24.04 runner, produced
  **bit-identical** files (same four hashes).
* The Windows executables import only `KERNEL32`, `ntdll`, `msvcrt` and
  `api-ms-win-core-synch-l1-2-0`, so no runtime installation is needed.
* All three Windows variants give the same `bench` node count. Only their
  speed differs.
* **No Stockfish weights:** each binary was scanned at every byte offset for
  the 1,390,584 indexed high-entropy 32-byte chunks of `nets/default.nnue`.
  **0 found.** The positive control found 147,936 of 147,936 chunks of
  `fzn2-a1.nnue`. The binaries are 6.3 MB. The Stockfish net alone is 47 MB.

## 3. Protocol, correctness and stability (`tools/release_check.py`)

10 checks: handshake and options; setting every option; perft on 6 standard
positions (16,046,250 leaf nodes, all exact); special positions (castling,
en passant, under/promotion, stalemate → `0000`, mate in 1); 30 random games
with mixed node limits (2,493 engine moves, all legal); PV replay legality
(72 lines); depth/nodes/movetime/infinite+stop/isready during search;
ponder/ponderhit/stop; extreme clocks (15 ms to 2 s, including `movestogo 1`
and `movestogo 40`); 4 threads + 256 MB hash for 20 s, 40 start/stop cycles,
Threads change, and quit during search.

| where | binary | result |
|---|---|---|
| native **Windows** (GitHub `windows-latest`) | `windows-x64-avx2-bmi2.exe` | **10/10 pass**. 4 threads: depth 23, 3.36M nps, hashfull 963. Quit 0.01 s |
| Linux (CI and local) | `linux-x64-avx2-bmi2` | 10/10 pass |
| Wine 9.0 (local) | `windows-x64-avx2-bmi2.exe` | 9/10 on the first run after a container restart (the handshake timed out at 10 s while the Wine prefix initialised); 10/10 on re-run, `uciok` in 0.06 s |

`cargo test --features ownnet` also passes natively on Windows (MSVC,
x86-64-v3).

## 4. Games

| match | TC | games | result for OwnNet (Elo, 95%) | forfeits / crashes / illegal |
|---|---|---|---|---|
| OwnNet vs Research edition | 10+0.1 | 200 | **−249.1** [−289.8, −213.5] | 0 / 0 / 0 |
| OwnNet (**Windows exe under Wine**) vs Research | **40/20 repeating** (scaled 40/15) | 48 | **−154.1** [−203.7, −109.9] | 0 / 0 / 0 |
| OwnNet Windows self-play, native Windows CI | 40/8 repeating | 8 | — | 0 / 0 / 0 |
| OwnNet (Windows exe under Wine) vs Research | 12+0.1 (CCRL-blitz-like 2'+1" ratio) | 48 | −221.1 [−280.2, −171.6] | 0 / 0 / 0 |
| **SMP**: OwnNet 2 threads vs 1 thread, 256 MB hash | 10+0.1 | 80 | **+93.4** [+57.7, +131.1] (2.26M vs 1.13M nps, +1 ply) | 0 / 0 / 0 |
| Regression: OwnNet 0.5 vs Farzin 0.4 + same net | 10+0.1 | 120 | +23.2 [−10.4, +57.2] (LTO speed; search identical) | 0 / 0 / 0 |

Total: **504 local games + 8 on native Windows, with 0 time forfeits, 0 crashes and 0 illegal moves**, all with `--time-margin 0`.

Clock safety (from `engine_stats`): at 40/20 the lowest clock that remained
was 0.085 s (Research: 0.123 s). This happens on the last moves before a time
control, where the time manager spends up to 70% of the remaining clock
because the clock refills right after. It is by design and leaves the
`Move Overhead` margin intact, but it is aggressive. A smoother 40/N
allocation is a candidate follow-up that should be tested for Elo.

## 5. Strength estimate (conservative; not a CCRL rating)

Measured chain on this machine (1 thread):

| link | result | source |
|---|---|---|
| OwnNet vs Research (same search) | −249 at 10+0.1; −154 at 40/20 | this report |
| Research (= 0.4 Track A) vs Stockfish 15.1 | −83 at 10+0.1 (100 pairs); −64 at 30+0.3 (30 pairs) | v7 report |
| OwnNet net (FZN2) **directly** vs Stockfish 15.1 | −215 [−275, −165] at 10+0.1 (40 pairs) | v7 report, `benchmarks/v7/fzn2-sf15-10s.json.gz` |

Stockfish 15.1 (1 CPU) is listed around **3540 on CCRL 40/15**. That figure
comes from a CCRL list quoted on TalkChess; I could not check it on the CCRL
site from this environment. Subtracting −215 (direct) or −249 − 83 = −332
(chain) gives **≈ 3200–3320** on that scale.

Conservative statement: **expect CCRL 40/15 around 3150–3300.** Lean toward
the lower half until real CCRL games exist. Reasons:
* Gaps measured at 10+0.1 between engines of different strength usually
  shrink at longer controls (here −249 → −154 at 40/20). That would push the
  number up.
* The anchor comes from a single opponent, the sample sizes are small, the
  hardware is different, and engine-pool effects are unknown. All of these
  argue for a wide margin.
* The Stockfish anchor itself is unverified (see above).

Reference engines from outside the repository could not be built here (the
environment's permission policy blocked compiling external engine sources), so
no new third-party gauntlet was run for this report.

## 6. Known limitations and follow-ups

1. The FZN2 net is about 150–250 Elo weaker than the CC0 teacher. The next
   gains are more data and a second hidden layer (see v7 §9).
2. 40/N time allocation near the control (see §4).
3. UCI_Chess960 and MultiPV are not supported. CCRL standard lists do not
   need them.
4. `go searchmoves` is ignored.
