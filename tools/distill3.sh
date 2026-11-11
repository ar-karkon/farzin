#!/bin/sh
# Farzin 0.5 Track B: *cumulative* teacher -> student distillation.
#
#   tools/distill3.sh INIT_NET POSITIONS KB HIDDEN [THREADS]
#
# Unlike distill2.sh (random init), every round warm-starts from the best
# existing own net (INIT_NET), so compute spent in earlier rounds - on any
# machine - is never thrown away. The student may grow between rounds
# (e.g. 4m x 768 -> 16m x 768, or a wider hidden layer): new king buckets
# start from the bucket that covered the same king squares and new neurons
# start with zero output weight, so round N+1 starts exactly at round N.
#
# Each round: fresh tree-sampled positions (new seed, so data never repeats
# across rounds/machines), a few epochs, then an SPRT against INIT_NET.
# Everything is resumable: the data file is appended and training resumes
# from its float checkpoint.
#
# Environment: WORK (distill3-work), EPOCHS (3), LR (0.0005), TREE_RATE (48),
# TC (5+0.05), PAIRS (300), TEACHER (empty = embedded net), PYTHON (python3).
set -eu
INIT=$1
TARGET=$2
KB=$3
HIDDEN=$4
THREADS=${5:-$(nproc)}
WORK=${WORK:-distill3-work}
EPOCHS=${EPOCHS:-3}
LR=${LR:-0.0005}
TREE_RATE=${TREE_RATE:-48}
TC=${TC:-5+0.05}
PAIRS=${PAIRS:-300}
PY=${PYTHON:-python3}
BIN=target/release/farzin
mkdir -p "$WORK"
DATA="$WORK/tree.data"

cargo build --release
have=0
[ -f "$DATA" ] && have=$(( $(stat -c %s "$DATA") / 32 ))
if [ "$have" -lt "$TARGET" ]; then
    need=$(( (TARGET - have) / 2900 + 1 ))   # ~2900 sampled positions per game
    echo "have $have positions, generating ~$need more games"
    set -- datagen threads="$THREADS" games="$need" soft_nodes=2500 hard_nodes=20000 \
        tree_rate="$TREE_RATE" random_move_pm=50 random_plies=8 out="$DATA" seed="$(date +%s)"
    [ -n "${TEACHER:-}" ] && set -- "$@" evalfile="$TEACHER"
    "$BIN" "$@"
fi

NET="$WORK/fzn2-$KB-$HIDDEN.nnue"
target/release/farzin-train2 data="$DATA" out="$NET" init="$INIT" hidden="$HIDDEN" kb="$KB" \
    epochs="$EPOCHS" lr="$LR" threads="$THREADS" resume=1

"$PY" tools/arena.py --engine name=student,cmd=$BIN,opt.EvalFile="$NET" \
    --engine name=init,cmd=$BIN,opt.EvalFile="$INIT" --tc "$TC" --pairs "$PAIRS" \
    --concurrency "$THREADS" --sprt 0 10 --out "$WORK/sprt-$KB-$HIDDEN.json"
