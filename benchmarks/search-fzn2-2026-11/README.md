# FZN2 search experiments

See [the Persian report](../../docs/search-strength-2026-11-fa.md) for decisions,
limitations and results. `manifest.json` identifies the baseline, tested core
commit, network, compiler and binaries. `*.resource.json` records commands and
aggregate child CPU; peak RSS is the largest individual child, not aggregate RSS.
Large accumulator bench reports and validation PGNs are losslessly compressed
as `.gz`; use `gzip -dc FILE.gz` to inspect them. The small
`accumulator-benchmark-summary.json` keeps their readable summary and raw hashes.

The baseline is main commit `0657431a154d3d69951173d4bdbde9af7474c535`.
All experiments explicitly use `nets/fzn2-a1.nnue`. No teacher-network result is
used for selection. Experimental patches are relative to this baseline unless
their headers say otherwise. Rejected patches are preserved for reproducibility.

- `qevasions.patch`: remove late quiet-evasion pruning in qsearch.
- `fullasp.patch`: preserve requested depth after aspiration fail-high.
- `lazy-compact.patch`, `lazy-frame.patch`: lazy FZN2 accumulator prototypes,
  with lane-by-lane correctness tests. Bench JSON also includes the earlier
  feature-index prototype, whose binary remains in the workspace.
- `threatorder.patch`: threat-based quiet ordering and move-picker tests.
  The tested validation binary additionally contains the fifty-move mate fix.
- `residual-correction.patch`: correction-history residual learning experiment.

`*-n8k.json` are screening games. `threatorder-n32k.json` is a separate higher
node-budget test. Different seeds do not guarantee disjoint opening sets: five
openings overlap between threat-order screening and its 32k validation.
`holdout-80.fen` is disjoint from both selections and the discarded initial
timed selection. `timed-calibration-aborted*` is retained for transparency and
is excluded from validation. It was stopped when observed per-pair wall time
made the original allocation too expensive relative to the fixed-node result.

To rebuild the exact **experimental** threat-order candidate and original
baseline in separate worktrees, and repeat their fixed-node and timed matches:

```sh
# Rust and Python 3 with python-chess must already be available.
bash benchmarks/search-fzn2-2026-11/reproduce.sh /absolute/new/output
```

The script pins both source commits and checks the network hash. It keeps both
checkouts and write-protected binaries. It does not change `main`. Runtime and
binary hashes can differ across compilers and CPUs (`target-cpu=native`).

`diagnose.py` checks UCI PV legality and records iteration depth/seldepth, process
CPU and peak RSS; use `--depth` for throughput, since UCI info omits the unfinished
iteration at a hard node limit. `examples/search_audit.rs` instead calls the Rust
search API directly and exports the **actual** `SearchResult.nodes`, including
an interrupted iteration. It checks every returned PV for legality.
