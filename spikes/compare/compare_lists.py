"""Compare cargo-mutants' mutants with Cherenkov's cargo-mutants set.

Usage:
  cargo mutants --list --json --gitignore=true > cm.json      (in the crate)
  cherenkov plan <crate> --operators cargo-mutants --format json > ch.json
  python3 -I compare_lists.py cm.json ch.json [show-missing] [show-extra]

Mutants are matched on (file, start line, start column, replacement text).
Only Rust files are compared, since cargo-mutants only mutates Rust.
"""
import json, sys
from collections import Counter
cm = json.load(open(sys.argv[1]))
ch = [p for p in json.load(open(sys.argv[2]))["plans"] if p["file"].endswith(".rs")]
SPELL = {"add": "+", "sub": "-", "mul": "*", "div": "/", "rem": "%", "bit_and": "&", "bit_or": "|", "bit_xor": "^",
         "shl": "<<", "shr": ">>", "lt": "<", "le": "<=", "gt": ">", "ge": ">=", "eq": "==", "ne": "!=", "and": "&&", "or": "||",
         "add_assign": "+=", "sub_assign": "-=", "mul_assign": "*=", "div_assign": "/=", "rem_assign": "%=",
         "bit_and_assign": "&=", "bit_or_assign": "|=", "bit_xor_assign": "^=", "shl_assign": "<<=", "shr_assign": ">>="}
def norm(s): return " ".join(s.split())
cm_keys = {}
for m in cm:
    k = (m["file"], m["span"]["start"]["line"], m["span"]["start"]["column"], norm(m["replacement"]))
    cm_keys[k] = m["genre"]
ch_keys = {}
for p in ch:
    e, a = p["edit"], p["action"]
    if e["kind"] == "replace_token":
        rep = SPELL[e["with"]]
    elif e["kind"] == "replace_with_literal" and isinstance(e["lit"], dict) and "bool" in e["lit"]:
        rep = "true" if e["lit"]["bool"] else "false"
    elif e["kind"] in ("delete_token", "delete_match"):
        rep = ""
    elif a["kind"] == "return-default":
        rep = a.get("value_text") or "<default>"
    else:
        rep = f"<{p['operator']}>"
    k = (p["file"], p["site"]["start"]["line"], p["site"]["start"]["col"], norm(rep))
    ch_keys[k] = p["operator"]
both = cm_keys.keys() & ch_keys.keys()
only_cm = sorted(cm_keys.keys() - ch_keys.keys())
only_ch = sorted(ch_keys.keys() - cm_keys.keys())
print(f"cargo-mutants {len(cm_keys)}, cherenkov {len(ch_keys)}, matched {len(both)}")
print("missing from cherenkov, by genre:", dict(Counter(cm_keys[k] for k in only_cm)))
print("extra in cherenkov, by operator:", dict(Counter(ch_keys[k] for k in only_ch)))
for k in only_cm[:int(sys.argv[3]) if len(sys.argv) > 3 else 0]:
    print("  missing", cm_keys[k], k)
for k in only_ch[:int(sys.argv[4]) if len(sys.argv) > 4 else 0]:
    print("  extra  ", ch_keys[k], k)
