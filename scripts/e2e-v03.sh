#!/usr/bin/env bash
# Drive the whole antumbra_vesting lifecycle against a LEZ v0.3 sequencer.
#
# Written for a local standalone sequencer (see docs/deploy-v03.md), where the
# genesis accounts are funded; pointed at the public testnet it is the same run
# once CREATOR and PAYER hold LGO.
#
# Every step prints the CLI's JSON line and is checked against what it should
# do: `applied` where the program must accept, `reverted` (included, and
# charged if public, with no effect) or `rejected` (never included) where it
# must refuse. State is read back from the chain around the steps.
#
# Needs: the v0.3 `wallet` and `antumbra-vesting` binaries, LEE_WALLET_HOME_DIR
# with CREATOR and PAYER imported, and the program deployed (ANTUMBRA_PROGRAM),
# or DEPLOY=1 to deploy it first.
#
# Writes $OUT (default evidence/v03/local-e2e.tsv): label expected verdict block tx
set -uo pipefail
cd "$(dirname "$0")/.."

: "${LEE_WALLET_HOME_DIR:?set LEE_WALLET_HOME_DIR to the v0.3 wallet home}"
: "${CREATOR:?base58 id of the funded creator key}"
: "${PAYER:?base58 id of a funded key that pays fees for the others}"
RPC="${RPC:-http://127.0.0.1:3040}"
W="${WALLET:-wallet}"
CLI="${CLI:-cli/target/release/antumbra-vesting}"
OUT="${OUT:-evidence/v03/local-e2e.tsv}"
TAG="${TAG:-e2e}"
BATCH_MAX="${BATCH_MAX:-}"
SECTIONS="${SECTIONS:-1 2 3 4 5 6 7 8 9 10}"
want() { case " $SECTIONS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }
export ANTUMBRA_ELF="${ANTUMBRA_ELF:-artifacts/programs/v0.3/antumbra_vesting.bin}"
mkdir -p "$(dirname "$OUT")"

if [ "${DEPLOY:-0}" = 1 ]; then
  line="$("$CLI" deploy --payer "$PAYER" | tail -1)"
  echo "$line"
  ANTUMBRA_PROGRAM="$(printf '%s' "$line" | python3 -c 'import json,sys;print(json.load(sys.stdin)["header"])')"
fi
: "${ANTUMBRA_PROGRAM:?deploy first (DEPLOY=1) or set ANTUMBRA_PROGRAM}"
export ANTUMBRA_PROGRAM
echo "# run against $RPC, $(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee "$OUT"
echo "# program $ANTUMBRA_PROGRAM $("$CLI" image-id)" | tee -a "$OUT"

new_pub() { "$W" account new public 2>/dev/null | grep -oE 'Public/[A-Za-z0-9]+' | head -1 | cut -d/ -f2; }
new_priv() { "$W" account new private 2>/dev/null | grep -oE 'Private/[A-Za-z0-9]+' | head -1 | cut -d/ -f2; }
rpc() { curl -s -m 20 -X POST "$RPC" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":[\"$2\"]}"; }
shard() { # account program -> hex of that shard
  rpc getAccount "$1" | python3 -c '
import json,sys
r=json.load(sys.stdin).get("result") or {}
print(bytes((r.get("data") or {}).get("shards",{}).get(sys.argv[1],[])).hex())' "$2"; }
NATIVE=11111111111111111111111111111111
TOKEN=AxDdLwqkWgR9qaSctvABV1ZtvWJJyzXB8199xZueifPj
bal() { python3 -c 'import sys;h=sys.argv[1];print(int.from_bytes(bytes.fromhex(h),"little") if h else 0)' "$(shard "$1" $NATIVE)"; }
tok() { python3 -c '
import sys;b=bytes.fromhex(sys.argv[1])
print(int.from_bytes(b[33:49],"little") if len(b)>=49 and b[0]==0 else 0)' "$(shard "$1" $TOKEN)"; }
ids() { "$CLI" ids --schedule-id "$1" ${2:+--batch-id "$2"}; }
# Schedules and escrows whose final state the manifest pins: `track id [batch] [asset-shard]`.
FINAL=()
track() {
  local j; j="$(ids "$1" "${2:-}")"
  FINAL+=("$(jget "$j" schedule) $ANTUMBRA_PROGRAM" "$(jget "$j" holding) ${3:-$NATIVE}")
}
jget() { python3 -c 'import json,sys;print(json.loads(sys.argv[1])[sys.argv[2]])' "$1" "$2"; }
show() { "$CLI" show --schedule-id "$1"; }

fail=0
check() { # label got want
  if [ "$2" = "$3" ]; then echo "  ok  $1: $2"; echo "# check ok: $1 = $2" >> "$OUT"
  else echo "  FAIL $1: got $2, want $3"; echo "# check FAIL: $1: $2 != $3" >> "$OUT"; fail=1; fi
}
# step label expected(applied|refused) -- cli args...
step() {
  local label="$1" expect="$2"; shift 3
  local line verdict block tx
  # The CLI prints one JSON line; the wallet may print around it.
  line="$("$CLI" --label "$label" "$@" 2>/tmp/antumbra-e2e.err | grep '^{' | tail -1)"
  [ -n "$line" ] || line="{\"verdict\":\"error\",\"error\":\"$(tail -1 /tmp/antumbra-e2e.err | tr '"' "'")\"}"
  echo "$line"
  verdict="$(jget "$line" verdict)"; block="$(python3 -c 'import json,sys;print(json.loads(sys.argv[1]).get("block"))' "$line")"
  tx="$(python3 -c 'import json,sys;print(json.loads(sys.argv[1]).get("tx","-"))' "$line")"
  printf '%s\t%s\t%s\t%s\t%s\n' "$label" "$expect" "$verdict" "$block" "$tx" >> "$OUT"
  case "$expect:$verdict" in
    applied:applied|refused:reverted|refused:rejected|refused:not-included) echo "  ok  $label: $verdict" ;;
    *) echo "  FAIL $label: $verdict, expected $expect"; fail=1 ;;
  esac
}

BEN=$(new_pub); AUTH2=$(new_pub); SECOND=$(new_pub); REFUND=$(new_pub); NDEST=$(new_pub)
PNAT=$(new_priv); PTOK=$(new_priv)
echo "# creator $CREATOR payer $PAYER beneficiary $BEN authority2 $AUTH2 second $SECOND refund $REFUND dest $NDEST private $PNAT $PTOK" >> "$OUT"
P=(--payer "$PAYER")

echo "== 1. linear native: create funded, claim to the unit, refusals"
if want 1; then
  S="$TAG-lin"
  step create-linear applied -- "${P[@]}" create --schedule-id "$S" --beneficiary "$BEN" --creator "$CREATOR" \
    --kind linear --start now --end now+30m --total 600 --refund-to "$REFUND"
  track "$S"
  H=$(jget "$(ids "$S")" holding)
  check "linear escrow funded exactly" "$(bal "$H")" 600
  step claim-overclaim refused -- "${P[@]}" --force claim --schedule-id "$S" --beneficiary "$BEN" --to "Public/$NDEST" --amount 601
  step claim-by-creator refused -- --force claim --schedule-id "$S" --beneficiary "$CREATOR" --to "Public/$CREATOR" --amount 1
  check "escrow untouched by refusals" "$(bal "$H")" 600
  step claim-linear applied -- "${P[@]}" claim --schedule-id "$S" --beneficiary "$BEN" --to "Public/$NDEST"
  s="$(show "$S")"; c=$(jget "$s" claimed)
  check "claimed equals floor(600*(t-start)/(end-start)) at the claim's time" "$c" \
    "$(python3 -c 'import json,sys;s=json.loads(sys.argv[1]);print(600*(s["last_seen"]-s["start"])//(s["end"]-s["start"]))' "$s")"
  check "destination received the claim" "$(bal "$NDEST")" "$c"
  step create-duplicate refused -- --force create --schedule-id "$S" --beneficiary "$BEN" --creator "$CREATOR" \
    --kind linear --start now --end now+30m --total 900 --refund-to "$REFUND"
  check "duplicate took no second funding" "$(bal "$H")" "$((600 - c))"
fi

echo "== 2. cancellation: unvested home, vested stays claimable"
if want 2; then
  S="$TAG-can"
  step create-cancelable applied -- "${P[@]}" create --schedule-id "$S" --beneficiary "$BEN" --creator "$CREATOR" \
    --kind linear --start now --end now+30m --total 600 --refund-to "$REFUND"
  track "$S"
  H=$(jget "$(ids "$S")" holding)
  before=$(bal "$REFUND")
  step cancel-wrong-refund refused -- --force cancel --schedule-id "$S" --authority "$CREATOR" --refund 1
  step cancel-by-beneficiary refused -- "${P[@]}" --force cancel --schedule-id "$S" --authority "$BEN"
  step cancel applied -- cancel --schedule-id "$S" --authority "$CREATOR"
  s="$(show "$S")"
  vested=$(python3 -c 'import json,sys;s=json.loads(sys.argv[1]);print(600*(s["cancelled_at"]-s["start"])//(s["end"]-s["start"]))' "$s")
  check "refund is exactly the unvested part" "$(( $(bal "$REFUND") - before ))" "$((600 - vested))"
  check "vested part stays in escrow" "$(bal "$H")" "$vested"
  step cancel-again refused -- --force cancel --schedule-id "$S" --authority "$CREATOR" --refund 0
  if [ "$vested" -gt 0 ]; then
    step claim-after-cancel applied -- "${P[@]}" claim --schedule-id "$S" --beneficiary "$BEN" --to "Public/$NDEST"
    check "escrow empty after the vested part is claimed" "$(bal "$H")" 0
  fi
fi

echo "== 3. token escrow: create funded in tokens, claim, cancel"
if want 3; then
  # The creator holds the supply in its own token shard, and pays the fee.
  DEF=$(new_pub); SUPPLY=$CREATOR
  "$W" token new --definition-account-id "Public/$DEF" --supply-account-id "Public/$SUPPLY" --name ANTV --total-supply 1000000 2>&1 | grep -E "hash|included" | sed "s/^/  wallet: /"
  for _ in $(seq 1 12); do [ "$(tok "$SUPPLY")" = 1000000 ] && break; sleep 5; done
  check "token supply minted" "$(tok "$SUPPLY")" 1000000
  echo "# token definition $DEF supply $SUPPLY" >> "$OUT"
  S="$TAG-tok"
  step create-token applied -- "${P[@]}" create --schedule-id "$S" --beneficiary "$BEN" --creator "$SUPPLY" \
    --kind linear --start now-1m --end now+29m --total 100000 --refund-to "$REFUND" --token "$DEF"
  track "$S" "" "$TOKEN"
  H=$(jget "$(ids "$S")" holding)
  check "token escrow funded exactly" "$(tok "$H")" 100000
  step claim-token applied -- "${P[@]}" claim --schedule-id "$S" --beneficiary "$BEN" --to "Public/$BEN"
  got=$(tok "$BEN")
  check "beneficiary token holding equals claimed" "$got" "$(jget "$(show "$S")" claimed)"
  step cancel-token applied -- "${P[@]}" cancel --schedule-id "$S" --authority "$SUPPLY"
  s="$(show "$S")"
  check "token refund plus escrow plus claimed is the total" \
    "$(( $(tok "$REFUND") + $(tok "$H") + got ))" 100000
fi

echo "== 4. milestones, signalled once by a nominated authority"
if want 4; then
  S="$TAG-mil"
  step create-milestones applied -- "${P[@]}" create --schedule-id "$S" --beneficiary "$BEN" --creator "$CREATOR" \
    --kind milestones --total 1000 --tranches 4 --milestone-authority "$AUTH2" --refund-to "$REFUND"
  track "$S"
  step signal-by-creator refused -- signal --schedule-id "$S" --authority "$CREATOR" --index 0
  step signal-0 applied -- "${P[@]}" signal --schedule-id "$S" --authority "$AUTH2" --index 0
  step signal-0-again refused -- "${P[@]}" signal --schedule-id "$S" --authority "$AUTH2" --index 0
  step signal-2 applied -- "${P[@]}" signal --schedule-id "$S" --authority "$AUTH2" --index 2
  before=$(bal "$NDEST")
  step claim-milestones applied -- "${P[@]}" claim --schedule-id "$S" --beneficiary "$BEN" --to "Public/$NDEST"
  check "two of four tranches paid" "$(( $(bal "$NDEST") - before ))" 500
fi

echo "== 5. holder-only transfer; one-way non-cancelable"
if want 5; then
  S="$TAG-tr"
  step create-transferable applied -- "${P[@]}" create --schedule-id "$S" --beneficiary "$BEN" --creator "$CREATOR" \
    --kind linear --start now-30m --end now-1m --total 300 --transferable --refund-to "$REFUND"
  track "$S"
  step transfer-by-creator refused -- transfer --schedule-id "$S" --beneficiary "$CREATOR" --to "$CREATOR"
  step transfer applied -- "${P[@]}" transfer --schedule-id "$S" --beneficiary "$BEN" --to "$SECOND"
  step claim-by-old-holder refused -- "${P[@]}" --force claim --schedule-id "$S" --beneficiary "$BEN" --to "Public/$NDEST" --amount 300
  step claim-by-new-holder applied -- "${P[@]}" claim --schedule-id "$S" --beneficiary "$SECOND" --to "Public/$SECOND"
  check "new holder received the whole position" "$(bal "$SECOND")" 300
  S="$TAG-nc"
  step create-for-nc applied -- "${P[@]}" create --schedule-id "$S" --beneficiary "$BEN" --creator "$CREATOR" \
    --kind linear --start now --end now+30m --total 300 --refund-to "$REFUND"
  track "$S"
  step nc-by-beneficiary refused -- "${P[@]}" make-non-cancelable --schedule-id "$S" --creator "$BEN"
  step make-non-cancelable applied -- make-non-cancelable --schedule-id "$S" --creator "$CREATOR"
  step cancel-after-nc refused -- --force cancel --schedule-id "$S" --authority "$CREATOR"
fi

echo "== 6. nominated cancel authority"
if want 6; then
  S="$TAG-auth"
  step create-with-authority applied -- "${P[@]}" create --schedule-id "$S" --beneficiary "$BEN" --creator "$CREATOR" \
    --kind linear --start now --end now+30m --total 600 --cancel-authority "$AUTH2" --refund-to "$REFUND"
  track "$S"
  step cancel-by-creator-not-authority refused -- --force cancel --schedule-id "$S" --authority "$CREATOR"
  step cancel-by-authority applied -- "${P[@]}" cancel --schedule-id "$S" --authority "$AUTH2"
fi

echo "== 7. batch: one transfer funds N, each schedule moves only its share"
if want 7; then
  B="$TAG-batch8"
  step batch-8 applied -- "${P[@]}" --gas-limit 10000000 batch --batch-id "$B" --beneficiaries "$BEN,$SECOND" --repeat 4 \
    --creator "$CREATOR" --kind linear --start now-10m --end now+20m --total 600 --refund-to "$REFUND"
  BH=$(jget "$(ids x "$B")" holding)
  check "batch holding funded with 8 x 600" "$(bal "$BH")" 4800
  S0=$(python3 -c 'import hashlib,sys;b=sys.argv[1].encode().ljust(32,b"\0");print(hashlib.sha256(b+(0).to_bytes(32,"little")).hexdigest())' "$B")
  S1=$(python3 -c 'import hashlib,sys;b=sys.argv[1].encode().ljust(32,b"\0");print(hashlib.sha256(b+(1).to_bytes(32,"little")).hexdigest())' "$B")
  track "$S0" "$B"; track "$S1" "$B"
  step batch-claim-0 applied -- "${P[@]}" claim --schedule-id "$S0" --batch-id "$B" --beneficiary "$BEN" --to "Public/$NDEST"
  c0=$(jget "$(show "$S0")" claimed)
  step batch-cancel-1 applied -- cancel --schedule-id "$S1" --batch-id "$B" --authority "$CREATOR"
  s1="$(show "$S1")"
  r1=$(python3 -c 'import json,sys;s=json.loads(sys.argv[1]);print(600-600*(s["cancelled_at"]-s["start"])//(s["end"]-s["start"]))' "$s1")
  check "batch holding lost only schedule 0's claim and schedule 1's refund" "$(bal "$BH")" "$((4800 - c0 - r1))"
fi

echo "== 8. private claims: native and token into shielded accounts"
if want 8; then
  S="$TAG-priv"
  step create-for-private applied -- "${P[@]}" create --schedule-id "$S" --beneficiary "$BEN" --creator "$CREATOR" \
    --kind linear --start now-30m --end now-1m --total 250 --refund-to "$REFUND"
  track "$S"
  step claim-private-native applied -- claim --schedule-id "$S" --beneficiary "$BEN" --to "Private/$PNAT"
  check "private native claim recorded on the schedule" "$(jget "$(show "$S")" claimed)" 250
  "$W" account get --account-id "Private/$PNAT" 2>&1 | head -3 | sed 's/^/  wallet: /'
  if [ -n "${DEF:-}" ]; then
    S="$TAG-ptok"
    step create-token-for-private applied -- "${P[@]}" create --schedule-id "$S" --beneficiary "$BEN" --creator "$SUPPLY" \
      --kind linear --start now-30m --end now-1m --total 5000 --refund-to "$REFUND" --token "$DEF"
    track "$S" "" "$TOKEN"
    step claim-private-token applied -- claim --schedule-id "$S" --beneficiary "$BEN" --to "Private/$PTOK"
    check "private token claim recorded on the schedule" "$(jget "$(show "$S")" claimed)" 5000
    "$W" account get --account-id "Private/$PTOK" 2>&1 | head -3 | sed 's/^/  wallet: /'
  fi
fi

echo "== 9. batch ceiling under the 10M gas cap"
if want 9 && [ -n "$BATCH_MAX" ]; then
  step "batch-$BATCH_MAX" applied -- "${P[@]}" --gas-limit 10000000 batch --batch-id "$TAG-max" --beneficiaries "$BEN" \
    --repeat "$BATCH_MAX" --creator "$CREATOR" --kind linear --start now --end now+30m --total 1 --refund-to "$REFUND"
  step "batch-$((BATCH_MAX + 1))" refused -- "${P[@]}" --gas-limit 10000000 batch --batch-id "$TAG-max1" --beneficiaries "$BEN" \
    --repeat "$((BATCH_MAX + 1))" --creator "$CREATOR" --kind linear --start now --end now+30m --total 1 --refund-to "$REFUND"
fi

echo "== 10. a claim for a time the chain has not reached is never included"
if want 10; then
  S="$TAG-lin"
  step claim-in-the-future refused -- "${P[@]}" --force claim --schedule-id "$S" --beneficiary "$BEN" --to "Public/$NDEST" --at now+1h --amount 1
fi

for f in "${FINAL[@]}"; do
  set -- $f
  echo "# final $1 $2 $(shard "$1" "$2" | python3 -c 'import hashlib,sys;print(hashlib.sha256(bytes.fromhex(sys.stdin.read().strip())).hexdigest())')" >> "$OUT"
done

applied=$(awk -F'\t' '$3=="applied"' "$OUT" | wc -l | tr -d ' ')
refused=$(awk -F'\t' '$2=="refused" && ($3=="reverted"||$3=="rejected"||$3=="not-included")' "$OUT" | wc -l | tr -d ' ')
checks=$(grep -c '^# check ok' "$OUT")
echo "summary: $applied applied, $refused refused as expected, $checks state checks ok, failures=$fail"
exit $fail
