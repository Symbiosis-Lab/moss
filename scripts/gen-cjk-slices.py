#!/usr/bin/env python3
"""Pack Google Fonts' CJK slicing strategies into the table moss embeds.

Usage: python3 -I scripts/gen-cjk-slices.py <nam-files checkout>   (needs the brotli module)

Reads `slices/<strategy>_default.txt` from github.com/googlefonts/nam-files
(Apache-2.0) and writes crates/moss-build/src/assets/fonts/cjk-slices.bin.br.

Each strategy file is a few frequency subsets plus bins that split the
remaining code points by code point. Every subset keeps its full list, so a
code point the file does not list stays unlisted. The file is brotli over
this layout, all integers LEB128:

  strategy count, then per strategy (in STRATEGIES order):
    subset count, frequency subset count, then per subset (frequency subsets
    by priority, then bins by code point): length, code points as deltas
"""
import pathlib
import re
import sys

import brotli

STRATEGIES = ["traditional-chinese", "hongkong-chinese", "simplified-chinese", "japanese", "korean"]
OUT = pathlib.Path(__file__).resolve().parent.parent / "crates/moss-build/src/assets/fonts/cjk-slices.bin.br"


def varint(n, out):
    while True:
        b = n & 0x7F
        n >>= 7
        out.append(b | (0x80 if n else 0))
        if not n:
            return


def deltas(points, out):
    prev = 0
    for p in points:
        varint(p - prev, out)
        prev = p


def parse(path):
    text = path.read_text()
    freq_count = len(re.findall(r"^# \d+: FreqRange", text, re.M))
    subsets = [sorted({int(c) for c in re.findall(r"codepoints: (\d+)", block)}) for block in text.split("subsets {")[1:]]
    # The file lists subsets from lowest priority to highest: the bins first,
    # the frequency subsets last.
    freq = subsets[len(subsets) - freq_count:][::-1]
    bins = sorted(subsets[: len(subsets) - freq_count], key=lambda s: s[0])
    for a, b in zip(bins, bins[1:]):
        if a[-1] >= b[0]:
            sys.exit(f"{path.name}: bins overlap at U+{b[0]:04X}; the frequency split is wrong")
    return freq, bins


def main():
    root = pathlib.Path(sys.argv[1]) / "slices"
    out = bytearray()
    varint(len(STRATEGIES), out)
    for name in STRATEGIES:
        freq, bins = parse(root / f"{name}_default.txt")
        varint(len(freq) + len(bins), out)
        varint(len(freq), out)
        for subset in freq + bins:
            varint(len(subset), out)
            deltas(subset, out)
        print(f"{name}: {len(freq)} frequency subsets, {len(bins)} bins")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    packed = brotli.compress(bytes(out), quality=11)
    OUT.write_bytes(packed)
    print(f"{OUT.name}: {len(out)} bytes, {len(packed)} compressed")


main()
