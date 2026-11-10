#!/usr/bin/env python3
"""Compare Caissa v15 scalar feature accumulators with its MIT-licensed reference.

Feature convention, file layout and scalar math: Caissa commit 395c2e6 by
Michał Witanowski, https://github.com/Witek902/Caissa (MIT; see
THIRD_PARTY_NOTICES.md). This diagnostic does not load weights into Farzin.
"""
import argparse
import os
import re
import struct
import subprocess

import chess
from inspect_nnue import HEADER_BYTES, validate_header

WIDTH = 1536
BUCKETS = 32
FEATURES_PER_BUCKET = 12 * 64
BIAS_OFFSET = HEADER_BYTES + BUCKETS * FEATURES_PER_BUCKET * WIDTH * 2
SUBNET_OFFSET = BIAS_OFFSET + WIDTH * 2
SUBNET_SIZE = 25_408


def feature_indices(board, perspective):
    king = board.king(perspective)
    if king is None:
        raise ValueError("position must contain both kings")
    mask = (7 if chess.square_file(king) >= 4 else 0) | (56 if perspective == chess.BLACK else 0)
    canonical_king = king ^ mask
    bucket = chess.square_rank(canonical_king) * 4 + chess.square_file(canonical_king)
    features = []
    for color_index, color in enumerate((perspective, not perspective)):
        for piece_type in range(chess.PAWN, chess.KING + 1):
            for square in board.pieces(piece_type, color):
                features.append(bucket * FEATURES_PER_BUCKET + (color_index * 6 + piece_type - 1) * 64 + (square ^ mask))
    return features


def scalar_accumulator(stream, board, perspective):
    stream.seek(BIAS_OFFSET)
    values = list(struct.unpack("<1536h", stream.read(WIDTH * 2)))
    for feature in feature_indices(board, perspective):
        stream.seek(HEADER_BYTES + feature * WIDTH * 2)
        row = struct.unpack("<1536h", stream.read(WIDTH * 2))
        values = [((a + b + 32768) % 65536) - 32768 for a, b in zip(values, row)]
    return values


def scalar_output(stream, accumulators, bucket):
    def clipped(value):
        return max(0, min(255, value))

    activations = []
    for accumulator in accumulators:
        activations.extend((clipped(accumulator[i]) * clipped(accumulator[i + WIDTH // 2])) >> 9
                           for i in range(WIDTH // 2))
    stream.seek(SUBNET_OFFSET + bucket * SUBNET_SIZE)
    l1_weights = struct.unpack("<24576b", stream.read(24576))
    l1_biases = struct.unpack("<16i", stream.read(64))
    l2_weights = struct.unpack("<512b", stream.read(512))
    l2_biases = struct.unpack("<32i", stream.read(128))
    output_weights = struct.unpack("<32h", stream.read(64))
    output_bias = struct.unpack("<i", stream.read(4))[0]

    def hidden(inputs, weights, biases):
        outputs = []
        count = len(biases)
        for out in range(count):
            total = biases[out]
            for i, value in enumerate(inputs):
                total += value * weights[(i // 4) * (count * 4) + out * 4 + i % 4]
            outputs.append(max(0, min(127, total >> 6)))
        return outputs

    l1 = hidden(activations, l1_weights, l1_biases)
    l2 = hidden(l1, l2_weights, l2_biases)
    return output_bias + sum(value * weight for value, weight in zip(l2, output_weights))


def reference_accumulators(binary, model, fen):
    model = os.path.abspath(model)
    if "\n" in model or "\r" in model:
        raise ValueError("network path cannot contain a line break")
    commands = f"uci\nsetoption name EvalFile value {model}\nisready\nposition fen {fen}\neval detailed\nquit\n"
    completed = subprocess.run([os.path.abspath(binary)], input=commands, text=True, capture_output=True,
                               cwd=os.path.dirname(os.path.abspath(binary)), timeout=60, check=True)
    lines = completed.stdout.splitlines()
    if f"info string Loaded neural network: {model}" not in lines:
        raise ValueError("reference did not confirm loading the requested network")
    groups = []
    for i, line in enumerate(lines):
        if line.startswith("Raw accumulator values (pre-activation)"):
            values = []
            for row in lines[i + 1:i + 1 + WIDTH // 16]:
                match = re.fullmatch(r"\[\s*\d+\]\s+(.+)", row)
                if not match:
                    raise ValueError("unexpected reference accumulator output")
                values.extend(int(value) for value in match.group(1).split())
            groups.append(values)
    if len(groups) != 2 or any(len(group) != WIDTH for group in groups):
        raise ValueError("reference did not return two complete accumulators")
    bucket_results = {}
    for line in lines:
        match = re.match(r"\s*bucket (\d+): internal =\s*(-?\d+),", line)
        if match:
            bucket_results[int(match.group(1))] = int(match.group(2))
    if set(bucket_results) != set(range(8)):
        raise ValueError("reference did not return all eight raw network outputs")
    return groups, bucket_results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("network")
    parser.add_argument("reference")
    parser.add_argument("--fen", default=chess.STARTING_FEN)
    args = parser.parse_args()
    board = chess.Board(args.fen)
    if not board.is_valid():
        parser.error("invalid position")
    with open(args.network, "rb") as stream:
        validate_header(stream.read(HEADER_BYTES), os.fstat(stream.fileno()).st_size)
        ours = [scalar_accumulator(stream, board, color) for color in (board.turn, not board.turn)]
        outputs = [scalar_output(stream, ours, bucket) for bucket in range(8)]
    reference, expected_outputs = reference_accumulators(args.reference, args.network, board.fen())
    for perspective in range(2):
        mismatches = sum(a != b for a, b in zip(ours[perspective], reference[perspective]))
        print(f"perspective={perspective} compared={WIDTH} mismatches={mismatches}")
        if mismatches:
            raise SystemExit(1)
    for bucket, output in enumerate(outputs):
        print(f"bucket={bucket} scalar={output} reference={expected_outputs[bucket]}")
        if output != expected_outputs[bucket]:
            raise SystemExit(1)


if __name__ == "__main__":
    main()
