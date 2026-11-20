#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Compare the reference's test ROM outcomes with this board's, case by case.

    testrom_compare.py REFERENCE.jsonl RUST.jsonl

REFERENCE.jsonl is scripts/testrom_outcomes.py's, RUST.jsonl famicom-testrom's.
A case agrees when its verdict, CPU cycles, instructions and detail text are
the same (for the "spans" protocol the detail prints Python sets, whose order
is the interpreter's: there only the rest is compared). Prints each
disagreement and a summary per suite: cases, agreeing, and how many the
reference itself passes. Stdlib only.
"""

from __future__ import annotations

import json
import sys
from collections import Counter


def main() -> int:
    reference = [json.loads(line) for line in open(sys.argv[1])]
    rust = {row["label"]: row for row in map(json.loads, open(sys.argv[2]))}
    agree, total, passed = Counter(), Counter(), Counter()
    for row in reference:
        suite, label = row["suite"], row["label"]
        if "outcome" not in row:
            continue
        total[suite] += 1
        mine = rust.get(label)
        want = row["outcome"]
        passed[suite] += want["passed"] is True
        if mine is None:
            print(f"{suite} {label}: not run here")
            continue
        if "error" in mine:
            print(f"{suite} {label}: rust error {mine['error']}")
            continue
        got = mine["outcome"]
        fields = ["passed", "cycles", "instructions"]
        if row["protocol"] != "spans":
            fields.append("detail")
        different = [f for f in fields if got[f] != want[f]]
        if different:
            print(f"{suite} {label}: differs in {different}")
            for f in different[:3]:
                print(f"    reference {f}: {str(want[f])[:300]!r}")
                print(f"    rust      {f}: {str(got[f])[:300]!r}")
        else:
            agree[suite] += 1
    for suite in sorted(total):
        print(f"{suite}: {agree[suite]} of {total[suite]} agree; reference passes {passed[suite]}")
    all_agree = sum(agree.values()) == sum(total.values())
    print(f"all: {sum(agree.values())} of {sum(total.values())} agree")
    return 0 if all_agree else 1


if __name__ == "__main__":
    sys.exit(main())
