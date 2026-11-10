#!/bin/sh
# Teacher -> student NNUE distillation for Farzin (resumable).
#
#   tools/distill.sh TEACHER.nnue POSITIONS HIDDEN [THREADS]
#
# 1. Self-play with a cheap mover (BASE_NET, 400 soft nodes) chooses
#    the moves; every quiet position is labelled with TEACHER's static eval
#    (`datagen teacher=`). Data is appended, so re-running continues where the
#    previous run stopped.
# 2. farzin-train fits an FZN1 (768 -> HIDDEN)x2 -> 1 student on the labels.
# 3. The student is screened against BASE_NET at fixed nodes, then
#    tested with SPRT at 10+0.1.
#
# Environment: BASE_NET (default nets/g2.nnue), WORK (default ./distill-work), PYTHON (default python3).
set -eu
TEACHER=$1
TARGET=$2
HIDDEN=$3
THREADS=${4:-$(nproc)}
WORK=${WORK:-distill-work}
PY=${PYTHON:-python3}
BIN=target/release/farzin
# Opponent net for screening (default: the previous self-trained g2 net).
BASE_NET=${BASE_NET:-nets/g2.nnue}
mkdir -p "$WORK"
DATA="$WORK/teacher.data"

cargo build --release
have=0
[ -f "$DATA" ] && have=$(( $(stat -c %s "$DATA") / 32 ))
if [ "$have" -lt "$TARGET" ]; then
    need=$(( (TARGET - have) / 90 + 1 ))   # ~90 stored positions per game
    echo "have $have positions, generating ~$need more games"
    "$BIN" datagen evalfile="$BASE_NET" threads="$THREADS" games="$need" soft_nodes=400 hard_nodes=2000 \
        teacher="$TEACHER" out="$DATA" seed="$(date +%s)"
fi

NET="$WORK/student-$HIDDEN.nnue"
target/release/farzin-train data="$DATA" out="$NET" hidden="$HIDDEN" epochs=12 \
    wdl=0.0 scale=400 threads="$THREADS" lr=0.001

"$PY" tools/arena.py --engine name=student,cmd=$BIN,opt.EvalFile="$NET" \
    --engine name=base,cmd=$BIN,opt.EvalFile="$BASE_NET" --nodes 20000 --pairs 100 --concurrency "$THREADS" \
    --out "$WORK/screen-$HIDDEN.json"
"$PY" tools/arena.py --engine name=student,cmd=$BIN,opt.EvalFile="$NET" \
    --engine name=base,cmd=$BIN,opt.EvalFile="$BASE_NET" --tc 10+0.1 --pairs 300 --concurrency "$THREADS" \
    --sprt 0 5 --out "$WORK/sprt-$HIDDEN.json"
