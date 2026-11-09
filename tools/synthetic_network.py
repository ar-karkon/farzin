#!/usr/bin/env python3
"""Write deterministic UNTRAINED Caissa-layout weights for correctness/performance tests.

Not a Caissa model and not useful for playing-strength measurements. Never auto-loaded.
"""
import argparse
from array import array
from pathlib import Path
import struct
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output')
    args = parser.parse_args()
    header = struct.pack('<II', 0x43534E4E, 15) + bytes(56)
    # Small repeating weights keep activations nontrivial without training data.
    row = array('h', [(i * 13 % 31) - 15 for i in range(1536)])
    bias = array('h', [100] * 1536)
    if sys.byteorder != 'little':
        row.byteswap()
        bias.byteswap()
    subnets = bytearray(25408 * 8)
    for bucket in range(8):
        base = bucket * 25408
        for start, count in [(0, 24576), (24640, 512)]:
            for i in range(count):
                subnets[base + start + i] = ((i * 19 + bucket * 3) % 17 - 8) & 255
        for start, count in [(24576, 16), (25152, 32)]:
            for i in range(count):
                struct.pack_into('<i', subnets, base + start + i * 4, 1024 + i * 79)
        for i in range(32):
            struct.pack_into('<h', subnets, base + 25280 + i * 2, (i % 9 - 4) * 13)
    with Path(args.output).open('wb') as output:
        output.write(header)
        for feature in range(32 * 12 * 64):
            # Rotate lanes so board changes affect outputs.
            shift = feature % 1536
            output.write((row[shift:] + row[:shift]).tobytes())
        output.write(bias.tobytes())
        output.write(subnets)


if __name__ == '__main__':
    main()
