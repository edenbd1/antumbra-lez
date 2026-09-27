#!/usr/bin/env python3
"""Join a replay log and the continuation that re-ran some of its sections.

    merge-replay-logs.py MAIN CONTINUATION "why the continuation exists" > merged.tsv

A step of MAIN that did not behave as expected is not dropped: it becomes a `#`
note saying what happened and that it was re-run below, so the evidence page
shows the failure and its repair rather than a clean run that never happened.
Checks that failed become notes the same way. The continuation follows, under a
note giving the reason.
"""
import sys
main, cont, why = sys.argv[1], sys.argv[2], sys.argv[3]
out = []
# Once a step that should have landed did not, a check that still passes is
# passing on an account the missing step never touched — a zero equal to a
# zero. It is kept, but marked as proving nothing, until a step lands again.
vacuous = False
for line in open(main).read().splitlines():
    if line.startswith("# check FAIL:"):
        out.append("# not met in this run, re-run below: " + line[len("# check FAIL:"):].strip())
        continue
    if line.startswith("# check ok:") and vacuous:
        out.append("# vacuous in this run, re-run below (its account was never funded): "
                   + line[len("# check ok:"):].strip())
        continue
    if line.startswith("#"):
        # A reading taken after a step that did not land reads an empty field;
        # say so rather than let it pass for a measurement.
        if "clock time 0 ms" in line:
            line = "# (empty reading: the step before it did not land) " + line[1:].strip()
        out.append(line); continue
    label, expect, verdict, block, h = line.split("\t")
    if (verdict == "LANDED") != (expect == "yes"):
        vacuous = vacuous or expect == "yes"
        out.append(f"# {label}: expected to {'land' if expect == 'yes' else 'be refused'}, "
                   f"{verdict.lower()} instead ({h[:16]}…); re-run below")
    else:
        vacuous = vacuous and verdict != "LANDED"
        out.append(line)
out.append(f"# continuation: {why}")
out += [l for l in open(cont).read().splitlines() if "clock at start" not in l]
print("\n".join(out))
