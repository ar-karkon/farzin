#!/usr/bin/env bash
# Package the primary OwnNet binary with provenance and upstream notices.
set -euo pipefail
cd "$(dirname "$0")/.."
out=${1:-dist}
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cp docs/README-RELEASE.md "$out"/README.md
cp nets/README.md "$out"/NETWORK-PROVENANCE.md
cp LICENSE "$out"/LICENSE
cp THIRD_PARTY_NOTICES.md "$out"/THIRD_PARTY_NOTICES.md
runtime="$out"/RUNTIME-LICENSES
mkdir -p "$runtime/rust/licenses" "$runtime/mingw"
rustdocs="$(rustc --print sysroot)/share/doc/rust"
cp "$rustdocs/COPYRIGHT-library.html" "$runtime/rust/"
cp "$rustdocs"/licenses/* "$runtime/rust/licenses/"
for pkg in mingw-w64-common mingw-w64-x86-64-dev gcc-mingw-w64-base gcc-mingw-w64-x86-64-posix-runtime gcc-mingw-w64-x86-64-win32-runtime; do
  cp -L "/usr/share/doc/$pkg/copyright" "$runtime/mingw/$pkg-copyright"
done
cd "$out" && zip -9 -r "farzin-$ver-windows-x64-avx2-bmi2.zip" \
  farzin-*-windows-x64-avx2-bmi2.exe SHA256SUMS README.md NETWORK-PROVENANCE.md LICENSE THIRD_PARTY_NOTICES.md RUNTIME-LICENSES
