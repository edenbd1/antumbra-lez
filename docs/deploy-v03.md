# Deploying antumbra_vesting on the public LEZ testnet v0.3

Status: **pending funding.** Everything below has been run end to end against a
local LEZ v0.3.0 sequencer (transcript in [`evidence/v03/`](../evidence/v03/));
the public run needs LGO, which on v0.3 only arrives through Bedrock.

- LEZ: tag `v0.3.0`, commit `db66590ab821a4e142c211017a3866d007f6fa77`
- RPC: `https://testnet.lez.logos.co` (it reports channel `0101…01` and the same
  builtin program ids as a local v0.3.0 node)
- Fees: every public transaction names an LGO fee payer and is charged even when
  the program refuses it. Private transactions are free. There is no LEZ faucet.

## 0. Build

```bash
# wallet and (for a local rehearsal) the standalone sequencer, from LEZ v0.3.0
git clone https://github.com/logos-blockchain/logos-execution-zone lez && cd lez
git checkout db66590ab821a4e142c211017a3866d007f6fa77
cargo build --release -p wallet
cargo build --release --features standalone -p sequencer_service   # local rehearsal only
export PATH="$PWD/target/release:$PATH"
cd -

# the guest, reproducibly, in RISC Zero's pinned builder (Docker)
RISC0_DOCKER_CONTAINER_TAG=r0.1.91.1 cargo risczero build --manifest-path programs/vesting/Cargo.toml
cmp programs/vesting/target/riscv32im-risc0-zkvm-elf/docker/antumbra_vesting.bin \
    artifacts/programs/v0.3/antumbra_vesting.bin && echo "rebuilt byte for byte"

# On Apple Silicon the amd64 builder runs emulated and can exhaust an 8 GB Docker
# VM; the committed artifact was built with CARGO_BUILD_JOBS=2 passed into the
# same image (with codegen-units = 1 the job count should not change the bytes;
# CI's reproduce job rebuilds with plain `cargo risczero build` on a Linux
# runner and compares, which is what settles it).

# the client
cargo build --release --manifest-path cli/Cargo.toml
export PATH="$PWD/cli/target/release:$PATH"
antumbra-vesting --elf artifacts/programs/v0.3/antumbra_vesting.bin image-id
```

## 1. Wallet

```bash
export LEE_WALLET_HOME_DIR=$HOME/.lee/antumbra-v03
mkdir -p $LEE_WALLET_HOME_DIR
cat > $LEE_WALLET_HOME_DIR/wallet_config.json <<'EOF'
{
  "sequencers": [{ "sequencer_addr": "https://testnet.lez.logos.co" }],
  "seq_poll_timeout": "30s",
  "seq_tx_poll_max_blocks": 15,
  "seq_poll_max_retries": 10,
  "seq_block_poll_max_amount": 100,
  "calibration_limit": 100
}
EOF
wallet check-health                       # asks for a password the first time
CREATOR=$(wallet account new public | grep -oE 'Public/[A-Za-z0-9]+' | cut -d/ -f2)
PAYER=$(wallet account new public   | grep -oE 'Public/[A-Za-z0-9]+' | cut -d/ -f2)
echo "CREATOR=$CREATOR PAYER=$PAYER"
```

## 2. LGO: Bedrock faucet, then a ChannelDeposit into LEZ

LGO lives on Bedrock. It reaches a LEZ account through a deposit into the LEZ
channel, which the sequencer turns into a `bridge::Deposit` once Bedrock
finalises it.

1. Run a Bedrock testnet node with a wallet key (its HTTP API on
   `$BEDROCK`, e.g. `http://127.0.0.1:8080`) and fund that key at
   <https://testnet.blockchain.logos.co/web/faucet/>.
2. Find an unspent note of exactly the amount to deposit, or make one with
   `POST $BEDROCK/wallet/transactions/transfer-funds` to yourself.
3. Deposit it into channel `0101…01`. The metadata is the borsh encoding of the
   recipient's LEZ account id, which is its 32 raw bytes:

```bash
RECIPIENT_HEX=$(python3 -c "import sys;B='123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';n=0
for c in sys.argv[1]: n=n*58+B.index(c)
print(n.to_bytes(32,'big').hex())" "$PAYER")
curl -s -X POST "$BEDROCK/channel/deposit" -H 'Content-Type: application/json' -d "{
  \"tip\": null,
  \"deposit\": {
    \"channel_id\": \"0101010101010101010101010101010101010101010101010101010101010101\",
    \"inputs\": [\"$NOTE_ID\"],
    \"metadata\": \"$RECIPIENT_HEX\"
  },
  \"change_public_key\": \"$BEDROCK_PK\",
  \"funding_public_keys\": [\"$BEDROCK_PK\"],
  \"max_tx_fee\": \"18446744073709551615\"
}"
```

4. Wait for finality (minutes), then `wallet account get --account-id Public/$PAYER`.
   Repeat for `$CREATOR`, or move LGO across with `wallet auth-transfer`.

Budget: on the local run the fee payer spent about 0.27 LGO for the deploy and
roughly forty transactions; refusals are charged too, and base fees move with
load, so fund both keys with a few LGO.

## 3. Deploy

```bash
antumbra-vesting --elf artifacts/programs/v0.3/antumbra_vesting.bin deploy --payer $PAYER
# {"op":"deploy","header":"<HEADER>","image_id":"<32-byte hex>","segments":N}
export ANTUMBRA_PROGRAM=<HEADER>
curl -s -X POST https://testnet.lez.logos.co -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getAccount\",\"params\":[\"$ANTUMBRA_PROGRAM\"]}"
```

The header's `program_loader` shard (`JAQ5MVHb…`, the id `[0xFE; 32]`) starts with the ImageID that
`antumbra-vesting image-id` prints; `verify-onchain.sh` checks exactly that.

## 4. Re-drive the evidence

```bash
RPC=https://testnet.lez.logos.co CREATOR=$CREATOR PAYER=$PAYER \
  ANTUMBRA_PROGRAM=$ANTUMBRA_PROGRAM BATCH_MAX=<ceiling from executor-tests/CYCLES.md> \
  WALLET=wallet CLI=antumbra-vesting TAG=testnet1 OUT=evidence/v03/testnet.tsv \
  ./scripts/e2e-v03.sh 2>&1 | tee evidence/v03/testnet-transcript.txt
```

Sections 1 to 10 cover create and fund, public and private claims (native and
token), cancellation with refund, milestones, transfer, non-cancelable, the
nominated authority, a batch, and the batch ceiling. The private claims prove
locally and take minutes each. Rerunning on the same chain needs a new `TAG`.

## 5. Verify and cite

```bash
./scripts/verify-onchain.sh          # reads evidence/v03/testnet.tsv against the public RPC
```

Then put the header, the ImageID, the deploy transactions and the manifest's
hashes in `DEPLOYMENTS.md`, replacing "testnet v0.3 deployment: pending funding".

## Local rehearsal (what produced `evidence/v03/`)

```bash
cd lez && cp lez/sequencer/service/configs/debug/sequencer_config.json /tmp/seq/ && cd /tmp/seq
RUST_LOG=info sequencer_service sequencer_config.json --listen-address 127.0.0.1 &   # standalone build, port 3040
export LEE_WALLET_HOME_DIR=/tmp/wallet-local   # wallet_config.json pointing at http://127.0.0.1:3040
wallet check-health
# the two funded genesis keys of the debug configuration (public test keys)
wallet account import public --private-key 10a26a9aec7d34b82364eeae45c5294dbb0a764b000b94eeb9b58511dc487c4d
wallet account import public --private-key 717940b1cc55e5d6b2066dbf1d9a3f26f212f4db08d02388177fcfedd8a9be1b
SUPPRESS_VERBOSE_PRINTS=1 RPC=http://127.0.0.1:3040 DEPLOY=1 BATCH_MAX=450 TAG=local1 \
  CREATOR=6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV PAYER=7wHg9sbJwc6h3NP1S9bekfAzB8CHifEcxKswCKUt3YQo \
  WALLET=wallet CLI=antumbra-vesting OUT=evidence/v03/local-e2e.tsv \
  ./scripts/e2e-v03.sh | tee evidence/v03/local-transcript.txt
./scripts/verify-onchain.sh --manifest evidence/v03/local-e2e.tsv --rpc http://127.0.0.1:3040
```

## Upgrades

`wallet program-loader update --header $ANTUMBRA_PROGRAM --elf <new.bin> --segments … --payer $PAYER`
rewrites the header in place; schedules and escrows are PDAs of the header, so
they keep their addresses and state across an upgrade.
