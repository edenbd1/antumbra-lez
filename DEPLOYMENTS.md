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
