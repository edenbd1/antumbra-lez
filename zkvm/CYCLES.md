# zkVM cycle cost

Measured with the RISC0 3.0.5 executor (no proving), guest toolchain 1.97.0,
guest built in release with `overflow-checks = true` — the same semantics an
on-chain program would ship. Reproduce with `cd zkvm && cargo run --release`. **The table below is
regenerated from that command's output**, so a figure here and a figure on your
screen cannot drift apart; if they do, the harness changed and the document is
wrong.

Counts are `env::cycle_count()` deltas taken inside the guest, with the 90-cycle cost of the measurement itself subtracted from every row.
`core::hint::black_box` wraps each call so the optimiser cannot hoist a pure
function out of the timed region. Repeated runs are byte-identical.

The number that matters is LEZ's 32,000,000-cycle public-execution cap.

| op | cases | median | min | max |
|---|---:|---:|---:|---:|
| `pow_frac` (decimal scale) | 12 | 314,248 | 109 | 457,193 |
| `pow_frac` (**binary scale**) | 12 | **27,181** | 117 | 32,029 |
| `weighted_buy` (decimal scale) | 6 | 275,811 | 18,770 | 462,753 |
| `weighted_buy` (**binary scale**) | 6 | **48,269** | 18,778 | 49,506 |
| `neg_ln` | 12 | 247,078 | 216,801 | 250,082 |
| `exp_neg` | 12 | 162,641 | 110 | 196,388 |
| `weight_at` | 12 | 8,803 | 91 | 8,803 |
| `curve_buy` | 12 | **10,622** | 10,622 | 10,622 |
| `curve_sell` | 12 | 10,623 | 10,623 | 10,623 |
| `vested_at` (linear) | 12 | 8,717 | 64 | 8,717 |
| `vested_at` (cliff+linear) | 12 | 8,711 | 56 | 8,711 |
| `vesting_claim` | 12 | 8,808 | 124 | 8,808 |
| `vesting_cancel` | 12 | 8,840 | 186 | 8,840 |
| `signal_milestone` | 12 | **30** | 30 | 30 |

A whole constant-product buy, from `Curve::new` through `buy`, is 10,722
cycles end to end.

## What the numbers say

**The constant-product path is cheap and flat.** 10,622 cycles, identical
across all twelve trade sizes — there is no input-dependent branch in the hot
path, so the worst case is the median. That is 0.03% of the execution cap; the
budget will be spent on the account and proof machinery around the math, not
on the math.

**`weight_at` costs 8,803 cycles and never varies with staleness**, because it
recomputes from the schedule rather than reading a refreshed value. Removing
the poke entirely is not a correctness risk here, and it is not a cost risk
either.

**Vesting costs one division, and milestone signalling costs almost nothing.**
`vested_at` is dominated by the single `mul_div` in the linear accrual, so a
claim and a cancellation both land near 8,800 cycles regardless of schedule
shape or elapsed time — the flatness again coming from the absence of an
input-dependent branch. `signal_milestone` is **30 cycles**, because idempotence
is a compare-and-set on a bitmap rather than a search through a list of
already-signalled indices. Choosing the data structure was the whole
optimisation.

**The fractional power was expensive, and the fix was worth measuring rather
than arguing.** The first version held its working scale in decimal, so each of
its 24 atanh terms carried a `mul_div` — a full 256-bit division. Two changes,
in `src/binfixed.rs`: work at 2^62 so dividing by the scale is a shift, and
reduce symmetrically into `[1/sqrt(2), sqrt(2))` so the series argument is
bounded at 0.1716 instead of 1/3 and thirteen terms suffice where
twenty-four were needed.

| | decimal | binary | |
|---|---:|---:|---|
| `pow_frac` cycles | 314,248 | **27,181** | 11.6x faster |
| `weighted_buy` cycles | 275,811 | **48,269** | 5.7x faster |
| worst error at 1e18 | 86 | **13** | 6.6x more accurate |
| results above exact, of 2,500 | 1,381 | 77 | |

Faster *and* more accurate, which is not the usual outcome and is the reason
both kernels are kept: the decimal one is the control, and the differential
test asserts the rewrite never loses to it on any of the 2,500 vectors.

**The remaining cost is the public scale, not the algorithm.** Three divisions
survive per call and none is in a loop: converting the argument in from 1e18,
forming `t = (m-1)/(m+1)` once, and applying the weight ratio. The return trip
is a multiply and a shift because 2^62 is a power of two. A kernel whose public
scale were also binary would drop the first of those.

**And the first attempt at the rewrite was wrong in an instructive way.**
Converting straight from 1e18 to 2^62 is a factor of only 4.6, so a small
argument arrives with almost no mantissa: `x = 19` (1.9e-17) became 87, six
bits, and the error surviving the exponent was 1e-4 rather than 1e-16 — worse
than the kernel it replaced. Normalising `x` into `[1e18/2, 1e18)` *before*
converting, and carrying the shift count into `-ln x = j*ln2 + (-ln m)`, is
what fixes it. The differential test caught this on the first run; reasoning
about it did not.
