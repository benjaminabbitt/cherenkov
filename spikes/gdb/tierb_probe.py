"""Throwaway probe: tier B (stop before, evaluate, resume after) under GDB DAP.

Usage: python3 -I tierb_probe.py <test-binary> <test-name> <line> <target> <mutated-text>

At every hit of a breakpoint on <line>: evaluate <mutated-text> in the
frame (e.g. "m = a / b"), find the instruction after the store to <target>'s
stack slot, set $pc there, and continue. Reports what each step saw.
"""
import json
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(__file__))
from gdb_probe import StdioDap  # noqa: E402


def repl(d, cmd):
    r = d.req("evaluate", expression=cmd, context="repl")
    return r.get("body", {}).get("result", ""), r["success"], r.get("message")


def main():
    binary, test, line, target, text = sys.argv[1:6]
    gdb = subprocess.Popen(["gdb", "-q", "-nx", "-i=dap"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    try:
        d = StdioDap(gdb)
        d.req("initialize", adapterID="gdb", linesStartAt1=True, columnsStartAt1=True)
        d.send("launch", program=binary, args=[test, "--exact", "--test-threads=1"], stopAtBeginningOfMainSubprogram=True)
        d.req("configurationDone")
        d.wait("stopped")
        helpers = os.path.join(os.path.dirname(os.path.abspath(__file__)), "cherenkov_gdb.py")
        print("source helpers", repl(d, f"source {helpers}")[1:])
        repl(d, "python from cherenkov_gdb import *")
        src = os.path.join(os.path.dirname(binary), "../../../../../src/lib.rs")
        bp = d.req("setBreakpoints", source={"path": "src/lib.rs"}, breakpoints=[{"line": int(line)}])
        print("breakpoint", [(b.get("verified"), b.get("line"), b.get("instructionReference")) for b in bp["body"]["breakpoints"]])
        d.req("continue", threadId=1)
        hits = 0
        while True:
            ev = None
            while ev is None:
                for i, e in enumerate(d.events):
                    if e["event"] in ("stopped", "exited"):
                        ev = d.events.pop(i)
                        break
                else:
                    m = d._read()
                    if m["type"] == "event":
                        d.events.append(m)
            if ev["event"] == "exited":
                print("exitCode", ev["body"]["exitCode"], "after", hits, "hits")
                return
            hits += 1
            tid = ev["body"].get("threadId", 1)
            st = d.req("stackTrace", threadId=tid, levels=1)["body"]["stackFrames"][0]
            fid = st["id"]
            value_expr = text.split("=", 1)[1].strip()
            out, _, _ = repl(d, f"python write_value({target!r}, {value_expr!r})")
            written = json.loads(out.strip().splitlines()[-1])
            if hits <= 2:
                print(f"hit {hits} at L{st['line']} pc={st.get('instructionPointerReference')}: wrote {written}")
            if "error" in written:
                print("evaluator error:", written["error"])
                return
            slot = written["addr"]
            pc = int(st["instructionPointerReference"], 16)
            rsp = int(repl(d, "python print(int(gdb.parse_and_eval('$rsp')))")[0].strip().splitlines()[-1])
            dis = d.req("disassemble", memoryReference=hex(pc), instructionCount=40)["body"]["instructions"]
            resume = None
            for k, ins in enumerate(dis):
                mm = re.match(r"mov\s+%?\w+,\s*(-?0x[0-9a-f]+)?\(%rsp\)", ins["instruction"]) or re.match(r"mov\s+(?:QWORD|DWORD) PTR \[rsp([+-]0x[0-9a-f]+)?\],", ins["instruction"])
                if mm:
                    disp = int(mm.group(1) or "0", 16)
                    if rsp + disp == slot:
                        resume = dis[k + 1]["address"]
                        # The next statement may keep using the register the
                        # store copied from, so it gets the mutated value too.
                        src = re.match(r"mov\s+%(\w+),", ins["instruction"])
                        if src:
                            repl(d, f"set var ${src.group(1)} = {written['value']}")
                        if hits <= 2:
                            print(f"  store to {target} at {ins['address']} ({ins['instruction']}), resume {resume}, set ${src.group(1) if src else '?'}")
                        break
            if resume is None:
                print("  no store found; instructions:", [i["instruction"] for i in dis[:12]])
                return
            out, ok, msg = repl(d, f"set var $pc = {resume}")
            if hits <= 2:
                st2 = d.req("stackTrace", threadId=tid, levels=1)["body"]["stackFrames"][0]
                print(f"  set $pc ok={ok} {msg or ''}; DAP now at L{st2['line']} pc={st2.get('instructionPointerReference')}")
            d.req("continue", threadId=tid)
    finally:
        gdb.kill()


if __name__ == "__main__":
    main()
