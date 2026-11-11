#!/usr/bin/env bash
# Reproducible release build of the Farzin competition (OwnNet) edition.
#
#   tools/build_release.sh [outdir]
#
# Produces (all with the same search and network):
#   farzin-<ver>-windows-x64-avx2-bmi2.exe   x86-64-v3: AVX2, BMI1/BMI2 (PEXT
#                                            sliders), FMA, LZCNT, POPCNT.
#                                            Primary build (CCRL).
#   farzin-<ver>-windows-x64-avx2.exe        x86-64-v3 without BMI2 (magic
#                                            sliders): for AMD Zen 1/Zen 2,
#                                            whose PEXT is microcoded and slow.
#   farzin-<ver>-windows-x64-popcnt.exe      x86-64-v2 (SSE4.2, POPCNT): older
#                                            CPUs; scalar network code, slower.
#   farzin-<ver>-linux-x64-avx2-bmi2         Linux build of the primary target.
# plus SHA256SUMS. Windows builds use x86_64-pc-windows-gnu with a static C
# runtime (no DLLs beyond the Windows system ones). Requires the rustup
# target x86_64-pc-windows-gnu and mingw-w64 (apt-get install mingw-w64).
#
# The embedded network is nets/fzn2-a1.nnue only; build.rs refuses any other
# file or hash for the `ownnet` feature, and this script additionally checks
# that no fragment of nets/default.nnue occurs in the binaries.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)
out=${1:-dist}
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
mkdir -p "$out"

export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || echo 0)}
remap="--remap-path-prefix=$root=/farzin --remap-path-prefix=$HOME/.cargo=/cargo --remap-path-prefix=$HOME/.rustup=/rustup"
win="-C target-feature=+crt-static -C link-arg=-Wl,--no-insert-timestamp -C link-arg=-s"

build() {
    local target=$1 name=$2 flags=$3 ext=""
    [[ $target == *windows* ]] && ext=.exe
    RUSTFLAGS="$flags $remap" cargo build --release --locked --features ownnet \
        --bin farzin --target "$target" --target-dir "target/release-build/$name"
    cp "target/release-build/$name/$target/release/farzin$ext" "$out/farzin-$ver-$name$ext"
    bins+=("farzin-$ver-$name$ext")
}

bins=()
build x86_64-pc-windows-gnu windows-x64-avx2-bmi2 "-C target-cpu=x86-64-v3 $win"
build x86_64-pc-windows-gnu windows-x64-avx2 "-C target-cpu=x86-64-v3 -C target-feature=-bmi2 $win"
build x86_64-pc-windows-gnu windows-x64-popcnt "-C target-cpu=x86-64-v2 $win"
build x86_64-unknown-linux-gnu linux-x64-avx2-bmi2 \
    "-C target-cpu=x86-64-v3 -C link-arg=-s -C link-arg=-Wl,--build-id=none"

(cd "$out" && python3 "$root/tools/check_no_foreign_weights.py" "$root/nets/default.nnue" \
    --expect-present "$root/nets/fzn2-a1.nnue" "${bins[@]}")
(cd "$out" && sha256sum "${bins[@]}" > SHA256SUMS)
echo "toolchain: $(rustc --version)"
cat "$out/SHA256SUMS"
