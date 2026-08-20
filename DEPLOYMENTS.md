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
