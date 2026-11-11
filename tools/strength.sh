#!/bin/sh
# Absolute-strength protocol: Farzin against independent reference engines
# at several time controls, with conservative adjudication, and a fixed-node
# run that separates search-tree quality from raw speed.
#
#   tools/strength.sh FARZIN_BIN REF_NAME=REF_CMD [REF_NAME=REF_CMD ...]
#
# Environment: PAIRS (default 100), CONC (default nproc-1), OUT (results/strength),
# TCS (default "5+0.05 10+0.1 30+0.3"), NODES (default 100000), PYTHON.
set -eu
BIN=$1; shift
PAIRS=${PAIRS:-100}
CONC=${CONC:-$(( $(nproc) - 1 ))}
OUT=${OUT:-results/strength}
TCS=${TCS:-"5+0.05 10+0.1 30+0.3"}
NODES=${NODES:-100000}
PY=${PYTHON:-python3}
mkdir -p "$OUT"
for ref in "$@"; do
    name=${ref%%=*}
    cmd=${ref#*=}
    for tc in $TCS; do
        "$PY" tools/arena.py --engine name=farzin,cmd="$BIN" --engine name="$name",cmd="$cmd" \
            --tc "$tc" --pairs "$PAIRS" --concurrency "$CONC" --adjudicate conservative \
            --out "$OUT/$name-$tc.json" --pgn "$OUT/$name-$tc.pgn"
    done
    "$PY" tools/arena.py --engine name=farzin,cmd="$BIN" --engine name="$name",cmd="$cmd" \
        --nodes "$NODES" --pairs "$PAIRS" --concurrency "$CONC" --adjudicate conservative \
        --out "$OUT/$name-nodes$NODES.json"
done
