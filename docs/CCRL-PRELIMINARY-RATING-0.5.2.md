# Farzin 0.5.2 OwnNet: screening evidence and CCRL calibration protocol

Prepared in English for independent review by Graham Banks. Study date: 12 November 2026.

## 1. Executive summary

**Status: screening study; the requested CCRL-aligned calibration remains incomplete.**
The exact public Linux OwnNet binary played **120 completed games** at nominal **10+0.1**, across 3 independently rated engine families/releases.
The anchored screening performance is **3648 Elo, provisional 95% interval 3603–3699**, using the CCRL Blitz one-CPU anchors. This is a short-time-control performance estimate, **not a measured CCRL Blitz 2+1 rating**. No 120+1 or repeating 40/900 results are included. No 40/15 estimate is justified. The pre-test expectation of 3300–3400 is not a result.

The supplied English assessment file was absent from this environment. This report follows the requested nine-section structure and preserves the shortfall explicitly. This document is a screening evidence report and a pre-test protocol for the remaining calibration, rather than a completed rating assessment.

## 2. Release identity and OwnNet origin

Public release: https://github.com/ar-karkon/farzin/releases/tag/v0.5.2

Linux release SHA-256: `324b236a5a097350d7bf3047f994ef1e3d69907f913aec753e1cb61d69158d1c`. This matches the downloaded public `SHA256SUMS`.

Verified executable identity:

```
Farzin 0.5.2 edition OwnNet target x86_64 features [popcnt,avx2,bmi2] sliders pext net fzn2-b1.nnue FZN2 4x768x768 buckets 8 mirrored sha256 3ec5aa7e6c16b1ea6bd366ad873f4b649da310c91204915000bfd5f1e90ebfc0
```

The embedded network is `fzn2-b1.nnue`, SHA-256 `3ec5aa7e6c16b1ea6bd366ad873f4b649da310c91204915000bfd5f1e90ebfc0`. Identity was verified through `--version` and independently by locating the complete v0.5.2 network byte sequence in the downloaded release executable and hashing those embedded bytes. The offset, length and extracted SHA-256 are archived in `embedded-network-verification.json`. No Research edition or locally rebuilt Farzin executable was used.

Teacher-label disclosure: FZN2-b1 was warm-started from Farzin FZN2-a1, originally trained from random initialization. Farzin's self-play/search supplied positions, while targets were static evaluations produced from the Stockfish-project CC0 `nn-ad9b42354671.nnue` using Farzin's inference implementation. The self-play that generated positions also used that teacher network. Thus the OwnNet is distilled; it does not establish independence from teacher knowledge. The release states that no Stockfish weights are embedded or used at runtime in OwnNet. Source: public release notes archived with the evidence.

No Farzin source, evaluation, search, network, strength setting, release tag or published asset was changed. Shared Hash=128 MB is the only non-default Farzin option set; Threads remains 1, Ponder remains false, EvalFile remains embedded default, and Move Overhead remains 50 ms.

## 3. Exact opponents and rating anchors

Official snapshot: **7 November 2026**, retrieved 12 November 2026. Every entry below is the exact release's **one-CPU** entry, with no 4CPU/8CPU substitutions. Errors are the published list half-widths. Local source builds were used for opponents; their source commits, build commands, licenses and executable hashes are frozen in `manifest.json`. These are version-matched local compiles, not demonstrated byte matches to the binaries used by CCRL.

| Exact release | CPU | Blitz anchor/error | 40/15 anchor/error | Executable SHA-256 |
|---|---:|---:|---:|---|
| Weiss-2.0 | 1 | 3318 ±7 | 3263 ±10 | `722463c2723e8549956e895ea413d4440f712cb0d1ecba8985a165813b2dffc1` |
| Ethereal-12.00 | 1 | 3390 ±17 | 3318 ±12 | `7ea698f8c78491ac2194e8550da4811efb3adb27c5327af205275b258b024380` |
| Stockfish-10 | 1 | 3509 ±19 | 3442 ±7 | `5f901d2c04e0eec1d2b2d88f62cff651ca20236ba5a305cf3d2a65f81fcbddc4` |
| Berserk-4.4.0 | 1 | 3334 ±14 | Not selected / not verified | `c72edaa66696423bd0371d6c364c6eee681a0d369d9406696f370456baa9f1ce` |

List sources: https://computerchess.org.uk/ccrl/404/rating_list_all.html and https://computerchess.org.uk/ccrl/4040/rating_list_all.html . The manifest preserves the exact source row and URL for each category, including source-list game counts. Berserk 4.4.0 is built and hash-frozen as a fourth Blitz anchor; it has no executed games in this dataset.

## 4. Hardware and testing conditions

AMD EPYC 9V74 under KVM, Linux/glibc 2.41, AVX2/BMI2 available; 3 visible virtual CPUs, **2 CPU-equivalents quota** (`200000 100000`). Concurrency=2, Threads=1. Engines search alternately, not simultaneously within a game; ponder is off (python-chess play default). Physical host core allocation and noisy-neighbor isolation cannot be established in this VM. CPU quota/throttling snapshots are archived. Brief opponent builds, benchmarks and infrastructure checks overlapped early screening, adding possible contention; results are preliminary.

Hash=128 MB for all engines, no tablebases installed or configured, and no private engine books. Weiss NoobBook defaults false. Opening suite: 20 standard openings explicitly curated before results, 8 plies (4 full moves) each, no evaluation-based selection. Same positions with reversed colors; seed=20261112. This small suite is independent of Farzin's existing evaluation-filtered opening book, but is neither a broad tournament book nor a guarantee against opening bias.

Conservative symmetric adjudication requires both engines' evaluations to agree on at least 1500 cp over 5 moves each; draws require |score|≤5 cp over 12 moves each after move 60. For this original screening run the EPD counters started at move 1 after the 4-move opening; the future runner explicitly passes `--book-plies 8` so repeating controls count book moves (the first 40/900 block then has 36 searched moves per side). This infrastructure correction does not affect any recorded screening clock limit. Maximum 400 searched plies in the original run, zero clock grace, wall-clock time includes protocol overhead. Per-game search times, node totals and reasons are retained in JSON. PGNs retain moves and evaluation comments; all PGNs were replayed for legal moves and cross-checked against JSON identities/results/reasons.

Stockfish 10 reference benchmark was attempted: `bench 16 1 12 default depth`: 2,225,866 nodes, 1,404 ms, 1,585,374 nodes/s. Farzin's release bench returned 992,681 nodes, 1,080 ms, 919,149 nodes/s. These single observations overlapped screening and are **not** a valid speed ratio to the official i7-4770K. No verified official reference measurement for identical benchmark parameters was obtained. All time controls are therefore nominal and unadjusted.

Official conditions verified at https://computerchess.org.uk/4040/about.html (also available under `/ccrl/4040/about.html`) and https://computerchess.org.uk/ccrl/404/about.html: reference i7-4770K with Stockfish 10 normalization; Blitz 2m+1s, repeating 40 moves/15 minutes, ponder off, generic book ≤12 moves per side; equal 128/256 MB hash for Blitz and 256/512 MB for 40/15; consistent 4–6-piece TB access. Our absence of TBs, small suite, virtualized hardware and short control are material differences.

Graham Banks's published update, 7 June 2025, https://talkchess.com/viewtopic.php?t=85157 , confirms i7-4770K (3.5 GHz)/Stockfish 10 adjustment, use of AVX2 compiles where available and warns about early rating fluctuation. It mentions 15m+10s as well; the requested future verification here deliberately uses repeating **40/900**, not sudden death or 15m+10s.

## 5. Executed empirical results

All W/D/L are from Farzin's perspective. Anchored figures in this table are **screening performance only**, with provisional paired bootstrap intervals including the assumed published anchor uncertainty.

| Opponent | W/D/L | Score | Games | Nominal control | Screening Elo | Provisional 95% interval |
|---|---:|---:|---:|---|---:|---:|
| Stockfish-10 | 20/17/3 | 71.2% | 40 | 10+0.1 | 3667 | 3590–3751 |
| Weiss-2.0 | 28/11/1 | 83.8% | 40 | 10+0.1 | 3603 | 3511–3728 |
| Ethereal-12.00 | 29/8/3 | 82.5% | 40 | 10+0.1 | 3659 | 3575–3776 |

Wall runtime recorded by the runner: 1764.4 seconds. Terminations: `{"adjudication": 88, "checkmate": 6, "fifty_moves": 4, "insufficient_material": 3, "max plies": 1, "threefold_repetition": 18}`. Scheduler status: `{"cancelled": 0, "completed": 60, "failed": 0, "skipped": 0}`. Measured game-search NPS and average depth are in the raw JSON `engine_stats`; these are workload measurements, not hardware normalization factors.

## 6. Statistics, consistency and limits

`tools/ccrl/estimate.py` solves a single-target, multi-anchor Bradley–Terry logistic score likelihood with fixed anchor centers. Draws contribute 0.5. Uncertainty uses 4,000 opening-block bootstrap samples, seed 20261112; both color games remain together. With identical opening coverage across anchors, shared FEN blocks are resampled jointly across opponents; otherwise sampling is stratified by opponent. Each published anchor is independently perturbed by a normal error with standard deviation equal to reported error/1.96. This interpretation of reference errors and independence is an explicit approximation. Correlated reference-list errors, draw-model mismatch, heterogeneous opponent styles and hardware/time-control conversion are **not covered** by the numeric interval. The paired bootstrap is conditional on this small opening suite and may under-cover, particularly at extreme scores.

All-loss/all-win samples return infinite estimates, rather than clipped Elo. If all within-anchor opening-pair scores are identical, the bootstrap cannot estimate sampling variation and the interval is explicitly withheld; reference-rating noise alone is not presented as a game-sampling interval. Missing anchors, incomplete pairs, invalid results and disconnected target pairs are rejected. Ten dedicated estimator tests, one book-clock regression test and thirteen existing arena tests passed. `tools/rating.py` was reviewed but not used: it silently drops an odd final game, lacks connectivity validation, uses unstabilized exponentials, and its finite iteration limit can disguise saturation; bootstrap samples can also lose players. Its general fitter was left untouched; the new restricted estimator avoids those paths.

Sensitivity: excluding whole pairs containing adjudication, max-plies, crash/illegal or time-forfeit endings leaves 5 pairs. The conditional natural-pair-only result is `{"bootstrap": 4000, "ci95": null, "ci_status": "not estimable: no observed within-anchor pair-score variation", "method": "stratified paired-opening percentile bootstrap; independent Gaussian anchor uncertainty", "pairs": {"Ethereal-12.00": 3, "Stockfish-10": 1, "Weiss-2.0": 1}, "rating": 3509.8272335744323, "seed": 20261112}`. This five-pair selected subset has no observed within-anchor pair-score variation, so no sampling interval is justified. It is a diagnostic, not an unbiased replacement or a counterfactual no-adjudication replay. No adjudicated games were secretly discarded from primary results. Per-opponent estimates above expose discrepancies rather than hiding them in a pooled number.

The reference repository `gustavoknudsen/chess-elo-gauntlet` was inspected at `06998b6536b0684bdfaa10ecd34623cef80641ba`. Its score-logit, inverse-variance aggregation and adaptive nearest-anchor sampling are useful design references, but require careful boundary handling and do not by themselves establish CCRL equivalence. Its archived results were not used as our evidence or rating anchors. No weakened Stockfish measurements were used.

## 7. Reproducibility package and pending execution

The accompanying PR contains `tools/ccrl/` (hash-gated runner, estimator, tests and independent EPD), `benchmarks/ccrl-calibration/manifest.json`, result summary, raw PGN/JSON evidence archive with checksums, and this document. Large binaries are excluded. Engine sources and licenses are linked in the manifest; reproduce local opponent builds at those exact commits using GCC and the recorded build commands. GCC version, build logs, release metadata, checksums, benchmarks, rating snapshots and method pages are in the evidence archive. Local paths in the manifest must be updated after rebuilding, and executable hashes must be checked rather than bypassed.

```sh
python3 -m venv .venv
.venv/bin/pip install python-chess==1.999 beautifulsoup4==4.15.0
.venv/bin/python -m unittest discover -s tools/ccrl
# Planned full first pass, NOT executed results:
.venv/bin/python tools/ccrl/run.py blitz --out results/blitz-first-pass
# Repeating 40 moves/900 seconds, 20 pairs each, 256 MB:
.venv/bin/python tools/ccrl/run.py 4015 --out results/4015-first-pass
# Recompute screening only from extracted raw data:
.venv/bin/python tools/ccrl/estimate.py results/raw/screening.json benchmarks/ccrl-calibration/manifest.json
```

The original screening command is archived verbatim. The default pending Blitz first pass is 25 opening pairs against each of four anchors (200 games), followed by fixed, declared batches extending toward ≥300 games, allocating extra pairs to the two nearest anchors while retaining family coverage. Any adaptive estimate/interval must account for the stopping design; the present bootstrap has no sequential-coverage guarantee. The current suite contains only 20 unique positions, so 25-pair runs necessarily repeat five positions; replace it with a frozen larger independent suite **before** the full calibration and cluster repeated positions in analysis. Pending 40/15 first pass uses the three verified 40/15 anchors (120 games). A complete 300-game 120+1 campaign requires on the order of ten or more wall hours at this two-CPU quota; actual game length varies. No such games are implied by these commands.

## 8. Interpretation

**NOT AN OFFICIAL CCRL RATING.** These are independent screening matches. They do not establish a 3300–3400 CCRL rating, eligibility or future inclusion. Reliable Blitz and 40/15 numbers require the longer controls, justified reference hardware normalization, more independent openings and adequate sample size. This bounded screening run preserves evidence and tooling; the primary calibration and long-control verification remain pending. No paid service was provisioned.

## 9. Summary for Graham Banks

Dear Graham,

Please find the reproducible preliminary screening evidence for the public Farzin 0.5.2 OwnNet release. We verified its release checksum and disclosed its distilled teacher-label provenance. The 120 recorded games use exact, independently rated opponent versions at one thread, with paired colors and preserved PGNs. Their 10+0.1 screening control and uncalibrated virtual hardware do not justify a CCRL rating claim; the longer calibration remains pending. We would welcome independent review of the release and evidence, subject entirely to your normal testing priorities and policies.

Evidence: [manifest](../benchmarks/ccrl-calibration/manifest.json), [summary](../benchmarks/ccrl-calibration/screening-summary.json), and [reproduction tools and raw archive](../tools/ccrl/).
