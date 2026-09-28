# Whole-instruction cost of each vesting operation

Measured by `cargo run --release --manifest-path executor-tests/Cargo.toml --bin measure`,
which executes the committed `artifacts/programs/antumbra_vesting.bin` exactly as the
LEZ sequencer executes a public transaction's program: the same four inputs in the
same order, the same 32M-cycle session limit, the same RISC0 executor. Against LEZ
**v0.2.4** (`lee_core` rev `47eba256`), the version the public testnet runs.

These are whole instructions — account validation, Borsh decoding, the accrual, the
payout and the output — not the arithmetic kernels in [`zkvm/CYCLES.md`](../zkvm/CYCLES.md),
which isolate the vesting math at about 8,800 cycles. CI re-measures this table and
fails if it drifts.

| operation | cycles | share of the 32M public-execution cap |
|---|---:|---:|
| create schedule | 249065 | 0.742% |
| claim (native) | 362926 | 1.082% |
| cancel (native) | 363210 | 1.082% |
| claim (token, chained transfer) | 466218 | 1.389% |
| signal milestone | 227422 | 0.678% |
| transfer beneficiary | 236842 | 0.706% |
| create a batch of 8 schedules | 1042949 | 3.108% |
