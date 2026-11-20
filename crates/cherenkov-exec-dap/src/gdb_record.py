import gdb, json

# Which tests call each planned function, recorded inside GDB: a breakpoint
# at the function's entry whose stop() notes the test on the stack and lets
# the program go on, so no stop reaches the debug adapter. The test is the
# frame whose name, without its crate, is one of the binary's tests
# (`deps::address::tests::test_x` is test `address::tests::test_x`): the
# thread's name would do, but Linux truncates it to 15 bytes.
#
# A function hit more than `limit` times, or called where no test is on
# the stack (a thread a test spawned) or while a once-initialised value is
# made, counts as called by every test, and
# its breakpoint is dropped: recording stays bounded and selection safe.


class _CherenkovEntry(gdb.Breakpoint):
    def __init__(self, addr, tests, limit):
        super().__init__(f"*{addr:#x}", internal=True)
        self.addr = addr
        self.tests = tests
        self.limit = limit
        self.seen = set()
        self.threads = set()
        self.hits = 0
        self.every = False

    def stop(self):
        self.hits += 1
        # libtest runs each test on a thread of its own: a thread already
        # seen here is the same test, and needs no stack walk.
        thread = gdb.selected_thread().global_num
        if thread in self.threads and self.hits <= self.limit:
            return False
        self.threads.add(thread)
        test = None if self.hits > self.limit else _cherenkov_test_on_stack(self.tests)
        if test is None:
            self.every = True
            self.enabled = False
        else:
            self.seen.add(test)
        return False


class _CherenkovReach(gdb.Breakpoint):
    """Mutated code: whether anything executes it. The first hit is all
    that is asked, so the breakpoint then goes."""

    def __init__(self, addr):
        super().__init__(f"*{addr:#x}", internal=True)
        self.addr = addr
        self.hit = False

    def stop(self):
        self.hit = True
        self.enabled = False
        return False


# A value computed once and kept (a lazy or once-initialised static) is
# read by later tests that never call the function that made it: such a
# call counts as every test's.
_CHERENKOV_ONCE = ("std::sync::once", "OnceLock", "OnceCell", "LazyLock", "LazyCell",
                   "lazy_static", "once_cell")


def _cherenkov_test_on_stack(tests):
    """The outermost test on the stack (a test may call another test's
    function), or None: no test, or a once-initialised value being made."""
    f = gdb.newest_frame()
    found = None
    for _ in range(256):
        if f is None:
            return found
        fn = f.function()
        name = (fn.name if fn is not None else f.name()) or ""
        if any(m in name for m in _CHERENKOV_ONCE):
            return None
        base = name.split("<", 1)[0]
        if base.startswith("test::run_test"):
            return found
        parts = base.split("::", 1)
        if len(parts) == 2 and parts[1] in tests:
            found = parts[1]
        f = f.older()
    return None


_cherenkov_entries = []
_cherenkov_reach = []


def cherenkov_record(addrs_json, reach_json, tests_json, limit):
    """Set a recording breakpoint at each entry address, and a reach one at
    each mutated address."""
    global _cherenkov_entries, _cherenkov_reach
    for b in _cherenkov_entries + _cherenkov_reach:
        b.delete()
    tests = set(json.loads(tests_json))
    _cherenkov_entries = [_CherenkovEntry(a, tests, limit) for a in json.loads(addrs_json)]
    _cherenkov_reach = [_CherenkovReach(a) for a in json.loads(reach_json)]
    print(json.dumps({"ok": True}))


def cherenkov_recorded():
    """What was recorded, and the breakpoints removed: {"calls": {"addr":
    {"every": bool, "tests": [...]}} for every entry hit, "reached": [addr]
    for every mutated address executed}."""
    global _cherenkov_entries, _cherenkov_reach
    calls = {}
    for b in _cherenkov_entries:
        if b.hits > 0:
            calls[str(b.addr)] = {"every": b.every, "tests": sorted(b.seen)}
        b.delete()
    reached = [b.addr for b in _cherenkov_reach if b.hit]
    for b in _cherenkov_reach:
        b.delete()
    _cherenkov_entries = []
    _cherenkov_reach = []
    print(json.dumps({"calls": calls, "reached": reached}))
