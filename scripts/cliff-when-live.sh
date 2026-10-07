#!/usr/bin/env bash
# Put a cliff schedule on the public testnet v0.3 once the chain moves again.
#
# The testnet stalled at block 13,982 before a cliff schedule was driven on it.
# This waits for the chain to advance, then, with the deployed program and the
# funded wallet:
#   1. creates a cliff schedule (start now-10m, cliff now+4m, end now+20m, 600 native),
#   2. sends a claim of 1 before the cliff, forced past the CLI's pre-check,
#      which the program must refuse (included, charged, nothing moves),
#   3. waits until the chain's clock passes the cliff and claims everything
#      claimable, which must be the lump accrued since the start.
# Each transaction is a manifest row in the format scripts/e2e-v03.sh writes, so
# scripts/verify-onchain.sh checks evidence/v03/cliff.tsv like any other run.
#
# Run by the LaunchAgent co.logos.rfp017-cliff every 30 minutes. It exits at
# once while the chain is stalled, never overlaps itself, resumes a run the
# chain interrupted, and disables itself after success. It never commits,
# pushes or edits anything but the evidence files it writes, and it posts a
# macOS notification when it is done.
#
#   DRY_RUN=1     every read, no transaction, no new wallet account; the
#                 manifest goes to a temporary file outside the repository
#   FORCE_LIVE=1  treat the chain as advancing (to exercise the whole path)
#
# Overrides: REPO, LEE_WALLET_HOME_DIR, WALLET, PAYER, CREATOR, RPC,
# ANTUMBRA_PROGRAM, ANTUMBRA_RPC, CLI, STATE_DIR, LOG, FUND_EACH, MIN_PAYER.
set -uo pipefail

REPO="${REPO:-$HOME/data/ns.com/antumbra-lez}"
STATE_DIR="${STATE_DIR:-$HOME/logos-monitor/rfp017-cliff}"
LOG="${LOG:-$HOME/logos-monitor/rfp017-cliff.log}"
RPC="${RPC:-https://testnet.lez.logos.co}"
export LEE_WALLET_HOME_DIR="${LEE_WALLET_HOME_DIR:-$HOME/logos-bedrock/lez-wallet-home}"
W="${WALLET:-$HOME/logos-bedrock/bin/lez-wallet}"
PAYER="${PAYER:-9nqn7xA7z54QZPHs8wWn5VzTeXTgbwq8SREr38tVK9aw}"
CREATOR="${CREATOR:-J4pQeHZtp4uE2xo8hN5j9DQ6hnyd5ikz5ZEKPgdkaCHK}"
export ANTUMBRA_PROGRAM="${ANTUMBRA_PROGRAM:-FCrja8g2ZKvxZwNZchdppKWQCDxPNUHxnEMidrmqrt6X}"
FUND_EACH="${FUND_EACH:-10000000}"
MIN_PAYER="${MIN_PAYER:-50000000}"
GAS=300000
DRY_RUN="${DRY_RUN:-0}"
FORCE_LIVE="${FORCE_LIVE:-0}"
LABEL=co.logos.rfp017-cliff
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
NATIVE=11111111111111111111111111111111

mkdir -p "$STATE_DIR" "$(dirname "$LOG")"
if [ -t 1 ] && [ "$DRY_RUN" = 1 ]; then
  exec > >(tee -a "$LOG") 2>&1
else
  exec >>"$LOG" 2>&1
fi
say() { echo "$(date '+%F %T') $*"; }
notify() { osascript -e "display notification \"$1\" with title \"RFP-017 cliff\"" >/dev/null 2>&1 || true; }

if [ -f "$STATE_DIR/DONE" ] && [ "$DRY_RUN" != 1 ]; then
  say "already done ($(cat "$STATE_DIR/DONE")); nothing to do"
  exit 0
fi

# One run at a time; a lock older than three hours is from a run that died.
LOCK="$STATE_DIR/lock"
if ! mkdir "$LOCK" 2>/dev/null; then
  if [ -n "$(find "$LOCK" -maxdepth 0 -mmin +180 2>/dev/null)" ]; then
    rmdir "$LOCK" 2>/dev/null; mkdir "$LOCK" || exit 0
  else
    say "another run holds the lock; exiting"; exit 0
  fi
fi
trap 'rmdir "$LOCK" 2>/dev/null' EXIT

rpc() { curl -s -m 20 -X POST "$RPC" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":$2}"; }
height() { rpc getLastBlockId '[]' | python3 -c 'import json,sys
try: print(json.load(sys.stdin)["result"])
except Exception: print("")'; }
balance() { rpc getAccountBalance "[\"$1\"]" | python3 -c 'import json,sys
try: print(int(json.load(sys.stdin)["result"]))
except Exception: print(0)'; }
shard() { # account program -> hex of that shard
  rpc getAccount "[\"$1\"]" | python3 -c '
import json,sys
r=json.load(sys.stdin).get("result") or {}
print(bytes((r.get("data") or {}).get("shards",{}).get(sys.argv[1],[])).hex())' "$2"; }
bal() { python3 -c 'import sys;h=sys.argv[1];print(int.from_bytes(bytes.fromhex(h),"little") if h else 0)' "$(shard "$1" $NATIVE)"; }
jget() { python3 -c 'import json,sys;print(json.loads(sys.argv[1]).get(sys.argv[2],""))' "$1" "$2"; }

# 1. Is the chain moving? Compare with the height the last run saw; only when
# that moved, confirm with a second read a minute later.
H=$(height)
[ -n "$H" ] || { say "RPC $RPC did not answer; exiting"; exit 0; }
PREV=$(cat "$STATE_DIR/last-height" 2>/dev/null || echo "")
echo "$H" > "$STATE_DIR/last-height"
if [ "$FORCE_LIVE" != 1 ]; then
  if [ -n "$PREV" ] && [ "$H" -le "$PREV" ]; then
    say "stalled at block $H (same as the last run); exiting"; exit 0
  fi
  sleep 60
  H2=$(height)
  if [ -z "$H2" ] || [ "$H2" -le "$H" ]; then
    say "stalled at block $H (no new block in 60 s); exiting"; echo "${H2:-$H}" > "$STATE_DIR/last-height"; exit 0
  fi
  echo "$H2" > "$STATE_DIR/last-height"
  say "chain is advancing: block $H then $H2"
else
  say "FORCE_LIVE: treating the chain as advancing (block $H)"
fi

# 2. The client: the repository's build if there is one, otherwise one built
# from the committed HEAD into a private directory, so a working tree in the
# middle of an edit cannot break the run. Reads go to $RPC, writes through the
# wallet's sequencer, which is the same node.
export ANTUMBRA_RPC="${ANTUMBRA_RPC:-$RPC}"
REV=$(git -C "$REPO" rev-parse HEAD)
CLI="${CLI:-$REPO/cli/target/release/antumbra-vesting}"
if [ ! -x "$CLI" ]; then
  SRC="$STATE_DIR/src"
  if [ "$(cat "$STATE_DIR/src.rev" 2>/dev/null)" != "$REV" ]; then
    rm -rf "$SRC"; mkdir -p "$SRC"
    git -C "$REPO" archive HEAD | tar -x -C "$SRC" && echo "$REV" > "$STATE_DIR/src.rev"
  fi
  CLI="$STATE_DIR/target/release/antumbra-vesting"
  if [ ! -x "$CLI" ] || [ "$SRC/cli/src/main.rs" -nt "$CLI" ]; then
    say "building the CLI at ${REV:0:7}"
    if ! (cd "$SRC" && CARGO_TARGET_DIR="$STATE_DIR/target" cargo build --release --manifest-path cli/Cargo.toml >"$STATE_DIR/build.log" 2>&1); then
      say "CLI build failed, see $STATE_DIR/build.log"; notify "CLI build failed"; exit 1
    fi
  fi
fi
export ANTUMBRA_ELF="$REPO/artifacts/programs/v0.3/antumbra_vesting.bin"
IMAGE=$("$CLI" image-id)
say "CLI $CLI (repository at ${REV:0:7}), ImageID $IMAGE"
NOW=$("$CLI" now) || { say "could not read the chain clock"; exit 1; }
say "chain clock $NOW ($(date -r $((NOW / 1000)) '+%F %T'))"

# 3. Money: the payer funds the beneficiary; the creator signs and funds the escrow.
PB=$(balance "$PAYER"); CB=$(balance "$CREATOR")
say "payer $PAYER holds $PB, creator $CREATOR holds $CB"
if [ "$PB" -lt "$MIN_PAYER" ]; then
  say "payer below $MIN_PAYER; not sending anything"; notify "payer balance too low ($PB)"; exit 1
fi
if [ "$CB" -lt $((FUND_EACH / 2)) ]; then
  say "creator below $((FUND_EACH / 2)); it will be funded from the payer"
fi

# 4. The run. Its state lives in run.env so a run the chain interrupts resumes.
RUN="$STATE_DIR/run.env"
if [ "$DRY_RUN" = 1 ]; then
  OUT=$(mktemp -t rfp017-cliff-dry).tsv
  TAG="dryrun-cliff-$(date +%Y%m%d%H%M)"; BEN=DRYRUNBENEFICIARY; NDEST=DRYRUNDESTINATION; PHASE=new
else
  # shellcheck disable=SC1090
  [ -f "$RUN" ] && . "$RUN"
  if [ -z "${TAG:-}" ]; then
    ATTEMPT=$(( $(cat "$STATE_DIR/attempts" 2>/dev/null || echo 0) + 1 ))
    echo "$ATTEMPT" > "$STATE_DIR/attempts"
    if [ "$ATTEMPT" -gt 3 ]; then
      say "three attempts already failed; giving up until reset (rm $STATE_DIR/attempts)"
      notify "three attempts failed; see the log"; exit 1
    fi
    TAG="testnet5-cliff-$(date +%Y%m%d%H%M)"
    OUT="$STATE_DIR/$TAG.tsv"
    new_pub() { "$W" account new public --label "$1" </dev/null 2>/dev/null | grep -oE 'Public/[A-Za-z0-9]+' | head -1 | cut -d/ -f2; }
    BEN=$(new_pub "$TAG-beneficiary"); NDEST=$(new_pub "$TAG-dest")
    [ -n "$BEN" ] && [ -n "$NDEST" ] || { say "could not create wallet accounts"; exit 1; }
    PHASE=new
    printf 'TAG=%s\nOUT=%s\nBEN=%s\nNDEST=%s\nPHASE=%s\n' "$TAG" "$OUT" "$BEN" "$NDEST" "$PHASE" > "$RUN"
    {
      echo "# run against $RPC, $(date -u +%Y-%m-%dT%H:%M:%SZ), scripts/cliff-when-live.sh"
      echo "# program $ANTUMBRA_PROGRAM $IMAGE"
      echo "# creator $CREATOR payer $PAYER beneficiary $BEN dest $NDEST"
    } > "$OUT"
  fi
fi
phase() { PHASE=$1; [ "$DRY_RUN" = 1 ] || sed -i '' "s/^PHASE=.*/PHASE=$1/" "$RUN"; }
S="$TAG"
P=(--payer "$PAYER" --gas-limit "$GAS")
say "run $TAG, phase $PHASE, manifest $OUT"

fail=0
check() { # label got want
  if [ "$2" = "$3" ]; then say "  ok  $1: $2"; echo "# check ok: $1 = $2" >> "$OUT"
  else say "  FAIL $1: got $2, want $3"; echo "# check FAIL: $1: $2 != $3" >> "$OUT"; fail=1; fi
}
# step label expected(applied|refused) -- cli args...
step() {
  local label="$1" expect="$2"; shift 3
  local line verdict block tx
  if [ "$DRY_RUN" = 1 ]; then
    say "  would run: antumbra-vesting --label $label $*"
    printf '%s\t%s\t%s\t%s\t%s\n' "$label" "$expect" dry-run - - >> "$OUT"
    return 0
  fi
  line="$("$CLI" --label "$label" "$@" 2>"$STATE_DIR/last.err" | grep '^{' | tail -1)"
  [ -n "$line" ] || line="{\"verdict\":\"error\",\"error\":\"$(tail -1 "$STATE_DIR/last.err" | tr '"' "'")\"}"
  say "  $line"
  verdict="$(jget "$line" verdict)"; block="$(jget "$line" block)"; tx="$(jget "$line" tx)"
  printf '%s\t%s\t%s\t%s\t%s\n' "$label" "$expect" "$verdict" "${block:-None}" "${tx:--}" >> "$OUT"
  case "$expect:$verdict" in
    applied:applied|refused:reverted) say "  ok  $label: $verdict" ;;
    *) say "  FAIL $label: $verdict, expected $expect"; fail=1 ;;
  esac
}
fund() { # account amount
  if [ "$DRY_RUN" = 1 ]; then say "  would fund $1 with $2 from $PAYER"; return 0; fi
  "$W" auth-transfer send --from "Public/$PAYER" --to "Public/$1" --amount "$2" </dev/null 2>&1 | grep -E "hash" | sed "s/^/  fund $1: /"
  for _ in $(seq 1 30); do [ "$(balance "$1")" -ge "$2" ] && return 0; sleep 10; done
  say "  funding $1 did not arrive"; return 1
}

if [ "$PHASE" = new ]; then
  fund "$BEN" "$FUND_EACH" || exit 1
  if [ "$CB" -lt $((FUND_EACH / 2)) ]; then fund "$CREATOR" "$FUND_EACH" || exit 1; fi
  step create-cliff applied -- "${P[@]}" create --schedule-id "$S" --beneficiary "$BEN" --creator "$CREATOR" \
    --kind cliff --start now-10m --cliff now+4m --end now+20m --total 600 --refund-to "$CREATOR"
  [ "$fail" = 0 ] || { say "creation failed; the next run starts a new attempt"; rm -f "$RUN"; exit 1; }
  phase created
fi

if [ "$DRY_RUN" = 1 ]; then
  START=$((NOW - 600000)); CLIFF=$((NOW + 240000)); END=$((NOW + 1200000))
  ids="$("$CLI" ids --schedule-id "$S")"; HOLD=$(jget "$ids" holding); SCHED=$(jget "$ids" schedule)
else
  s="$("$CLI" show --schedule-id "$S")" || { say "cannot read the schedule back"; exit 1; }
  START=$(jget "$s" start); CLIFF=$(jget "$s" cliff); END=$(jget "$s" end)
  ids="$("$CLI" ids --schedule-id "$S")"; HOLD=$(jget "$ids" holding); SCHED=$(jget "$ids" schedule)
fi
say "schedule $SCHED: start $START cliff $CLIFF end $END, escrow $HOLD"

if [ "$PHASE" = created ]; then
  if [ "$DRY_RUN" != 1 ]; then
    check "cliff escrow funded exactly" "$(bal "$HOLD")" 600
    check "kind is cliff" "$(jget "$s" kind)" 0
  fi
  T=$("$CLI" now)
  if [ "$DRY_RUN" = 1 ] || [ "$T" -lt "$((CLIFF - 30000))" ]; then
    step claim-before-cliff refused -- "${P[@]}" --force claim --schedule-id "$S" --beneficiary "$BEN" \
      --to "Public/$NDEST" --amount 1
    [ "$DRY_RUN" = 1 ] || check "escrow untouched by the refused claim" "$(bal "$HOLD")" 600
  else
    say "the chain clock is already within 30 s of the cliff; the refusal is skipped"
  fi
  phase refused
fi

if [ "$PHASE" = refused ]; then
  if [ "$DRY_RUN" != 1 ]; then
    # Wait for the chain's clock, not the computer's, to pass the cliff.
    for _ in $(seq 1 120); do
      T=$("$CLI" now 2>/dev/null || echo 0)
      [ "$T" -gt "$((CLIFF + 5000))" ] && break
      sleep 15
    done
    if [ "$T" -le "$((CLIFF + 5000))" ]; then
      say "the chain clock ($T) did not pass the cliff ($CLIFF) in 30 minutes; the next run resumes here"; exit 1
    fi
  fi
  step claim-after-cliff applied -- "${P[@]}" claim --schedule-id "$S" --beneficiary "$BEN" --to "Public/$NDEST"
  if [ "$DRY_RUN" != 1 ]; then
    s="$("$CLI" show --schedule-id "$S")"; c=$(jget "$s" claimed); seen=$(jget "$s" last_seen)
    lump=$(python3 -c 'import sys;a=[int(x) for x in sys.argv[1:]];print(600*(a[1]-a[0])//(a[2]-a[0]))' "$START" "$CLIFF" "$END")
    check "claimed equals floor(600*(t-start)/(end-start)) at the claim's time" "$c" \
      "$(python3 -c 'import sys;a=[int(x) for x in sys.argv[1:]];print(600*(a[1]-a[0])//(a[2]-a[0]))' "$START" "$seen" "$END")"
    check "the claim was dated at or after the cliff" "$([ "$seen" -ge "$CLIFF" ] && echo yes || echo no)" yes
    check "the claim includes the whole lump accrued by the cliff ($lump)" "$([ "$c" -ge "$lump" ] && echo yes || echo no)" yes
    check "destination received the claim" "$(bal "$NDEST")" "$c"
    check "escrow holds the rest" "$(bal "$HOLD")" "$((600 - c))"
  fi
  phase claimed
fi

# 5. Pin the final state, publish the evidence locally, verify, and stop.
if [ "$DRY_RUN" = 1 ]; then
  say "dry run complete; manifest at $OUT:"; cat "$OUT"; exit 0
fi
for f in "$SCHED $ANTUMBRA_PROGRAM" "$HOLD $NATIVE"; do
  # shellcheck disable=SC2086
  set -- $f
  echo "# final $1 $2 $(shard "$1" "$2" | python3 -c 'import hashlib,sys;print(hashlib.sha256(bytes.fromhex(sys.stdin.read().strip())).hexdigest())')" >> "$OUT"
done
applied=$(awk -F'\t' '$3=="applied"' "$OUT" | wc -l | tr -d ' ')
refused=$(awk -F'\t' '$2=="refused" && $3=="reverted"' "$OUT" | wc -l | tr -d ' ')
say "summary: $applied applied, $refused refused as expected, failures=$fail"
cp "$OUT" "$REPO/evidence/v03/cliff.tsv"
awk -v t="run $TAG" 'index($0, t) {on = 1} on' "$LOG" > "$REPO/evidence/v03/cliff-transcript.txt"
"$REPO/scripts/verify-onchain.sh" --manifest "$REPO/evidence/v03/cliff.tsv" --rpc "$RPC" > "$REPO/evidence/v03/cliff-verify.txt" 2>&1
vstat=$?
verdict="$(tail -1 "$REPO/evidence/v03/cliff-verify.txt")"
say "verify-onchain: $verdict"
echo "$(date '+%F %T') $TAG fail=$fail verify=$vstat" > "$STATE_DIR/DONE"
rm -f "$RUN"
notify "cliff on chain: $applied applied, $refused refused; verify: $verdict. Evidence in evidence/v03/cliff.tsv (not committed)"
# Disable the agent: the DONE marker already makes any run exit at once.
if [ -f "$PLIST" ]; then mv "$PLIST" "$PLIST.done"; fi
say "disabling $LABEL"
(sleep 3; launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null) </dev/null >/dev/null 2>&1 &
exit 0
