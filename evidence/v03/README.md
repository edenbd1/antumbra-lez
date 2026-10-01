# antumbra_vesting on LEZ v0.3: a local end-to-end run

A fresh local LEZ v0.3.0 standalone sequencer (`db66590a`, 5 s blocks), the
committed `artifacts/programs/v0.3/antumbra_vesting.bin` deployed through
`program_loader` at header `EAkwPsUMwyKivN6KGkLAMUDMaZM929QiR9o7X3Z342tR`, ImageID
`72d5cdc05004a9502be72239829071982237638b49b8fe7d1b45fd7371a425f4`, and
`scripts/e2e-v03.sh` driving the whole lifecycle through the `antumbra-vesting`
CLI. Nothing here was sent to the public testnet.

**Result: 27 transactions applied, 15 refused exactly where they must be, 20
balance and state checks read back from the chain, 0 failures.**

| File | What it is |
|---|---|
| `local-transcript.txt` | the run's output, one JSON line per transaction and one line per check |
| `local-e2e.tsv` | the manifest: every transaction with its expectation, verdict, block and hash, the program header and ImageID, and the final state of every schedule and escrow |
| `local-verify.txt` | `scripts/verify-onchain.sh --manifest evidence/v03/local-e2e.tsv --rpc http://127.0.0.1:3140`: 66 checks, 0 failures, including that the header names the committed ImageID and a random hash resolves to nothing |
| `basecamp-bridge-read.txt` | the Basecamp panel's `ChainBridge` (v0.3 shard reads) decoding a native and a cancelled token schedule from the same chain, beside the CLI's decoding of the first |

What the run covers: creation funded exactly in one transaction (native and
token); claims paid to the unit at the time they name; over-claims, claims by
the wrong key and claims for a future time refused; a duplicate creation refused
with no second funding; cancellation with an exact refund, wrong refunds and
wrong authorities refused, the vested part still claimable; milestones signalled
once by a nominated authority; holder-only transfer; one-way non-cancelable; a
batch of 8 funded by one transfer; **private claims of native balance and of a
token into shielded accounts**, each recorded on the public schedule while the
amount is visible only in the beneficiary's wallet; and the batch ceiling on the
live sequencer, **450 schedules applied in one transaction and 451 refused**
under a declared gas limit of 10,000,000.

On v0.3 a refused public transaction is included in a block and charged its
fee, so "reverted" in the manifest means included with no effect, which the
final-state lines pin down. The fee payer spent 269,211,440 atomic units
(about 0.27 LGO) over the run, including the deploy.

Reproduce: see `docs/deploy-v03.md`, with `RPC=http://127.0.0.1:3040` and the
genesis keys of the debug sequencer configuration.
