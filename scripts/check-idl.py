#!/usr/bin/env python3
"""Fail if idl/antumbra_vesting.v03.idl.json has drifted from the Rust types.

SPEL generated the v0.2.4 IDL from the program source. It has no v0.3
support, so the v0.3 IDL is written by hand, and a hand-written IDL is only
worth anything if something notices when it stops describing the program.

This is that something. The core crate's `idl_vectors` example borsh-encodes
one sample of every instruction variant, of the stored schedule and of every
event, and prints the values it used. This script decodes each sample using
only the IDL and requires the same values back with every byte consumed. It
also compares the discriminants, the event selectors, the refusal codes and
the cancellation window. A field added, dropped, retyped or reordered on one
side only fails here.

Usage: scripts/check-idl.py            (runs the example itself)
       scripts/check-idl.py vectors.json
"""
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
IDL = json.loads((ROOT / "idl/antumbra_vesting.v03.idl.json").read_text())
TYPES = {t["name"]: t["type"] for t in IDL["types"]}
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


def main():
    if len(sys.argv) > 1:
        vec = json.loads(pathlib.Path(sys.argv[1]).read_text())
    else:
        out = subprocess.run(
            ["cargo", "run", "-q", "--example", "idl_vectors", "-p", "antumbra-vesting-core",
             "--manifest-path", str(ROOT / "programs/vesting/Cargo.toml")],
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

    st = IDL["accounts"][0]["type"]
    try:
        got = whole(lambda r: decode_defined(st, r), vec["state"]["hex"], "VestingSchedule")
        if got != vec["state"]["values"]:
            bad.append(f"VestingSchedule: decodes to {got}")
    except ValueError as e:
        bad.append(f"VestingSchedule: {e}")

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
    window = next(i for i in IDL["instructions"] if i["name"] == "cancel")["timestamp_window"]
    if window != f"[at, at + {vec['cancel_window_ms']})":
        bad.append(f"cancel window is {vec['cancel_window_ms']} ms, IDL says {window}")

    if bad:
        print("IDL drift:")
        for b in bad:
            print("  " + b)
        sys.exit(1)
    n = len(vec["instructions"]) + 1 + len(vec["events"])
    print(f"IDL matches the program: {n} borsh vectors decoded exactly, "
          f"{len(vec['selectors'])} selectors and {len(vec['errors'])} codes agree")


if __name__ == "__main__":
    main()
