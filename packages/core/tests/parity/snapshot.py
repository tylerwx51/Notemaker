#!/usr/bin/env python3
"""Write the committed Java snapshot used by tests/parity_snapshot.rs:
expected/all.sha256 (one SHA-256 per section of `Parity all` output) and
expected/text-sections.txt (full text of sections compared with tolerance)."""
import hashlib
import sys

TEXT_SECTIONS = {"geometry/capsule", "json/objects", "svg/bad-not-xml.svg", "svg/bad-undeclared-prefix.svg"}
source, out_dir = sys.argv[1], sys.argv[2]
sections, order, current = {}, [], None
for line in open(source, encoding="utf-8").read().split("\n")[:-1]:
    if line.startswith("== "):
        current = line[3:]
        order.append(current)
        sections[current] = []
    else:
        sections[current].append(line)
with open(f"{out_dir}/all.sha256", "w", encoding="utf-8") as f:
    for name in order:
        text = "".join(l + "\n" for l in sections[name])
        f.write(hashlib.sha256(text.encode("utf-8")).hexdigest() + " " + name + "\n")
with open(f"{out_dir}/text-sections.txt", "w", encoding="utf-8") as f:
    for name in order:
        if name in TEXT_SECTIONS:
            f.write("== " + name + "\n" + "".join(l + "\n" for l in sections[name]))
