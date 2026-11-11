#!/usr/bin/env bash
# Requires Rust, Python 3 with python-chess, Git, and an unused output directory.
# Keeps the immutable baseline checkout/binary. Does not change main or release files.
set -euo pipefail
research_repo=$(git rev-parse --show-toplevel)
research_output=${1:?Usage: bash benchmarks/search-fzn2-2026-11/reproduce.sh /absolute/new/output}
if [[ -e "$research_output" ]]; then
    echo 'Choose a new output directory to preserve existing experiments.' >&2
    exit 1
fi
python3 -c 'import chess.engine'
git diff --quiet
git diff --cached --quiet
mkdir -p "$research_output"
research_output=$(cd "$research_output" && pwd)
research_base=0657431a154d3d69951173d4bdbde9af7474c535
research_candidate=d114bab6eef8389b84f0af9da42ffb135fa7c625
research_network="$research_output/fzn2-a1.nnue"
cp "$research_repo/nets/fzn2-a1.nnue" "$research_network"
python3 - "$research_network" <<'PY'
import hashlib, sys
assert hashlib.sha256(open(sys.argv[1], 'rb').read()).hexdigest() == \
    'f7bb0b7ef9857c3a4f24b39f5152e381e978d76d0c24817b0bebbefb1b6e220c'
PY
git -C "$research_repo" worktree add --detach "$research_output/baseline-src" "$research_base"
cp "$research_repo/examples/search_audit.rs" "$research_output/baseline-src/examples/"
(
    cd "$research_output/baseline-src"
    FARZIN_EVALFILE="$research_network" CARGO_TARGET_DIR="$research_output/baseline-target" \
        cargo build --release --locked --bin farzin --example search_audit
)
cp "$research_output/baseline-target/release/farzin" "$research_output/baseline"
git -C "$research_repo" worktree add --detach "$research_output/candidate-src" "$research_candidate"
cp "$research_repo/examples/search_audit.rs" "$research_output/candidate-src/examples/"
(
    cd "$research_output/candidate-src"
    FARZIN_EVALFILE="$research_network" CARGO_TARGET_DIR="$research_output/candidate-target" \
        cargo build --release --locked --bin farzin --example search_audit
)
cp "$research_output/candidate-target/release/farzin" "$research_output/candidate"
chmod a-w "$research_output/baseline" "$research_output/candidate" "$research_network"
echo "$research_candidate" > "$research_output/candidate.commit"
rustc --version > "$research_output/rustc.version"
sha256sum "$research_output/baseline" "$research_output/candidate" "$research_network" > "$research_output/SHA256SUMS"
cd "$research_repo"
research_tools="$research_repo/benchmarks/search-fzn2-2026-11"
python3 "$research_tools/measure.py" "$research_output/fixed.resource.json" \
    python3 tools/arena.py \
    --engine "name=candidate,cmd=$research_output/candidate,opt.EvalFile=$research_network" \
    --engine "name=baseline,cmd=$research_output/baseline,opt.EvalFile=$research_network" \
    --nodes 32000 --pairs 150 --seed 2 --threads 1 --hash 16 --concurrency 2 \
    --adjudicate conservative --out "$research_output/fixed.json" --pgn "$research_output/fixed.pgn"
python3 "$research_tools/measure.py" "$research_output/timed.resource.json" \
    python3 tools/arena.py \
    --engine "name=candidate,cmd=$research_output/candidate,opt.EvalFile=$research_network" \
    --engine "name=baseline,cmd=$research_output/baseline,opt.EvalFile=$research_network" \
    --tc 5+0.05 --pairs 80 --seed 13 --threads 1 --hash 16 --concurrency 2 \
    --book "$research_tools/holdout-80.fen" \
    --adjudicate conservative --out "$research_output/timed.json" --pgn "$research_output/timed.pgn"
python3 "$research_tools/diagnose.py" --baseline "$research_output/baseline" \
    --candidate "$research_output/candidate" --network "$research_network" \
    --out "$research_output/diagnostics.json"
python3 "$research_tools/diagnose.py" --baseline "$research_output/baseline" \
    --candidate "$research_output/candidate" --network "$research_network" \
    --depth 14 --out "$research_output/diagnostics-depth14.json"
python3 "$research_tools/profile.py" \
    --binary "baseline=$research_output/baseline-target/release/examples/search_audit" \
    --binary "threat=$research_output/candidate-target/release/examples/search_audit" \
    --network "$research_network" --out "$research_output/api-profiles"
