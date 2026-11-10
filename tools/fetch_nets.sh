#!/bin/sh
# Download CC0-licensed HalfKAv2_hm networks published by the Stockfish
# project (https://github.com/official-stockfish/networks, LICENSE: CC0-1.0)
# and verify their SHA-256 digests.
#
#   tools/fetch_nets.sh [DIR]      (default DIR = nets/external)
#
# Networks:
#   nn-ad9b42354671  L1=1024  15.1 generation (uncompressed layout)
#   nn-b1a57edbea57  L1=2560  16.1 "big" net (LEB128-compressed layout)
#   nn-baff1ede1f90  L1=128   16.1 "small" net (trained on lopsided positions)
set -eu
DIR=${1:-nets/external}
BASE=https://raw.githubusercontent.com/official-stockfish/networks/master
mkdir -p "$DIR"
while read -r name sum; do
    file="$DIR/$name.nnue"
    if [ ! -f "$file" ]; then
        echo "fetching $name"
        curl -fsSL -o "$file.part" "$BASE/$name.nnue"
        mv "$file.part" "$file"
    fi
    echo "$sum  $file" | sha256sum -c -
done <<EOF
nn-ad9b42354671 ad9b423546714137916bd38978af6fd68d7b8951bef25ff76bf43da72d6cb786
nn-b1a57edbea57 b1a57edbea574ca8b88d6837473845791beb53d885f87f86d5ccdd5659fbf3b2
nn-baff1ede1f90 baff1ede1f90c1dd1b4f772f1eff29848821801e8186345da7f0eb4121bd6f63
EOF
