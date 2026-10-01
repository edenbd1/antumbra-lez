#!/usr/bin/env python3
"""Fail if a hand-maintained v0.3 IDL has drifted from the Rust types.

SPEL generated the v0.2.4 IDLs from the program source. It has no v0.3
support, so the v0.3 IDLs (idl/antumbra_{vesting,curve,lbp}.v03.idl.json) are
written by hand, and a hand-written IDL is only worth anything if something
notices when it stops describing the program.

This is that something. Each core crate's `idl_vectors` example borsh-encodes
one sample of every instruction variant, of the stored state and of every
event, and prints the values it used. This script decodes each sample using
only the IDL and requires the same values back with every byte consumed. It
also compares the discriminants, the event selectors, the refusal codes and
the timestamp windows. A field added, dropped, retyped or reordered on one
side only fails here.

Usage: scripts/check-idl.py                      (all three programs)
       scripts/check-idl.py curve                (one program)
       scripts/check-idl.py vesting vectors.json (one program, given vectors)
"""
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PROGRAMS = ("vesting", "curve", "lbp")
IDL = {}
TYPES = {}
INTS = {"u8": 1, "u16": 2, "u32": 4, "u64": 8, "u128": 16}


class Reader:
    def __init__(self, data):
        self.b = data
        self.i = 0

    def take(self, n):
        if self.i + n > len(self.b):
            raise ValueError("ran out of bytes")
        out = self.b[self.i:self.i + n]
        self.i += n
        return out


def decode(ty, r):
    if isinstance(ty, str):
        if ty in INTS:
            v = int.from_bytes(r.take(INTS[ty]), "little")
            # The vectors carry u128 as decimal strings (JSON has no u128).
            return str(v) if ty == "u128" else v
        if ty == "bool":
            b = r.take(1)[0]
            if b > 1:
                raise ValueError("non-canonical bool")
            return bool(b)
        if ty == "account_id":
            return r.take(32).hex()
        raise ValueError(f"unknown type {ty}")
    if "array" in ty:
        inner, n = ty["array"]
        assert inner == "u8", "only byte arrays are used"
        return r.take(n).hex()
    if "option" in ty:
        tag = r.take(1)[0]
        return None if tag == 0 else decode(ty["option"], r)
    if "vec" in ty:
        n = int.from_bytes(r.take(4), "little")
        return [decode(ty["vec"], r) for _ in range(n)]
    if "defined" in ty:
        return decode_defined(TYPES[ty["defined"]], r)
    raise ValueError(f"unknown type {ty}")


def decode_fields(fields, r):
    return {f["name"]: decode(f["type"], r) for f in fields}


def decode_defined(t, r):
    if t["kind"] == "struct":
        return decode_fields(t["fields"], r)
    if t["kind"] == "enum":
        tag = r.take(1)[0]
        var = t["variants"][tag]
        return {var["name"]: decode_fields(var["fields"], r)}
    raise ValueError(t)


def whole(fields, hexstr, name):
    r = Reader(bytes.fromhex(hexstr))
    out = fields(r)
    if r.i != len(r.b):
        raise ValueError(f"{name}: {len(r.b) - r.i} bytes left over")
    return out


def check(program, vectors=None):
    global IDL, TYPES
    IDL = json.loads((ROOT / f"idl/antumbra_{program}.v03.idl.json").read_text())
    TYPES = {t["name"]: t["type"] for t in IDL["types"]}
    if vectors:
        vec = json.loads(pathlib.Path(vectors).read_text())
    else:
        out = subprocess.run(
            ["cargo", "run", "-q", "--example", "idl_vectors", "-p", f"antumbra-{program}-core",
             "--manifest-path", str(ROOT / f"programs/{program}/Cargo.toml")],
            check=True, capture_output=True, text=True)
        vec = json.loads(out.stdout)

    bad = []
    by_name = {i["name"]: i for i in IDL["instructions"]}
    if [i["name"] for i in vec["instructions"]] != [i["name"] for i in IDL["instructions"]]:
        bad.append("instruction list or order differs")
    for sample in vec["instructions"]:
        spec = by_name.get(sample["name"])
        if spec is None:
            bad.append(f"{sample['name']}: missing from the IDL")
            continue
        def ix(r, spec=spec):
            tag = r.take(1)[0]
            if tag != spec["discriminant"]:
                raise ValueError(f"discriminant {tag}, IDL says {spec['discriminant']}")
            return decode_fields(spec["args"], r)
        try:
            got = whole(ix, sample["hex"], sample["name"])
            if got != sample["values"]:
                bad.append(f"{sample['name']}: decodes to {got}, expected {sample['values']}")
        except ValueError as e:
            bad.append(f"{sample['name']}: {e}")

    st = IDL["accounts"][0]
    if st["name"] != vec["state"]["name"]:
        bad.append(f"state account is {vec['state']['name']}, IDL says {st['name']}")
    try:
        got = whole(lambda r: decode_defined(st["type"], r), vec["state"]["hex"], st["name"])
        if got != vec["state"]["values"]:
            bad.append(f"{st['name']}: decodes to {got}")
    except ValueError as e:
        bad.append(f"{st['name']}: {e}")

    ev = {e["name"]: e for e in IDL["events"]}
    if [e["name"] for e in vec["events"]] != [e["name"] for e in IDL["events"]]:
        bad.append("event list or order differs")
    for sample in vec["events"]:
        spec = ev.get(sample["name"])
        if spec is None:
            bad.append(f"event {sample['name']}: missing from the IDL")
            continue
        try:
            got = whole(lambda r, s=spec: decode_fields(s["fields"], r), sample["hex"], sample["name"])
            if got != sample["values"]:
                bad.append(f"event {sample['name']}: decodes to {got}")
        except ValueError as e:
            bad.append(f"event {sample['name']}: {e}")
    for s in vec["selectors"]:
        if ev.get(s["name"], {}).get("selector") != s["selector"]:
            bad.append(f"event {s['name']}: selector is {s['selector']}")

    if vec["errors"] != IDL["errors"]:
        bad.append("error codes differ")
    windows = dict(vec.get("windows", {}))
    if "cancel_window_ms" in vec:
        windows["cancel"] = f"[at, at + {vec['cancel_window_ms']})"
    declared = {i["name"]: i["timestamp_window"] for i in IDL["instructions"] if "timestamp_window" in i}
    for name in sorted(set(windows) | set(declared)):
        if name == "claim" and program == "vesting":
            continue  # [at, inf): open-ended, nothing in the program to compare
        if windows.get(name) != declared.get(name):
            bad.append(f"{name}: timestamp window is {windows.get(name)}, IDL says {declared.get(name)}")

    if bad:
        print(f"antumbra_{program} IDL drift:")
        for b in bad:
            print("  " + b)
        return False
    n = len(vec["instructions"]) + 1 + len(vec["events"])
    print(f"antumbra_{program} IDL matches the program: {n} borsh vectors decoded exactly, "
          f"{len(vec['selectors'])} selectors and {len(vec['errors'])} codes agree")
    return True


def main():
    args = sys.argv[1:]
    if args and args[0] in PROGRAMS:
        ok = check(args[0], args[1] if len(args) > 1 else None)
    elif args:
        ok = check("vesting", args[0])
    else:
        ok = all([check(p) for p in PROGRAMS])
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
