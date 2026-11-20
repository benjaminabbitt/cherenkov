"""GDB-side helpers for the tier B probe, loaded with `source` in GDB.

Each helper prints one JSON line so the DAP client can parse the repl output.
"""
import json

import gdb


def _function_blocks(frame):
    """Every lexical block of the frame's function, found by visiting each
    instruction of the function (GDB has no child-block iterator)."""
    fn = frame.block()
    while fn.function is None:
        fn = fn.superblock
    arch = frame.architecture()
    seen, blocks = set(), []
    for insn in arch.disassemble(fn.start, fn.end - 1):
        b = gdb.block_for_pc(insn["addr"])
        while b is not None:
            key = (b.start, b.end)
            if key not in seen:
                seen.add(key)
                blocks.append(b)
            if b == fn:
                break
            b = b.superblock
    return blocks


def slot(name):
    """Address and type of local `name`, even outside its lexical block.

    A Rust `let` begins its variable's block after the statement, but the
    variable's stack slot is frame-relative and exists for the whole call.
    """
    frame = gdb.selected_frame()
    for block in _function_blocks(frame):
        for sym in block:
            if sym.is_variable and sym.name == name:
                value = sym.value(frame)
                t = sym.type.strip_typedefs()
                print(json.dumps({
                    "addr": int(value.address),
                    "size": t.sizeof,
                    "signed": t.code == gdb.TYPE_CODE_INT and t.is_signed,
                    "type": str(sym.type),
                }))
                return
    print(json.dumps({"error": f"no local {name}"}))


def write_value(name, expression):
    """Evaluate `expression` in the frame and store it in local `name`'s slot."""
    frame = gdb.selected_frame()
    try:
        value = gdb.parse_and_eval(expression)
    except gdb.error as e:
        print(json.dumps({"error": str(e)}))
        return
    for block in _function_blocks(frame):
        for sym in block:
            if sym.is_variable and sym.name == name:
                t = sym.type.strip_typedefs()
                addr = int(sym.value(frame).address)
                raw = int(value).to_bytes(t.sizeof, "little", signed=t.is_signed)
                gdb.selected_inferior().write_memory(addr, raw)
                print(json.dumps({"addr": addr, "value": int(value)}))
                return
    print(json.dumps({"error": f"no local {name}"}))
