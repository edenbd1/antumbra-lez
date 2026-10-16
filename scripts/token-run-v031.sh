#!/usr/bin/env bash
# Token escrow on a LEZ v0.3.1 chain, which has no builtin token program.
#
# v0.3.1 removed the builtin token program, so the deployed antumbra_vesting
# (ImageID 72d5cdc0…) has no token to chain into there: it reaches the token
# program at the id derived from the builtin's name, and nothing can be loaded
# at that id. This run instead
#   1. loads LEZ v0.3.0's own token program (artifacts/programs/v0.3.1/token.bin,
#      byte for byte LEZ's committed artifacts/lez/programs/token.bin) as an
#      ordinary program, IMMUTABLE, at header TOKEN_HEADER;
#   2. loads the `deployed-token` build of antumbra_vesting, identical to the
#      deployed one except that its token program is the constant TOKEN_HEADER
#      (programs/vesting/core/src/lib.rs, token_program), at header VEST_HEADER;
#   3. runs sections 3 and 8 of scripts/e2e-v03.sh against them: a token minted,
#      a schedule funded in tokens, a claim, a cancellation with its refund, and
#      a private token claim into a shielded account.
# The same script, with the same wallet, keys and binaries, runs against a local
# v0.3.1 sequencer (RPC=http://127.0.0.1:3045) and against the public testnet.
#
# Each step that is already on chain is skipped (a header that already names
# its ImageID is not loaded again), so an interrupted run resumes with a new TAG.
#
#   DRY_RUN=1   check binaries, balances and headers, send nothing
#
# Needs: the v0.3.0 wallet, the CLI built with `--features deployed-token`,
# LEE_WALLET_HOME_DIR holding PAYER, CREATOR and the header and segment keys.
set -euo pipefail
cd "$(dirname "$0")/.."

RPC="${RPC:-https://testnet.lez.logos.co}"
export LEE_WALLET_HOME_DIR="${LEE_WALLET_HOME_DIR:-$HOME/logos-bedrock/rfp017-token-home}"
W="${WALLET:-$HOME/logos-bedrock/bin/lez-wallet}"
CLI="${CLI:-cli/target/deployed-token/release/antumbra-vesting}"
PAYER="${PAYER:-7qpwKKZBjZ9bhd9FeuuFWiF33uxTZnUvYaH92AdxXZMy}"
CREATOR="${CREATOR:-FFzic2nHPasHzuwUPg9nAGNJeycZFpY2yNFGPhe5KZti}"
TOKEN_HEADER="${TOKEN_HEADER:-3cwJjDqD17a7k5zMaYM82sgai7WdRzWfLCtRGntZ6SBb}"
TOKEN_SEGMENTS="${TOKEN_SEGMENTS:-6ZLxdKC8zwxsgh2B4V7X6LoGPYxChFUzCw62HeDNo77V 6uNrhoD7GUeo6M9FFvMtxq6nMgtht5BqaqnCxVLayyY6 4WKwFiuVua4oWE5UGiMm3ZmrpTQ9gNsKtTY1yk6wNd7L H1mQNQbNntvt7KF5NDgMs2q627KuVyWpGmwwWYFU9byY 74UtTpPuhRsNhbmNwGjH4PCLFp69EUDd1r1duBzqaZV4}"
VEST_HEADER="${VEST_HEADER:-B2ZDRw7fJCo5ucMNNZDVmAPpFVCcueSggXWZ1aWfgiMy}"
VEST_SEGMENTS="${VEST_SEGMENTS:-D2gCxuFQKgVsgQiVFvoKAbzaZTG81PwvFKEDRmFYw2WV CxPXRUfAoopqt9yemjSA6TomAeh9B725LEHdqGqBQJe9 DEbtuW71hpNWnUXaHM9UuG5G6SGh5XVmsTo5xkeLxMfa 3ZWQgp3CFhweRnZ6pKxBxQg2zhM6iBbuZYKtit8VBwVh}"
TOKEN_ELF="${TOKEN_ELF:-artifacts/programs/v0.3.1/token.bin}"
VEST_ELF="${VEST_ELF:-artifacts/programs/v0.3.1/antumbra_vesting.bin}"
TAG="${TAG:-tok031-$(date +%Y%m%d%H%M)}"
OUT="${OUT:-evidence/v03/token-v031.tsv}"
FUND_EACH="${FUND_EACH:-3000000}"
# What the payer must hold before anything is sent (measured on the local
# rehearsal, see evidence/v03/README.md), and what the creator is topped up to.
NEED="${NEED:-0}"
CREATOR_FEES="${CREATOR_FEES:-6000000}"
DRY_RUN="${DRY_RUN:-0}"
# The bytes this run may load; anything else is refused before a transaction.
TOKEN_SHA=728e7a331cd357385c22f55d5724c440bb846dd8bdaab5d3c5e2bec28fa330d2
VEST_SHA="${VEST_SHA:-$(cat artifacts/programs/v0.3.1/antumbra_vesting.bin.sha256)}"

say() { echo "$(date '+%F %T') $*"; }
rpc() { curl -s -m 20 -X POST "$RPC" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":[\"$2\"]}"; }
balance() { rpc getAccountBalance "$1" | python3 -c 'import json,sys
try: print(int(json.load(sys.stdin)["result"]))
except Exception: print(0)'; }
LOADER=JAQ5MVHbCkSYRzXunsrNuM2m1LS859PGveHfoYPAmcvZ
loaded() { # header -> first 32 bytes of its program_loader shard, hex
  rpc getAccount "$1" | python3 -c '
import json,sys
r=json.load(sys.stdin).get("result") or {}
print(bytes((r.get("data") or {}).get("shards",{}).get(sys.argv[1],[]))[:32].hex())' "$LOADER"; }
sha() { shasum -a 256 "$1" | cut -d' ' -f1; }

# 0. The wallet writes to the sequencer this run reads.
python3 - "$LEE_WALLET_HOME_DIR/wallet_config.json" "$RPC" <<'PY'
import json, sys
p, rpc = sys.argv[1], sys.argv[2].rstrip("/") + "/"
c = json.load(open(p))
if c["sequencers"] != [{"sequencer_addr": rpc}]:
    c["sequencers"] = [{"sequencer_addr": rpc}]
    json.dump(c, open(p, "w"), indent=2)
PY
say "run $TAG against $RPC (block $(rpc getLastBlockId '' | python3 -c 'import json,sys;print(json.load(sys.stdin).get("result"))'))"

# 1. The binaries are the ones built reproducibly, and the CLI pays out through
# the header this run loads the token program at.
[ "$(sha "$TOKEN_ELF")" = "$TOKEN_SHA" ] || { say "$TOKEN_ELF is not LEZ v0.3.0's token.bin"; exit 1; }
[ "$(sha "$VEST_ELF")" = "$VEST_SHA" ] || { say "$VEST_ELF is not the committed deployed-token build"; exit 1; }
TOKEN_IMAGE=$("$CLI" --elf "$TOKEN_ELF" image-id)
VEST_IMAGE=$("$CLI" --elf "$VEST_ELF" image-id)
say "token.bin ImageID $TOKEN_IMAGE, antumbra_vesting (deployed-token) ImageID $VEST_IMAGE"

PB=$(balance "$PAYER"); CB=$(balance "$CREATOR")
say "payer $PAYER holds $PB (needs $NEED), creator $CREATOR holds $CB"
if [ "$PB" -lt "$NEED" ]; then say "payer below $NEED; nothing sent"; exit 1; fi

deploy() { # name elf image header segments immutable(0|1)
  local name=$1 elf=$2 image=$3 header=$4 segs=$5 imm=$6 args=()
  if [ "$(loaded "$header")" = "$image" ]; then say "$name already loaded at $header"; return 0; fi
  for s in $segs; do args+=(--segment "$s"); done
  [ "$imm" = 1 ] && args+=(--immutable)
  if [ "$DRY_RUN" = 1 ]; then say "would load $name at $header (${#args[@]} args)"; return 0; fi
  say "loading $name at $header"
  "$CLI" --elf "$elf" deploy --payer "$PAYER" --header "$header" "${args[@]}"
  for _ in $(seq 1 40); do [ "$(loaded "$header")" = "$image" ] && break; sleep 15; done
  [ "$(loaded "$header")" = "$image" ] || { say "$name: header does not name $image"; exit 1; }
  say "$name loaded: header $header names $image"
}
deploy token "$TOKEN_ELF" "$TOKEN_IMAGE" "$TOKEN_HEADER" "$TOKEN_SEGMENTS" 1
deploy antumbra_vesting "$VEST_ELF" "$VEST_IMAGE" "$VEST_HEADER" "$VEST_SEGMENTS" 0

if [ "$DRY_RUN" = 1 ]; then say "dry run: stopping before the schedules"; exit 0; fi

# 2. The creator signs create and cancel, and pays its own fee for them.
if [ "$CB" -lt "$CREATOR_FEES" ]; then
  "$W" auth-transfer send --from "Public/$PAYER" --to "Public/$CREATOR" --amount "$CREATOR_FEES" </dev/null 2>&1 | grep -E "hash" | sed "s/^/  fund creator: /" || true
  for _ in $(seq 1 30); do [ "$(balance "$CREATOR")" -ge "$CREATOR_FEES" ] && break; sleep 10; done
fi

# 3. Sections 3 and 8 of the lifecycle, against the two programs just loaded.
PB0=$(balance "$PAYER")
RPC="$RPC" CREATOR="$CREATOR" PAYER="$PAYER" ANTUMBRA_PROGRAM="$VEST_HEADER" ANTUMBRA_ELF="$VEST_ELF" \
  TOKEN_PROGRAM="$TOKEN_HEADER" TOKEN_IMAGE="$TOKEN_IMAGE" WALLET="$W" CLI="$CLI" TAG="$TAG" OUT="$OUT" \
  SECTIONS="3 8" PRIVATE_NATIVE=0 FUND_EACH="$FUND_EACH" SUPPRESS_VERBOSE_PRINTS=1 \
  ./scripts/e2e-v03.sh || true
echo "# immutable $TOKEN_HEADER" >> "$OUT"
echo "# token program loaded immutable at $TOKEN_HEADER from $TOKEN_ELF (sha256 $TOKEN_SHA)" >> "$OUT"
echo "# antumbra_vesting deployed-token build at $VEST_HEADER from $VEST_ELF (sha256 $VEST_SHA)" >> "$OUT"
say "payer spent $((PB - $(balance "$PAYER"))) in all, $((PB0 - $(balance "$PAYER"))) on the schedules"

# 4. Check the manifest against the chain.
./scripts/verify-onchain.sh --manifest "$OUT" --rpc "$RPC"
