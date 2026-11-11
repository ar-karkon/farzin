#!/usr/bin/env bash
# Reproducible release build of the Farzin competition (OwnNet) edition.
#
#   tools/build_release.sh [outdir]
#
# Produces, for x86-64-v3 (AVX2 + BMI1/BMI2/PEXT + FMA + LZCNT + POPCNT):
#   farzin-<ver>-windows-x64-avx2-bmi2.exe   (x86_64-pc-windows-gnu, static CRT)
#   farzin-<ver>-linux-x64-avx2-bmi2         (x86_64-unknown-linux-gnu)
# plus SHA256SUMS. Requires: rustup target x86_64-pc-windows-gnu and the
# mingw-w64 cross linker (Debian/Ubuntu: apt-get install mingw-w64).
#
# The embedded network is nets/fzn2-a1.nnue only; build.rs refuses any other
# file or hash for the `ownnet` feature, and this script additionally checks
# that no 64-byte window of nets/default.nnue occurs in the binaries.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)
out=${1:-dist}
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
mkdir -p "$out"

export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || echo 0)}
remap="--remap-path-prefix=$root=/farzin --remap-path-prefix=$HOME/.cargo=/cargo --remap-path-prefix=$HOME/.rustup=/rustup"
cpu="-C target-cpu=x86-64-v3"

build() {
    local target=$1 extra=$2
    RUSTFLAGS="$cpu $extra $remap" cargo build --release --locked --features ownnet \
        --bin farzin --target "$target" --target-dir target/release-build
}

build x86_64-pc-windows-gnu "-C target-feature=+crt-static -C link-arg=-Wl,--no-insert-timestamp -C link-arg=-s"
cp target/release-build/x86_64-pc-windows-gnu/release/farzin.exe \
    "$out/farzin-$ver-windows-x64-avx2-bmi2.exe"
build x86_64-unknown-linux-gnu "-C link-arg=-s -C link-arg=-Wl,--build-id=none"
cp target/release-build/x86_64-unknown-linux-gnu/release/farzin \
    "$out/farzin-$ver-linux-x64-avx2-bmi2"

python3 tools/check_no_foreign_weights.py nets/default.nnue --expect-present nets/fzn2-a1.nnue \
    "$out/farzin-$ver-windows-x64-avx2-bmi2.exe" "$out/farzin-$ver-linux-x64-avx2-bmi2"

(cd "$out" && sha256sum "farzin-$ver-windows-x64-avx2-bmi2.exe" "farzin-$ver-linux-x64-avx2-bmi2" > SHA256SUMS)
echo "toolchain: $(rustc --version)"
cat "$out/SHA256SUMS"
