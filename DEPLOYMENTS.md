# Deployed on the public LEZ testnet

Three SPEL programs, one per open RFP proposal, live on
`https://testnet.lez.logos.co` and fetchable by anyone.

The four facts below are the convention `logos-co/lez-payment-streams` sets for
its own live program, and they are what makes a deployment checkable rather than
asserted: the commit the guest was frozen at, the ImageID, the deploy
transaction, and the block.

| Program | RFP | ImageID | Deploy transaction | Block |
|---|---|---|---|---|
| `antumbra_curve` | [015](https://github.com/logos-co/rfp/issues/179) | `bcd6d07d27bb0d2ea8c237c46125018e5115815173025a1a24aca505835f1a23` | [`25a8f405…b42f1718`](https://explorer.testnet.lez.logos.co/transaction/25a8f4051b60ff471cb30d9655217e7b172b9b43f3977be327956fd2b42f1718) | 16339 |
| `antumbra_lbp` | [016](https://github.com/logos-co/rfp/issues/180) | `249648dcf6e2fe70e81c0315bdc5737037d3f343e3362697575dd0a30bbe0e08` | [`f765ec06…98b4eac2`](https://explorer.testnet.lez.logos.co/transaction/f765ec06ae391c8d9e754f40947398cf15d66c9967f2fda23894d30098b4eac2) | 16342 |
| `antumbra_vesting` | [017](https://github.com/logos-co/rfp/issues/178) | `26134c7901b2cb8c2dac5889155ef17be988d5cd7b77f2af8df10e39a6c235be` | [`f45a7b2f…0b928030`](https://explorer.testnet.lez.logos.co/transaction/f45a7b2fc835e75e9633e6fe8cd00687146f2b05b22591ff38baeec80b928030) | 16335 |

Deployment is content-addressed — `SHA256(u32_le(len) ‖ bytecode)` — so the
ImageID **is** the version, and rebuilding from the same source reproduces the
same address whoever runs the build.

## They are not only deployed — they have been driven, and the answers match

A deployed program proves the code compiles and the bytecode landed. It does not
prove the program works. So each one was invoked on the public testnet, and the
state it wrote was read back and compared against the same computation run on
the host.

| Program | Instruction | Transaction | Block |
|---|---|---|---|
| `antumbra_curve` | `create_sale` | [`ec7f1bed…9e7604ed`](https://explorer.testnet.lez.logos.co/transaction/ec7f1bede8afebff0048d9dcd374e0e2bd73a937bed350ae61ff22ef9e7604ed) | 16416 |
| `antumbra_curve` | `execute_buy` | [`1b886f82…fa4ba71a`](https://explorer.testnet.lez.logos.co/transaction/1b886f82a9966e94fb2ba2d9181fe69945ceacbd6de4318e99e3d902fa4ba71a) | 16417 |
| `antumbra_lbp` | `create_pool` | [`417d64e3…9b5dd7af`](https://explorer.testnet.lez.logos.co/transaction/417d64e3ec33b71ea9ae5e6d4a354f063c6b91ee2f4405b6e788e9d69b5dd7af) | 16418 |
| `antumbra_lbp` | `execute_buy` | [`45fa7b91…e2eda6b0`](https://explorer.testnet.lez.logos.co/transaction/45fa7b915283369d9c6eac61ae2a599a7a4b0042064f788ecb7540b2e2eda6b0) | 16419 |
| `antumbra_vesting` | `create_schedule` | [`dbe8c753…b1bfa475`](https://explorer.testnet.lez.logos.co/transaction/dbe8c7538ca3c759e0668c9fa285e6fd343aab574fa92d861514e0bcb1bfa475) | 16413 |
| `antumbra_vesting` | `record_claim` | [`3aff5549…ec203685`](https://explorer.testnet.lez.logos.co/transaction/3aff5549434a0573a4d98895e7fd28afbdc4353c90ebf217320e3e59ec203685) | 16415 |

The host side of every comparison below is reproducible too — a claim that the
chain agrees with this crate is only checkable if both halves are:

```bash
cargo run --release --example onchain_reference
```

### What the chain came back with

**The bonding curve.** A sale opened at `Vt = 1e24`, `Vc = 1e21`, sale reserve
`8e23`, seed reserve `2e23`, then a buy of 500 collateral. Reading the PDA
`gweZAVxdgb2NmveTwf25hQVkueEZdxQEJyUpJgUE8N2` afterwards, against the same
`Curve::buy` run on the host:

| field | on chain | host |
|---|---|---|
| `vt` | 666666666666666666666667 | identical |
| `vc` | 1500000000000000000000 | identical |
| `sale_reserve` | 466666666666666666666667 | identical |
| `real_collateral` | 500000000000000000000 | identical |
| `seed_reserve` | 200000000000000000000000 | **untouched**, as the two-bucket rule requires |

Note the pair: `k = Vt · Vc` here is 1e45, thirteen orders of magnitude past
`u128::MAX`. The sale priced anyway, because `k` is never materialised.

**The LBP.** A pool at 99/1 shifting to 1/99 over ten thousand seconds, bought
at `now = 3500` — a weight of 0.745, derived from the schedule rather than read
from storage. `25ekuB2nQ84WLvoVjWejf63Z714X9vvjnb7Jz4R3Kkdg` afterwards:

| field | on chain | host |
|---|---|---|
| tokens out | 16561317272817989000000 | identical |
| `reserve_token` | 983438682727182011000000 | identical |
| `reserve_collateral` | 105000000000000000000000 | identical |

That number is the fractional power `x^(w_c/w_t)` evaluated **inside the zkVM
guest**, agreeing to the unit with the host kernel. The pool account stores
`w_start` and `w_end`; it stores no current weight, because there is none to
store.

**Vesting.** A linear schedule of 1e21 from t=1000 to t=8919, claimed at
t=4959. The PDA `SqLgfYnsQ6STtHRvjNxj1MmXjM3EsUBbsCCvDCcpX9B` came back with
`claimed = 499936860714736709180` — the same value as `total × (4959−1000) ÷
(8919−1000)` computed off chain, to the unit.

## The buy now takes payment, and that changes what these programs are

The first version of `antumbra_curve` priced a buy and moved nothing, because
LEZ rule 5 forbids a program from debiting an account it does not own. That is
still true — so the program stops trying to, and instead **declares a chained
call into the program that does own the balance**, the native
`authenticated_transfer`. The runtime executes that call inside the same
transaction, so the buyer is debited and the sale treasury credited, or nothing
happens at all. There is no state where the tokens are priced but not paid.

| | | |
|---|---|---|
| paying program | ImageID | `e6003fae1a29e1537ff91174f48d985129853d690ff91957addc9a038e716d19` |
| deploy | [`b6ea6b6d…acf0211d`](https://explorer.testnet.lez.logos.co/transaction/b6ea6b6d79ac7e32ee52982426255412471d15d156ab197b73896aa2acf0211d) | block 16644 |
| `create_sale` bound to a treasury | [`7fa6b18c…866071b8`](https://explorer.testnet.lez.logos.co/transaction/7fa6b18cf81eb91624ecd9fa5e4e4d10ea8bd1da353a0a08c9786902866071b8) | |
| `execute_buy`, paid | [`ea0eeb93…f581ddb9`](https://explorer.testnet.lez.logos.co/transaction/ea0eeb936cd43850354f44989d6dd1cda15e1e7353ee1f5a5348da3af581ddb9) | |

**Balances read from the chain on both sides:**

| | before | after |
|---|---|---|
| buyer | 4 | **2** |
| sale treasury | 0 | **2** |

And the curve advanced to `vt = 998004`, `vc = 1002`, `sale_reserve = 798004`,
`real_collateral = 2` — which is what `antumbra::Curve::buy(2, 0)` returns on
the host, to the unit.

**One token in that result is worth pointing at.** A naive reading gives 1997
tokens out; the program paid **1996**. The difference is the rounding rule:
`tokens_out = Vt − ceil(Vt·Vc / (Vc + C))`, so the residue stays with the pool.
An implementation that floored the subtrahend would have paid one token too many
on this trade, and one token too many on every trade after it. That is the
solvency argument, visible in a single on-chain result.

**This does not wait on LP-0013.** Those authorities would let a program move a
*token* balance it does not own, and they are still absent from the runtime.
Chaining into `authenticated_transfer` moves the *native* balance today, which
is enough to run a native-collateral sale and enough to prove the composition
end to end. The token path is one seam away, not one dependency away.

The treasury address is fixed at `create_sale` and checked on every buy, so a
buyer cannot redirect the proceeds by naming a different account.

## The private path, run twice, with what it cost to learn

RFP-015 and RFP-016 both ask for `deshield → buy → re-shield` through a fresh
single-use public account. It was run end to end, twice, with a **different**
ephemeral account each time — which is requirement Pr4, non-reuse, demonstrated
rather than asserted.

| Step | Transaction | Block |
|---|---|---|
| initialise ephemeral #1 | [`646f91b2…49a8880d`](https://explorer.testnet.lez.logos.co/transaction/646f91b21d8faf80a249ee8a6ad5ad1a1e07c74517ee03ff3f4e305b49a8880d) | 16467 |
| deshield → ephemeral #1 | [`921b9e4f…75de83ec`](https://explorer.testnet.lez.logos.co/transaction/921b9e4f72425b65a5e0622e248ea7834de32215d785e24670d32ceb75de83ec) | 16473 |
| buy signed by ephemeral #1 | [`70f19695…e1c95a29`](https://explorer.testnet.lez.logos.co/transaction/70f19695cd81be4210a304896090686529c0f5f547ad15aa062d1498e1c95a29) | 16474 |
| initialise ephemeral #2 | [`6c57df67…40da71fd`](https://explorer.testnet.lez.logos.co/transaction/6c57df67d732854779e4f90e36a3c07339ead8f50a2117c86e1f4f1340da71fd) | 16475 |
| deshield → ephemeral #2 | [`9da4fe4b…cd2167aa`](https://explorer.testnet.lez.logos.co/transaction/9da4fe4bf848c54d1b6324e05cb873ac4daa8312d41905ff2b31490ecd2167aa) | 16481 |
| buy signed by ephemeral #2 | [`ab1d9564…625cbe832`](https://explorer.testnet.lez.logos.co/transaction/ab1d956440c3cc0c83527d0b08b85e6003caf5e17e5dca1251c935c625cbe832) | 16482 |

Ephemeral #1 is `H2tR44XMAmS3a2Rt4HsowMrCwJmSomo56jM29rjhXcoS`, #2 is
`6ADXxHqN9LitGPyi4gQQAWuLVTethRj13zAjNwkH5LRm`. Different addresses, so the two
purchases share no on-chain handle.

**Balances, read from the chain on both sides of each move**, which is the only
form of this claim worth making:

| | before | after |
|---|---|---|
| shield: funded public account | 10 | 4 |
| shield: private account | 0 | 6 |
| deshield: private account | 6 | 4 |
| deshield: ephemeral #1 | 0 | 2 |

The deshield is a genuine `PrivacyPreserving` transaction: its public side
carries only the destination and the amount, while the source appears as a
nullifier and a commitment with the account data encrypted. Nothing on the
public side names the payer.

### The thing the pattern does not do, which is worth more than those six transactions

**A fresh public account cannot receive a deshield.** The first attempt failed
inside the privacy circuit with `Cannot claim unauthorized account`, and the
destination has to be initialised by its own separate transaction first.

That matters for **U06**, which asks the app to enforce the deshield of
collateral *and* gas as **one indivisible user action**. It can be one *user*
action, but it is **two on-chain transactions**, and the first one is public.

We checked what that first transaction reveals rather than assuming: decoding
the init transaction's 197 bytes, the only account identifier in it is the
ephemeral's own. **The buyer's funded account does not appear.** So there is no
hard link — but there is a timing signal, because a freshly initialised account
that immediately receives a deshield is a recognisable shape, and an
implementation that ignores this is offering a privacy guarantee it has not
measured. RFP-015's soft requirement on minimum sale duration points at the same
latency from the price side.

We would rather have found this now, at the cost of one failed transaction, than
have written "atomic deshield" into a milestone and discovered it in month four.

## Check it yourself

```bash
curl -s -X POST https://testnet.lez.logos.co -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getTransaction","params":["f45a7b2fc835e75e9633e6fe8cd00687146f2b05b22591ff38baeec80b928030"]}'
```

A deployed transaction returns `"result":[<tx>,<block>]`. The control that makes
the check mean something is a hash that was never deployed:

```bash
curl -s -X POST https://testnet.lez.logos.co -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getTransaction","params":["dededededededededededededededededededededededededededededededede"]}'
```

which returns `"result":null`. Without that second call, the first one only
proves the endpoint answers.

`scripts/verify-onchain.sh` runs all ten checks — nine transactions and the
control — and exits non-zero if any expected one is missing **or if the control
unexpectedly resolves**.

## Reproduce the binaries

```bash
cargo risczero build --manifest-path programs/curve/Cargo.toml
cargo risczero build --manifest-path programs/lbp/Cargo.toml
cargo risczero build --manifest-path programs/vesting/Cargo.toml
spel program-id artifacts/programs/antumbra_curve.bin
```

Run from the repository root, not from the program directory: the Docker build
context is taken from there, and the guests depend on `antumbra` by path.

Needs Docker (the guest builder image is `linux/amd64` and runs under emulation
on Apple silicon), `cargo risczero` 3.0.5, and `spel`.

## What these programs are, and what they are not

They are **not** the RFP deliverables. They are the parts of each that can be
settled before a grant exists: the state machines and the arithmetic, running on
chain, against real accounts.

None of them custodies tokens, and the reason is specific rather than
convenient. LEZ rule 5 refuses any post-state that debits an account the
executing program does not own, so a real escrow is a chained call into the
program that owns the balance — and that depends on LP-0013's transfer
authorities, which are awarded and merged into the prize repository but **absent
from the runtime**: at tag `v0.2.4`, `lez/programs/token/src/` carries
`initialize`, `mint`, `burn`, `transfer`, `new_definition` and `print_nft`, and
no authority module. Deploying something that pretended otherwise would be the
kind of claim that survives a proposal and fails an audit.

What each one does settle:

- **`antumbra_curve`** — the two reserve buckets kept apart, and pricing through
  `Curve::buy`, which never materialises `k` because `k = Vt · Vc` does not fit
  in a `u128` for an 18-decimal pair. That claim is worth more executed than
  argued.
- **`antumbra_lbp`** — the fractional power running inside the guest, and
  weights **derived on every call** rather than stored. RFP-016 asks for the
  correct weight "regardless of how recently the last poke occurred"; read
  strictly, that rules out storing them, so there is no stored weight here to be
  stale and no poke to be idempotent about.
- **`antumbra_vesting`** — the schedule state machine, with accrual recomputed
  from the stored terms on every claim.

Each also refuses a timestamp earlier than one it has already honoured. Accrual
and the weight schedule are both monotone in time, so a caller who can rewind
the clock can re-price an unlock or replay a weight the pool has moved past.
