# Whole-transaction cost of each operation on LEZ v0.3

Measured by `cargo run --release --manifest-path executor-tests/Cargo.toml --bin measure`,
which runs the committed `artifacts/programs/v0.3/antumbra_vesting.bin`,
`antumbra_curve.bin` and `antumbra_lbp.bin` through
`lee::V03State` exactly as a v0.3 sequencer settles a public transaction: the plan,
one apply per effect, and every chained call (the native transfer, or the v0.3.0
token program), metered together against a budget of **10,000,000 cycles**, the
per-transaction execution-gas cap (`fee_core::market::MAX_GAS_EXEC`; gas is cycles
one for one). Against LEZ **v0.3.0** (`db66590a`).

Creations here are funded in the same transaction, so "create" includes the
transfer into the escrow. CI re-measures this table and fails if it drifts.

| op | cycles | share of the 10M gas cap |
|---|---:|---:|
| create schedule (native, funded) | 40152 | 0.40% |
| create schedule (token, funded) | 78428 | 0.78% |
| claim (native) | 49024 | 0.49% |
| claim (token) | 87913 | 0.88% |
| cancel (native) | 50456 | 0.50% |
| cancel (token) | 89332 | 0.89% |
| signal milestone | 29589 | 0.30% |
| transfer beneficiary | 30982 | 0.31% |
| make non-cancelable | 29549 | 0.30% |
| create a batch of 8 schedules | 194531 | 1.95% |

## The batch ceiling

**450 schedules fit one creation transaction; 451 do not.** The bisection in
`measure` creates a native batch of `n` and finds the largest `n` that settles
within the 10M budget: 450 uses 9,980,185 cycles. Each schedule adds about 22,100
cycles, one apply session writing its shard.

This replaces the v0.2.4 figure of 299, which was measured against the 2^25-cycle
session cap of a different execution model. The two numbers are not comparable
as "faster" or "slower": v0.3 meters a whole transaction against a 10M gas cap,
and the v0.3 program does less per schedule because it reads no clock account
and validates no owner field.

The same ceiling was confirmed on a local v0.3 sequencer with a declared gas
limit of 10,000,000: see `evidence/v03/`.

## antumbra_curve

A buy is the payment (one chained native or token transfer into the holding)
plus one apply pricing it with `antumbra::Curve::buy` after the per-swap fee.
Fee sweeps and withdrawals are a seeded payout out of the holding. The native
buy is the 18-decimal sale of the v0.2.4 evidence (Vt 1e24, Vc 1e21, a buy of
500e18), where `k = Vt * Vc` is 1e45 and is never formed.

| op | cycles | share of the 10M gas cap |
|---|---:|---:|
| create sale | 31002 | 0.31% |
| buy (native, 18 decimals, 1% fee) | 53589 | 0.54% |
| buy (token), closing the sale | 91682 | 0.92% |
| collect fees (native) | 36326 | 0.36% |
| withdraw (token) | 76270 | 0.76% |

## antumbra_lbp

The native buy is the 18-decimal pool of the v0.2.4 evidence (reserves 1e24 /
1e23, a buy of 5e21 a quarter of the way through a 99/1 to 1/99 schedule, so a
token weight of 0.745): the fractional power `x^(w_c/w_t)` of
`antumbra::binfixed::weighted_buy` runs inside the guest. A withdrawal makes
two seeded payouts out of the holding, the creator's share and the at-close
fee.

| op | cycles | share of the 10M gas cap |
|---|---:|---:|
| create pool | 32827 | 0.33% |
| buy (native, 18 decimals, weight 0.745) | 87890 | 0.88% |
| buy (token) | 126580 | 1.27% |
| set paused | 27854 | 0.28% |
| withdraw (native, creator and fee) | 52225 | 0.52% |
| withdraw (token, creator and fee) | 128397 | 1.28% |
