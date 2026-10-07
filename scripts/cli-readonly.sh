#!/usr/bin/env bash
# The CLI's read-only commands, run the way a reviewer runs them: a fresh HOME,
# no wallet, nothing but the sequencer's URL.
#
#   scripts/cli-readonly.sh [--rpc URL] [--expect FILE] [--out FILE]
#
# Runs image-id, ids, now, `show testnet4-lin` and `show --batch-id
# testnet4-batch8`, writes their transcript to --out (default
# /tmp/cli-readonly.txt), and checks what the testnet holds for those two
# schedules: 540 claimable on testnet4-lin, 8 schedules and 4,221 claimable in
# the batch, the deployed ImageID. It then checks that a command that signs
# still asks for a wallet, and that nothing was written under HOME. With
# --expect the transcript must also equal FILE byte for byte, which is how CI
# runs it against a recorded fixture (scripts/rpc-fixture.py) rather than the
# live testnet.
#
# CLI defaults to cli/target/release/antumbra-vesting.
set -euo pipefail
cd "$(dirname "$0")/.."

RPC="https://testnet.lez.logos.co"
EXPECT=""
OUT="/tmp/cli-readonly.txt"
while [ $# -gt 0 ]; do
  case "$1" in
    --rpc) RPC="$2"; shift 2 ;;
    --expect) EXPECT="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
CLI="$(cd "$(dirname "${CLI:-cli/target/release/antumbra-vesting}")" && pwd)/$(basename "${CLI:-cli/target/release/antumbra-vesting}")"
ELF="$PWD/artifacts/programs/v0.3/antumbra_vesting.bin"

HOME_DIR="$(mktemp -d)"
trap 'rm -rf "$HOME_DIR"' EXIT
run() {
  echo "\$ antumbra-vesting $*"
  # A failing command is recorded, not fatal: the checks below say which.
  (cd "$HOME_DIR" && env -i PATH="$PATH" HOME="$HOME_DIR" ANTUMBRA_RPC="$RPC" "$CLI" --elf "$ELF" "$@" 2>&1) \
    || echo "[exit $?]"
}

fail=0
ok() { echo "  ok  $1"; }
bad() { echo "  FAIL $1"; fail=1; }
field() { python3 -c '
import json,sys
try: print(json.loads(sys.argv[1].strip().splitlines()[-1])[sys.argv[2]])
except Exception: print("?")' "$1" "$2"; }

{
  run image-id
  run ids --schedule-id testnet4-lin
  run ids --schedule-id x --batch-id testnet4-batch8
  run show testnet4-lin
  run show --batch-id testnet4-batch8
} > "$OUT"

img="$(run image-id | tail -1)"
[ "$img" = 72d5cdc05004a9502be72239829071982237638b49b8fe7d1b45fd7371a425f4 ] && ok "image-id is the deployed ImageID" || bad "image-id: $img"

now="$(run now | tail -1)"
[[ "$now" =~ ^[0-9]{13}$ ]] && ok "now reads the chain clock ($now ms)" || bad "now: $now"

lin="$(run show testnet4-lin | tail -1)"
[ "$(field "$lin" claimable)" = 540 ] && [ "$(field "$lin" claimed)" = 60 ] && [ "$(field "$lin" total)" = 600 ] \
  && ok "show testnet4-lin: 600 total, 60 claimed, 540 claimable" || bad "show testnet4-lin: $lin"

batch="$(run show --batch-id testnet4-batch8)"
[ "$(field "$batch" schedules)" = 8 ] && [ "$(field "$batch" claimable)" = 4221 ] && [ "$(field "$batch" total)" = 4800 ] \
  && ok "show --batch-id testnet4-batch8: 8 schedules, 4800 total, 4221 claimable" || bad "show --batch-id: $(echo "$batch" | tail -1)"
members="$(echo "$batch" | grep -c '"index"')"
[ "$members" = 8 ] && ok "one line per member" || bad "$members member lines"

signs="$(run claim --schedule-id testnet4-lin --beneficiary 11111111111111111111111111111111 --to Public/11111111111111111111111111111111)"
echo "$signs" | grep -q "needs a v0.3 wallet home" && ok "a command that signs still asks for a wallet" || bad "claim without a wallet: $signs"

left="$(cd "$HOME_DIR" && find . -mindepth 1 | head -5)"
[ -z "$left" ] && ok "nothing written under HOME" || bad "files left under HOME: $left"

if [ -n "$EXPECT" ]; then
  if diff -u "$EXPECT" "$OUT"; then ok "transcript equals $EXPECT"; else bad "transcript differs from $EXPECT"; fi
fi

echo "transcript in $OUT"
exit $fail
