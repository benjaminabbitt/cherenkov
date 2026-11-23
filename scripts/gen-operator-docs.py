#!/usr/bin/env python3
"""Regenerate the blocks of docs/*.md between
`<!-- generated: cherenkov ARGS -->` and `<!-- /generated -->` with the output
of `cherenkov ARGS` (the operator inventory). `just docs`.

usage: gen-operator-docs.py [--cherenkov BIN] FILE...
"""
import re
import subprocess
import sys

binary = "target/debug/cherenkov"
args = sys.argv[1:]
if args[:1] == ["--cherenkov"]:
    binary, args = args[1], args[2:]
BLOCK = re.compile(r"(<!-- generated: cherenkov (.*?) -->\n)(.*?)(<!-- /generated -->)", re.S)


def fill(m):
    out = subprocess.run([binary, *m.group(2).split()], capture_output=True, text=True, check=True).stdout
    return m.group(1) + out + m.group(4)


for path in args:
    text = open(path).read()
    new = BLOCK.sub(fill, text)
    if new != text:
        open(path, "w").write(new)
