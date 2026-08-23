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
    *curve*|*sale*|*buy*|*fee*|*close*|*sell*)           echo curve ;;
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
check vesting deploy          f45a7b2fc835e75e9633e6fe8cd00687146f2b05b22591ff38baeec80b928030 yes
check vesting create_schedule dbe8c7538ca3c759e0668c9fa285e6fd343aab574fa92d861514e0bcb1bfa475 yes
check vesting record_claim    3aff5549434a0573a4d98895e7fd28afbdc4353c90ebf217320e3e59ec203685 yes
section "-- the paying buy: collateral moved by a chained call --"
check paid    deploy-paying   b6ea6b6d79ac7e32ee52982426255412471d15d156ab197b73896aa2acf0211d yes
check paid    create_sale     7fa6b18cf81eb91624ecd9fa5e4e4d10ea8bd1da353a0a08c9786902866071b8 yes
check paid    execute_buy     ea0eeb936cd43850354f44989d6dd1cda15e1e7353ee1f5a5348da3af581ddb9 yes

check paid    lbp-deploy      65ccfc975bf88f589f91a1440fa5b40de4f9ee9f052dd59929f5ea36d6bea8e5 yes
check paid    lbp-create      583aa01747742f7db3f2fdbf0632b2ddd7c09c3f1dd13df5e774ea4b6536e8f4 yes
check paid    lbp-buy         9d981f120ec4b75d0b189691b014cff38c31bcd14df41833c509eb45e867d34c yes
check paid    vest-deploy     ef50f00718096f428aa59ec79492eb8563a1011d1b1fbb5b82c97b371251e700 yes
check paid    vest-create     f54e045da3acc684fa94561fcc7d649f614b9824a05e5615cb41cd24b1bcfc21 yes
check paid    vest-fund       9d9f0a9256b0893b2cdae7899d51a55bafedf1913361d4850003de99778fb2d9 yes
check paid    vest-payout     a84e5ff1efda083de4f94f2ec9f89dc800e0ea4d864e071efda2ec0883b647e2 yes

# The FIRST payout design chained a transfer out of an account that had signed
# nothing, and was refused. Keeping the assertion keeps the fact: an unauthorized
# third-party account cannot be debited by a chained call. The working design
# debits the program's own holding PDA instead, and is checked above.
check REFUSED vesting-payout  b97945c950df1134dcbdf14700b572f026b4446eb289b5054448a35f457ee29a no

section "-- pause, and what it cannot pause --"
check pause   set-paused      f51fa03e27edc9fad0ec62cd4a702532e73eef16772d37293933c78f2bc8fe8a yes
check pause   resume          117ca8eeadc8f5afa889ca5c5675265ec152a2ab18e84c092135922b52dfccad yes

section "-- vesting: cancellation and milestones --"
check vest    cancel-create   85316f14c130cc58cd08a6b6f76ced688220203cc9188ff1b4de11159a20aa76 yes
check vest    cancel-fund     190fcddad8c722107f538397492639f2a454380eb9ec831887078cfce5ba3297 yes
check vest    cancel          1d4935cd06a03feaec9bd421bd89209224b8b7b31d8b59d1726ad8b0c493fca1 yes
check vest    claim-post-canc 708978b02411b78b8115d3911a5f30ee468f7b57937ab7d49f0af9855b0db84a yes
check vest    ms-signal-0     3ae1ffc4862cf0afcc757c1e23bc03ebccb2941f554b24fb2805b85fc354ee54 yes
check vest    ms-claim-1      a47a1d3383abaa17e9cdff57977b2d18e542ba711fe5f0de77e5ab8178753422 yes
check vest    ms-signal-1     9b2d01803ea362c61e4f1a87d0305d8d3da3c53ebf2023234836062a4a82b278 yes
check vest    ms-claim-2      7e7b87ab9e97a2cce98bab3cb11156ef0bb66d8811940fa84e18080186921e4e yes

check vest    xfer-create     a5c1eef17d852681b1e993fa0dff2ca55a370d48aa43fe6f1995e175ec53487b yes
check vest    xfer-by-holder  9b87fc9d37839828733d736c4e1bf36f129fbe257d5e0b8bf5ed63c372e4dd89 yes
check vest    make-noncancel  ced7d77ea0495943cb2faac477212cd8699d6cb28aebf63ccb7a65da68cefffe yes

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
    echo "All fifty-two expected transactions resolve."
    echo "Neither the never-deployed hash nor the refused vesting payout does, which is"
    echo "what makes the other fifty-two mean something."
  fi
}
else
  echo "Something above did not hold." >&2
fi
exit "$fail"
