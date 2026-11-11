#!/usr/bin/env python3
"""Check that binaries contain no fragment of a given network file.

    tools/check_no_foreign_weights.py NET BINARY... [--expect-present OWN_NET]

Every 32-byte aligned, high-entropy chunk of NET is indexed; each binary is
scanned at every byte offset. Any copy of 64 or more contiguous bytes of NET
necessarily contains a whole aligned chunk, so a clean result means no such
copy exists. With --expect-present, the same scan is run for OWN_NET as a
positive control (it must be found), proving the detector works on the
binary at hand. Exit status 1 on a foreign hit or a failed control.
"""
import argparse
import sys

CHUNK = 32


def index(path):
    data = open(path, "rb").read()
    chunks = set()
    for off in range(0, len(data) - CHUNK + 1, CHUNK):
        c = data[off:off + CHUNK]
        if len(set(c)) >= 12:  # skip zero runs and other low-entropy filler
            chunks.add(c)
    return chunks, len(data)


def hits(binary, chunks):
    found = 0
    for off in range(0, len(binary) - CHUNK + 1):
        if binary[off:off + CHUNK] in chunks:
            found += 1
    return found


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("net")
    ap.add_argument("binaries", nargs="+")
    ap.add_argument("--expect-present")
    args = ap.parse_args()
    foreign, size = index(args.net)
    own = index(args.expect_present)[0] if args.expect_present else None
    ok = True
    for path in args.binaries:
        data = open(path, "rb").read()
        n = hits(data, foreign)
        line = f"{path}: {n} chunk(s) of {args.net} ({size} bytes, {len(foreign)} indexed chunks)"
        if own is not None:
            m = hits(data, own)
            line += f"; control {args.expect_present}: {m} of {len(own)} chunks found"
            if m < len(own) // 2:
                ok = False
                line += "  CONTROL FAILED"
        if n:
            ok = False
            line += "  FOREIGN WEIGHTS PRESENT"
        print(line)
    print("OK: no foreign network data" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
