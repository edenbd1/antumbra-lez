# Deployed on the public LEZ testnet

Three SPEL programs, one per open RFP proposal, live on
`https://testnet.lez.logos.co` and fetchable by anyone.

**They do not merely exist. They move value.** A bonding-curve buy debits the
buyer and credits the sale's escrow; the fee is accrued, swept, and the creator
is paid at close. A weighted-pool buy does the same on its own at-close fee
model. A vesting schedule is funded, and a claim pays the beneficiary out of an
escrow the program owns. Every figure below was read back from the chain on both
sides of the transaction that changed it, and
[`scripts/verify-onchain.sh`](scripts/verify-onchain.sh) re-checks all thirty-six
in one command — including two that must **not** resolve, because a check with
no negative control is not a check.

## The testnet was reset on 2026-09-08, and vesting has been driven again since

The public testnet's state was wiped: its current genesis block is dated
**2026-09-08 13:33 UTC**, and every transaction from before then — ours, and
every other team's — now returns `null` from the sequencer and "Transaction not
found" from the explorer. The runtime did not change; the wallet's own
`check-health` still matches every builtin program against v0.2.4.

**`antumbra_vesting` was redeployed on 2026-09-26 from the unchanged binary, and
the deploy transaction came back with the same hash it had before the reset**,
`9b35fc31…d1691ee2`, now at block 25898. That is not a coincidence to be
explained away: a LEZ deploy hash is `SHA256(len ‖ bytecode)`, so the same file
gives the same hash and the same ImageID on any chain. It is the strongest
evidence this repository can offer that the program running today is the one
the source builds.

Its whole lifecycle was then driven again — paying claim, cancellation,
milestones, transfer, the one-way conversion, and an accrual that does not
divide evenly — by [`scripts/replay-vesting.sh`](scripts/replay-vesting.sh), so
the next reset costs one command instead of an afternoon. 29 transactions: 22
landed and 7 were refused by the program exactly where the RFP says it must
refuse. `./scripts/verify-onchain.sh --only vesting` checks every one of them.

**The curve and pool tables further down predate the reset and have not been
re-driven yet.** They are left as the record of what ran, not presented as live:
a full `verify-onchain.sh` run reports them missing, which is the truth.

**The current deployments.** Each is content-addressed, so the ImageID *is* the
version: rebuilding from the same source reproduces the same program id whoever
runs the build. That also means the earlier ImageIDs quoted further down are not
stale links but earlier *programs*, each one still on chain and still fetchable.

Two digests are involved and this document previously ran them together, so they
are worth separating. The **ImageID** is what a program is called by, derived by
RISC0 from the guest ELF; `spel program-id` prints it, and it is the number in
the table below. `SHA256(u32_le(len) ‖ bytecode)` over the packaged `.bin` is a
different number — it is what the **deploy transaction** hashes to. For
`antumbra_curve` those are `49db0fc9…` and `f074ffe1…` respectively, and the
second is exactly the deploy hash in the table, for all three programs. Both are
useful: the ImageID identifies the program, the digest lets you check that a file
you hold is the file that was deployed without asking an explorer to vouch for
it. Neither substitutes for the other.

| Program | RFP | Freeze commit | ImageID | Deploy | Block |
|---|---|---|---|---|---|
| `antumbra_curve` | [015](https://github.com/logos-co/rfp/issues/179) | [`b5aa3da`](https://github.com/edenbd1/antumbra-lez/commit/b5aa3da) | `49db0fc91883668a5ccb85242aa1cdf923a557a7327f7c0613ad526fa56fc510` | [`f074ffe1…4d8c3855`](https://explorer.testnet.lez.logos.co/transaction/f074ffe110131ed108d7ea37d6445d7492ff36842ed63399b005dc364d8c3855) | 17265 |
| `antumbra_lbp` | [016](https://github.com/logos-co/rfp/issues/180) | [`b5aa3da`](https://github.com/edenbd1/antumbra-lez/commit/b5aa3da) | `51f28557602ae6daff97e51127ba6dbc5ddfe6048ea7cd89ca5e170ab6c7a82d` | [`fbfe7e39…7bbe4859`](https://explorer.testnet.lez.logos.co/transaction/fbfe7e3960cd787a26699cd2690d6a663f88c895f4a68ee6bf7dffa47bbe4859) | 17266 |
| `antumbra_vesting` | [017](https://github.com/logos-co/rfp/issues/178) | [`b5aa3da`](https://github.com/edenbd1/antumbra-lez/commit/b5aa3da) | `4c6e62a585934f87ef919c63a252fb33faef14d9ed1af2bd17da62deaf93ea7f` | [`9b35fc31…d1691ee2`](https://explorer.testnet.lez.logos.co/transaction/9b35fc31a93a276d13a354863f0ed3c870f6b957a90086775a943837d1691ee2) | 25898 (redeployed after the reset; the hash is unchanged) |

The four facts in that table — freeze commit, ImageID, deploy transaction,
block — are the convention `logos-co/lez-payment-streams` sets for its own live
program. They are what makes a deployment checkable rather than asserted.

**The builds the RFP issues cite, and where they went.** The three proposals
quote the deployments that were *driven*, which are the earlier programs, not the
top table. Both sets were live side by side until the 2026-09-08 reset removed
them; the earlier ImageIDs are recorded here rather than left for a reviewer to
reconcile. RFP-017 now cites the redeployed `4c6e62a5…` above, which is the build
every post-reset vesting transaction ran against:

| Program | RFP | Freeze commit | ImageID | Deploy | Block | Driven by |
|---|---|---|---|---|---|---|
| `antumbra_curve` | [015](https://github.com/logos-co/rfp/issues/179) | [`8c09b33`](https://github.com/edenbd1/antumbra-lez/commit/8c09b33) | `bcd6d07d27bb0d2ea8c237c46125018e5115815173025a1a24aca505835f1a23` | [`25a8f405…b42f1718`](https://explorer.testnet.lez.logos.co/transaction/25a8f4051b60ff471cb30d9655217e7b172b9b43f3977be327956fd2b42f1718) | 16339 | `create_sale`, `execute_buy` |
| `antumbra_lbp` | [016](https://github.com/logos-co/rfp/issues/180) | [`8c09b33`](https://github.com/edenbd1/antumbra-lez/commit/8c09b33) | `249648dcf6e2fe70e81c0315bdc5737037d3f343e3362697575dd0a30bbe0e08` | [`f765ec06…98b4eac2`](https://explorer.testnet.lez.logos.co/transaction/f765ec06ae391c8d9e754f40947398cf15d66c9967f2fda23894d30098b4eac2) | 16342 | `create_pool`, `execute_buy` |
| `antumbra_vesting` | [017](https://github.com/logos-co/rfp/issues/178) | [`8c09b33`](https://github.com/edenbd1/antumbra-lez/commit/8c09b33) | `26134c7901b2cb8c2dac5889155ef17be988d5cd7b77f2af8df10e39a6c235be` | [`f45a7b2f…0b928030`](https://explorer.testnet.lez.logos.co/transaction/f45a7b2fc835e75e9633e6fe8cd00687146f2b05b22591ff38baeec80b928030) | 16335 | `create_schedule`, `record_claim` |

Each ImageID there is checkable without trusting this table: fetch any driven
transaction and read its `program_id` field, which is the ImageID that executed
it. `scripts/verify-onchain.sh` covers the deploy hashes.

**If you check an ImageID against this repository, check out the freeze commit
first.** `spel program-id artifacts/programs/antumbra_curve.bin` on `main` prints
`49db0fc9…`, the top table's — not `bcd6d07d…`, the one the RFP issue quotes. Both
are correct: the binaries moved on `b5aa3da`, and the proposals cite the programs
that were driven, which `8c09b33` builds. `git checkout 8c09b33` and the same
command prints what the proposal says. The column above is there so that this
costs a checkout rather than an accusation, and the same applies to
`antumbra_lbp` and `antumbra_vesting`.

**These three carry a fix for an attack the earlier ones were open to.** LEZ
deployment is permissionless, so anyone may deploy a program and own accounts
with it. The buy path used to chain into `buyer.account.program_owner` — the
program owning whatever account the caller supplied. A caller could therefore
pass an account owned by a program they wrote, and this program would obediently
chain into it; that program could decline to move anything while the curve state
here still advanced, and the buyer would leave with tokens and keep their money.

The `authenticated_transfer` program id is now **pinned as a constant**, verified
against
[artifacts/lez/programs/authenticated\_transfer.bin](https://github.com/logos-blockchain/logos-execution-zone/blob/v0.2.4/artifacts/lez/programs/authenticated_transfer.bin)
at tag v0.2.4 — that path is in `logos-blockchain/logos-execution-zone`, not in
this repository, and it is written here without backticks for that reason. Its
ImageID is `fe96c422…5eef875a`, reproducible with `spel program-id`, and every
payer is checked against it. The question is no longer "is this account owned by
something" but "is it owned by *the* transfer program", and on a permissionless
chain those are very different questions.

**And several sections below record something the platform refused.** Those are
not failures kept for honesty's sake; each one changed the design, and two of
them corrected an explanation this document had already published. The
corrections are left visible because the loop that produced them — measure,
explain, let a test refuse the explanation, correct — is the part worth
evaluating.

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
| `antumbra_vesting` | `claim_and_pay`, t = 4959 | [`4355400e…e3e32e43`](https://explorer.testnet.lez.logos.co/transaction/4355400eac80b42832b46cbbf6566819bd42d850eb8a86daf9363e8de3e32e43) | 25968 |
| `antumbra_vesting` | `claim_and_pay`, t = 8919 | [`d7a53e7f…014c8f18`](https://explorer.testnet.lez.logos.co/transaction/d7a53e7f32504a8ef1fdd7772ec8bfcbca968b2b58cf88a855abf909014c8f18) | 25969 |

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

**Vesting, after the reset.** A linear schedule of **97** from t=1000 to t=8919,
funded and then claimed at t=4959. `total × (4959−1000) ÷ (8919−1000)` is
48.49…, and the beneficiary's balance went **45 → 93**: the program paid **48**,
floored, and left the residue in the holding (**97 → 49**). The claim at t=8919
then paid exactly the **49** that remained, and the holding reads **0**. Floored
in the middle, exact at the end, to the unit, both read back from the chain.
(The pre-reset run did the same on a schedule of 1e21 with the since-deleted
`record_claim`; that deployment is gone with the old chain.)

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

## All three programs move real value now, and the last one corrected us

`antumbra_lbp` was given the same chained-call escrow and behaves the same way:
buyer **2 → 1**, treasury **2 → 3**, and the pool advanced to
`reserve_token = 999658`, `reserve_collateral = 1001`, paying out **342**
tokens — the number `binfixed::weighted_buy` returns on the host, to the unit,
with the weight 0.745 derived from the schedule rather than stored.

| | transaction | block |
|---|---|---|
| paying LBP, deploy (`321c6a1e…`) | [`65ccfc97…d6bea8e5`](https://explorer.testnet.lez.logos.co/transaction/65ccfc975bf88f589f91a1440fa5b40de4f9ee9f052dd59929f5ea36d6bea8e5) | 16653 |
| `create_pool` with a treasury | [`583aa017…6536e8f4`](https://explorer.testnet.lez.logos.co/transaction/583aa01747742f7db3f2fdbf0632b2ddd7c09c3f1dd13df5e774ea4b6536e8f4) | |
| `execute_buy`, paid | [`9d981f12…e867d34c`](https://explorer.testnet.lez.logos.co/transaction/9d981f120ec4b75d0b189691b014cff38c31bcd14df41833c509eb45e867d34c) | |

### The vesting payout works too — and getting there corrected a claim we had already published

The first attempt chained a transfer **out of the escrow**, and it was refused.
The reading at the time was that this is what LP-0013 exists for. **That reading
was wrong**, and the transaction that disproves it is below.

**Why the first attempt failed.** A chained transfer debits an account only if
that account is *authorized*, and the runtime authorizes an account because it
signed. On a buy the payer is the buyer and the buyer signs. On a claim the payer
was a plain account that had signed nothing. So the transfer was refused —
correctly.

**Why that does not mean vesting is blocked.** LEZ rule 5 forbids a program from
**decreasing** a balance it does not own. It says nothing about increasing one,
and RFP-017 states the same thing from the other side: *"any program may increase
any account's balance"*. So make the escrow **a PDA of the vesting program
itself**, and the payout needs no authority over anyone: the program debits its
own account and credits the beneficiary, directly, with no chained call at all.

That is precisely how `logos-co/lez-payment-streams` performs its own live
withdrawals — and payment streams are continuous vesting. The answer was in the
ecosystem's own reference program the whole time.

**One structural detail, learned by failing at it.** An account cannot be
initialised and paid into in the same transaction: the chained transfer reads a
pre-state the initialisation has not written yet. `create_schedule` therefore
creates the holding and `fund_schedule` fills it, exactly as
`lez-payment-streams` splits `initialize_vault` from `deposit`.

**The full lifecycle, on chain, balances read at every step:**

| step | transaction | effect |
|---|---|---|
| deploy (`4c6e62a5…`, the hardened build) | [`9b35fc31…d1691ee2`](https://explorer.testnet.lez.logos.co/transaction/9b35fc31a93a276d13a354863f0ed3c870f6b957a90086775a943837d1691ee2) | block 25898 |
| `create_schedule` | [`4c841d44…d246d78b`](https://explorer.testnet.lez.logos.co/transaction/4c841d44f16347536687ee4ad9f33d3a68b179530c8c37a10b016a0ed246d78b) | schedule + holding created |
| `fund_schedule` | [`f35e03b5…436e2837`](https://explorer.testnet.lez.logos.co/transaction/f35e03b5b1c7264c8b605d98994264e1379eb0afb6deeb2457b6256b436e2837) | creator **30 → 28**, holding **0 → 2** |
| `claim_and_pay` | [`2a4f2331…1e97ce6c`](https://explorer.testnet.lez.logos.co/transaction/2a4f2331f9fe5ba0cb5d00f9d25598acec4304ef5292386871d0e1f91e97ce6c) | holding **2 → 0**, beneficiary **40 → 42** |

The schedule afterwards reads `total = 2`, `claimed = 2`, `last_seen = 2000`, and
a **second** claim at the same timestamp is refused, because nothing is
claimable (`a439e53b…`, submitted and never landed). Paid once, recorded once.

**So what is LP-0013 actually for, then?** Moving a balance held by a *different*
program — an SPL-style token account — where the payer is neither the signer nor
the program. Native-collateral vesting does not need it. A token-denominated
sale still does, and that distinction is now measured rather than assumed, which
is the only reason it is worth stating at all.

The earlier refused transaction `b97945c9…f457ee29a` is kept and still asserted
to return `null`, because the fact it establishes — an unauthorized third-party
account cannot be debited by a chained call — is true and worth keeping. What
changed is the conclusion drawn from it.

## The rest of the vesting lifecycle: cancellation, milestones, and what each refusal proves

The launchpad requirements were backed by transactions while the vesting ones
rested on host tests. That asymmetry is gone.

**Cancellation, and the half of it that is easy to get wrong.** A linear schedule
of 2 over `t = 1000…3000`, cancelled at the midpoint:

| step | transaction | effect |
|---|---|---|
| create + fund | [`3895ac75…ae3aaae0`](https://explorer.testnet.lez.logos.co/transaction/3895ac750ee725a01043e7315ba25ea2883e6701da46becd12697fc4ae3aaae0) · [`49606fcc…94ef11b6`](https://explorer.testnet.lez.logos.co/transaction/49606fcc7894f2536ec8cd33fa1283e7a774a7e1de8f5bb83eac1f0d94ef11b6) | creator **28 → 26**, holding **0 → 2** |
| `cancel` at t = 2000 | [`44bee081…e997e341`](https://explorer.testnet.lez.logos.co/transaction/44bee081400ea6d15cea69926b1857e17b0a08b31bed68ccbd08a76be997e341) | creator **26 → 27**, holding **2 → 1** |
| `claim_and_pay` **after** cancelling, at t = 9999 | [`0c777ed7…7b0d4785`](https://explorer.testnet.lez.logos.co/transaction/0c777ed768da314b36aef92072c3f54eb70c7f6da2368ff464fb03417b0d4785) | beneficiary **42 → 43**, holding **1 → 0** |

That last row is the requirement most implementations get wrong. The RFP says
tokens already vested but unclaimed **remain claimable after cancellation** — so
the beneficiary claims, and receives **1**: the amount vested at the instant of
cancellation, not the total, even though they asked at `t = 9999`. Accrual is
frozen where the cancellation put it. The three parts close exactly: 0 already
claimed, 1 vested and paid, 1 unvested and returned, summing to the original 2.

**Milestones, where idempotence is the whole requirement.** A schedule of 2 in
two equal tranches:

| step | outcome |
|---|---|
| claim before any signal | **refused** — nothing released (`20a96362…`) |
| `signal_milestone(0)` | [`13ae7130…00c0c605`](https://explorer.testnet.lez.logos.co/transaction/13ae7130d2e54892057bab878ba0a83f3c68755dcce6ff0623da620c00c0c605) |
| `signal_milestone(0)` **again** | **refused** — the bit is already set (`4d0c5781…`) |
| claim | [`12b0bb25…804ecfea`](https://explorer.testnet.lez.logos.co/transaction/12b0bb25490f909b40ca51f65021c49d43601206bd2b43ac0ac56f68804ecfea) — beneficiary **43 → 44**, exactly one tranche |
| `signal_milestone(1)` + claim | [`54db03c6…f590f527`](https://explorer.testnet.lez.logos.co/transaction/54db03c6ed75e5aed14cbed406c89410ffa1afe457c5e799b95f08e4f590f527) · [`7990a21e…70e54e76`](https://explorer.testnet.lez.logos.co/transaction/7990a21e636e2bc981df854f88a0407dbc991440e334df3749b4bcdd70e54e76) — beneficiary **44 → 45**, holding **→ 0** |
| `signal_milestone(2)` | **refused** — past the tranche count (`00d89ff5…`) |

The holding reaching exactly **zero** is not luck. Released is
`total × signalled / tranches` floored, so when every bit is set the numerator
equals the denominator and the whole total comes out with no residue to strand —
the same exactness the linear path buys by special-casing its final step, here
obtained for free.

**Transferability and the one-way conversion, with the refusals that define
them.** A transferable schedule
([`c4f4a691…e04a9ac3`](https://explorer.testnet.lez.logos.co/transaction/c4f4a691e8abd06125f09dc08e32686841818c5b11d8365954e6d47ce04a9ac3)):

| attempt | outcome |
|---|---|
| the **creator** transfers the beneficiary | **refused** (`9f19fe87…`) |
| the **holder** transfers it | [`c035d3e3…b09aa7c9`](https://explorer.testnet.lez.logos.co/transaction/c035d3e3003650917216c0a6b32c9c6c09fd4b4f820325bf45ebac66b09aa7c9) — the account reads the new beneficiary |
| `make_non_cancelable` | [`8189e3ff…375f48bd`](https://explorer.testnet.lez.logos.co/transaction/8189e3ffdf2be9af390348de2343979bd3fe6b25f414f5772378cb87375f48bd) — the flag reads 0 |
| `cancel` afterwards | **refused** (`72014d82…`), and `cancelled_at` still reads 0 |
| `make_non_cancelable` again | **refused** (`7f392224…`) |

Every "reads" in that table is [`scripts/read-schedule.py`](scripts/read-schedule.py)
decoding the schedule account from the chain, field by field.

The first row is the one worth having. A creator who could reassign a
beneficiary could redirect vested compensation to themselves, so the holder moves
the position and nobody else — and the transferability flag is read on that path
and written on none, which is what **F4**'s "cannot be changed after creation"
requires.

**And `record_claim` was deleted rather than kept.** It advanced a schedule
without paying, from before the payout worked. Shipping it beside
`claim_and_pay` would offer two instructions where one records a claim that never
happened — precisely the state a vesting program exists to make impossible.

## Fees are charged, swept, and taught us two things about the platform

The bonding curve now charges the per-swap fee RFP-015 specifies: rounded **up**
against the trader, taken **before** pricing so the constant product sees what
the pool actually receives, with the rate capped at **1% in the program** rather
than in policy.

| step | transaction | effect |
|---|---|---|
| deploy (`112284cf…`) | [`53e149f9…ba72bc91`](https://explorer.testnet.lez.logos.co/transaction/53e149f997a343c91af6223b101889330cca46a1ad4ec92dadd5d8d9ba72bc91) | block 16729 |
| `create_sale` at 1% | [`679ec10a…2479a406`](https://explorer.testnet.lez.logos.co/transaction/679ec10a355bf65722bc20fe3ed1e17c05d77f6f439bf17c02b629592479a406) | sale + holding created |
| `execute_buy` of 2 | [`9512887a…2a21d9d8`](https://explorer.testnet.lez.logos.co/transaction/9512887af1df329d7d9a201ebf550be9ee6a551e77ce14988b7ea03d2a21d9d8) | buyer **3 → 1**, holding **0 → 2** |
| `collect_fees` | [`63e4e5f2…690337d8`](https://explorer.testnet.lez.logos.co/transaction/63e4e5f22214bbc57a92648e9b9a3a34080bdb8abeaa5757cd6d2eab690337d8) | holding **2 → 1**, fee treasury **2 → 3** |

The sale's `real_collateral` reads **1**, not 2 — the curve was priced on what it
received after the fee, which is the ordering §8 argues, visible on chain.

### The pool closes on its own model, and the rounding shows its teeth

RFP-016 takes its fee **at close**, not per swap, because an LBP is
time-bounded — every sale reaches its end timestamp, so the fee is always
collectible. A bonding curve is demand-bounded, under 1.4% ever graduate, which
is why the sibling program charges per swap instead. Same codebase, different
fee model, and the difference is in the mechanism rather than in taste.

| step | transaction | effect |
|---|---|---|
| deploy (`566105bb…`) | [`9138f911…0015b3ba`](https://explorer.testnet.lez.logos.co/transaction/9138f9111e708ba1c39feded3413352e1efd341c5fa1cfc08c003e3d0015b3ba) | block 16766 |
| `create_pool` at 5% | [`e25299d8…b929af8d`](https://explorer.testnet.lez.logos.co/transaction/e25299d867b147a6904f6a09eb61ca91b65d78b1eb5412b8498e0e22b929af8d) | |
| `execute_buy` at `now = 3500` | [`37c6cf16…27e3b1fa`](https://explorer.testnet.lez.logos.co/transaction/37c6cf16765809ad6091749e8b9e181d660d18e2a3dbb1f3561baa8027e3b1fa) | holding **0 → 1** |
| `withdraw` **before** `t_end` | — | **refused** |
| `withdraw` after | | holding **1 → 0**, fee treasury **2 → 3**, creator **0 → 0** |

**And that last row is worth staring at.** The pool raised 1 unit. A 5% fee
rounded **up** is 1 unit. So the fee took all of it and the creator received
nothing — arithmetically correct, and rounding in the direction the design
demands, but an outcome no creator would expect.

It is not a bug to fix in the arithmetic: rounding the other way would let a
one-unit raise pay no fee at all, and the same leniency at scale is what makes a
protocol insolvent.

**The first explanation written here was wrong, and the test that replaced it
says so.** It claimed the raise is consumed below `1/fee_rate` — 20 units at 5%.
It is not. The creator receives nothing exactly when `ceil(a·r/ONE) == a`, which
solves to `a < ONE / (ONE − r)`; at any rate a sane protocol would charge, that
is **only a raise of 1**. What is true more generally is that the **effective**
rate is worst at the smallest raises and reaches 100% at a raise of one unit —
the nominal rate is a ceiling on large raises and a floor on small ones. Three
tests in `tests/fees.rs` now pin the exact boundary, because a UI quoting the
intuitive formula would misinform every creator who read it.

So it is a **minimum-raise question**, and a deployment should either enforce a
floor or state the real threshold. We would rather have met this at a raise of 1
unit on testnet, and been wrong about it in a document we could still fix, than
at a real raise.

The CLI also reported nothing for this transaction — it timed out polling —
while the transaction landed. Third instance of the same lesson: **read the
account state, never the client's verdict.**

### And the sale closes and pays the creator

A second sale was sized so that a single buy exhausts its reserve, which is
**F4**'s automatic close, and then the creator withdrew — which is **F5**.

| step | transaction | effect |
|---|---|---|
| deploy (`00125787…`) | [`e63783c8…533f9b9c`](https://explorer.testnet.lez.logos.co/transaction/e63783c89976833aaa033394e89f1db302a01f8a3c99bf786648de02533f9b9c) | block 16743 |
| `create_sale` (Vt 1000, Vc 1, reserve 500) | [`6fedb9b3…af2822c0`](https://explorer.testnet.lez.logos.co/transaction/6fedb9b30ce2f702dc0733f612563315f2770e042467f2945a04638eaf2822c0) | |
| `withdraw` **before** the close | — | **refused**, tokens remain unsold |
| `execute_buy` of 1 | [`554ed18d…66ef680d`](https://explorer.testnet.lez.logos.co/transaction/554ed18d74ac875077be52a39308b1440e5707bf269f299317c73aac66ef680d) | buys exactly 500, reserve → 0 |
| `withdraw` **after** | [`ad96e838…7f6c2bce`](https://explorer.testnet.lez.logos.co/transaction/ad96e838b802d9e998944e9e67f5717c8490bfeef64b028d7f4484bc7f6c2bce) | holding **1 → 0**, creator **0 → 1** |
| `withdraw` a second time | — | **refused**, nothing left |

Two refusals bracket the payout, and they are the reason to trust it. A creator
who could withdraw mid-sale would be taking collateral that still backs unsold
tokens, and one who could withdraw twice would be taking it from the next sale.

`withdraw` also pays **the holding less the accrued fee**, explicitly rather
than by arithmetic accident: the fee is the protocol's and `collect_fees` moves
it, so paying out the whole holding would quietly hand over revenue already
earned.

### Two things the platform taught us here, both by refusing something

**A transaction cannot both chain a call that credits an account and write that
account's balance itself.** The first fee design did exactly that — chained the
net payment to the holding and moved the fee out of it in the same instruction —
and every buy was refused, silently. Two competing post-states for one account.
So the buy **accrues** the fee into the sale's state and `collect_fees` sweeps
it in its own transaction. Not a workaround: it is also how a protocol should
account for revenue it has not yet taken.

**Two chained calls that both name the same payer are refused too**, for the
same reason from the other side: each carries its own pre-state for that
account. That killed an earlier design paying the sale and the fee treasury in
one buy. The holding PDA solves it — one call in, and the program splits
afterwards out of an account it owns.

**And a permissionless instruction has no signer, so it has no nonce.** Calling
`collect_fees` twice builds a **byte-identical transaction with the same hash**.
The chain includes it once, so the sweep is idempotent for free — the balances
above did not move on the second call — but `getTransaction` on that hash finds
the *first* transaction and a client reports success. **An SDK built here must
read account state to know what happened, never the transaction hash.** That is
the same lesson as the silent refusals above, arriving from the opposite
direction: on this platform, the hash tells you much less than you would expect.

## Two refusals that are requirements in their own right

**Slippage, checked the strongest way there is.** A buy demanding more tokens
than the curve would give is refused — and the sale account is **byte-identical
before and after**, compared as a whole rather than field by field. A partial
write here would be the kind of corruption that only shows up three trades
later.

**Emergency stop, and the thing it cannot stop.** Pausing the pool
([`f51fa03e…2bc8fe8a`](https://explorer.testnet.lez.logos.co/transaction/f51fa03e27edc9fad0ec62cd4a702532e73eef16772d37293933c78f2bc8fe8a))
makes the account read `paused = 1` and the next buy is refused; resuming
([`117ca8ee…52dfccad`](https://explorer.testnet.lez.logos.co/transaction/117ca8eeadc8f5afa889ca5c5675265ec152a2ab18e84c092135922b52dfccad))
lets it through again.

But the **weight schedule keeps moving**: 0.745 at t = 3500, 0.500 at 6000,
0.206 at 9000, paused or not. A program cannot pause the clock, and the weight
is a function of the clock. So a creator who pauses expecting the price schedule
to freeze will find it did not — which is stated in the mini-app and in the docs
rather than discovered during the incident the pause was reached for.

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
  -d '{"jsonrpc":"2.0","id":1,"method":"getTransaction","params":["9b35fc31a93a276d13a354863f0ed3c870f6b957a90086775a943837d1691ee2"]}'
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
