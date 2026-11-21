#!/usr/bin/env python3
"""Differential test: `cherenkov plan --operators cargo-mutants --names` against
`cargo mutants --list --json --no-config`, crate by crate.

Both sides are compared by mutant name (`path:line:col: replace X with Y in fn`).
Per crate it reports cm-only (cargo-mutants has it, Cherenkov does not),
cherenkov-only, and spelling differences (the same place and replacement named
differently). Every difference must be explained by a rule in the explained
file (scripts/cm-differences.txt); the exit status is 1 if any is not.

usage: cm-differential.py [--cherenkov BIN] [--corpus FILE] [--explained FILE]
                          [--root DIR] [--only NAME ...] [--json OUT]

The corpus file has one crate a line: `name dir workspace_root`, the paths
relative to --root (default $CM_CORPUS_ROOT or ~/workspace), or `@repo/...` for
the path inside this repository. A crate whose
directory is missing is reported and skipped. The explained file has one rule
a line, tab separated: `side  crate  regex  reason` where side is cm-only,
ch-only or spelling, crate a name or `*`, and regex is searched in the name
(for spelling, in the cargo-mutants name).
"""
import argparse
import collections
import json
import os
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
LOOSE = re.compile(r"^(\S+:\d+:\d+): (replace|delete) .*?(?: with (\S+))?(?: in .*)?$")


def run(cmd):
    out = subprocess.run(cmd, capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f"{' '.join(map(str, cmd))} failed:\n{out.stderr[-2000:]}")
    return out.stdout


def cm_names(d, prefix):
    listing = json.loads(run(["cargo-mutants", "mutants", "--list", "--json", "--no-config", "-d", str(d)]))
    names = []
    for m in listing:
        name = m["name"]
        names.append(name[len(prefix):] if prefix and name.startswith(prefix) else name)
    return names


def cherenkov_names(binary, d):
    out = run([str(binary), "plan", str(d), "--operators", "cargo-mutants", "--no-config", "--names"])
    return [line for line in out.splitlines() if line]


def loose(name):
    m = LOOSE.match(name)
    return (m.group(1), m.group(2), m.group(3)) if m else (name,)


def compare(cm, ch):
    a, b = collections.Counter(cm), collections.Counter(ch)
    cm_only = list((a - b).elements())
    ch_only = list((b - a).elements())
    index = collections.defaultdict(list)
    for n in ch_only:
        index[loose(n)].append(n)
    spelling, rest = [], []
    for n in cm_only:
        hit = index.get(loose(n))
        if hit:
            spelling.append((n, hit.pop()))
        else:
            rest.append(n)
    left = [n for v in index.values() for n in v]
    return rest, left, spelling


def load_rules(path):
    rules = []
    if path.exists():
        for line in path.read_text().splitlines():
            if line.strip() and not line.startswith("#"):
                side, crate, regex, reason = (line.split("\t") + ["", "", "", ""])[:4]
                rules.append((side, crate, re.compile(regex), reason))
    return rules


def explained(rules, side, crate, name):
    for s, c, rx, reason in rules:
        if s == side and c in ("*", crate) and rx.search(name):
            return reason
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cherenkov", default=str(HERE.parent / "target/debug/cherenkov"))
    ap.add_argument("--corpus", default=str(HERE / "cm-corpus.txt"))
    ap.add_argument("--explained", default=str(HERE / "cm-differences.txt"))
    ap.add_argument("--root", default=os.environ.get("CM_CORPUS_ROOT", str(Path.home() / "workspace")))
    ap.add_argument("--only", nargs="*", default=[])
    ap.add_argument("--json")
    args = ap.parse_args()
    rules = load_rules(Path(args.explained))
    root = Path(args.root)
    rows, bad, report = [], 0, {}
    for line in Path(args.corpus).read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        name, d, ws = line.split()
        if args.only and name not in args.only:
            continue
        d, ws = (HERE.parent / x[6:] if x.startswith("@repo/") else root / x for x in (d, ws))
        if not d.is_dir():
            print(f"skip {name}: {d} is missing")
            continue
        rel = os.path.relpath(d, ws)
        prefix = "" if rel == "." else rel + "/"
        cm, ch = cm_names(d, prefix), cherenkov_names(args.cherenkov, d)
        cm_only, ch_only, spell = compare(cm, ch)
        un = {"cm-only": [], "ch-only": [], "spelling": []}
        counts = collections.Counter()
        for n in cm_only:
            r = explained(rules, "cm-only", name, n)
            counts["cm-only explained" if r else "cm-only"] += 1
            if not r:
                un["cm-only"].append(n)
        for n in ch_only:
            r = explained(rules, "ch-only", name, n)
            counts["ch-only explained" if r else "ch-only"] += 1
            if not r:
                un["ch-only"].append(n)
        for a, b in spell:
            r = explained(rules, "spelling", name, a)
            counts["spelling explained" if r else "spelling"] += 1
            if not r:
                un["spelling"].append((a, b))
        unexplained = sum(len(v) for v in un.values())
        bad += unexplained
        rows.append((name, len(cm), len(ch), len(cm_only), len(ch_only), len(spell), unexplained))
        report[name] = un
        for kind, items in un.items():
            for item in items[:25]:
                print(f"  {name} {kind}: {item}")
            if len(items) > 25:
                print(f"  {name} {kind}: ... {len(items) - 25} more")
    print(f"{'crate':14} {'cm':>6} {'cherenkov':>9} {'cm-only':>8} {'ch-only':>8} {'spelling':>8} {'unexplained':>11}")
    for r in rows:
        print(f"{r[0]:14} {r[1]:6} {r[2]:9} {r[3]:8} {r[4]:8} {r[5]:8} {r[6]:11}")
    if args.json:
        Path(args.json).write_text(json.dumps(report, indent=1))
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
