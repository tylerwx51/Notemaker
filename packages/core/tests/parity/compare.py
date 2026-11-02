#!/usr/bin/env python3
"""Compare two parity outputs: exact lines, then lines that differ only in
floating-point values within 1e-9 relative error (1-ulp sin/cos differences
between HotSpot intrinsics and libm). Prints a summary; exits 1 if any other
difference remains that is not listed in KNOWN (one substring per line)."""
import re
import sys

java, rust = (open(p, encoding="utf-8").read().split("\n") for p in sys.argv[1:3])
known = []
if len(sys.argv) > 3:
    known = [l for l in open(sys.argv[3], encoding="utf-8").read().split("\n") if l.strip() and not l.startswith("#")]
NUM = re.compile(r"-?\d+(?:\.\d+)?(?:E-?\d+)?")


def close(a, b):
    ta, tb = NUM.split(a), NUM.split(b)
    na, nb = NUM.findall(a), NUM.findall(b)
    if ta != tb or len(na) != len(nb):
        return False
    for x, y in zip(na, nb):
        if x != y:
            u, v = float(x), float(y)
            if abs(u - v) > 1e-9 * max(abs(u), abs(v), 1e-300):
                return False
    return True


section = "?"
exact = tolerated = 0
unexpected = []
known_hits = []
if len(java) != len(rust):
    unexpected.append(f"line count differs: java {len(java)} rust {len(rust)}")
for i, (a, b) in enumerate(zip(java, rust)):
    if a.startswith("== "):
        section = a
    if a == b:
        exact += 1
    elif close(a, b):
        tolerated += 1
    elif any(k in a or k in b or k in section for k in known):
        known_hits.append(f"{section} line {i + 1}")
    else:
        unexpected.append(f"{section} line {i + 1}\n  java: {a[:300]}\n  rust: {b[:300]}")
print(f"{sys.argv[1]} vs {sys.argv[2]}: {exact} identical, {tolerated} within 1e-9 (float ulps), {len(known_hits)} known differences, {len(unexpected)} unexpected")
for k in known_hits:
    print("  known:", k)
for u in unexpected[:40]:
    print("  UNEXPECTED:", u)
sys.exit(1 if unexpected else 0)
