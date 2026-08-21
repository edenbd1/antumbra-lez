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
check() { # program label tx expect_present
  local body height
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
      printf '  ✅ %-8s %-16s returns null, as a never-deployed hash must\n' "$1" "$2"
    else
      printf '  ❌ %-8s %-16s MISSING %s\n' "$1" "$2" "$3"; fail=1
    fi
  fi
}

echo "Antumbra on the public LEZ testnet — $RPC"
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
echo "  -- the paying buy: collateral moved by a chained call --"
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

echo
echo "  -- the per-swap fee: accrued on the buy, swept on its own --"
check fee     deploy          53e149f997a343c91af6223b101889330cca46a1ad4ec92dadd5d8d9ba72bc91 yes
check fee     create_sale     679ec10a355bf65722bc20fe3ed1e17c05d77f6f439bf17c02b629592479a406 yes
check fee     execute_buy     9512887af1df329d7d9a201ebf550be9ee6a551e77ce14988b7ea03d2a21d9d8 yes
check fee     collect_fees    63e4e5f22214bbc57a92648e9b9a3a34080bdb8abeaa5757cd6d2eab690337d8 yes

echo
echo "  -- the private path, run twice with distinct ephemeral accounts --"
check private init-eph-1     646f91b21d8faf80a249ee8a6ad5ad1a1e07c74517ee03ff3f4e305b49a8880d yes
check private deshield-1     921b9e4f72425b65a5e0622e248ea7834de32215d785e24670d32ceb75de83ec yes
check private buy-eph-1      70f19695cd81be4210a304896090686529c0f5f547ad15aa062d1498e1c95a29 yes
check private init-eph-2     6c57df67d732854779e4f90e36a3c07339ead8f50a2117c86e1f4f1340da71fd yes
check private deshield-2     9da4fe4bf848c54d1b6324e05cb873ac4daa8312d41905ff2b31490ecd2167aa yes
check private buy-eph-2      ab1d956440c3cc0c83527d0b08b85e6003caf5e17e5dca1251c935c625cbe832 yes

echo
check CONTROL never-deployed  dededededededededededededededededededededededededededededededede no

echo
if [ "$fail" -eq 0 ]; then
  {
  echo "All twenty-nine expected transactions resolve."
  echo "Neither the never-deployed hash nor the refused vesting payout does, which is"
  echo "what makes the other twenty-nine mean something."
}
else
  echo "Something above did not hold." >&2
fi
exit "$fail"
