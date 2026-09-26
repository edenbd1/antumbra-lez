#!/usr/bin/env python3
"""Print an antumbra_vesting schedule account, decoded from the chain.

    scripts/read-schedule.py <schedule-PDA-base58> [RPC]

The layout is VestingSchedule's Borsh encoding, field for field; the script
refuses to guess if the account's length disagrees with it.
"""
import json, struct, sys, urllib.request
B = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
def b58(b):
    n = int.from_bytes(b, 'big'); s = ''
    while n: n, r = divmod(n, 58); s = B[r] + s
    return '1' * (len(b) - len(b.lstrip(b'\0'))) + s
addr = sys.argv[1]
rpc = sys.argv[2] if len(sys.argv) > 2 else 'https://testnet.lez.logos.co'
req = urllib.request.Request(rpc, json.dumps({"jsonrpc": "2.0", "id": 1, "method": "getAccount",
      "params": [addr]}).encode(), {"content-type": "application/json"})
acc = json.load(urllib.request.urlopen(req, timeout=30))["result"]
d = bytes(acc["data"])
F = [("kind","B"),("start","Q"),("cliff","Q"),("end","Q"),("total","16"),("claimed","16"),
     ("last_seen","Q"),("beneficiary","32"),("escrow","32"),("creator","32"),("cancelable","B"),
     ("transferable","B"),("cancelled_at","Q"),("signalled","Q"),("tranches","I")]
size = sum({"B":1,"Q":8,"I":4}.get(t) or int(t) for _, t in F)
if len(d) != size:
    sys.exit(f"{addr}: {len(d)} bytes, VestingSchedule is {size} — not decoding a layout that does not match")
o = 0
for name, t in F:
    if t in ("B","Q","I"):
        v = struct.unpack_from("<" + t, d, o)[0]; o += struct.calcsize(t)
    elif t == "16":
        v = int.from_bytes(d[o:o+16], "little"); o += 16
    else:
        v = b58(d[o:o+32]); o += 32
    print(f"  {name:13} {v}")
