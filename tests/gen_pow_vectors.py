#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Exact reference vectors for the fixed-point pow, at 60 significant digits.

    python3 tests/gen_pow_vectors.py > tests/vectors/pow.txt

Python's `decimal` is the reference. It is a different implementation in a
different language, which is the only kind of differential test worth running.
"""
from decimal import Decimal, getcontext
import random

getcontext().prec = 60
ONE = 10**18
random.seed(0xB0BA_2026)

print("# x num den expected   -- all at scale 1e18, expected = floor(x^(num/den))")
rows = 0
while rows < 2500:
    mode = rows % 5
    if mode == 0:                      # x close to 1: the precision-hostile end
        x = random.randint(ONE - 10**15, ONE - 1)
    elif mode == 1:                    # x very small: many halvings in ln
        x = random.randint(1, 10**12)
    elif mode == 2:                    # mid range
        x = random.randint(10**15, ONE - 1)
    elif mode == 3:                    # dust
        x = random.randint(1, 1000)
    else:
        x = random.randint(1, ONE - 1)

    # Weight ratios an LBP actually uses: 99/1 down to 1/99.
    num = random.randint(1, 99)
    den = random.randint(1, 99)

    xd = Decimal(x) / Decimal(ONE)
    e = Decimal(num) / Decimal(den)
    val = xd ** e
    exact = int((val * ONE).to_integral_value(rounding="ROUND_FLOOR"))
    print(f"{x} {num} {den} {exact}")
    rows += 1
