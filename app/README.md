# Antumbra Vesting for Basecamp

A Basecamp app that looks up vesting schedules of the `antumbra_vesting`
program on the LEZ testnet v0.3 and shows what the chain holds: what has
vested, what is claimable now against the chain's own clock, what the escrow
actually holds, who can move it, and every transaction on the schedule with
the program's verdict. It holds no keys and signs nothing: to claim, it gives
the exact `antumbra-vesting` command for the beneficiary's wallet.

![A schedule in Basecamp 0.3.0, read from the public testnet](../docs/screens/basecamp-schedule.png)

## What it does

- **One field** takes a schedule id (text of at most 32 bytes, or 64 hex
  digits, as the CLI reads `--schedule-id`), a batch id (every schedule of the
  batch, numbered from 0), or a base58 account: the schedule's own account, or
  an account whose schedules are wanted. Typing works: this is a `ui_qml`
  module, not the legacy widget the v0.2 panel was.
- **Examples** are the schedules the end-to-end run of 2 Oct 2026 created on
  the public testnet ([`evidence/v03/testnet.tsv`](../evidence/v03/testnet.tsv)),
  read live: linear, milestones, a token escrow, cancelled, transferred,
  non-cancelable, claimed privately, and a batch of eight.
- **A schedule** shows the claimable amount, status chips, a claimed /
  claimable / locked bar, the vesting curve (or one bar per milestone) with
  the start, the cliff, the end, each claim, a cancellation and a marker for
  now from the chain's clock account, the escrow's native or token balance
  against what it still owes, the accounts with Copy and Explorer buttons, the
  claim command with Copy, and the activity list.
- **Activity** comes from the block explorer's index of the schedule's
  account. The index does not mark refused transactions, so each one is
  replayed against the schedule's rules (signer, amount against what had
  vested, the cancel refund, the milestone bit, last_seen for a claim at a time
  the chain had not reached) and the replay is checked against what the chain
  holds; when they disagree the view says so. A private claim shows its amount
  from the effect it recorded on the schedule; its destination stays shielded.
- **What comes next**, under the claimable amount: the accrual rate and when
  the schedule is fully vested, the amount the cliff releases and when, or
  which milestone authority has to signal what.
- **The claim command** sits where Logos Forum's reply box is, with "Copy
  command" where "Reply" is, and a choice between a public destination and a
  shielded one (`--to Private/<account>`: the schedule records the claim, the
  destination and what it received stay private).
- **The chain's clock** is its last block's time. The status line says when it
  trails the computer's clock by more than five minutes, which a quiet testnet
  does; every amount follows the chain's clock, not the computer's. An open
  schedule says when it was read, is read again every minute, and has a ↻.
- **The empty screen** offers example ids to try, one click each.
- **Settings** (node, program, explorer) are saved under the Basecamp user
  directory, `$LOGOS_USER_DIR/module_data/antumbra_lez/settings.json`, so a
  throwaway `--user-dir` never touches another instance's.
  `ANTUMBRA_RPC`, `ANTUMBRA_PROGRAM` and `ANTUMBRA_EXPLORER` win over them and
  the dialog says when they do. The defaults are `https://testnet.lez.logos.co`,
  the deployed header `FCrja8g2ZKvxZwNZchdppKWQCDxPNUHxnEMidrmqrt6X` and
  `https://explorer.testnet.lez.logos.co`.

**What a search by account cannot find.** The sequencer has no way to list the
schedules of a beneficiary, and creating a schedule is not recorded under the
beneficiary's account (only under the schedule, its escrow and the creator).
A search by account therefore finds the schedules that account has signed for
(claims, transfers, creations) plus the examples, and the result says so. A
schedule its beneficiary has never touched is found by its id.

![An account's schedules](../docs/screens/basecamp-account.png)

Below 720 px the two panes become one, with a way back, as in Logos Forum;
Basecamp's window does not go below 800 px, which is that layout:

![One pane in Basecamp's narrowest window](../docs/screens/basecamp-narrow.png)

![Milestones](../docs/screens/basecamp-milestones.png)
![A batch](../docs/screens/basecamp-batch.png)

At phone width (rendered outside Basecamp by `app/tests/qml_host.cpp`, the same
`Main.qml` against the same reader):

![Phone width](../docs/screens/view-phone-schedule.png)
![Phone width, the examples](../docs/screens/view-phone-examples.png)

![Settings](../docs/screens/basecamp-settings.png)

## Design

The look and the layout are Logos Forum's (LP-0026), control for control: its
palette and orange accent, the system font Basecamp uses, its sizes, radii,
the outlined main button, the "1 new" badge shape for status chips, the
status line with its dot, the account picker's shape for the network, two
panes becoming one. The schedules list is laid out as the Forum's Topics, a
schedule as its thread (each section with an author line: an accent label and
a muted meta), and the claim command as its reply box. The only drawing the
Forum has no equivalent for is the vesting curve, in the Forum's colours.

The icon is the author's logo redrawn as vector paths on the Forum's tile,
cream on orange, the mark filling 80% of it: [`app/design/`](design/) holds
the geometry (`gen.py`, fitted to the original bitmap with an overlap of
0.978), the icon and marks (`make.py`) and the variants that were considered
(`variants.py`). See [D-35](../docs/DECISIONS.md) for the choices.

## How it is built

A `ui_qml` module built with `mkLogosQmlModule` (logos-module-builder 0.3.1, as
Logos Forum is): [`app/src/qml/Main.qml`](src/qml/Main.qml) is the view, which
Basecamp runs; the backend runs in its own process and talks to the view over
Qt Remote Objects through [`app/src/antumbra_lez.rep`](src/antumbra_lez.rep). The
backend ([`app/src/antumbra_lez_backend.cpp`](src/antumbra_lez_backend.cpp)) only
holds the settings, opens https links on a click (Basecamp's QML sandbox
refuses `Qt.openUrlExternally` for remote URLs, so the view hands the URL over
and the backend checks it again) and hands JSON to the view. The reading and
decoding is [`app/src/vesting_chain.cpp`](src/vesting_chain.cpp), QtCore and
QtNetwork only: shards of v0.3 accounts, the 312-byte `VestingSchedule`, PDAs
of the header, the program's accrual rule, and the replay.

Two things carried over from the v0.2 panel because they still bite:

- Qt's macOS system-proxy lookup JIT-compiles a regular expression, and PCRE2's
  JIT traps under a hardened runtime without `allow-jit`, taking the process
  down on the first request. The reader declines the lookup (`NoProxy`), and
  the backend uses no `QRegularExpression` at all.
- `metadata.json` is read by CMake's `string(JSON)`, which splits on
  semicolons: a `;` in the description breaks the configure step.

```bash
nix build ./app#lgx-portable --accept-flake-config -o result     # from the repo root; or cd app first
scripts/install-local.sh /tmp/bc-antumbra app/result/*.lgx
~/Applications/LogosBasecamp-0.3.0.app/Contents/MacOS/LogosBasecamp --user-dir /tmp/bc-antumbra
```

Always a throwaway `--user-dir`, never your own Basecamp profile.
[`app/antumbra-lez.lgx`](antumbra-lez.lgx) is the darwin-arm64 package of this
version; CI builds linux-amd64, darwin-arm64 and windows-x86_64 and merges
them into one.

## Tests

```bash
cmake -S app/tests -B app/build-tests -DCMAKE_PREFIX_PATH=<Qt 6.9>
cmake --build app/build-tests
app/build-tests/chain_test app/tests/fixtures          # offline: 89 checks
app/build-tests/chain_test app/tests/fixtures --live   # also asks the public testnet
QT_QPA_PLATFORM=offscreen app/build-tests/qml_host app/src/qml/Main.qml 1500 950 shot.png testnet4-lin
```

[`app/tests/chain_test.cpp`](tests/chain_test.cpp) checks the addresses against the
ones `antumbra-vesting ids` prints, decodes nine testnet schedules saved in
[`app/tests/fixtures/`](tests/fixtures/) (vested, claimed, claimable and status at
a fixed chain time), and replays the explorer's transactions for each: the
verdicts must be the ones the run's manifest recorded, step for step, and the
replay must end where the chain is. [`app/tests/qml_host.cpp`](tests/qml_host.cpp)
runs the real `Main.qml` against the real reader with a stand-in for what
Basecamp gives a view; CI loads it at three widths with the node unreachable
and fails on any QML warning.

Verified in Basecamp 0.3.0 on macOS with a throwaway user directory, against
the public testnet: the examples load, typing an id and pressing Return opens
it (testnet4-lin, testnet4-mil), a batch id lists its eight schedules, a
beneficiary's account lists the schedules it has a role in, Settings opens,
refuses a bad URL and saves a good one to the profile's `module_data`, and
Basecamp's narrowest window gives one pane. Copy (the view's clipboard) and
Explorer (the backend's https-only opener) were exercised in Basecamp while
the view was built; the final run leaves the clipboard and the browser alone.
