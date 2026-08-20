#!/usr/bin/env python3
"""Fail if CYCLES.md no longer matches what the executor actually reports.

The document is quoted from three grant proposals. A figure that drifts is not
a cosmetic problem: it is a public claim that stopped being true, and nobody
notices until a reader reproduces it. So the check is a build step rather than
a habit.

Usage:  cargo run --release | python3 verify_cycles.py
        python3 verify_cycles.py < saved-output.txt
"""
import re, sys, pathlib

DOC = pathlib.Path(__file__).with_name("CYCLES.md")

# Every row in the document, keyed by the operation the harness prints.
DOC_LABEL = {
    "pow_frac":            "`pow_frac` (decimal scale)",
    "pow_frac_binary":     "`pow_frac` (**binary scale**)",
    "weighted_buy":        "`weighted_buy` (decimal scale)",
    "weighted_buy_binary": "`weighted_buy` (**binary scale**)",
    "neg_ln":              "`neg_ln`",
    "exp_neg":             "`exp_neg`",
    "weight_at":           "`weight_at`",
    "curve_buy":           "`curve_buy`",
    "curve_sell":          "`curve_sell`",
    "vested_at_linear":    "`vested_at` (linear)",
    "vested_at_cliff":     "`vested_at` (cliff+linear)",
    "vesting_claim":       "`vesting_claim`",
    "vesting_cancel":      "`vesting_cancel`",
    "signal_milestone":    "`signal_milestone`",
    "buy_fee":             "`buy_fee`",
    "close_fee":           "`close_fee`",
}

def main():
    run = sys.stdin.read()
    measured = {m.group(1): int(m.group(2))
                for m in re.finditer(r"^\| `([a-z_]+)` \| \d+ \| (\d+) \|", run, re.M)}
    if not measured:
        print("no measurements on stdin — did the harness run?", file=sys.stderr)
        return 2

    doc = DOC.read_text()
    fails, checked = [], 0

    for op, cycles in measured.items():
        label = DOC_LABEL.get(op)
        if label is None:
            fails.append(f"{op} is measured but has no row in CYCLES.md")
            continue
        row = re.search(r"^\| " + re.escape(label) + r" \| \d+ \| \*{0,2}([\d,]+)\*{0,2} \|",
                        doc, re.M)
        if row is None:
            fails.append(f"{op}: no row matching '{label}'")
            continue
        published = int(row.group(1).replace(",", ""))
        checked += 1
        if published != cycles:
            fails.append(f"{op}: CYCLES.md says {published:,}, the executor says {cycles:,}")

    for label in DOC_LABEL.values():
        if label not in doc:
            fails.append(f"CYCLES.md is missing the row for {label}")

    whole = re.search(r"end to end: (\d+) cycles", run)
    if whole:
        want = int(whole.group(1))
        got = re.search(r"is ([\d,]+)\ncycles end to end", doc)
        if not got or int(got.group(1).replace(",", "")) != want:
            fails.append(f"end-to-end buy: CYCLES.md disagrees with the measured {want:,}")

    if fails:
        print("CYCLES.md is out of date:", file=sys.stderr)
        for f in fails:
            print("  - " + f, file=sys.stderr)
        print("\nRegenerate it from the run rather than editing it by hand.", file=sys.stderr)
        return 1

    print(f"CYCLES.md matches the executor on all {checked} operations.")
    return 0

if __name__ == "__main__":
    sys.exit(main())
