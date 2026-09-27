#!/usr/bin/env bash
# Re-drive the whole antumbra_vesting lifecycle on the public LEZ testnet.
#
# The testnet gets reset. When it is, every transaction DEPLOYMENTS.md cites
# stops resolving, for everyone, and the only honest repair is to run the
# lifecycle again and cite the new hashes. This is that run, as a script rather
# than as a session history, so the next reset costs one command.
#
# Time is the chain's, not ours. `claim` and `cancel` read the sequencer-written
# clock account, so every schedule here is laid out around the clock's current
# reading and every amount is checked against the timestamp the program actually
# recorded (`last_seen`, `cancelled_at`), read back from the chain.
#
# Every step prints the transaction hash and whether it LANDED or was REFUSED,
# and that verdict comes from `getTransaction` on the sequencer, not from spel's
# own output. Refusals are expected where marked: a refused step that lands is a
# failure of this script, exactly like a paying step that does not.
#
# Needs: `spel` on $PATH and a v0.2.4 wallet home ($LEE_WALLET_HOME_DIR) holding
# the accounts below. Public accounts that sign or receive native balance are
# initialised under authenticated_transfer first (`wallet auth-transfer init`);
# token holdings are ATAs (`wallet ata create`); the two private destinations
# are `wallet account new private`, one initialised with `auth-transfer init`,
# one by shielding a unit of the token into it.
#
# Set `seq_tx_poll_max_blocks` to about 6 in the wallet config first. At the
# default of 60, spel waits an hour on every call the program refuses.
#
# Writes one TSV line per step to $OUT (default /tmp/vesting-replay.tsv):
#   label  expected  verdict  block  tx_hash
set -uo pipefail
cd "$(dirname "$0")/.."

RPC="${SEQUENCER_URL:-https://testnet.lez.logos.co}"
OUT="${OUT:-/tmp/vesting-replay.tsv}"
: "${LEE_WALLET_HOME_DIR:?set LEE_WALLET_HOME_DIR to the wallet home}"
: "${CREATOR:?base58 id of the funded creator key}"
: "${CREFUND:?base58 id of the creator account a native cancellation refunds}"
: "${BENEFICIARY:?base58 id of the beneficiary key}"
: "${NDEST:?base58 id of the public account native claims pay into}"
: "${AUTH2:?base58 id of a key nominated as a separate cancel/milestone authority}"
: "${SECOND:?base58 id of the key a position is transferred to}"
: "${DEF:?base58 id of the token definition}"
: "${SUPPLY:?base58 id of the creator token holding that funds token schedules}"
: "${REFUND_ATA:?base58 id of the creator token holding a token cancellation refunds}"
: "${BEN_ATA:?base58 id of the beneficiary token holding public token claims pay into}"
: "${PNAT:?base58 id of the beneficiary private native account}"
: "${PTOK:?base58 id of the beneficiary private token holding}"
W="${WALLET:-$HOME/data/ns.com/lp-0002/_external/lez/target/release/wallet}"
TAG="${TAG:-v2}"   # suffix for schedule ids, so a rerun on the same chain does not collide
SECTIONS="${SECTIONS:-1 2 3 4 5 6 7 8 9 10 11 12}"
want() { case " $SECTIONS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

IDL=idl/antumbra_vesting.idl.json
BIN=artifacts/programs/antumbra_vesting.bin
CLOCK=4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU   # /LEZ/ClockProgramAccount/0000001
# The 50-block clock, for private claims: a privacy-preserving transaction is
# proved against its public inputs as they were when proving began, and the
# per-block clock has moved on before a proof of minutes can land.
CLOCK50=4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWkX  # /LEZ/ClockProgramAccount/0000050
# The token program's binary, which a privacy-preserving claim of a token must
# declare as a dependency: the claim chains into it.
TOKEN_BIN="${TOKEN_BIN:-$HOME/data/ns.com/lp-0002/_external/lez/artifacts/lez/programs/token.bin}"
Z=0000000000000000000000000000000000000000000000000000000000000000

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
tokbal() { rpc getAccount "$1" | python3 -c '
import json,sys
d=bytes((json.load(sys.stdin).get("result") or {}).get("data") or [])
print(int.from_bytes(d[33:49],"little") if len(d)>=49 and d[0]==0 else 0)'; }
clock_ms() { rpc getAccount "$CLOCK" | python3 -c '
import json,sys,struct
print(struct.unpack("<QQ",bytes(json.load(sys.stdin)["result"]["data"])[:16])[1])'; }
pda() { # schedule_id -> schedule PDA, from spel's own derivation
  spel --idl "$IDL" --program "$BIN" --dry-run -- \
    make-non-cancelable --schedule-id "$1" --creator "$CREATOR" 2>/dev/null \
    | grep -oE "PDA schedule → [A-Za-z0-9]+" | awk '{print $NF}'
}
hold() { # schedule_id -> holding PDA
  spel --idl "$IDL" --program "$BIN" --dry-run -- \
    fund-schedule --schedule-id "$1" --amount 1 --creator "$CREATOR" 2>/dev/null \
    | grep -oE "PDA holding → [A-Za-z0-9]+" | awk '{print $NF}'
}
field() { python3 scripts/read-schedule.py "$1" | awk -v f="$2" '$1==f{print $2}'; }
linear_vested() { python3 -c "import sys;T,s,e,t=map(int,sys.argv[1:]);print(0 if t<s else T if t>=e else T*(t-s)//(e-s))" "$@"; }

fail=0
expect_eq() { # label got want
  if [ "$2" = "$3" ]; then echo "  ✅ $1: $2"; echo "# check ok: $1 = $2" >> "$OUT"
  else echo "  ❌ $1: got $2, want $3"; echo "# check FAIL: $1: $2 != $3" >> "$OUT"; fail=1; fi
}
# step label expected(yes|no) -- spel args...   (SPEL_PRE: extra spel options)
step() {
  local label="$1" expect="$2"; shift 3
  local log hash verdict block="-" i r
  log="$(spel --idl "$IDL" --program "$BIN" ${SPEL_PRE:-} -- "$@" 2>&1)"
  hash="$(printf '%s' "$log" | grep -oE 'tx_hash: [0-9a-f]{64}' | head -1 | awk '{print $2}')"
  verdict=REFUSED
  if [ -n "$hash" ]; then
    # spel has already waited three blocks; a landed transaction is visible by
    # then, so a further minute and a half settles "refused" rather than "late".
    for i in $(seq 1 9); do
      r="$(rpc getTransaction "$hash")"
      if printf '%s' "$r" | grep -q '"result":\['; then
        verdict=LANDED
        block="$(printf '%s' "$r" | python3 -c 'import json,sys;r=json.load(sys.stdin)["result"];b=r[1] if len(r)>1 else None;print(b.get("height") if isinstance(b,dict) else b)')"
        break
      fi
      sleep 10
    done
  else
    hash="(none: $(printf '%s' "$log" | grep -iE 'error|refus|fail|panic' | head -1 | cut -c1-90))"
  fi
  # A private claim proved across a 50-block clock tick is rejected at
  # inclusion because its clock input moved; that is the platform, not the
  # program, and one retry in the next epoch settles it. The first attempt stays
  # in the log as a note, so nothing is hidden.
  if [ "${RETRY:-0}" = 1 ] && [ "$expect" = yes ] && [ "$verdict" != LANDED ]; then
    note "$label: first attempt $hash did not land (clock tick during proving?); retrying once"
    RETRY=0 step "$label" "$expect" -- "$@"
    return
  fi
  local ok="✅"
  if { [ "$expect" = yes ] && [ "$verdict" != LANDED ]; } || { [ "$expect" = no ] && [ "$verdict" = LANDED ]; }; then
    ok="❌"; fail=1
  fi
  printf '  %s %-32s %-8s block %-6s %s\n' "$ok" "$label" "$verdict" "$block" "$hash"
  printf '%s\t%s\t%s\t%s\t%s\n' "$label" "$expect" "$verdict" "$block" "$hash" >> "$OUT"
}
note() { echo "  · $*"; echo "# $*" >> "$OUT"; }
# A private account's balance, as its owner's wallet decrypts it. spel can leave
# the wallet's sync marker past blocks it never scanned, so the marker is moved
# back to just before `$2` and the wallet re-scans from there.
privbal() { # account kind(native|token) from_block
  python3 - "$LEE_WALLET_HOME_DIR/storage.json" "$3" <<'PY'
import json, sys
p, b = sys.argv[1], int(sys.argv[2])
d = json.load(open(p)); d["last_synced_block"] = min(d["last_synced_block"], b - 1); json.dump(d, open(p, "w"))
PY
  "$W" account sync-private >/dev/null 2>&1
  "$W" account get --account-id "Private/$1" 2>/dev/null | sed -n 2p | python3 -c '
import json,sys
d=json.loads(sys.stdin.read() or "{}")
print(d["balance"] if "balance" in d else d.get("Fungible",{}).get("balance"))'
}
lastblock() { grep -v "^#" "$OUT" | tail -1 | cut -f4; }

BHEX="$(hex "$BENEFICIARY")"; SHEX="$(hex "$SECOND")"; AHEX="$(hex "$AUTH2")"; RHEX="$(hex "$CREFUND")"
MIN=60000
: > "$OUT"
echo "antumbra_vesting on $RPC"
T0="$(clock_ms)"; note "clock at start: $T0 ms"

if want 1; then
echo; echo "-- 1. native: a vested schedule pays its whole total once; the clock cannot be faked --"
ID=pay-$TAG; S=$((T0 - 20*MIN)); E=$((T0 - 10*MIN))
step create_schedule yes -- create-schedule --schedule-id "$ID" --kind 1 --start $S --cliff $S --end $E \
  --total 2 --beneficiary "$BHEX" --cancelable 0 --transferable 0 --tranches 0 \
  --cancel-authority $Z --milestone-authority $Z --refund-to "$RHEX" --creator "$CREATOR"
H="$(hold "$ID")"
step fund_schedule yes -- fund-schedule --schedule-id "$ID" --amount 2 --creator "$CREATOR"
B0="$(bal "$NDEST")"
# The negative control that matters most: an account the caller controls, in
# place of the clock. Refused, so "now" is not a number the caller chooses.
step claim_with_fake_clock_refused no -- claim --schedule-id "$ID" --destination "$NDEST" --beneficiary "$BENEFICIARY" --clock "$SECOND"
step claim yes -- claim --schedule-id "$ID" --destination "$NDEST" --beneficiary "$BENEFICIARY" --clock "$CLOCK"
expect_eq "destination received the total" "$(( $(bal "$NDEST") - B0 ))" 2
expect_eq "holding emptied" "$(bal "$H")" 0
step claim_again_refused no -- claim --schedule-id "$ID" --destination "$NDEST" --beneficiary "$BENEFICIARY" --clock "$CLOCK"
step claim_by_non_beneficiary_refused no -- claim --schedule-id "$ID" --destination "$NDEST" --beneficiary "$SECOND" --clock "$CLOCK"
fi

if want 2; then
echo; echo "-- 2. native, live accrual: the amount paid is the clock's, to the unit --"
ID=accrue-$TAG; S="$(clock_ms)"; E=$((S + 30*MIN))
step accrual_create yes -- create-schedule --schedule-id "$ID" --kind 1 --start $S --cliff $S --end $E \
  --total 600 --beneficiary "$BHEX" --cancelable 0 --transferable 0 --tranches 0 \
  --cancel-authority $Z --milestone-authority $Z --refund-to "$RHEX" --creator "$CREATOR"
step accrual_fund yes -- fund-schedule --schedule-id "$ID" --amount 600 --creator "$CREATOR"
B0="$(bal "$NDEST")"
step accrual_claim yes -- claim --schedule-id "$ID" --destination "$NDEST" --beneficiary "$BENEFICIARY" --clock "$CLOCK"
P="$(pda "$ID")"; T="$(field "$P" last_seen)"
note "claim recorded clock time $T ms, $(( (T - S) / 1000 )) s into a 1800 s schedule"
expect_eq "paid = floor(total × elapsed ÷ duration) at the recorded time" \
  "$(( $(bal "$NDEST") - B0 ))" "$(linear_vested 600 $S $E $T)"
expect_eq "the schedule's claimed field agrees" "$(field "$P" claimed)" "$(linear_vested 600 $S $E $T)"
fi

if want 3; then
echo; echo "-- 3. native cancel by the clock: unvested returns, vested stays claimable --"
ID=cancel-$TAG; S="$(clock_ms)"; E=$((S + 30*MIN))
step cancel_create yes -- create-schedule --schedule-id "$ID" --kind 1 --start $S --cliff $S --end $E \
  --total 600 --beneficiary "$BHEX" --cancelable 1 --transferable 0 --tranches 0 \
  --cancel-authority $Z --milestone-authority $Z --refund-to "$RHEX" --creator "$CREATOR"
H="$(hold "$ID")"
step cancel_fund yes -- fund-schedule --schedule-id "$ID" --amount 600 --creator "$CREATOR"
R0="$(bal "$CREFUND")"
step cancel_by_stranger_refused no -- cancel --schedule-id "$ID" --refund "$CREFUND" --authority "$SECOND" --clock "$CLOCK"
step cancel_to_other_refund_refused no -- cancel --schedule-id "$ID" --refund "$NDEST" --authority "$CREATOR" --clock "$CLOCK"
step cancel yes -- cancel --schedule-id "$ID" --refund "$CREFUND" --authority "$CREATOR" --clock "$CLOCK"
P="$(pda "$ID")"; C="$(field "$P" cancelled_at)"; V="$(linear_vested 600 $S $E $C)"
note "cancelled at clock time $C ms; vested then: $V of 600"
expect_eq "refund received the unvested part" "$(( $(bal "$CREFUND") - R0 ))" "$(( 600 - V ))"
expect_eq "holding keeps exactly the vested part" "$(bal "$H")" "$V"
B0="$(bal "$NDEST")"
step claim_after_cancel yes -- claim --schedule-id "$ID" --destination "$NDEST" --beneficiary "$BENEFICIARY" --clock "$CLOCK"
expect_eq "beneficiary still receives the vested part" "$(( $(bal "$NDEST") - B0 ))" "$V"
expect_eq "holding closes at zero" "$(bal "$H")" 0
step cancel_twice_refused no -- cancel --schedule-id "$ID" --refund "$CREFUND" --authority "$CREATOR" --clock "$CLOCK"
fi

if want 4; then
echo; echo "-- 4. milestones, signalled by a separate authority, each once --"
ID=ms-$TAG
step ms_create yes -- create-schedule --schedule-id "$ID" --kind 2 --start 0 --cliff 0 --end 1 \
  --total 2 --beneficiary "$BHEX" --cancelable 1 --transferable 0 --tranches 2 \
  --cancel-authority $Z --milestone-authority "$AHEX" --refund-to "$RHEX" --creator "$CREATOR"
H="$(hold "$ID")"
step ms_fund yes -- fund-schedule --schedule-id "$ID" --amount 2 --creator "$CREATOR"
step ms_claim_before_refused no -- claim --schedule-id "$ID" --destination "$NDEST" --beneficiary "$BENEFICIARY" --clock "$CLOCK"
step ms_signal_by_creator_refused no -- signal-milestone --schedule-id "$ID" --index 0 --authority "$CREATOR"
step ms_signal_0 yes -- signal-milestone --schedule-id "$ID" --index 0 --authority "$AUTH2"
step ms_signal_0_again_refused no -- signal-milestone --schedule-id "$ID" --index 0 --authority "$AUTH2"
B0="$(bal "$NDEST")"
step ms_claim_1 yes -- claim --schedule-id "$ID" --destination "$NDEST" --beneficiary "$BENEFICIARY" --clock "$CLOCK"
expect_eq "one tranche paid" "$(( $(bal "$NDEST") - B0 ))" 1
step ms_signal_1 yes -- signal-milestone --schedule-id "$ID" --index 1 --authority "$AUTH2"
step ms_claim_2 yes -- claim --schedule-id "$ID" --destination "$NDEST" --beneficiary "$BENEFICIARY" --clock "$CLOCK"
expect_eq "both tranches drain the holding to zero" "$(bal "$H")" 0
step ms_signal_2_refused no -- signal-milestone --schedule-id "$ID" --index 2 --authority "$AUTH2"
fi

if want 5; then
echo; echo "-- 5. a nominated cancel authority, holder-only transfer, the one-way conversion --"
ID=xfer-$TAG; S="$(clock_ms)"; E=$((S + 60*MIN))
step xfer_create yes -- create-schedule --schedule-id "$ID" --kind 1 --start $S --cliff $S --end $E \
  --total 2 --beneficiary "$BHEX" --cancelable 1 --transferable 1 --tranches 0 \
  --cancel-authority "$AHEX" --milestone-authority $Z --refund-to "$RHEX" --creator "$CREATOR"
step xfer_by_creator_refused no -- transfer-beneficiary --schedule-id "$ID" --new-beneficiary "$SHEX" --beneficiary "$CREATOR"
step xfer_by_holder yes -- transfer-beneficiary --schedule-id "$ID" --new-beneficiary "$SHEX" --beneficiary "$BENEFICIARY"
P="$(pda "$ID")"
expect_eq "the schedule names the new beneficiary" "$(field "$P" beneficiary)" "$SECOND"
step cancel_by_creator_when_delegated_refused no -- cancel --schedule-id "$ID" --refund "$CREFUND" --authority "$CREATOR" --clock "$CLOCK"
step make_non_cancelable yes -- make-non-cancelable --schedule-id "$ID" --creator "$CREATOR"
step cancel_after_refused no -- cancel --schedule-id "$ID" --refund "$CREFUND" --authority "$AUTH2" --clock "$CLOCK"
step make_non_cancelable_again_refused no -- make-non-cancelable --schedule-id "$ID" --creator "$CREATOR"
expect_eq "cancelable reads 0" "$(field "$P" cancelable)" 0
expect_eq "cancelled_at still reads 0" "$(field "$P" cancelled_at)" 0
fi

if want 6; then
echo; echo "-- 6. a real token: escrowed by the token program, paid out under our PDA seed --"
ID=tok-$TAG; S=$((T0 - 20*MIN)); E=$((T0 - 10*MIN))
step token_create yes -- create-token-schedule --schedule-id "$ID" --definition "$DEF" --refund "$REFUND_ATA" \
  --kind 1 --start $S --cliff $S --end $E --total 100 --beneficiary "$BHEX" --cancelable 0 --transferable 0 \
  --tranches 0 --cancel-authority $Z --milestone-authority $Z --creator "$CREATOR"
H="$(hold "$ID")"
step token_fund yes -- fund-token-schedule --schedule-id "$ID" --amount 100 --source "$SUPPLY"
expect_eq "token escrow holds 100, owned by the token program" \
  "$(tokbal "$H")/$(rpc getAccount "$H" | python3 -c 'import json,sys;print(json.load(sys.stdin)["result"]["program_owner"][0])')" "100/1047643340"
B0="$(tokbal "$BEN_ATA")"
step token_claim_into_native_refused no -- claim --schedule-id "$ID" --destination "$NDEST" --beneficiary "$BENEFICIARY" --clock "$CLOCK"
step token_claim yes -- claim --schedule-id "$ID" --destination "$BEN_ATA" --beneficiary "$BENEFICIARY" --clock "$CLOCK"
expect_eq "beneficiary token holding received 100" "$(( $(tokbal "$BEN_ATA") - B0 ))" 100
expect_eq "token escrow emptied" "$(tokbal "$H")" 0
fi

if want 7; then
echo; echo "-- 7. a real token, cancelled by the clock: the unvested tokens go home --"
ID=tokcancel-$TAG; S="$(clock_ms)"; E=$((S + 30*MIN))
step token_cancel_create yes -- create-token-schedule --schedule-id "$ID" --definition "$DEF" --refund "$REFUND_ATA" \
  --kind 1 --start $S --cliff $S --end $E --total 100000 --beneficiary "$BHEX" --cancelable 1 --transferable 0 \
  --tranches 0 --cancel-authority $Z --milestone-authority $Z --creator "$CREATOR"
H="$(hold "$ID")"
step token_cancel_fund yes -- fund-token-schedule --schedule-id "$ID" --amount 100000 --source "$SUPPLY"
R0="$(tokbal "$REFUND_ATA")"
step token_cancel yes -- cancel --schedule-id "$ID" --refund "$REFUND_ATA" --authority "$CREATOR" --clock "$CLOCK"
P="$(pda "$ID")"; C="$(field "$P" cancelled_at)"; V="$(linear_vested 100000 $S $E $C)"
note "cancelled at clock time $C ms; vested then: $V of 100000"
expect_eq "creator's token holding got the unvested part back" "$(( $(tokbal "$REFUND_ATA") - R0 ))" "$(( 100000 - V ))"
expect_eq "token escrow keeps exactly the vested part" "$(tokbal "$H")" "$V"
fi

if want 8; then
echo; echo "-- 8. claims into PRIVATE accounts: native and token --"
ID=privnat-$TAG; S=$((T0 - 20*MIN)); E=$((T0 - 10*MIN))
step private_native_create yes -- create-schedule --schedule-id "$ID" --kind 1 --start $S --cliff $S --end $E \
  --total 3 --beneficiary "$BHEX" --cancelable 0 --transferable 0 --tranches 0 \
  --cancel-authority $Z --milestone-authority $Z --refund-to "$RHEX" --creator "$CREATOR"
H="$(hold "$ID")"
step private_native_fund yes -- fund-schedule --schedule-id "$ID" --amount 3 --creator "$CREATOR"
P0="$(privbal "$PNAT" native "$(curl -s -m 25 -X POST "$RPC" -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":[]}' | python3 -c 'import json,sys;print(json.load(sys.stdin)["result"])')")"
RETRY=1 step private_native_claim yes -- claim --schedule-id "$ID" --destination "Private/$PNAT" --beneficiary "$BENEFICIARY" --clock "$CLOCK50"
expect_eq "public holding emptied into the private account" "$(bal "$H")" 0
expect_eq "the private account, as its owner decrypts it, gained the 3" "$(( $(privbal "$PNAT" native "$(lastblock)") - P0 ))" 3
ID=privtok-$TAG
step private_token_create yes -- create-token-schedule --schedule-id "$ID" --definition "$DEF" --refund "$REFUND_ATA" \
  --kind 1 --start $S --cliff $S --end $E --total 50 --beneficiary "$BHEX" --cancelable 0 --transferable 0 \
  --tranches 0 --cancel-authority $Z --milestone-authority $Z --creator "$CREATOR"
H="$(hold "$ID")"
step private_token_fund yes -- fund-token-schedule --schedule-id "$ID" --amount 50 --source "$SUPPLY"
Q0="$(privbal "$PTOK" token "$(curl -s -m 25 -X POST "$RPC" -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":[]}' | python3 -c 'import json,sys;print(json.load(sys.stdin)["result"])')")"
RETRY=1 SPEL_PRE="--bin-token $TOKEN_BIN" step private_token_claim yes -- claim --schedule-id "$ID" --destination "Private/$PTOK" --beneficiary "$BENEFICIARY" --clock "$CLOCK50"
expect_eq "token escrow emptied into the private holding" "$(tokbal "$H")" 0
expect_eq "the private token holding, as its owner decrypts it, gained the 50" "$(( $(privbal "$PTOK" token "$(lastblock)") - Q0 ))" 50
fi

if want 9; then
# The schedule the Basecamp panel reads: a year-long token position whose
# claimable amount grows with every block, so the panel shows vesting happening
# rather than a finished record.
echo; echo "-- 9. the schedule the Basecamp panel shows: live, accruing, token-denominated --"
ID=showcase-$TAG; S=$((T0 - 30*24*60*MIN)); E=$((T0 + 335*24*60*MIN))
step showcase_create yes -- create-token-schedule --schedule-id "$ID" --definition "$DEF" --refund "$REFUND_ATA" \
  --kind 0 --start $S --cliff $((S + 7*24*60*MIN)) --end $E --total 365000 --beneficiary "$BHEX" --cancelable 1 \
  --transferable 0 --tranches 0 --cancel-authority $Z --milestone-authority $Z --creator "$CREATOR"
step showcase_fund yes -- fund-token-schedule --schedule-id "$ID" --amount 365000 --source "$SUPPLY"
step showcase_claim yes -- claim --schedule-id "$ID" --destination "$BEN_ATA" --beneficiary "$BENEFICIARY" --clock "$CLOCK"
note "kScheduleAccount = $(pda "$ID")"
note "kScheduleHolding = $(hold "$ID")"
fi

if want 10; then
echo; echo "-- 10. the nominated cancel authority cancels, and the creator could not --"
ID=delegated-$TAG; S="$(clock_ms)"; E=$((S + 30*MIN))
step delegated_create yes -- create-schedule --schedule-id "$ID" --kind 1 --start $S --cliff $S --end $E \
  --total 60 --beneficiary "$BHEX" --cancelable 1 --transferable 0 --tranches 0 \
  --cancel-authority "$AHEX" --milestone-authority $Z --refund-to "$RHEX" --creator "$CREATOR"
H="$(hold "$ID")"
step delegated_fund yes -- fund-schedule --schedule-id "$ID" --amount 60 --creator "$CREATOR"
step delegated_cancel_by_creator_refused no -- cancel --schedule-id "$ID" --refund "$CREFUND" --authority "$CREATOR" --clock "$CLOCK"
R0="$(bal "$CREFUND")"
step delegated_cancel_by_authority yes -- cancel --schedule-id "$ID" --refund "$CREFUND" --authority "$AUTH2" --clock "$CLOCK"
P="$(pda "$ID")"; C="$(field "$P" cancelled_at)"; V="$(linear_vested 60 $S $E $C)"
note "cancelled by the nominated authority at clock time $C ms; vested then: $V of 60"
expect_eq "the refund account got the unvested part" "$(( $(bal "$CREFUND") - R0 ))" "$(( 60 - V ))"
fi

if want 11; then
# F5: one creation for N beneficiaries, one funding for all of them. LEZ refuses
# a transaction in which two chained calls debit one payer, so the batch shares
# one holding that a single transfer fills; each schedule still keeps its own
# total, claimed amount and cancellation.
echo; echo "-- 11. batch creation: N beneficiaries, one creation, one funding, and the measured maximum --"
BID=batch-$TAG; N=8; S="$(clock_ms)"; E=$((S + 30*MIN))
WHO="$BHEX"; for i in $(seq 2 $N); do WHO="$WHO,$SHEX"; done
step batch_create yes -- create-schedule-batch --batch-id "$BID" --kind 1 --start $S --cliff $S --end $E \
  --total-each 60 --beneficiaries "$WHO" --cancelable 1 --transferable 0 --tranches 0 \
  --cancel-authority $Z --milestone-authority $Z --refund-to "$RHEX" --creator "$CREATOR" \
  --schedules "$(python3 scripts/batch-pdas.py "$BIN" "$BID" $N pdas)"
BH="$(spel --idl "$IDL" --program "$BIN" --dry-run -- fund-batch --batch-id "$BID" --amount 1 --creator "$CREATOR" 2>/dev/null | grep -oE "PDA holding → [A-Za-z0-9]+" | awk '{print $NF}')"
step batch_fund yes -- fund-batch --batch-id "$BID" --amount $((60 * N)) --creator "$CREATOR"
expect_eq "one transfer funded all $N schedules" "$(bal "$BH")" "$((60 * N))"
SID0="$(python3 scripts/batch-pdas.py "$BIN" "$BID" 1 ids)"
B0="$(bal "$NDEST")"
step batch_claim yes -- claim-batch --schedule-id "$SID0" --batch-id "$BID" --destination "$NDEST" --beneficiary "$BENEFICIARY" --clock "$CLOCK"
P="$(pda "$SID0")"; T="$(field "$P" last_seen)"; PAID=$(( $(bal "$NDEST") - B0 ))
expect_eq "schedule 0 of the batch paid its own accrual" "$PAID" "$(linear_vested 60 $S $E $T)"
SID1="$(python3 scripts/batch-pdas.py "$BIN" "$BID" 2 ids | cut -d, -f2)"
R0="$(bal "$CREFUND")"
step batch_cancel_one yes -- cancel-batch --schedule-id "$SID1" --batch-id "$BID" --refund "$CREFUND" --authority "$CREATOR" --clock "$CLOCK"
C="$(field "$(pda "$SID1")" cancelled_at)"
expect_eq "cancelling schedule 1 returned only its unvested part" "$(( $(bal "$CREFUND") - R0 ))" "$(( 60 - $(linear_vested 60 $S $E $C) ))"
expect_eq "the other schedules' escrow is untouched" "$(bal "$BH")" "$(( 60 * N - PAID - ($(bal "$CREFUND") - R0) ))"
# The maximum, measured: creation alone, doubling until the chain refuses.
MAX=$N
for M in 16 32 64 128; do
  WHO="$BHEX"; for i in $(seq 2 $M); do WHO="$WHO,$BHEX"; done
  MB=max$M-$TAG
  log="$(spel --idl "$IDL" --program "$BIN" -- create-schedule-batch --batch-id "$MB" --kind 1 --start $S --cliff $S \
    --end $E --total-each 1 --beneficiaries "$WHO" --cancelable 0 --transferable 0 --tranches 0 \
    --cancel-authority $Z --milestone-authority $Z --refund-to "$RHEX" --creator "$CREATOR" \
    --schedules "$(python3 scripts/batch-pdas.py "$BIN" "$MB" $M pdas)" 2>&1)"
  h="$(printf '%s' "$log" | grep -oE 'tx_hash: [0-9a-f]{64}' | head -1 | awk '{print $2}')"
  landed=no
  if [ -n "$h" ]; then for i in $(seq 1 9); do rpc getTransaction "$h" | grep -q '"result":\[' && { landed=yes; break; }; sleep 10; done; fi
  if [ "$landed" = yes ]; then
    MAX=$M; printf 'batch_of_%s\tyes\tLANDED\t-\t%s\n' "$M" "$h" >> "$OUT"; echo "  ✅ a batch of $M landed: $h"
  else
    note "a batch of $M did not land ($(printf '%s' "$log" | grep -iE 'error|too|limit|size' | head -1 | cut -c1-120))"; break
  fi
done
note "largest batch tried in section 11: $MAX schedules, all landed"
fi

if want 12; then
# The ceiling, found rather than assumed: keep doubling a creation-only batch
# until the chain refuses one. Reported as the largest that landed and the
# smallest that did not.
echo; echo "-- 12. the batch ceiling: doubling until the chain refuses --"
S="$(clock_ms)"; E=$((S + 30*MIN)); LAST=0; FIRST_NO=""
for M in ${BATCH_SIZES:-256 512 1024 2048 4096}; do
  WHO="$BHEX"; for i in $(seq 2 $M); do WHO="$WHO,$BHEX"; done
  MB=ceil$M-$TAG
  log="$(spel --idl "$IDL" --program "$BIN" -- create-schedule-batch --batch-id "$MB" --kind 1 --start $S --cliff $S \
    --end $E --total-each 1 --beneficiaries "$WHO" --cancelable 0 --transferable 0 --tranches 0 \
    --cancel-authority $Z --milestone-authority $Z --refund-to "$RHEX" --creator "$CREATOR" \
    --schedules "$(python3 scripts/batch-pdas.py "$BIN" "$MB" $M pdas)" 2>&1)"
  h="$(printf '%s' "$log" | grep -oE 'tx_hash: [0-9a-f]{64}' | head -1 | awk '{print $2}')"
  landed=no
  if [ -n "$h" ]; then for i in $(seq 1 9); do rpc getTransaction "$h" | grep -q '"result":\[' && { landed=yes; break; }; sleep 10; done; fi
  if [ "$landed" = yes ]; then
    LAST=$M; printf 'batch_of_%s\tyes\tLANDED\t-\t%s\n' "$M" "$h" >> "$OUT"; echo "  ✅ a batch of $M landed: $h"
  else
    FIRST_NO=$M
    note "a batch of $M did not land: $(printf '%s' "$log" | grep -iE 'error|too|limit|size|exceed' | head -1 | cut -c1-140)"
    break
  fi
done
if [ -n "$FIRST_NO" ]; then note "measured maximum batch: $LAST schedules in one creation transaction; $FIRST_NO did not land"
else note "measured maximum batch: $LAST schedules in one creation transaction (largest tried)"; fi
fi

echo
[ "$fail" -eq 0 ] && echo "Every step landed or was refused exactly as expected, and every amount matched. Log: $OUT" \
                  || echo "A step did not behave as expected — see ❌ above. Log: $OUT" >&2
exit "$fail"
