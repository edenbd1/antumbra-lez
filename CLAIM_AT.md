# `claim_at` — a claim that reads no clock account

**Status: implemented and tested on this branch, not deployed.** The program
deployed on the public testnet (`main`, ImageID `7763458b…`) does not have it.

## Why

A private claim is a privacy-preserving transaction: its proof is built against
the public inputs as they are when proving starts, and the sequencer re-checks it
against them as they are at inclusion. `claim` reads a LEZ clock account, and a
proof takes minutes, so the deployed program accepts the 50-block clock for
claims — it holds still long enough. That works, and it is on chain today; it
also ties every private claim to a clock cadence the runtime happens to offer.

## How

`claim_at(schedule_id, as_of)` takes no clock account. It prices the claim at
`as_of` and binds the transaction to it with the output's **timestamp validity
window**, which starts at `as_of`. The sequencer checks that window against the
block's timestamp for public and privacy-preserving transactions alike
(`lee/state_machine/src/validated_state_diff/mod.rs` at LEZ v0.2.4), so:

- `as_of` can never be later than the chain's own time — a claim can never pay
  more than has vested;
- an earlier `as_of` only pays less, and `as_of` before the schedule's last
  event is refused (7005);
- nothing in the proof's public inputs moves with time, so a proof that takes
  minutes cannot go stale.

On the explorer such a claim shows `Timestamp Validity Window: [as_of, ∞)`.

## Evidence

`executor-tests/tests/program.rs`, run against this branch's binary
(ImageID `1e076029…`) through the sequencer's execution path:

- `pr1_claim_at_prices_at_as_of_and_binds_the_transaction_to_it`
- `pr1_claim_at_refuses_an_instant_before_the_schedules_last_event`
- `f2_claim_at_is_the_named_beneficiarys_alone`

and all 33 earlier tests still pass on the same binary. It ships in M1, with the
whole lifecycle re-driven on testnet.
