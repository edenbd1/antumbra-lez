#!/usr/bin/env bash
# Re-check every claim DEPLOYMENTS.md makes, against the public LEZ testnet.
#
# The point of the last line is the point of the whole script: a getTransaction
# that returns data proves nothing until a hash that was never deployed is shown
# to return null. Without that control, this only proves the endpoint answers.
#
# Needs curl and python3. Exits non-zero if any expected transaction is missing
# or if the control unexpectedly resolves.
set -uo pipefail
RPC="${SEQUENCER_URL:-https://testnet.lez.logos.co}"

fetch() {
  curl -s -m 25 -X POST "$RPC" -H 'Content-Type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getTransaction\",\"params\":[\"$1\"]}"
}

fail=0
ran=0
# `--only curve|lbp|vesting` narrows the run to one program, plus the controls.
#
# The three programs live in one repository and one script, which is right for the
# repository and wrong for a demo: a film about the bonding curve should not spend
# twenty lines on vesting cancellation. The controls are never filtered out — the
# never-deployed hash and the refused payout are what make any of the other lines
# mean something, and a run that dropped them would be a worse check, not a
# shorter one.
ONLY="${ONLY:-}"
case "${1:-}" in
  --only) ONLY="${2:?--only needs curve, lbp or vesting}" ;;
  "") ;;
  *) echo "usage: $0 [--only curve|lbp|vesting]" >&2; exit 2 ;;
esac
case "$ONLY" in
  ""|curve|lbp|vesting) ;;
  *) echo "--only takes curve, lbp or vesting; got '$ONLY'" >&2; exit 2 ;;
esac

# Which program a line belongs to, from the two labels it already carries. The
# labels were written for a human reading the output; this reuses them rather
# than adding a fourth column nobody would keep in step.
belongs() { # section label -> the programs it belongs to, or "shared"
  case "$1 $2" in
    # `deshield → buy → re-shield` is asked for by RFP-015 and RFP-016 and by
    # neither of the others, so the path belongs to both and to no one else.
    # It has to be decided before the patterns below, or `buy-eph-*` reads as a
    # plain curve buy and the path stops at the deshield in the pool's run.
    private\ *)                                          echo "curve lbp" ;;
    *lbp*|*pool*|*pause*|*weight*)                       echo lbp ;;
    *vest*|*schedule*|*claim*|*milestone*|*cancel*)      echo vesting ;;
    *curve*|*sale*|*buy*|*fee*|*close*|*sell*|*paying*) echo curve ;;
    *)                                                    echo shared ;;
  esac
}

SECTION=""
section() { SECTION="$1"; }         # remembered, not printed
flush_section() {
  [ -n "$SECTION" ] || return 0
  echo; echo "  $SECTION"; SECTION=""
}

check() { # program label tx expect_present
  local body height mine why
  if [ -n "$ONLY" ]; then
    mine="$(belongs "$1" "$2")"
    # CONTROL and REFUSED are the negative controls; they always run.
    case "$1" in CONTROL|rpc) mine=shared ;; REFUSED) mine=vesting ;; esac
    # `mine` may name more than one program, so this is membership, not equality.
    case " $mine " in *" shared "*|*" $ONLY "*) : ;; *) return 0 ;; esac
  fi
  flush_section
  ran=$((ran + 1))
  body="$(fetch "$3")"
  if printf '%s' "$body" | grep -q '"result":\['; then
    height="$(printf '%s' "$body" | python3 -c 'import json,sys;r=json.load(sys.stdin)["result"];b=r[1] if len(r)>1 else None;print(b.get("height") if isinstance(b,dict) else b)' 2>/dev/null)"
    if [ "$4" = "yes" ]; then
      printf '  ✅ %-8s %-16s block %-6s %s\n' "$1" "$2" "$height" "${3:0:16}…"
    else
      printf '  ❌ %-8s %-16s RESOLVED, and it must not\n' "$1" "$2"; fail=1
    fi
  else
    if [ "$4" = "no" ]; then
      # Both of these must fail to resolve, but not for the same reason, and a
      # line that gives the wrong reason is worse than one that gives none: it
      # reads as a canned string, which is exactly what a negative control is
      # there to rule out.
      case "$1" in
        REFUSED) why='was refused on submission, and never landed' ;;
        *)       why='returns null, as a never-deployed hash must' ;;
      esac
      printf '  ✅ %-8s %-16s %s\n' "$1" "$2" "$why"
    else
      printf '  ❌ %-8s %-16s MISSING %s\n' "$1" "$2" "$3"; fail=1
    fi
  fi
}

echo "Antumbra on the public LEZ testnet — $RPC"
section "-- the hardened programs, with the transfer program id pinned --"
echo
check hard    curve-deploy    f074ffe110131ed108d7ea37d6445d7492ff36842ed63399b005dc364d8c3855 yes
check hard    lbp-deploy      fbfe7e3960cd787a26699cd2690d6a663f88c895f4a68ee6bf7dffa47bbe4859 yes
check hard    vesting-deploy  9b35fc31a93a276d13a354863f0ed3c870f6b957a90086775a943837d1691ee2 yes

echo
check curve   deploy          25a8f4051b60ff471cb30d9655217e7b172b9b43f3977be327956fd2b42f1718 yes
check curve   create_sale     ec7f1bede8afebff0048d9dcd374e0e2bd73a937bed350ae61ff22ef9e7604ed yes
check curve   execute_buy     1b886f82a9966e94fb2ba2d9181fe69945ceacbd6de4318e99e3d902fa4ba71a yes
check lbp     deploy          f765ec06ae391c8d9e754f40947398cf15d66c9967f2fda23894d30098b4eac2 yes
check lbp     create_pool     417d64e3ec33b71ea9ae5e6d4a354f063c6b91ee2f4405b6e788e9d69b5dd7af yes
check lbp     execute_buy     45fa7b915283369d9c6eac61ae2a599a7a4b0042064f788ecb7540b2e2eda6b0 yes
section "-- the paying buy: collateral moved by a chained call --"
check paid    deploy-paying   b6ea6b6d79ac7e32ee52982426255412471d15d156ab197b73896aa2acf0211d yes
check paid    create_sale     7fa6b18cf81eb91624ecd9fa5e4e4d10ea8bd1da353a0a08c9786902866071b8 yes
check paid    execute_buy     ea0eeb936cd43850354f44989d6dd1cda15e1e7353ee1f5a5348da3af581ddb9 yes

check paid    lbp-deploy      65ccfc975bf88f589f91a1440fa5b40de4f9ee9f052dd59929f5ea36d6bea8e5 yes
check paid    lbp-create      583aa01747742f7db3f2fdbf0632b2ddd7c09c3f1dd13df5e774ea4b6536e8f4 yes
check paid    lbp-buy         9d981f120ec4b75d0b189691b014cff38c31bcd14df41833c509eb45e867d34c yes

# The FIRST payout design chained a transfer out of an account that had signed
# nothing, and was refused. Keeping the assertion keeps the fact: an unauthorized
# third-party account cannot be debited by a chained call. The working design
# debits the program's own holding PDA instead, and is checked above.
check REFUSED vesting-payout  b97945c950df1134dcbdf14700b572f026b4446eb289b5054448a35f457ee29a no

section "-- pause, and what it cannot pause --"
check pause   set-paused      f51fa03e27edc9fad0ec62cd4a702532e73eef16772d37293933c78f2bc8fe8a yes
check pause   resume          117ca8eeadc8f5afa889ca5c5675265ec152a2ab18e84c092135922b52dfccad yes

# The public testnet was reset on 2026-09-08 (its genesis block is dated 13:33
# UTC that day), and every transaction from before then returns null — ours and
# everyone else's. The vesting program was redeployed from the same binary on
# 2026-09-26, which reproduced the same deploy hash (9b35fc31… above, content
# addressed) and the same ImageID, and its whole lifecycle was driven again with
# scripts/replay-vesting.sh. These are those transactions. The curve and pool
# rows elsewhere in this file predate the reset and have not been re-driven yet,
# so a full run reports them missing — which is true.
section "-- vesting on the current testnet: pay, cancel, milestones, transfer --"
check vest    create          4c841d44f16347536687ee4ad9f33d3a68b179530c8c37a10b016a0ed246d78b yes
check vest    fund            f35e03b5b1c7264c8b605d98994264e1379eb0afb6deeb2457b6256b436e2837 yes
check vest    claim_and_pay   2a4f2331f9fe5ba0cb5d00f9d25598acec4304ef5292386871d0e1f91e97ce6c yes
check vest    cancel-create   3895ac750ee725a01043e7315ba25ea2883e6701da46becd12697fc4ae3aaae0 yes
check vest    cancel-fund     49606fcc7894f2536ec8cd33fa1283e7a774a7e1de8f5bb83eac1f0d94ef11b6 yes
check vest    cancel          44bee081400ea6d15cea69926b1857e17b0a08b31bed68ccbd08a76be997e341 yes
check vest    claim-post-canc 0c777ed768da314b36aef92072c3f54eb70c7f6da2368ff464fb03417b0d4785 yes
check vest    ms-create       9148abc204b554572478a867a12bd6181a60fd350138401b0ae7f3879ef3ceff yes
check vest    ms-fund         5b660ba7ab9b7887e147829178d5fb09f2c1e2faf356fc35776b250ca375fcab yes
check vest    ms-signal-0     13ae7130d2e54892057bab878ba0a83f3c68755dcce6ff0623da620c00c0c605 yes
check vest    ms-claim-1      12b0bb25490f909b40ca51f65021c49d43601206bd2b43ac0ac56f68804ecfea yes
check vest    ms-signal-1     54db03c6ed75e5aed14cbed406c89410ffa1afe457c5e799b95f08e4f590f527 yes
check vest    ms-claim-2      7990a21e636e2bc981df854f88a0407dbc991440e334df3749b4bcdd70e54e76 yes
check vest    xfer-create     c4f4a691e8abd06125f09dc08e32686841818c5b11d8365954e6d47ce04a9ac3 yes
check vest    xfer-by-holder  c035d3e3003650917216c0a6b32c9c6c09fd4b4f820325bf45ebac66b09aa7c9 yes
check vest    make-noncancel  8189e3ffdf2be9af390348de2343979bd3fe6b25f414f5772378cb87375f48bd yes
check vest    accrual-create  90281b913b27deb7ddaed36c7173df2023af34ff8c92d41ae2ed1a80a172ccfa yes
check vest    accrual-fund    a221ef0ae34b7439fe559a3e179bdd3f159092b6159f5bc820e8b3cdb703b12e yes
check vest    claim-t4959     4355400eac80b42832b46cbbf6566819bd42d850eb8a86daf9363e8de3e32e43 yes
check vest    claim-t8919     d7a53e7f32504a8ef1fdd7772ec8bfcbca968b2b58cf88a855abf909014c8f18 yes

# Each of these was submitted by the same run and refused by the program, so it
# never landed. They are the requirements stated as what the program will not do.
section "-- vesting: the refusals, each submitted and none landed --"
check REFUSED claim-again     a439e53b3f6751fc3ceedb059bbcabbf67521c6b26b0a05bdd92d2ebae21b9f4 no
check REFUSED ms-claim-early  20a96362070106a077d3d45e02e5512d0c7650331dc13de74b60faefaf55b164 no
check REFUSED ms-signal-twice 4d0c5781b62fc008d5ea2c07d4af38bb4b0dcd007bbec6245d9bb605c542db23 no
check REFUSED ms-signal-2of2  00d89ff58b3324607fc17cc66ae7a61c3bb9604e10ef5737794526caddeb319a no
check REFUSED xfer-by-creator 9f19fe874f544e6c08565278a71370307af97b5e9ae6dcdc70184b06c9820f5c no
check REFUSED cancel-noncanc  72014d82090aadf62f7a04f2af4fba0d7e5ccfc5170175cb16ad458311aa0157 no
check REFUSED noncanc-twice   7f3922243553d51ac0bd080c589b708aea349a6de94763124b77c3483c9b6129 no

section "-- the per-swap fee: accrued on the buy, swept on its own --"
check fee     deploy          53e149f997a343c91af6223b101889330cca46a1ad4ec92dadd5d8d9ba72bc91 yes
check fee     create_sale     679ec10a355bf65722bc20fe3ed1e17c05d77f6f439bf17c02b629592479a406 yes
check fee     execute_buy     9512887af1df329d7d9a201ebf550be9ee6a551e77ce14988b7ea03d2a21d9d8 yes
check fee     collect_fees    63e4e5f22214bbc57a92648e9b9a3a34080bdb8abeaa5757cd6d2eab690337d8 yes

section "-- the pool's fee, taken at close rather than per swap --"
check lbpclose deploy         9138f9111e708ba1c39feded3413352e1efd341c5fa1cfc08c003e3d0015b3ba yes
check lbpclose create_pool    e25299d867b147a6904f6a09eb61ca91b65d78b1eb5412b8498e0e22b929af8d yes
check lbpclose buy            37c6cf16765809ad6091749e8b9e181d660d18e2a3dbb1f3561baa8027e3b1fa yes

section "-- the sale that closes when its reserve empties, and pays the creator --"
check close   deploy          e63783c89976833aaa033394e89f1db302a01f8a3c99bf786648de02533f9b9c yes
check close   create_sale     6fedb9b30ce2f702dc0733f612563315f2770e042467f2945a04638eaf2822c0 yes
check close   closing_buy     554ed18d74ac875077be52a39308b1440e5707bf269f299317c73aac66ef680d yes
check close   withdraw        ad96e838b802d9e998944e9e67f5717c8490bfeef64b028d7f4484bc7f6c2bce yes

section "-- the private path, run twice with distinct ephemeral accounts --"
check private init-eph-1     646f91b21d8faf80a249ee8a6ad5ad1a1e07c74517ee03ff3f4e305b49a8880d yes
check private deshield-1     921b9e4f72425b65a5e0622e248ea7834de32215d785e24670d32ceb75de83ec yes
check private buy-eph-1      70f19695cd81be4210a304896090686529c0f5f547ad15aa062d1498e1c95a29 yes
check private init-eph-2     6c57df67d732854779e4f90e36a3c07339ead8f50a2117c86e1f4f1340da71fd yes
check private deshield-2     9da4fe4bf848c54d1b6324e05cb873ac4daa8312d41905ff2b31490ecd2167aa yes
check private buy-eph-2      ab1d956440c3cc0c83527d0b08b85e6003caf5e17e5dca1251c935c625cbe832 yes

section "-- the controls: the lines that must not resolve --"
check CONTROL never-deployed  dededededededededededededededededededededededededededededededede no

# The absence of the event mechanism is a claim this repository makes in several
# places, so it is asserted here rather than left as prose. LP-0012's awarded
# implementation added a getTransactionReceipt RPC; if it ever lands, this stops
# failing quietly and starts failing loudly, which is the point.
section "-- the event mechanism LP-0012 delivered, which the runtime does not carry --"
for method in getTransactionReceipt getEvents getLogs; do
  body="$(curl -s -m 25 -X POST "$RPC" -H 'Content-Type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$method\",\"params\":[]}")"
  if printf '%s' "$body" | grep -q 'Method not found'; then
    flush_section
    printf '  ✅ %-8s %-16s absent, as documented\n' rpc "$method"
    ran=$((ran + 1))
  else
    printf '  ❌ %-8s %-16s ANSWERS — the runtime gained events; update the docs\n' rpc "$method"
    fail=1
  fi
done

echo
if [ "$fail" -eq 0 ]; then
  {
  if [ -n "$ONLY" ]; then
    echo "All $ran expected checks for \`$ONLY\` resolve — run without --only for all of them."
    echo "The never-deployed hash still does not, which is what makes the rest mean"
    echo "something."
  else
    echo "All $ran expected checks hold."
    echo "The never-deployed hash and every refused call still do not resolve, which is"
    echo "what makes the rest mean something."
  fi
}
else
  echo "Something above did not hold." >&2
fi
exit "$fail"
