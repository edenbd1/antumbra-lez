#!/usr/bin/env bash
# Re-drive the whole antumbra_vesting lifecycle on the public LEZ testnet.
#
# The testnet gets reset. When it is, every transaction DEPLOYMENTS.md cites
# stops resolving, for everyone, and the only honest repair is to run the
# lifecycle again and cite the new hashes. This is that run, as a script rather
# than as a session history, so the next reset costs one command.
#
# Every step prints the transaction hash and whether it LANDED or was REFUSED,
# and that verdict comes from `getTransaction` on the sequencer, not from spel's
# own output. The refusals are expected where marked: a refused step that lands
# is a failure of this script, exactly like a paying step that does not.
#
# Needs: `spel` on $PATH, a v0.2.4 wallet home whose CREATOR is funded and
# initialised under authenticated_transfer (`wallet auth-transfer init` first,
# or the vesting program becomes the account's owner on its first signature),
# and the program deployed from artifacts/programs/antumbra_vesting.bin.
#
# Set `seq_tx_poll_max_blocks` to about 4 in the wallet config before running.
# At the default of 60, spel waits an hour on every call the program refuses,
# and seven of them are refused on purpose.
#
# Writes one TSV line per step to $OUT (default /tmp/vesting-replay.tsv):
#   label  expected  verdict  block  tx_hash
set -uo pipefail
cd "$(dirname "$0")/.."

RPC="${SEQUENCER_URL:-https://testnet.lez.logos.co}"
OUT="${OUT:-/tmp/vesting-replay.tsv}"
: "${LEE_WALLET_HOME_DIR:?set LEE_WALLET_HOME_DIR to the wallet home}"
: "${CREATOR:?base58 id of the funded creator}"
: "${BENEFICIARY:?base58 id of the beneficiary}"
: "${SECOND:?base58 id of the account the position is transferred to}"
TAG="${TAG:-r2}"   # suffix for schedule ids, so a rerun on the same chain does not collide
SECTIONS="${SECTIONS:-1 2 3 4 5}"   # run a subset, e.g. SECTIONS=5
want() { case " $SECTIONS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

IDL=idl/antumbra_vesting.idl.json
BIN=artifacts/programs/antumbra_vesting.bin

hex() { python3 - "$1" <<'PY'
import sys
B='123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
n=0
for c in sys.argv[1]: n=n*58+B.index(c)
print(n.to_bytes(32,'big').hex())
PY
}
rpc() { curl -s -m 25 -X POST "$RPC" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":[\"$2\"]}"; }
bal() { rpc getAccount "$1" | python3 -c 'import json,sys;print((json.load(sys.stdin).get("result") or {}).get("balance"))'; }
pda() { # schedule_id which -> base58 address, from spel's own derivation
  spel --idl "$IDL" --program "$BIN" --dry-run -- \
    fund-schedule --schedule-id "$1" --amount 1 --creator "$CREATOR" 2>/dev/null \
    | grep -oE "PDA $2 → [A-Za-z0-9]+" | awk '{print $NF}'
}

BHEX="$(hex "$BENEFICIARY")"
SHEX="$(hex "$SECOND")"
fail=0

# step label expected(yes|no) -- spel args...
step() {
  local label="$1" expect="$2"; shift 3
  local log hash verdict block="-" i r
  log="$(spel --idl "$IDL" --program "$BIN" -- "$@" 2>&1)"
  hash="$(printf '%s' "$log" | grep -oE 'tx_hash: [0-9a-f]{64}' | head -1 | awk '{print $2}')"
  verdict=REFUSED
  if [ -n "$hash" ]; then
    # A landed transaction is visible within a block or two; a refused one never
    # is. Wait long enough that "not seen" means refused, not "not yet".
    for i in $(seq 1 18); do
      r="$(rpc getTransaction "$hash")"
      if printf '%s' "$r" | grep -q '"result":\['; then
        verdict=LANDED
        block="$(printf '%s' "$r" | python3 -c 'import json,sys;r=json.load(sys.stdin)["result"];b=r[1] if len(r)>1 else None;print(b.get("height") if isinstance(b,dict) else b)')"
        break
      fi
      sleep 10
    done
  else
    hash="(none: $(printf '%s' "$log" | grep -iE 'error|refus|fail' | head -1 | cut -c1-80))"
  fi
  local ok="✅"
  if { [ "$expect" = yes ] && [ "$verdict" != LANDED ]; } || { [ "$expect" = no ] && [ "$verdict" = LANDED ]; }; then
    ok="❌"; fail=1
  fi
  printf '  %s %-22s %-8s block %-6s %s\n' "$ok" "$label" "$verdict" "$block" "$hash"
  printf '%s\t%s\t%s\t%s\t%s\n' "$label" "$expect" "$verdict" "$block" "$hash" >> "$OUT"
}
balances() { # label then name=id pairs
  local out="  · $1:" kv
  shift
  for kv in "$@"; do out="$out ${kv%%=*}=$(bal "${kv#*=}")"; done
  echo "$out"; echo "#$out" >> "$OUT"
}

: > "$OUT"
echo "antumbra_vesting on $RPC — $(spel program-id "$BIN" 2>/dev/null | grep -oE 'hex\): .*' | head -1)"

if want 1; then
echo; echo "-- 1. fund, claim and pay; a second claim is refused --"
ID=antumbra-pay-$TAG
step create_schedule yes -- create-schedule --schedule-id "$ID" --kind 1 --start 1000 --cliff 1000 --end 2000 \
  --total 2 --beneficiary "$BHEX" --cancelable 0 --transferable 0 --tranches 0 --creator "$CREATOR"
HOLD="$(pda "$ID" holding)"
balances "before funding" creator="$CREATOR" holding="$HOLD"
step fund_schedule yes -- fund-schedule --schedule-id "$ID" --amount 2 --creator "$CREATOR"
balances "after funding" creator="$CREATOR" holding="$HOLD" beneficiary="$BENEFICIARY"
step claim_and_pay yes -- claim-and-pay --schedule-id "$ID" --now 2000 --beneficiary "$BENEFICIARY"
balances "after claim" holding="$HOLD" beneficiary="$BENEFICIARY"
step claim_again_refused no -- claim-and-pay --schedule-id "$ID" --now 2000 --beneficiary "$BENEFICIARY"
fi

if want 2; then
echo; echo "-- 2. cancel at the midpoint; the vested half stays claimable --"
ID=antumbra-cancel-$TAG
step cancel_create yes -- create-schedule --schedule-id "$ID" --kind 1 --start 1000 --cliff 1000 --end 3000 \
  --total 2 --beneficiary "$BHEX" --cancelable 1 --transferable 1 --tranches 0 --creator "$CREATOR"
HOLD="$(pda "$ID" holding)"
step cancel_fund yes -- fund-schedule --schedule-id "$ID" --amount 2 --creator "$CREATOR"
balances "after funding" creator="$CREATOR" holding="$HOLD"
step cancel yes -- cancel --schedule-id "$ID" --now 2000 --creator "$CREATOR"
balances "after cancel at t=2000" creator="$CREATOR" holding="$HOLD"
step claim_post_cancel yes -- claim-and-pay --schedule-id "$ID" --now 9999 --beneficiary "$BENEFICIARY"
balances "after claim at t=9999" holding="$HOLD" beneficiary="$BENEFICIARY"
fi

if want 3; then
echo; echo "-- 3. milestones: two tranches, each signal once --"
ID=antumbra-ms-$TAG
step ms_create yes -- create-schedule --schedule-id "$ID" --kind 2 --start 0 --cliff 0 --end 1 \
  --total 2 --beneficiary "$BHEX" --cancelable 1 --transferable 0 --tranches 2 --creator "$CREATOR"
HOLD="$(pda "$ID" holding)"
step ms_fund yes -- fund-schedule --schedule-id "$ID" --amount 2 --creator "$CREATOR"
step ms_claim_before_refused no -- claim-and-pay --schedule-id "$ID" --now 0 --beneficiary "$BENEFICIARY"
step ms_signal_0 yes -- signal-milestone --schedule-id "$ID" --index 0 --creator "$CREATOR"
step ms_signal_0_again_refused no -- signal-milestone --schedule-id "$ID" --index 0 --creator "$CREATOR"
balances "before claim" holding="$HOLD" beneficiary="$BENEFICIARY"
step ms_claim_1 yes -- claim-and-pay --schedule-id "$ID" --now 0 --beneficiary "$BENEFICIARY"
balances "after one tranche" holding="$HOLD" beneficiary="$BENEFICIARY"
step ms_signal_1 yes -- signal-milestone --schedule-id "$ID" --index 1 --creator "$CREATOR"
step ms_claim_2 yes -- claim-and-pay --schedule-id "$ID" --now 0 --beneficiary "$BENEFICIARY"
balances "after both tranches" holding="$HOLD" beneficiary="$BENEFICIARY"
step ms_signal_2_refused no -- signal-milestone --schedule-id "$ID" --index 2 --creator "$CREATOR"
fi

if want 4; then
echo; echo "-- 4. transfer by the holder only; the one-way conversion --"
ID=antumbra-xfer-$TAG
step xfer_create yes -- create-schedule --schedule-id "$ID" --kind 1 --start 1000 --cliff 1000 --end 3000 \
  --total 2 --beneficiary "$BHEX" --cancelable 1 --transferable 1 --tranches 0 --creator "$CREATOR"
step xfer_by_creator_refused no -- transfer-beneficiary --schedule-id "$ID" --new-beneficiary "$SHEX" --beneficiary "$CREATOR"
step xfer_by_holder yes -- transfer-beneficiary --schedule-id "$ID" --new-beneficiary "$SHEX" --beneficiary "$BENEFICIARY"
step make_non_cancelable yes -- make-non-cancelable --schedule-id "$ID" --creator "$CREATOR"
step cancel_after_refused no -- cancel --schedule-id "$ID" --now 2000 --creator "$CREATOR"
step make_non_cancelable_again_refused no -- make-non-cancelable --schedule-id "$ID" --creator "$CREATOR"
fi

if want 5; then
# The accrual, to the unit, on a ratio that does not divide: 97 over a 7919-second
# window, claimed at 3959 seconds in. total × elapsed ÷ duration is 48.49…, so the
# program must pay 48 — floored, the residue left in the holding — and the claim
# at the end must pay exactly the 49 that remain, leaving the holding at zero.
echo; echo "-- 5. accrual to the unit: floored mid-schedule, exact at the end --"
ID=antumbra-accrual-$TAG
step accrual_create yes -- create-schedule --schedule-id "$ID" --kind 1 --start 1000 --cliff 1000 --end 8919 \
  --total 97 --beneficiary "$BHEX" --cancelable 0 --transferable 0 --tranches 0 --creator "$CREATOR"
HOLD="$(pda "$ID" holding)"
step accrual_fund yes -- fund-schedule --schedule-id "$ID" --amount 97 --creator "$CREATOR"
balances "after funding" holding="$HOLD" beneficiary="$BENEFICIARY"
step accrual_claim_mid yes -- claim-and-pay --schedule-id "$ID" --now 4959 --beneficiary "$BENEFICIARY"
balances "after claim at t=4959 (expect +48)" holding="$HOLD" beneficiary="$BENEFICIARY"
step accrual_claim_end yes -- claim-and-pay --schedule-id "$ID" --now 8919 --beneficiary "$BENEFICIARY"
balances "after claim at t=8919 (expect +49, holding 0)" holding="$HOLD" beneficiary="$BENEFICIARY"
fi

echo
[ "$fail" -eq 0 ] && echo "Every step landed or was refused exactly as expected. Log: $OUT" \
                  || echo "A step did not behave as expected — see ❌ above. Log: $OUT" >&2
exit "$fail"
