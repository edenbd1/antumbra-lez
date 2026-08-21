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
  echo "All eighteen transactions resolve; the control does not."
else
  echo "Something above did not hold." >&2
fi
exit "$fail"
