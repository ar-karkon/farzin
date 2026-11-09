#!/usr/bin/env python3
"""Inspect the header, size and SHA-256 of a Caissa v15 .pnn file.

This is a format preflight, not an inference implementation or license check.
Reference: Caissa 395c2e6 src/backend/PackedNeuralNetwork.hpp.
"""
import argparse
import hashlib
import json
import os
import struct

MAGIC = 0x43534E4E
VERSION = 15
HEADER_BYTES = 64
EXPECTED_BYTES = 75_703_872


def validate_header(header, size):
    if len(header) != HEADER_BYTES:
        raise ValueError("truncated 64-byte header")
    magic, version = struct.unpack_from("<II", header)
    if magic != MAGIC:
        raise ValueError("incorrect magic (expected Caissa CSNN)")
    if version != VERSION:
        raise ValueError(f"unsupported Caissa network version: {version}")
    if size != EXPECTED_BYTES:
        raise ValueError(f"incorrect file size: expected {EXPECTED_BYTES}, got {size}")
    return {"format": "Caissa PNN", "version": version, "bytes": size}


def inspect(path):
    with open(path, "rb") as stream:
        result = validate_header(stream.read(HEADER_BYTES), os.fstat(stream.fileno()).st_size)
        stream.seek(0)
        digest = hashlib.sha256()
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    result["sha256"] = digest.hexdigest()
    result["warning"] = "Header and length only; inference compatibility and usage rights are unverified."
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("network", help="path to local .pnn file")
    args = parser.parse_args()
    try:
        result = inspect(args.network)
    except (OSError, ValueError) as error:
        parser.exit(1, f"error: {error}\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
