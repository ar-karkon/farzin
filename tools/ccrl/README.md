# Farzin 0.5.2 independent calibration

Read `docs/CCRL-PRELIMINARY-RATING-0.5.2.md` before interpreting results.
Screening Elo does not estimate an official CCRL 2+1 or 40/15 rating.

- `manifest.json` in `benchmarks/ccrl-calibration/` freezes release and opponent binaries, source commits, build commands, licenses, book and official one-CPU rating rows.
- `run.py` refuses changed binary/book hashes and existing result filenames; it preserves nominal time controls and uses a quota-aware maximum of two concurrent single-thread games. Do not run phases simultaneously.
- `estimate.py` implements a stable one-target anchored score model and reproducible paired bootstrap. It rejects unknown anchors and malformed pairs, and exposes infinite boundary estimates. It is not a general connected-pool rating solver. Numeric intervals do not cover hardware or time-control systematics.
- `report.py` replays raw PGNs, cross-checks them against JSON and generates a screening report. It expects a single completed screening dataset, not arbitrary mixed time controls. Run it on extracted `screening.json` from the evidence archive.
- `freeze.py` records the original workspace's inputs and source-list rows. It depends on the original `/workspace/ccrl-inputs` snapshots and exact local source directories; it is a provenance capture utility, not a portable installer.

## Rebuild opponents

Clone source repositories at the manifest commits, then run the recorded build command **from the opponent repository root**. GCC 14.2.0 was used. For Weiss, removing `-Werror` allowed old source to compile with the current compiler; no source code was edited. A rebuild with different toolchains need not produce identical bytes. Preserve original binary checksums and label any new build as a new study input rather than silently replacing hashes.

Farzin must be downloaded from the linked public release, not rebuilt. Verify against public SHA256SUMS. Run `--version` and confirm OwnNet and the FZN2-b1 hash. Download links and source commit URLs are sufficient to retrieve legal inputs; no third-party executable is redistributed in this PR.

```sh
python3 -m venv .venv
.venv/bin/pip install python-chess==1.999 beautifulsoup4==4.15.0
.venv/bin/python -m unittest discover -s tools/ccrl -v
.venv/bin/python -m unittest discover -s tests -p test_arena.py
# Update local executable paths while retaining verified SHA-256 values.
.venv/bin/python tools/ccrl/run.py screening --out results/new-screening
.venv/bin/python tools/ccrl/run.py blitz --out results/blitz
.venv/bin/python tools/ccrl/run.py 4015 --out results/4015
```

The last two commands are pending protocols, not evidence of performed games.
Before a full 25-pair Blitz pass, freeze a larger independent opening suite:
the current suite contains only 20 unique positions and would repeat positions.
Repeated positions must be clustered in uncertainty estimates. No optional stopping
was applied to the original fixed 20-pair screening design.

## Evidence

The small compressed evidence archive preserves all original screening PGNs,
JSON per-game timing, runner logs, release metadata, official rating/method
snapshots, benchmark logs and build logs. It contains no engine binaries.
Verify its checksum using the accompanying `SHA256SUMS` before extraction.

Extract the evidence into an independent directory before analysis:

```sh
(cd tools/ccrl/evidence && sha256sum -c SHA256SUMS)
mkdir -p results
tar -xzf tools/ccrl/evidence/screening-evidence.tar.gz -C results
.venv/bin/python tools/ccrl/estimate.py results/raw/screening.json benchmarks/ccrl-calibration/manifest.json
.venv/bin/python tools/ccrl/report.py results/raw/screening.json
```

`--book-plies 8` in the new runner counts the four opening moves toward
repeating control boundaries; the original 10+0.1 command predates this
explicit counter option and retains its original recorded semantics.

## Phase 2 nominal 120+1 pilot

See `docs/FARZIN-0.5.2-CCRL-RATING-ASSESSMENT.md` and the separate frozen
`benchmarks/ccrl-phase2/manifest.json`. This is a small local pilot, not a
completed 300-game calibration. Existing Phase 1 data and tooling remain intact.

`make_phase2_book.py CHECKOUT` reproduces the 150-position suite using the
Lichess opening source commit in `opening-provenance.json`. Download official
Farzin and Stockfish 16 assets to the manifest paths (or adapt paths in a copy
without changing hashes). Reproduce with a fresh prefix:

```sh
.venv/bin/python tools/ccrl/run_phase2.py --out results/phase2-replication
.venv/bin/python tools/ccrl/audit_phase2.py results/phase2-replication.json benchmarks/ccrl-phase2/manifest.json
```

The runner refuses existing outputs and changed input bytes. It does not resume
an interrupted pair; completed pairs are saved atomically to JSON, with PGNs
flushed after each pair. Do not pool reruns or partially overlapping opening
batches without a global opening-cluster analysis. Compressed source HTML
snapshots are evidence, not executable instructions.
