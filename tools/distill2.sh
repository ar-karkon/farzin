#!/bin/sh
# Farzin 0.4 teacher -> student distillation into a king-bucketed FZN2 net.
# Resumable: data is appended, training resumes from its checkpoint.
#
#   tools/distill2.sh POSITIONS HIDDEN [THREADS]
#
# 1. "Tree sampling" datagen: Farzin plays itself with the teacher net
#    (default: the embedded HalfKAv2_hm net) at 2500 soft nodes per move and
#    records one in TREE_RATE positions evaluated *inside* its search trees,
#    labelled with the teacher's static evaluation. This samples exactly the
#    distribution the search queries, at ~9k positions/s per core (vs ~1.5k
#    for one labelled game position per move).
# 2. farzin-train2 fits an FZN2 (KB x 768 -> HIDDEN) x 2 -> 1 x 8 student
#    (king buckets KB, default 16m = 16 mirrored buckets).
# 3. SPRT of the student against the previous own net (BASE_NET).
#
# Environment: WORK (default ./distill2-work), KB (16m), TREE_RATE (48),
# EPOCHS (10), TEACHER (empty = embedded net), BASE_NET (nets/d1-512.nnue),
# TC (10+0.1), PYTHON (python3).
set -eu
TARGET=$1
HIDDEN=$2
THREADS=${3:-$(nproc)}
WORK=${WORK:-distill2-work}
KB=${KB:-16m}
TREE_RATE=${TREE_RATE:-48}
EPOCHS=${EPOCHS:-10}
BASE_NET=${BASE_NET:-nets/d1-512.nnue}
TC=${TC:-10+0.1}
PY=${PYTHON:-python3}
BIN=target/release/farzin
mkdir -p "$WORK"
DATA="$WORK/tree.data"

cargo build --release
have=0
[ -f "$DATA" ] && have=$(( $(stat -c %s "$DATA") / 32 ))
if [ "$have" -lt "$TARGET" ]; then
    need=$(( (TARGET - have) / 4000 + 1 ))   # ~4000 sampled positions per game
    echo "have $have positions, generating ~$need more games"
    set -- datagen threads="$THREADS" games="$need" soft_nodes=2500 hard_nodes=20000 \
        tree_rate="$TREE_RATE" random_move_pm=50 random_plies=8 out="$DATA" seed="$(date +%s)"
    [ -n "${TEACHER:-}" ] && set -- "$@" evalfile="$TEACHER"
    "$BIN" "$@"
fi

NET="$WORK/fzn2-$KB-$HIDDEN.nnue"
target/release/farzin-train2 data="$DATA" out="$NET" hidden="$HIDDEN" kb="$KB" \
    epochs="$EPOCHS" threads="$THREADS" resume=1

"$PY" tools/arena.py --engine name=student,cmd=$BIN,opt.EvalFile="$NET" \
    --engine name=base,cmd=$BIN,opt.EvalFile="$BASE_NET" --tc "$TC" --pairs 500 \
    --concurrency "$THREADS" --sprt 0 10 --out "$WORK/sprt-$KB-$HIDDEN.json"
