# zkVM cycle cost

Measured with the RISC0 3.0.5 executor (no proving), guest toolchain 1.97.0,
guest built in release with `overflow-checks = true` — the same semantics an
on-chain program would ship. Reproduce with `cd zkvm && cargo run --release`.

Counts are `env::cycle_count()` deltas taken inside the guest, with the
90-cycle cost of the measurement itself subtracted from every row.
`core::hint::black_box` wraps each call so the optimiser cannot hoist a pure
function out of the timed region. Repeated runs are byte-identical.

The number that matters is LEZ's 32,000,000-cycle public-execution cap.

| op | cases | median | min | max |
|---|---:|---:|---:|---:|
| `pow_frac` | 12 | 314,248 | 109 | 457,193 |
| `neg_ln` | 12 | 247,078 | 216,801 | 250,082 |
| `exp_neg` | 12 | 162,641 | 110 | 196,388 |
| `weighted_buy` | 6 | 275,830 | 18,789 | 462,772 |
| `weight_at` | 12 | 8,810 | 98 | 8,810 |
| `curve_buy` | 12 | 10,622 | 10,622 | 10,622 |
| `curve_sell` | 12 | 10,623 | 10,623 | 10,623 |

A whole constant-product buy, from `Curve::new` through `buy`, is 10,710
cycles end to end.

## What the numbers say

**The constant-product path is cheap and flat.** 10,622 cycles, identical
across all twelve trade sizes — there is no input-dependent branch in the hot
path, so the worst case is the median. That is 0.03% of the execution cap; the
budget will be spent on the account and proof machinery around the math, not
on the math.

**`weight_at` costs 8,810 cycles and never varies with staleness**, because it
recomputes from the schedule rather than reading a refreshed value. Removing
the poke entirely is not a correctness risk here, and it is not a cost risk
either.

**The fractional power is expensive, and the reason is a design choice made
for the wrong constraint.** `pow_frac` is dominated by 24 atanh terms, each
carrying a `mul_div` — a full 256-bit division by 1e18. Holding the working
scale in decimal makes every series step a division instead of a shift.

A binary working scale (2^62) with a symmetric reduction into `[√2/2, √2)`
would bound the series argument at |t| ≤ 0.1716 instead of 1/3, cutting the
term count by more than half *and* turning each remaining step into a shift.
That is the right shape for a zkVM, and it is the first thing to change:
correctness is settled at 8.6e-17, so the remaining work is cost, and this
measurement is what says so. Accuracy was tuned before cost was measured,
which is the wrong order — recorded here rather than quietly fixed.
