"""Throwaway M1 probe: SetExecutionPoint and ~r0 under a breakpoint.

Stops at IsSmall's `return true` (calc.go:13) via DAP, then:
  mode "jump":  JSON-RPC SetExecutionPoint to the else-block (calc.go:15).
  mode "r0":    set ~r0 = false, jump to the epilogue's ADDQ (skipping the
                constant load into AX), to test the plan's forced-return idea.
Reports DAP's view of the stop after the jump and the test outcome.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from delve_probe import Dap, Rpc, connect, free_port  # noqa: E402
import subprocess  # noqa: E402


def main():
    binary, src, mode = sys.argv[1], sys.argv[2], sys.argv[3]
    port = free_port()
    dlv = subprocess.Popen(
        [os.path.expanduser("~/go/bin/dlv"), "exec", binary, "--headless", "--accept-multiclient",
         "--api-version=2", f"--listen=127.0.0.1:{port}", "--", "-test.run", "^TestIsSmall$", "-test.v"],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
    )
    rep = {}
    try:
        dap = Dap(connect(port))
        rpc = Rpc(connect(port))
        dap.req("initialize", adapterID="cherenkov", linesStartAt1=True, columnsStartAt1=True)
        dap.req("attach", mode="remote", stopOnEntry=True)
        bp = dap.req("setBreakpoints", source={"path": src}, breakpoints=[{"line": 13}])
        rep["bp"] = [b.get("verified") for b in bp["body"]["breakpoints"]]
        dap.req("configurationDone")
        dap.req("continue", threadId=1)
        stop = dap.wait_event("stopped")
        while stop["body"]["reason"] != "breakpoint":
            rep.setdefault("skipped_stops", []).append(stop["body"]["reason"])
            stop = dap.wait_event("stopped")
        tid = stop["body"].get("threadId")
        rep["stop_body"] = stop["body"]
        rep["stopped"] = stop["body"]["reason"]
        st = dap.req("stackTrace", threadId=tid, levels=1)
        rep["frame_before"] = st["body"].get("stackFrames", [{}])[0].get("line", st.get("message"))

        loc = lambda line: rpc.call("FindLocation", Scope={"GoroutineID": -1}, Loc=f"{src}:{line}")["result"]["Locations"][0]["pc"]
        if mode == "jump":
            target = loc(15)
        else:
            fa = rpc.call("ListFunctionArgs", Scope={"GoroutineID": -1, "Frame": 0}, Cfg={"FollowPointers": False, "MaxStringLen": 8, "MaxArrayValues": 1, "MaxStructFields": 1, "MaxVariableRecurse": 0})
            rep["args"] = [(v["name"], v.get("value"), v.get("flags")) for v in (fa.get("result") or {}).get("Args", [])]
            r = rpc.call("Eval", Scope={"GoroutineID": -1, "Frame": 0}, Expr="~r0")
            rep["r0_before"] = (r.get("result") or {}).get("Variable", {}).get("value", r.get("error"))
            s = rpc.call("Set", Scope={"GoroutineID": -1, "Frame": 0}, Symbol="~r0", Value="false")
            rep["set_r0_error"] = s.get("error")
            target = loc(13) + 10  # the ADDQ after MOVB/MOVL at line 13
        j = rpc.call("SetExecutionPoint", Addr=target)
        rep["jump_error"] = j.get("error")
        rep["rpc_pc_after"] = hex(((j.get("result") or {}).get("State") or {}).get("currentThread", {}).get("pc", 0))
        st = dap.req("stackTrace", threadId=tid, levels=1)
        rep["dap_frame_after_jump"] = st["body"].get("stackFrames", [{}])[0].get("line", st.get("message"))
        dap.req("setBreakpoints", source={"path": src}, breakpoints=[])
        dap.req("continue", threadId=tid)
        dap.wait_event("terminated", timeout=30)
        out = dlv.stdout.read1(1 << 20).decode(errors="replace")
        rep["test"] = out.strip().splitlines()[-2:]
    finally:
        dlv.kill()
    print(json.dumps(rep))


if __name__ == "__main__":
    main()
