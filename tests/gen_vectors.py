#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Generate exact test vectors for the curve math.

The reference is Python's arbitrary-precision integers, so the Rust
implementation is compared against a DIFFERENT language's arithmetic rather
than against a second copy of itself. A differential test whose reference
shares the implementation's assumptions tests nothing.

    python3 tests/gen_vectors.py > tests/vectors/mul_div.txt
"""
import random

U128 = (1 << 128) - 1
random.seed(0x5EED_1234)          # deterministic: a failure is reproducible

print("# a b d floor ceil   -- '-' means the quotient exceeds u128")
n = 0
while n < 4000:
    # Deliberately biased towards the hard cases: huge products, small divisors,
    # and divisors just above/below the product's square root.
    a = random.randint(1, U128)
    b = random.randint(1, U128)
    mode = n % 4
    if mode == 0:
        d = random.randint(1, U128)
    elif mode == 1:
        d = random.randint(1, max(1, a // 2))          # quotient likely > u128
    elif mode == 2:
        d = max(1, (a * b) >> 128)                      # right at the boundary
    else:
        d = max(1, min(U128, (a * b) // random.randint(1, 1 << 60)))
    prod = a * b
    q, r = divmod(prod, d)
    c = q + (1 if r else 0)
    fl = str(q) if q <= U128 else "-"
    ce = str(c) if c <= U128 else "-"
    print(f"{a} {b} {d} {fl} {ce}")
    n += 1
