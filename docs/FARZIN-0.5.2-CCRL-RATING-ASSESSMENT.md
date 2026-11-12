# Farzin 0.5.2 OwnNet — Independent CCRL-Aligned Strength Assessment

## 1. Executive Summary

This Phase 2 feasibility study tests the published Farzin 0.5.2 OwnNet executable at the nominal CCRL Blitz time control, **120 seconds plus one second per move**, against an independently rated reference engine. It is an anchored local experiment, not an official CCRL rating or a hardware-normalized CCRL submission. The primary pilot and separate adaptive diagnostic are substantially below the requested 300 games. Numerical results below must be interpreted as pilot evidence.

The fixed primary batch completed **eight games against Stockfish 16: 0 wins, 2 draws, 6 losses (12.5%)**. Its anchored local score estimate is **3422 Elo**. Four independent openings are too few for a stable rating: the descriptive paired bootstrap is −∞–3570, while a conservative fixed-anchor bounded-pair check is −∞–4005. These bounds do not include hardware/configuration uncertainty. The earlier 3648 estimate is not corroborated by this pilot, but it is not precisely disproved.

The released Farzin executable is unchanged. No search, evaluation, network, time-management, or strength settings were altered. The earlier 3648 estimate was a 10+0.1 screening result; neither that number nor the informal 3300–3400 expectation was used as an experimental observation in Phase 2.

## 2. Engine Identification and Provenance

The official Linux x64 AVX2/BMI2 asset was downloaded from [release v0.5.2](https://github.com/ar-karkon/farzin/releases/tag/v0.5.2).

| Input | SHA-256 |
|---|---|
| Farzin executable | `324b236a5a097350d7bf3047f994ef1e3d69907f913aec753e1cb61d69158d1c` |
| Embedded FZN2-b1 network | `3ec5aa7e6c16b1ea6bd366ad873f4b649da310c91204915000bfd5f1e90ebfc0` |

Both hashes were calculated locally. Network verification hashes 4,744,824 bytes at release offset 994,387, independently of the version string. `--version` reports Farzin 0.5.2, edition OwnNet, target x86_64, popcnt/avx2/bmi2, PEXT sliders and FZN2-b1. The UCI identity and advertised options are preserved in `benchmarks/ccrl-phase2/uci-options.json`. Stockfish's network is used only inside the Stockfish opponent.

## 3. Experimental Environment

The environment is a Linux KVM virtual machine; exact CPU identification, OS and quota are recorded in `environment.json`. The cgroup permits 200,000 microseconds per 100,000-microsecond period: two CPU equivalents. Two concurrent games use one search thread per engine. Pondering is off; only the side to move searches. Engine processes are reused within each worker. There is no dedicated physical-core isolation or guarantee against host contention. Quota and throttling observations do not prove dedicated hardware.

Equal 128 MB hash is requested for each engine. The published Farzin defaults otherwise remain unchanged. Neither engine uses tablebases or an internal opening book. Python-chess 1.999 (chess 1.11.2) runs the games. Wall-clock elapsed search time is charged to clocks with zero additional tournament margin; release-internal time-management defaults remain active. Scheduling noise is therefore a possible contributor to forfeits, which are retained.

## 4. CCRL Rating Anchors

The [CCRL Blitz complete list](https://computerchess.org.uk/ccrl/404/rating_list_all.html), retrieved November 12, identifies November 7, 2026 as its latest-games date. The exact source HTML is retained.

| Candidate, exact release | One-CPU Blitz Elo | Published ± error | Use |
|---|---:|---:|---|
| Stockfish 16 64-bit | 3760 | 9 | Measured pilot anchor |
| Ethereal 14.25 64-bit | 3683 | 7 | Investigated; no games |
| Stockfish 13 64-bit | 3661 | 13 | Investigated; no games |
| Stockfish 12 64-bit | 3656 | 12 | Investigated; no games |
| Berserk 9 64-bit | 3631 | 13 | Adaptive independent-family diagnostic |

All entries above are single-CPU rows, not the separately listed 8CPU entries. Stockfish 16 is the official `sf_16` Ubuntu x86-64 AVX2 release, with its embedded release network, GPLv3 license, full strength and default search settings except equal hash and one thread. Its checksum and asset URL are recorded in the manifest. This verifies the release version and local configuration, but does not establish byte identity with CCRL testers' executables or reconstruct their individual games' tablebase and hardware configurations.

The fixed primary pilot has one engine family. After its first four games were all checkmate losses, a separate two-pair diagnostic against the independently developed Berserk 9 was frozen. This is adaptive selection, explicitly recorded before Berserk outcomes in `diagnostic-decision.json`, and is not pooled as confirmatory calibration evidence. Berserk was built from the exact release tag source, commit `d6c81447c0bddfdf56a30628f4f1ca36ec04a117`, with its release network, using GCC and AVX2/BMI2/PEXT. The full compiler command, log, binary and network checksums are preserved. It was compiled and played only after the primary batch ended. Weakened Stockfish is not used.

## 5. Testing Methodology

### Openings and fixed stopping rule

The opening suite was frozen before outcomes. It contains 150 unique four-move-per-side positions from the CC0 Lichess opening database at the source commit in `opening-provenance.json`. The reproduction script parses all five ECO TSV files, takes the first eight plies of eligible lines, deduplicates FEN positions, sorts them and samples 150 with seed 20261112. It uses no engine evaluation, result, or Farzin-specific selection. The source labels allow external inspection of the range of open, semi-open, closed and flank structures. No claim of uniform structure sampling is made: this is a generic opening catalogue, not a balanced stratified experimental design. Different branches may share substantial ancestry, even with distinct final positions.

The pilot uses four randomly scheduled positions, each twice with colors reversed, for eight planned games. Its size and stopping rule were fixed in the committed manifest before results. There is no score-based stopping within the primary batch. The separate Berserk diagnostic is adaptive and uses two of the same opening positions; these must not be counted as two additional independent openings. Four observed positions cannot demonstrate coverage of the full 150-position suite. The suite must not be retrospectively filtered. Book plies count toward move numbers and recurring-control boundaries; eight plies is within CCRL's maximum 12 moves per side.

### Adjudication and game integrity

Evaluation adjudication is disabled for every pilot game. Games continue to checkmate or rule termination, with a 1000-ply guard. Guard draws, crashes, illegal moves and time forfeits are separately reported and retained. Claimable repetition and fifty-move draws are automatically claimed by the interface. No result is inferred by removing previously adjudicated games.

The Phase 1 adjudicator required alternating white-relative evaluations from both engines to agree across consecutive plies. This reduces unilateral resignation errors but cannot eliminate incompatible evaluation scales, calibration differences or shared errors. Its draw thresholds can also terminate positions that would later become decisive. The earlier 88/120 adjudication fraction therefore warrants concern but is not itself proof of bias. Phase 2 removes that mechanism. Without matched adjudication-on/off batches at the same control, the effect cannot be isolated numerically.

PGNs and JSON are written after complete color pairs. An interruption during a pair can lose its unfinished or completed first game from persistent output; such games must never be silently counted. This pilot is valid only after clean scheduler completion and PGN replay. Existing output prefixes must not be reused. The runner is not a general restart-from-incomplete-pair system.

## 6. Results by Opponent

### Fixed primary batch: Stockfish 16

| Opponent | Games / pairs | Farzin W/D/L | Points | Score | Anchored local score estimate |
|---|---:|---:|---:|---:|---:|
| Stockfish 16 (3760 ±9) | 8 / 4 | 0 / 2 / 6 | 1 / 8 | 12.5% | 3422 |

All eight PGNs replay legally and agree with JSON and rule outcomes. Six games ended in checkmate, one in threefold repetition and one in insufficient material. Stalemate, fifty-move rule, resignation adjudication, draw adjudication, maximum-ply guard, time forfeits, crashes and illegal moves each occurred zero times. Farzin scored 0.5/4 (12.5%) with each color. Equal color scores in four games per color are not evidence that color effects are absent.

The descriptive paired bootstrap interval is **−∞ to 3570 Elo**, including the stated Gaussian anchor perturbation. This is not a calibrated 95% real-world confidence interval. Some resamples contain only losses, giving an unbounded lower estimate. The arena log's clamped “−2400” interim display is not a rating measurement. The audited anchored estimate uses the unclamped score equation.

A conservative sensitivity calculation treats each opening pair’s score fraction as a bounded observation in [0,1]. Under independent pair sampling, Hoeffding’s two-sided 95% bound is the observed score ±sqrt(log(40)/(2n)), clipped to [0,1]. With n=4 this gives [0, 0.8041], mapping through the fixed-3760-anchor score equation to **−∞ to 4005 Elo**. This bound excludes anchor and hardware uncertainty and applies only under the stated sampling assumptions. Its width demonstrates that the bootstrap endpoint should not be used to rule out 3648. The generic arena `elo_95`, `los` and normalized-Elo fields are small-sample approximations and are not used as confidence evidence here.

Dropping one opening pair at a time gives estimates of 3343 or 3480. This shows opening sensitivity but is not an independent validation sample. Four openings are insufficient for reliable tail inference or a stable performance rating.

### Adaptive independent-family diagnostic

DIAGNOSTIC_PENDING

## 7. Rating Estimation and Confidence Intervals

The existing estimator solves the anchored logistic expected-score equation, with draws worth half a point. This is a Bradley–Terry-style score fit, not a fully specified likelihood for three-outcome chess data. Calling it a maximum-likelihood rating requires an explicit outcome likelihood and assumptions that are not justified here. Consequently this report labels the number an anchored score estimate.

Resampling keeps both colors of each opening together. The estimator includes independent Gaussian anchor perturbations, interpreting the published error as a 95% half-width. That interpretation is a modeling assumption; reference errors can be correlated through the common rating pool. The script rejects repeated positions within an opponent's sample rather than counting them independently. Identical opening coverage across opponents is block-resampled together. An extended design with partially overlapping repeated openings needs a global FEN-cluster bootstrap before pooling; the existing estimator is not sufficient for that design.

Four pairs are too few for reliable bootstrap tail coverage. Any percentile interval is a descriptive conditional calculation, not evidence of 95% real-world coverage. Degenerate pair-score samples cannot produce an informative empirical interval. A Davidson draw-aware fit would introduce a draw parameter, and opponent-specific draw tendencies would add more; this sample cannot establish meaningful improvement or identify a robust draw model. More data are required for defensible maximum likelihood, profile intervals, color effects and draw-model comparison. A distribution-free bounded-pair sensitivity bound is also reported; it is much wider than the empirical bootstrap and still conditional on independent pair sampling and a fixed anchor. No narrow rating claim is warranted.

A four-game adaptive second-opponent diagnostic can indicate opponent sensitivity but cannot quantify it reliably. Color scores and pair counts are descriptive; opening-selection and batch-stability conclusions need independent batches. No bootstrap accounts for hardware normalization, configuration mismatch, unobserved host contention or time-control transfer.

## 8. Hardware and Time-Control Limitations

The preserved [CCRL Blitz conditions](https://computerchess.org.uk/ccrl/404/about.html) specify 2+1 on an Intel i7-4770K, using Stockfish 10 to determine the equivalent time control on other machines. Conditions include pondering off, equal 128 or 256 MB hash, generic books limited to 12 moves per side, learning off and 4–6-piece tablebases.

We match the nominal clock, thread count, hash, pondering and paired generic openings. We do not reproduce physical reference hardware, validated speed normalization or tablebases. No identical-binary i7-4770K benchmark measurement was available to establish a defensible normalization factor. A local NPS measurement alone cannot solve that problem or justify an Elo correction. No factor or universal NPS-to-Elo adjustment has been invented. Results are anchored local measurements.

### Comparison with Phase 1

| Study | Control | Games | Anchored score estimate | Interpretation |
|---|---|---:|---|---|
| Phase 1 | 10+0.1 | 120 | 3648; conditional 95% 3603–3699 | Short-control screening |
| Phase 2 primary | 120+1 | 8 | 3422; descriptive bootstrap −∞–3570; conservative fixed-anchor bound −∞–4005 | Unnormalized four-opening pilot |

The Farzin binary is identical. Changes in estimate could reflect sampling, stronger opponents, opening selection, removal of adjudication, relative time-control scaling, or virtual hardware conditions. This is not a controlled before/after experiment, and overlap or non-overlap of conditional intervals would not identify an engine-strength change. The primary result does not corroborate 3648: its point estimate is approximately 226 Elo lower. Although its descriptive bootstrap upper endpoint is below the Phase 1 interval, four pairs do not justify a calibrated significance test or a claim of a 226-point strength change. Changes in control, opponents and adjudication are confounded. Phase 2 cannot precisely validate or refute 3648 with this sample.

### CCRL 40/15

No 40/15 games were completed in this study. Its rating is **not estimable**. The runner implements `40/900` as recurring 900-second allocations every 40 moves, not sudden death; tests cover the first boundary after four book moves. That implementation check is not long-control strength evidence. Subsequent work must verify exact 40/15 opponent entries, normalize hardware, retain appropriate book-move accounting, and play independent reversed-color pairs. Blitz anchors or fixed rating offsets must not be substituted.

## 9. Reproducibility and Raw Evidence

The [primary PGN](../benchmarks/ccrl-phase2/raw/blitz.pgn), [primary JSON](../benchmarks/ccrl-phase2/raw/blitz.json), [audited analysis](../benchmarks/ccrl-phase2/primary-analysis.json), and [manifest](../benchmarks/ccrl-phase2/manifest.json) are available for inspection.

All new evidence is in `benchmarks/ccrl-phase2/`; the earlier archive remains unchanged. Inputs include release and executable checksums, opening commit and labels, frozen manifest, UCI options, hardware observations and source rating-list HTML. `raw/blitz.command.json` records the exact command; `raw/blitz.pgn`, `raw/blitz.json` and the log retain the completed match. `SHA256SUMS` covers evidence files. External engine executables and large temporary downloads are excluded from Git.

Recreate the environment with the pinned packages in the calibration README, obtain the release assets, verify manifest hashes and adapt executable paths to the new machine. Run the recorded command with a fresh output prefix. To reproduce the opening suite and audit results:

```sh
.venv/bin/python tools/ccrl/make_phase2_book.py /path/to/pinned/chess-openings > /tmp/openings.epd
cmp /tmp/openings.epd benchmarks/ccrl-phase2/openings.epd
.venv/bin/python tools/ccrl/audit_phase2.py benchmarks/ccrl-phase2/raw/blitz.json benchmarks/ccrl-phase2/manifest.json
(cd benchmarks/ccrl-phase2 && sha256sum -c SHA256SUMS)
```

Reproducibility means inspectable inputs and legal game replay; time-controlled searches on virtual hardware are not expected to reproduce identical moves or results.

## 10. Conclusions and Interpretation

The experiment contributes actual nominal-120+1, evaluation-adjudication-free games against a much stronger anchor than the screening opponents. It does not meet the 300-game objective, establish a stable result across opponent families, establish a reliable CCRL-equivalent number or supply 40/15 evidence. The primary anchor did not achieve the desired near-50% score. Further selection should include verified anchors in roughly the 3300–3500 range as well as independent stronger references, without treating this pilot point as established strength. A stable multi-anchor estimate needs substantially more independent pairs, including independent engine families near the measured score, matched adjudication validation, and reference-hardware benchmarking. Resources should prioritize those measurements over fitting increasingly elaborate models to this pilot.

## 11. Short Summary for CCRL Testers

Farzin 0.5.2 OwnNet was tested as the exact published Linux executable, with verified binary and embedded-network hashes, at nominal 120+1, one thread, 128 MB hash and pondering off. The pilot uses reversed-color opening pairs and no evaluation adjudication. Full PGNs and configuration evidence are supplied. These are local virtual-machine results without validated CCRL hardware normalization or tablebases. Please treat them as preliminary evidence and a reproducible starting point for independent CCRL testing; no official rating is claimed.
