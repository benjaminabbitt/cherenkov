# Tier B helpers, loaded into GDB once per session (docs/rust-tier-b-findings.md).
# Each prints one JSON line for the DAP client to read from the repl output.
import json
import re

import gdb


def _cherenkov_eval(expression):
    """Evaluate a mutated expression with only this thread running: an
    expression that calls a function runs it in the program, and another
    test's thread stopping meanwhile would abandon the call."""
    gdb.execute("set scheduler-locking on")
    try:
        return gdb.parse_and_eval(expression)
    finally:
        gdb.execute("set scheduler-locking off")


def _cherenkov_blocks(frame):
    """Every lexical block of the frame's function. A Rust `let` starts its
    variable's block after the statement, but the stack slot exists for the
    whole call, so locals are searched in all of them."""
    fn = frame.block()
    while fn.function is None:
        fn = fn.superblock
    seen, blocks = set(), []
    for insn in frame.architecture().disassemble(fn.start, fn.end - 1):
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


def _cherenkov_local(frame, name):
    for block in _cherenkov_blocks(frame):
        for sym in block:
            if sym.is_variable or sym.is_argument:
                if sym.name == name:
                    return sym
    return None


_STORE = re.compile(r"mov[lqwb]?\s+%(\w+),\s*(-?0x[0-9a-f]+)?\(%(\w+)\)")


def _cherenkov_place(frame, target):
    """The scalar place `target` names: its address and type. A local is
    found in any of the function's blocks (its own block starts after the
    statement); anything else (`self.total`, `*count`) is a place the
    debugger evaluates at the stop."""
    sym = _cherenkov_local(frame, target)
    if sym is not None:
        return int(sym.value(frame).address), sym.type.strip_typedefs()
    try:
        place = gdb.parse_and_eval(target)
        return int(place.address), place.type.strip_typedefs()
    except (gdb.error, TypeError, ValueError) as e:
        raise gdb.error(f"no local {target}: {e}")


def cherenkov_assign(target, expression):
    """Evaluate `expression`, store it in the place `target`, and resume
    after the statement's store to that place (setting the store's source
    register, which the next statement may read instead of the slot)."""
    frame = gdb.selected_frame()
    try:
        value = _cherenkov_eval(expression)
        slot, t = _cherenkov_place(frame, target)
    except gdb.error as e:
        print(json.dumps({"error": str(e)}))
        return
    if t.code not in (gdb.TYPE_CODE_INT, gdb.TYPE_CODE_BOOL, gdb.TYPE_CODE_CHAR):
        print(json.dumps({"error": f"{target}: only scalar targets ({t})"}))
        return
    signed = t.code == gdb.TYPE_CODE_INT and t.is_signed
    as_int = int(value)
    bits = t.sizeof * 8
    raw = (as_int % (1 << bits)).to_bytes(t.sizeof, "little")
    gdb.selected_inferior().write_memory(slot, raw)
    pc = int(frame.pc())
    insns = frame.architecture().disassemble(pc, count=64)
    regs = _cherenkov_registers()
    for k, ins in enumerate(insns[:-1]):
        m = _STORE.match(ins["asm"])
        if m and m.group(3) in regs and regs[m.group(3)] + int(m.group(2) or "0", 16) == slot:
            gdb.execute(f"set var ${m.group(1)} = {as_int if signed else as_int % (1 << bits)}")
            gdb.execute(f"set var $pc = {insns[k + 1]['addr']}")
            print(json.dumps({"ok": True}))
            return
        _cherenkov_track(regs, ins["asm"])
    print(json.dumps({"error": f"no store to {target} after {pc:#x}"}))


_GPRS = ("rax rbx rcx rdx rsi rdi rbp rsp r8 r9 r10 r11 r12 r13 r14 r15").split()
_CALLER_SAVED = ("rax", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11")
_LOAD = re.compile(r"(mov[a-z]*|lea)\s+(-?0x[0-9a-f]+)?\(%(\w+)\),\s*%(\w+)$")
_MOVE = re.compile(r"mov[a-z]*\s+%(\w+),\s*%(\w+)$")
_DEST = re.compile(r",\s*%(\w+)$")


def _cherenkov_registers():
    """The general registers' values at the stop, to follow what the
    statement's code does to them on its way to the store."""
    frame = gdb.selected_frame()
    return {r: int(frame.read_register(r)) & ((1 << 64) - 1) for r in _GPRS}


def _cherenkov_track(regs, asm):
    """Update `regs` for the effect of one instruction: loads from a known
    address, moves and `lea` are followed; any other write to a register, and
    a call for the caller-saved ones, makes it unknown."""
    if asm.startswith("call"):
        for r in _CALLER_SAVED:
            regs.pop(r, None)
        return
    m = _LOAD.match(asm)
    if m and m.group(3) in regs and m.group(4) in _GPRS:
        addr = regs[m.group(3)] + int(m.group(2) or "0", 16)
        if m.group(1) == "lea":
            regs[m.group(4)] = addr
        elif m.group(1) in ("mov", "movq"):
            try:
                raw = gdb.selected_inferior().read_memory(addr, 8)
                regs[m.group(4)] = int.from_bytes(bytes(raw), "little")
            except gdb.error:
                regs.pop(m.group(4), None)
        else:
            regs.pop(m.group(4), None)
        return
    m = _MOVE.match(asm)
    if m and m.group(1) in regs and m.group(2) in _GPRS:
        regs[m.group(2)] = regs[m.group(1)]
        return
    m = _DEST.search(asm)
    if m:
        regs.pop(m.group(1), None)
        # A write to a part of a register (eax, al) leaves it unknown too.
        for r in _GPRS:
            if m.group(1) != r and (m.group(1).lstrip("re") == r.lstrip("re") or m.group(1)[1:] == r[1:]):
                regs.pop(r, None)


_SCALARS = (gdb.TYPE_CODE_INT, gdb.TYPE_CODE_BOOL, gdb.TYPE_CODE_CHAR, gdb.TYPE_CODE_FLT)


def cherenkov_overflows(lhs, rhs, op):
    """Whether `lhs op rhs` overflows its integer type, as rustc's checks
    would find (GDB's own arithmetic wraps)."""
    left = _cherenkov_eval(lhs)
    right = _cherenkov_eval(rhs)
    ty = left.type.strip_typedefs()
    if ty.code != gdb.TYPE_CODE_INT:
        return False
    bits = ty.sizeof * 8
    a, b = int(left), int(right)
    result = {"+": a + b, "-": a - b, "*": a * b}[op]
    lo, hi = (-(1 << (bits - 1)), (1 << (bits - 1)) - 1) if ty.is_signed else (0, (1 << bits) - 1)
    return not lo <= result <= hi


def cherenkov_by_pc(calls_json):
    """Apply the effect of the copy of a generic function the stop is in:
    `calls` maps each stop's address (decimal) to that copy's call."""
    calls = json.loads(calls_json)
    call = calls.get(str(int(gdb.selected_frame().pc())))
    if call is None:
        print(json.dumps({"error": "a stop that belongs to no copy"}))
        return
    exec(call)


def cherenkov_guarded(guard_json, call):
    """Apply a mutant whose operation rustc checks for overflow: when it
    overflows, panic as the compiled operation would (its overflow block);
    otherwise run `call`, the mutant's own effect."""
    guard = json.loads(guard_json)
    try:
        over = cherenkov_overflows(guard["lhs"], guard["rhs"], guard["op"])
    except gdb.error as e:
        print(json.dumps({"error": str(e)}))
        return
    if over:
        gdb.execute(f"set var $pc = {guard['panic']}")
        print(json.dumps({"ok": True}))
        return
    exec(call)


def cherenkov_return(expression):
    """Evaluate `expression` and return it from the selected frame. Only for
    scalar results: GDB accepts `return` for a result passed through a hidden
    pointer, but the caller does not get the value, so that is refused."""
    frame = gdb.selected_frame()
    fn = frame.function()
    result = fn.type.target().strip_typedefs() if fn is not None else None
    if result is None or result.code not in _SCALARS:
        print(json.dumps({"error": f"only scalar results are returned ({result})"}))
        return
    try:
        # Evaluated once: a value that calls a method with side effects
        # must not run it twice. `return` takes the computed value.
        value = _cherenkov_eval(expression)
        gdb.set_convenience_variable("cherenkov_return", value)
        gdb.execute("return $cherenkov_return", to_string=True)
    except gdb.error as e:
        print(json.dumps({"error": str(e)}))
        return
    print(json.dumps({"ok": True}))


def cherenkov_branch(expression, if_true, if_false):
    """Evaluate the mutated condition and continue at the exit it selects,
    skipping the condition's own code. The exits are run-time addresses."""
    try:
        taken = bool(_cherenkov_eval(expression))
    except gdb.error as e:
        print(json.dumps({"error": str(e)}))
        return
    gdb.execute(f"set var $pc = {if_true if taken else if_false}")
    print(json.dumps({"ok": True}))


# Kept when this script is loaded again: the session's program does not
# change, and listing its functions takes seconds in a large binary.
_CHERENKOV_CUSTOM_ALLOCATOR = globals().get("_CHERENKOV_CUSTOM_ALLOCATOR")


def _cherenkov_custom_allocator():
    """Whether the program sets a #[global_allocator]: rustc then generates
    `__rg_alloc` for it. Its frees would not match libc's malloc."""
    global _CHERENKOV_CUSTOM_ALLOCATOR
    if _CHERENKOV_CUSTOM_ALLOCATOR is None:
        listing = gdb.execute("info functions __rg_alloc", to_string=True)
        _CHERENKOV_CUSTOM_ALLOCATOR = "__rg_alloc" in listing.split("\n", 1)[-1]
    return _CHERENKOV_CUSTOM_ALLOCATOR


def _cherenkov_alloc(n):
    """Allocate n bytes with the program's malloc (the default global
    allocator, so the program frees them as its own). Only this thread runs
    during the call. Refused for a program with its own global allocator."""
    if _cherenkov_custom_allocator():
        raise gdb.error("the program has its own global allocator; heap values are not built")
    gdb.execute("set scheduler-locking on")
    gdb.execute("set language c")
    try:
        return int(gdb.parse_and_eval(f"(unsigned long)malloc({max(n, 1)})"))
    finally:
        gdb.execute("set language auto")
        gdb.execute("set scheduler-locking off")


def _cherenkov_default(addr):
    """The bytes the program's own `Default` function at addr returns,
    called with its DWARF type, so GDB applies its return convention. Only
    this thread runs during the call."""
    fn = gdb.block_for_pc(addr).function
    if fn is None:
        raise gdb.error(f"no function at {addr:#x}")
    gdb.execute("set scheduler-locking on")
    try:
        return bytes(fn.value()().bytes)
    finally:
        gdb.execute("set scheduler-locking off")


_CHERENKOV_LIBRARIES = {}


def _cherenkov_c(expression):
    """Evaluate a C expression in the program with only this thread running."""
    gdb.execute("set scheduler-locking on")
    gdb.execute("set language c")
    try:
        return gdb.parse_and_eval(expression)
    finally:
        gdb.execute("set language auto")
        gdb.execute("set scheduler-locking off")


def _cherenkov_string(text):
    """A C string in the program's heap, for a call's argument. GDB would
    allocate a literal with a call of its own nested in the call it is an
    argument of, and DAP loses track of nested calls (the program counts
    as running after them)."""
    data = text.encode() + b"\0"
    at = int(_cherenkov_c(f"(unsigned long)malloc({len(data)})"))
    gdb.selected_inferior().write_memory(at, data)
    return at


def _cherenkov_free(at):
    _cherenkov_c(f"((void (*)(void *))free)((void *){at})")


def cherenkov_load(path):
    """dlopen the library at path into the program (RTLD_NOW). Its values
    are allocated by its own copy of std with the system allocator, so a
    program with its own global allocator would free them with another."""
    try:
        if _cherenkov_custom_allocator():
            raise gdb.error("the program has its own global allocator; values from the support library are not used")
        # Its symbols are not needed: constructors are found with dlsym.
        gdb.execute("set auto-solib-add off")
        try:
            name = _cherenkov_string(path)
            handle = int(_cherenkov_c(f"(unsigned long)dlopen((char *){name}, 2)"))
            _cherenkov_free(name)
        finally:
            gdb.execute("set auto-solib-add on")
        if handle == 0:
            why = _cherenkov_c("(char *)dlerror()").string()
            raise gdb.error(f"dlopen {path}: {why}")
        _CHERENKOV_LIBRARIES[path] = handle
    except gdb.error as e:
        print(json.dumps({"error": str(e)}))
        return
    print(json.dumps({"ok": True}))


def _cherenkov_construct(c):
    """The bytes the library's constructor writes: called with a scratch
    block of the value's size, which is freed after (the value is moved
    out of it, as Rust moves values: by its bytes)."""
    handle = _CHERENKOV_LIBRARIES.get(c["library"])
    if handle is None:
        raise gdb.error(f"{c['library']} is not loaded")
    name = _cherenkov_string(c["symbol"])
    fn = int(_cherenkov_c(f"(unsigned long)dlsym((void *){handle}, (char *){name})"))
    _cherenkov_free(name)
    if fn == 0:
        raise gdb.error(f"no {c['symbol']} in {c['library']}")
    size = c["size"]
    slot = int(_cherenkov_c(f"(unsigned long)malloc({max(size, 1)})"))
    try:
        _cherenkov_c(f"((void (*)(void *)){fn})((void *){slot})")
        return bytes(gdb.selected_inferior().read_memory(slot, size))
    finally:
        _cherenkov_free(slot)


def _cherenkov_materialize(spec):
    """The bytes of a built value: its heap blocks allocated and pointed
    to, and the parts the program's own code makes, made."""
    data = bytearray.fromhex(spec["bytes"])
    for c in spec.get("calls", []):
        made = _cherenkov_default(c["addr"])
        data[c["at"]:c["at"] + len(made)] = made
    for c in spec.get("constructs", []):
        made = _cherenkov_construct(c)
        data[c["at"]:c["at"] + len(made)] = made
    for h in spec["heap"]:
        block = _cherenkov_materialize(h["value"])
        addr = _cherenkov_alloc(len(block))
        gdb.selected_inferior().write_memory(addr, bytes(block))
        at = h["at"]
        data[at:at + 8] = addr.to_bytes(8, "little")
    return bytes(data)


def cherenkov_return_built(spec_json, in_registers):
    """Return a built value from the function at its entry: in rax:rdx, or
    written through the caller's result pointer (rdi at entry), which rax
    then holds."""
    try:
        data = _cherenkov_materialize(json.loads(spec_json))
        if in_registers:
            padded = data.ljust(16, b"\0")
            rax = int.from_bytes(padded[:8], "little")
            rdx = int.from_bytes(padded[8:], "little")
            gdb.execute("return", to_string=True)
            gdb.execute(f"set var $rax = {rax}")
            gdb.execute(f"set var $rdx = {rdx}")
        else:
            out = int(gdb.parse_and_eval("$rdi"))
            gdb.selected_inferior().write_memory(out, data)
            gdb.execute("return", to_string=True)
            gdb.execute(f"set var $rax = {out}")
    except gdb.error as e:
        print(json.dumps({"error": str(e)}))
        return
    print(json.dumps({"ok": True}))


def _cherenkov_holds(cc, flags):
    """Whether x86 condition code cc holds for eflags."""
    cf, pf, zf, sf, of = (flags >> 0) & 1, (flags >> 2) & 1, (flags >> 6) & 1, (flags >> 7) & 1, (flags >> 11) & 1
    base = [of, cf, zf, cf | zf, sf, pf, sf != of, zf | (sf != of)][cc >> 1]
    return bool(base) != bool(cc & 1)


def cherenkov_steer(spec):
    """At a jump out of a swapped operator's left operand: read from the
    flags which successor the program takes, then go where the mutant goes
    from it, storing a constant result to its stack slot first if any.
    GDB does not report a breakpoint at the address it resumes from, so a
    successor that is another steered jump is steered here too (nothing has
    run in between, so the flags still hold)."""
    steers = json.loads(spec)
    pc = int(gdb.parse_and_eval("$pc"))
    flags = int(gdb.parse_and_eval("$eflags"))
    for _ in range(len(steers)):
        steer = steers.get(str(pc))
        if steer is None:
            break
        cc = steer["cc"]
        goes = steer["taken"] if cc is None or _cherenkov_holds(cc, flags) else steer["fall"]
        store = goes["store"]
        if store:
            at = int(gdb.parse_and_eval("$rsp")) + store["disp"]
            value = store["value"].to_bytes(store["size"], "little")
            gdb.selected_inferior().write_memory(at, value)
        pc = goes["to"]
    gdb.execute(f"set var $pc = {pc}")
    print(json.dumps({"ok": True}))


def cherenkov_flip():
    """Flip the bool a call just returned (bit 0 of al)."""
    gdb.execute("set var $rax = $rax ^ 1")
    print(json.dumps({"ok": True}))
