# Porting antumbra_vesting to LEZ testnet v0.3

Target: `logos-execution-zone` tag `v0.3.0`, commit `db66590ab821a4e142c211017a3866d007f6fa77`.
The v0.2.4 deployment and its evidence stay in the repository as history; nothing
below changes a hash cited against it.

## What v0.3 changes for this program

| v0.2.4 | v0.3 | Consequence here |
|---|---|---|
| One call reads and writes whole accounts | `Plan(PlanInput, instr)` then `Apply(ApplyInput)` per effect. The plan sees ids, signer flags and shard selectors, never data; each apply sees one shard and writes only its own program's shard | Every check against stored state moves to apply. The plan carries what the caller claims (amount, time, signer id) in the effect; apply compares it with the schedule and panics on a mismatch, which rolls back the whole transaction, payout included |
| Accounts have one `program_owner` and one `data` | Accounts carry a `shards` map keyed by program account id; native balance is the shard at `[0; 32]` | No program-owned escrow: a holding is a PDA whose native (or token) shard holds the funds. The Basecamp panel reads shards, not `program_owner` |
| Program id is the ImageID | A program is addressed by its header account, chosen at deploy; PDAs derive from that header id | All PDAs change. Multi-seed PDAs keep SPEL's rule (one seed direct, several hashed with SHA-256) so the layout is familiar |
| `claim` read the clock account and checked its owner | Plans cannot read the clock. A plan sets a timestamp validity window the sequencer checks against the block time | `claim(amount, t)` sets `[t, ∞)`. `cancel(t, refund)` sets `[t, t + 2 min)`, so an authority cannot back-date a cancellation by more than two minutes |
| `init` refused an existing account | Nothing is refused unless a program refuses it | `create_*` apply asserts the schedule shard is empty |
| Native escrow owned by this program, debited directly | `native_token::custody_transfer(holding, seed, to, amount)` | Payouts are chained calls with the holding's PDA seed attached, for native and token alike |
| Token `Transfer { amount }`, program by ImageID | Token program by account id, `Transfer { amount_to_transfer, descriptor: { definition_id, kind } }` | Holdings are the token shard of any account; the token program itself checks the definition |
| Session cap 2^25 cycles | Declared execution gas, at most 10,000,000 per transaction | The batch ceiling is re-measured; the "299 schedules" figure is v0.2.4 only |
| Private claim proved against a 50-block clock, error 7005 on drift | Public effects of a private transaction are deferred and applied at settlement | The claim window is just `[t, ∞)`; no coarse clock, no drift refusal |
| No fees | Public transactions name an LGO fee payer and are charged even when they fail; private ones are exempt | The CLI always names a payer and pre-checks locally before sending |
| SPEL generates IDL and CLI | SPEL has no v0.3 support | Guest written against `lee_core` directly; IDL hand-maintained in SPEL's JSON shape, with a drift check against the core crate |
| Events: none | `Plan::event`, public transactions only | Each instruction emits one event; a private claim emits nothing, by design |

## What does not change

The `antumbra` arithmetic crate, its tests and its Kani proofs. The program still
rebuilds `antumbra::vesting::Schedule` from stored terms and asks it what has
vested; there is still no cached "vested" field.

## Work items

1. `programs/vesting/core`: instruction, effect, state, seeds, events, shared by guest, CLI and tests.
2. `programs/vesting`: the v0.3 guest.
3. `cli/`: a Rust CLI on the v0.3 `wallet` crate (create, claim public and private, cancel, milestone, transfer, batch, show).
4. `executor-tests/`: the committed binary run through `lee::V03State`, the same state machine the sequencer uses, including the timestamp windows and the 10M gas cap; the batch ceiling measured there and on a local sequencer.
5. The Basecamp app's chain reader: decode the `shards` map (first in the panel's ChainBridge, now in `app/src/vesting_chain.cpp`).
6. A local v0.3 sequencer run of the whole lifecycle, transcript in `evidence/v03/`.
7. `scripts/verify-onchain.sh` reads hashes from a manifest; `docs/deploy-v03.md` for the public testnet once LGO is funded.
