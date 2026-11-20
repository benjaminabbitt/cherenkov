"""Throwaway probe: tier P on Rust through GDB's built-in DAP server.

Usage: python3 -I gdb_probe.py <test-binary> <function> [<line>:<old-hex>:<new-hex>]

Launches `gdb -i=dap` over stdio, stops at the start of main, resolves the
function's runtime address, prints its instructions with line numbers,
optionally patches bytes on <line>, runs, and reports the exit code.
"""
import base64
import json
import subprocess
import sys


class StdioDap:
    def __init__(self, proc):
        self.p = proc
        self.seq = 0
        self.events = []

    def _read(self):
        header = b""
        while not header.endswith(b"\r\n\r\n"):
            header += self.p.stdout.read(1)
        n = int(header.split(b"Content-Length:")[1].split(b"\r\n")[0])
        return json.loads(self.p.stdout.read(n))

    def send(self, command, **args):
        self.seq += 1
        body = json.dumps({"seq": self.seq, "type": "request", "command": command, "arguments": args}).encode()
        self.p.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        self.p.stdin.flush()
        return self.seq

    def req(self, command, **args):
        self.seq += 1
        body = json.dumps({"seq": self.seq, "type": "request", "command": command, "arguments": args}).encode()
        self.p.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        self.p.stdin.flush()
        while True:
            m = self._read()
            if m["type"] == "event":
                self.events.append(m)
            elif m["type"] == "response" and m["request_seq"] == self.seq:
                return m

    def wait(self, name):
        while True:
            for i, e in enumerate(self.events):
                if e["event"] == name:
                    return self.events.pop(i)
            m = self._read()
            if m["type"] == "event":
                self.events.append(m)


def main():
    binary, function = sys.argv[1], sys.argv[2]
    patch = sys.argv[3] if len(sys.argv) > 3 else None
    gdb = subprocess.Popen(["gdb", "-q", "-i=dap"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    try:
        d = StdioDap(gdb)
        caps = d.req("initialize", adapterID="gdb", linesStartAt1=True, columnsStartAt1=True)["body"]
        print("caps", {k: caps.get(k) for k in ("supportsReadMemoryRequest", "supportsWriteMemoryRequest", "supportsDisassembleRequest", "supportsGotoTargetsRequest")})
        # GDB answers `launch` only after configurationDone.
        d.send("launch", program=binary, args=["--test-threads=1"], stopAtBeginningOfMainSubprogram=True)
        d.req("configurationDone")
        stop = d.wait("stopped")
        print("stopped", stop["body"].get("reason"))
        ev = d.req("evaluate", expression=f"info address {function}", context="repl")
        print("evaluate", ev["success"], ev.get("body", {}).get("result", ev.get("message")))
        import re
        addr = int(re.search(r"0x[0-9a-f]+", ev["body"]["result"]).group(0), 16)
        dis = d.req("disassemble", memoryReference=hex(addr), instructionCount=40)
        insns = dis["body"]["instructions"]
        for i in insns:
            print(f"  {i['address']} L{i.get('line', '?'):>3} {i.get('instructionBytes', ''):<20} {i['instruction']}")
            if i["instruction"].startswith("ret") and i.get("line") and int(i["address"], 16) > addr:
                pass
        if patch:
            line, old, new = patch.split(":")
            site = next(i for i in insns if str(i.get("line")) == line and i.get("instructionBytes", "").startswith(old))
            ref = site["address"]
            r = d.req("readMemory", memoryReference=ref, count=len(old) // 2)
            print("before", base64.b64decode(r["body"]["data"]).hex())
            w = d.req("writeMemory", memoryReference=ref, data=base64.b64encode(bytes.fromhex(new)).decode())
            print("write", w["success"], w.get("message"))
            r = d.req("readMemory", memoryReference=ref, count=len(old) // 2)
            print("after", base64.b64decode(r["body"]["data"]).hex())
        d.req("continue", threadId=stop["body"].get("threadId", 1))
        exited = d.wait("exited")
        print("exitCode", exited["body"]["exitCode"])
    finally:
        gdb.kill()


if __name__ == "__main__":
    main()
