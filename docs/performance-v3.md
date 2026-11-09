# Performance/search v3: reproducible experiments

This branch keeps classical evaluation and existing search defaults. `NNUE Stack`
is an opt-in UCI experiment; no trained weights are bundled or downloaded.

## Reproduction

Use the same stable Rust toolchain, release profile, CPU affinity, hash, threads,
and network file for both builds. Build the baseline in a separate worktree:

```sh
git worktree add --detach ../farzin-baseline efdb2307bded597dbf8c3fbcb01e9d33de3c4c29
cd ../farzin-baseline
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

From the development checkout, use the new runner with either executable and its
**actual source SHA**. The runner does not infer source identity from the current
working directory. It records the executable hash, explicit source SHA, tool
hash, compiler, OS/CPU features, affinity, cgroup quota, options and opening hash.
Reported wall NPS includes per-go startup overhead. Raw UCI depth/score/nodes/NPS
and individual timings are retained. Repeated totals use median, min/max and
sample standard deviation. Fixed-depth throughput is not a strength test.

```sh
python3 tools/bench_v3.py ../farzin-baseline/target/release/farzin \
  --commit efdb2307bded597dbf8c3fbcb01e9d33de3c4c29 --depth 4 --repeats 5 \
  --json benchmarks/v3-baseline/classical.json
python3 tools/synthetic_network.py /tmp/farzin-synthetic.pnn
# This file is deterministic UNTRAINED test data, never use it in strength tests.
python3 tools/bench_v3.py target/release/farzin --commit SOURCE_SHA \
  --depth 3 --repeats 5 --network-kind synthetic \
  --option EvalFile=/tmp/farzin-synthetic.pnn --option 'NNUE Stack=true' \
  --json /tmp/stack.json
cargo build --release --locked --example caissa_bench
./target/release/examples/caissa_bench /tmp/farzin-synthetic.pnn 10000
```

`tools/positions-v3.fen` has 22 manually selected positions spanning openings,
middlegames, quiet pawn endings, tactical exchanges, castling, en passant and
promotion. This is not a balanced competitive opening book. Network correctness
uses nonzero synthetic weights across all accumulator lanes and all eight raw
outputs, not only rounded/clamped centipawn scores.

## Accumulator architecture

`SearchEvaluator` reserves 128 frames (roughly 0.8 MiB per worker) once. Each
frame stores a Board, two 1536-wide i16 arrays and a validity bit. Push records
the Board and invalidates that depth; pop restores the parent by moving the
index. No network or heap buffer is copied/allocated on push/pop. Evaluation
finds the nearest valid ancestor, then lazily materializes the needed path.

The transformer transition uses piece/color bitboard XOR differences to obtain
removed/added features; this includes captures, promotions, en passant and the
rook displacement in castling. Each perspective's king bucket and mirror are
checked before applying deltas; a changed context refreshes that perspective.
All arithmetic preserves the original wrapping i16 semantics. Refresh uses a
fixed 32-feature buffer without allocating. Sibling states are invalidated even
when search skips evaluation. Search unwinds both Game and evaluator before
propagating stop or alpha-beta/PVS return values. Maximum supported search depth
64 plus quiescence 16 plus boundary evasion stays below frame capacity.

The old last-evaluated-board cache remains available with `NNUE Stack=false`.
Its transformer transition also benefits from the allocation-free bitboard
update; compare against the original baseline executable to isolate that change.
Toy `src/nnue.rs` remains separate and unused in the engine.

## Acceptance limits and remaining milestones

A local licensed Caissa v15 model and its reference engine are absent in this
environment. Historical reference JSON is not a fresh validation. Real-model
raw-output compatibility, throughput and playing-strength acceptance remain
pending. Do not enable experiments by default from synthetic timings.

After real-model accumulator/SIMD validation, proceed independently with history
persistence/aging, PVS validation, conservative LMR, guarded null move, persistent
TT ownership/invalidation, then parallel scaling and statistically designed
paired matches. Keep TT's halfmove-zero score guard. Shared TT requires separate
memory-model validation and measured benefit. No Elo, SPRT or external-engine
strength conclusion follows from these performance experiments.
