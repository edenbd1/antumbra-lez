#!/usr/bin/env python3
"""The schedule PDAs of a batch, computed the way the program derives them.

    scripts/batch-pdas.py <program.bin> <batch-id> <n> [ids|pdas]

Schedule i of batch B has id SHA-256(B ‖ i as 32 little-endian bytes) and lives
at the public PDA of that id: SHA-256("/LEE/v0.2/AccountId/PDA/" padded to 32 ‖
program id ‖ id). `pdas` prints the base58 addresses comma-separated, as
`spel --schedules` takes them; `ids` prints the schedule ids in hex.
"""
import hashlib, subprocess, sys, re
B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

def b58(b):
    n = int.from_bytes(b, "big"); s = ""
    while n: n, r = divmod(n, 58); s = B58[r] + s
    return "1" * (len(b) - len(b.lstrip(b"\0"))) + s

def main():
    binf, bid, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
    mode = sys.argv[4] if len(sys.argv) > 4 else "pdas"
    out = subprocess.run(["spel", "program-id", binf], capture_output=True, text=True).stdout
    image = bytes.fromhex(re.search(r"ImageID \(hex bytes\):\s*([0-9a-f]{64})", out).group(1))
    batch = bid.encode().ljust(32, b"\0") if not re.fullmatch(r"[0-9a-f]{64}", bid) else bytes.fromhex(bid)
    prefix = b"/LEE/v0.2/AccountId/PDA/".ljust(32, b"\0")
    ids, pdas = [], []
    for i in range(n):
        sid = hashlib.sha256(batch + i.to_bytes(4, "little").ljust(32, b"\0")).digest()
        ids.append(sid.hex())
        pdas.append(b58(hashlib.sha256(prefix + image + sid).digest()))
    print(",".join(ids if mode == "ids" else pdas))

if __name__ == "__main__":
    main()
