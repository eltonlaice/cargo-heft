"""Markdown table from a bench/results/<date> directory.

  python3 bench/table.py bench/results/2026-11-14
"""
import json
import pathlib
import sys

out = pathlib.Path(sys.argv[1])
rows, failed = [], []
for meta_path in sorted(out.glob("*.meta")):
    meta = dict(line.split("=", 1) for line in meta_path.read_text().splitlines())
    name = meta_path.stem
    if meta["status"] != "ok":
        failed.append("%s (%s)" % (name, meta["status"]))
        continue
    d = json.loads((out / (name + ".json")).read_text())
    total = d["total_bytes"]
    c = d["by_content"]
    meta_in = c.get("metadata (inside rlib/dylib)", 0)
    meta_out = c.get("metadata (.rmeta files)", 0)
    rows.append((total, name, meta["commit"][:7], int(meta["build_seconds"]), [
        c.get("debuginfo", 0),
        c.get("incremental cache", 0),
        meta_in + meta_out,
        d["duplicated_metadata_bytes"],
        c.get("code, data and symbols", 0),
    ]))

print("| Project | Commit | `target/` | Debuginfo | Incremental | Metadata | of which twice | Code |")
print("|---|---|--:|--:|--:|--:|--:|--:|")
sums = [0] * 5
grand = 0
for total, name, commit, secs, parts in sorted(rows):
    grand += total
    sums = [a + b for a, b in zip(sums, parts)]
    size = "%.2f GB" % (total / 1e9) if total >= 1e9 else "%.0f MB" % (total / 1e6)
    print("| %s | `%s` | %s | %s |" % (
        name, commit, size, " | ".join("%.0f%%" % (100 * p / total) for p in parts)))
print("| **all %d** | | **%.1f GB** | %s |" % (
    len(rows), grand / 1e9, " | ".join("**%.0f%%**" % (100 * s / grand) for s in sums)))
if failed:
    print("\nNot measured: " + ", ".join(failed) + ".")
