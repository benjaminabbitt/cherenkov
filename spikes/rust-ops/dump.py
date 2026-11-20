"""Print each probe function's instructions grouped by source line:
python3 -I dump.py <lib.so> <function>..."""
import re
import subprocess
import sys

libs, names = sys.argv[1].split(","), sys.argv[2:]
text = "".join(subprocess.run(["objdump", "-d", "-l", "-M", "intel", "-w", "-C", l], capture_output=True, text=True).stdout for l in libs)
blocks = re.split(r"\n(?=[0-9a-f]+ <)", text)
for name in names:
    for b in blocks:
        head = b.splitlines()[0] if b else ""
        if re.search(rf"<ops::{re.escape(name)}>:", head):
            print(f"=== {name}")
            line = None
            for l in b.splitlines()[1:]:
                m = re.search(r"src/lib\.rs:(\d+)", l)
                if m:
                    line = m.group(1)
                    continue
                if re.match(r"\s+[0-9a-f]+:", l):
                    parts = l.split("\t")
                    if len(parts) >= 3:
                        print(f"  L{line or '?':>4} {parts[1].strip():<24} {parts[2].strip()[:60]}")
