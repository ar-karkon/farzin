#!/usr/bin/env bash
# One NNUE generation: self-play data -> train -> fixed-node screen -> SPRT.
#
#   tools/pipeline.sh GEN POSITIONS [THREADS]
#   e.g. tools/pipeline.sh 2 20000000 64
#
# Environment (optional):
#   WORK=/data/farzin      where data, nets and results live (default ./work)
#   PREV_NET=nets/default.nnue   network used for self-play and as the baseline
#   HIDDEN=256 EPOCHS=12 WDL=0.0 SCALE=400 SOFT=4000 HARD=12000 TC=10+0.1
# Requires python-chess (pip install chess) for the matches.
set -euo pipefail
GEN=${1:?generation number}
POSITIONS=${2:?positions to generate}
THREADS=${3:-$(nproc)}
WORK=${WORK:-work}
PREV_NET=${PREV_NET:-nets/default.nnue}
HIDDEN=${HIDDEN:-256}; EPOCHS=${EPOCHS:-12}; WDL=${WDL:-0.0}; SCALE=${SCALE:-400}
SOFT=${SOFT:-4000}; HARD=${HARD:-12000}; TC=${TC:-10+0.1}
mkdir -p "$WORK"/{data,nets,results,engines}

cargo build --release --locked
ENGINE="$WORK/engines/farzin-gen$GEN"
cp target/release/farzin "$ENGINE"

DATA="$WORK/data/gen$GEN.data"
# ~87 positions per game; stop once the target count is reached.
GAMES=$(( POSITIONS / 85 + 1 ))
echo "== datagen gen$GEN: $POSITIONS positions on $THREADS threads"
"$ENGINE" datagen threads="$THREADS" games="$GAMES" soft_nodes="$SOFT" hard_nodes="$HARD" \
    out="$DATA" seed="$GEN$RANDOM" evalfile="$PREV_NET" 2> "$WORK/data/gen$GEN.log"

# Train on every generation collected so far.
ALL=$(ls "$WORK"/data/gen*.data | paste -sd, -)
NET="$WORK/nets/gen$GEN-h$HIDDEN.nnue"
echo "== training $NET on $ALL"
target/release/farzin-train data="$ALL" out="$NET" hidden="$HIDDEN" epochs="$EPOCHS" \
    wdl="$WDL" scale="$SCALE" threads="$THREADS"

echo "== fixed-node screen vs previous net"
python3 tools/arena.py --engine name=new,cmd="$ENGINE",opt.EvalFile="$NET" \
    --engine name=prev,cmd="$ENGINE",opt.EvalFile="$PREV_NET" \
    --nodes 20000 --pairs 200 --concurrency "$THREADS" --out "$WORK/results/gen$GEN-screen.json" | tail -n 20

echo "== SPRT [0, 5] at $TC"
python3 tools/arena.py --engine name=new,cmd="$ENGINE",opt.EvalFile="$NET" \
    --engine name=prev,cmd="$ENGINE",opt.EvalFile="$PREV_NET" \
    --tc "$TC" --pairs 3000 --concurrency "$THREADS" --sprt 0 5 \
    --out "$WORK/results/gen$GEN-sprt.json" | tail -n 20
echo "If H1 was accepted: cp $NET nets/default.nnue and record it in nets/README.md"
