"""Throwaway M1 probe: answer Delve's open questions empirically.

Usage: python3 -I delve_probe.py <test-binary> <test-name> <patch-spec>
patch-spec: "none" | "<function>:<line>:<old-hex>:<new-hex>"
  The patch is written at the first instruction of <line> inside
  <function> whose bytes start with <old-hex>.
"""
import base64
import json
import os
import socket
import subprocess
import sys
import time


class Dap:
    def __init__(self, sock):
        self.s = sock
        self.buf = b""
        self.seq = 0
        self.events = []

    def _read_msg(self):
        while b"\r\n\r\n" not in self.buf:
            self.buf += self.s.recv(65536)
        head, rest = self.buf.split(b"\r\n\r\n", 1)
        n = int(head.split(b":")[1])
        while len(rest) < n:
            rest += self.s.recv(65536)
        self.buf = rest[n:]
        return json.loads(rest[:n])

    def req(self, command, **args):
        self.seq += 1
        body = json.dumps({"seq": self.seq, "type": "request", "command": command, "arguments": args}).encode()
        self.s.sendall(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        while True:
            m = self._read_msg()
            if m["type"] == "event":
                self.events.append(m)
            elif m["type"] == "response" and m["request_seq"] == self.seq:
                return m

    def wait_event(self, name, timeout=20):
        deadline = time.time() + timeout
        self.s.settimeout(timeout)
        while time.time() < deadline:
            for i, e in enumerate(self.events):
                if e["event"] == name:
                    return self.events.pop(i)
            m = self._read_msg()
            if m["type"] == "event":
                self.events.append(m)
        raise TimeoutError(name)


class Rpc:
    def __init__(self, sock):
        self.f = sock.makefile("rwb")
        self.id = 0

    def call(self, method, **params):
        self.id += 1
        self.f.write(json.dumps({"method": "RPCServer." + method, "params": [params], "id": self.id}).encode() + b"\n")
        self.f.flush()
        resp = json.loads(self.f.readline())
        return resp


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def connect(port):
    for _ in range(100):
        try:
            return socket.create_connection(("127.0.0.1", port))
        except OSError:
            time.sleep(0.05)
    raise RuntimeError("dlv did not listen")


def main():
    binary, test, spec = sys.argv[1], sys.argv[2], sys.argv[3]
    port = free_port()
    dlv = subprocess.Popen(
        [os.path.expanduser("~/go/bin/dlv"), "exec", binary, "--headless", "--accept-multiclient",
         "--api-version=2", f"--listen=127.0.0.1:{port}", "--", "-test.run", f"^{test}$", "-test.v"],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
    )
    report = {}
    try:
        dap = Dap(connect(port))
        rpc = Rpc(connect(port))
        init = dap.req("initialize", adapterID="cherenkov", clientID="cherenkov", linesStartAt1=True, columnsStartAt1=True)
        caps = init["body"]
        report["caps"] = {k: caps.get(k) for k in ("supportsGotoTargetsRequest", "supportsWriteMemoryRequest", "supportsReadMemoryRequest", "supportsSetVariable", "supportsDisassembleRequest")}
        att = dap.req("attach", mode="remote", stopOnEntry=True)
        report["attach"] = att["success"], att.get("message")
        report["rpc_state_running"] = rpc.call("State", NonBlocking=True).get("result", {}).get("State", {}).get("Running")

        patch = None
        if spec != "none":
            fn, line, old_hex, new_hex = spec.split(":")
            locs = rpc.call("FindLocation", Scope={"GoroutineID": -1}, Loc=f"calc.go:{line}", IncludeNonExecutableLines=False)
            loc0 = locs["result"]["Locations"][0]; report["location"] = {k: v for k, v in loc0.items() if k != "function"}
            pcs = loc0.get("PCs") or [loc0["pc"]]
            start = loc0["function"]["value"] if os.environ.get("AT_ENTRY") else min(pcs)
            old = bytes.fromhex(old_hex)
            new = bytes.fromhex(new_hex)

            def read(addr, n):
                r = rpc.call("ExamineMemory", Address=addr, Length=n)
                if r.get("error"):
                    raise RuntimeError(r["error"])
                return base64.b64decode(r["result"]["Mem"])

            def write(addr, data):
                errs = []
                for i, b in enumerate(data):
                    r = rpc.call("Set", Scope={"GoroutineID": -1, "Frame": 0}, Symbol=f"*(*uint8)({addr + i:#x})", Value=str(b))
                    if r.get("error"):
                        errs.append(r["error"])
                return errs

            window = read(start, 32)
            idx = window.find(old)
            assert idx >= 0, f"{old.hex()} not in {window.hex()}"
            addr = start + idx
            report["patch_at"] = hex(addr)
            report["write_errors"] = write(addr, new)
            report["after_arm"] = read(addr, len(old)).hex()
            if os.environ.get("DISARM_BEFORE_RUN"):
                write(addr, old)
                report["disarm_byte_identical"] = read(start, 32) == window

        dap.req("configurationDone")
        dap.req("continue", threadId=1)
        ev = dap.wait_event("terminated", timeout=30)
        report["terminated"] = True
        out = dlv.stdout.read1(1 << 20).decode(errors="replace") if dlv.stdout else ""
        report["test_output_tail"] = out.strip().splitlines()[-3:]
    finally:
        dlv.kill()
    print(json.dumps(report, indent=1))


if __name__ == "__main__":
    main()
