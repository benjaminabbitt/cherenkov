//! The product's races with GDB's output, forced: a fake `gdb` that speaks
//! just enough DAP over stdio delivers the program's last output after the
//! executor's own wait for it, as a loaded machine does. The interleaving is
//! injected (the fake's delay is longer than any wait the session makes),
//! never waited for.
//!
//! These are product-race tests: the race is in `GdbSession`, not in the
//! test.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cherenkov_exec_dap::gdb::{GdbConfig, GdbSession};
use cherenkov_exec_dap::native::{NativeSession, RunEnd};

/// A fake GDB: answers every request, runs nothing, and on `continue` plays
/// `PLAY`: events, each after its delay in seconds.
const FAKE_GDB: &str = r#"#!/usr/bin/env python3
import json, sys, threading, time, os

out = sys.stdout.buffer
lock = threading.Lock()
seq = [0]

def send(msg):
    with lock:
        seq[0] += 1
        msg["seq"] = seq[0]
        data = json.dumps(msg).encode()
        out.write(b"Content-Length: %d\r\n\r\n" % len(data) + data)
        out.flush()

def event(name, body):
    send({"type": "event", "event": name, "body": body})

def respond(req, body=None, success=True):
    send({"type": "response", "request_seq": req["seq"], "success": success,
          "command": req["command"], "body": body or {}})

def read():
    inp = sys.stdin.buffer
    length = None
    while True:
        line = inp.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":")[1])
    return json.loads(inp.read(length))

PLAY = __PLAY__

def play():
    for delay, name, body in PLAY:
        time.sleep(delay)
        event(name, body)

launch_req = None
while True:
    req = read()
    if req is None:
        break
    cmd = req["command"]
    if cmd == "launch":
        launch_req = req
    elif cmd == "configurationDone":
        respond(req)
        respond(launch_req)
        event("stopped", {"reason": "breakpoint", "threadId": 1})
    elif cmd == "evaluate":
        expr = req["arguments"]["expression"]
        result = ""
        if "selected_inferior().pid" in expr:
            result = str(os.getpid())
        elif "$rdi" in expr:
            result = "101"
        respond(req, {"result": result, "variablesReference": 0})
    elif cmd == "continue":
        respond(req, {"allThreadsContinued": True})
        threading.Thread(target=play, daemon=True).start()
    elif cmd == "disconnect":
        respond(req)
        break
    else:
        respond(req)
"#;

/// A session over a fake GDB that plays `play` (a Python list literal of
/// `(delay seconds, event name, body)`) when the program continues.
fn session(name: &str, play: &str) -> (GdbSession, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("cherenkov-gdb-fake-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let gdb = dir.join("gdb");
    std::fs::write(&gdb, FAKE_GDB.replace("__PLAY__", play)).unwrap();
    std::fs::set_permissions(&gdb, std::fs::Permissions::from_mode(0o755)).unwrap();
    let config = GdbConfig {
        gdb,
        test_binary: Path::new("/nonexistent/tests").to_path_buf(),
        test_args: Vec::new(),
        cwd: None,
        env: Vec::new(),
        source_root: None,
    };
    (start(&config), dir)
}

/// Start the session. Another test's thread forking while this one has the
/// script open for writing makes the kernel refuse to execute it
/// (`ETXTBSY`) until that child has exec'd: the start is retried.
fn start(config: &GdbConfig) -> GdbSession {
    let mut tries = 0;
    loop {
        match GdbSession::start(config) {
            Ok(session) => return session,
            Err(e) if e.to_string().contains("busy") && tries < 50 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => panic!("{e}"),
        }
    }
}

fn output(text: &str) -> String {
    format!(
        r#"(0, "output", {{"category": "stdout", "output": {}}})"#,
        serde_json::to_string(text).unwrap()
    )
}

/// The program fails a test and exits with 101; GDB forwards the rest of its
/// output (the failure report and libtest's summary) 3 s after the `exited`
/// event, past any wait the session makes for it, then says `terminated`.
#[test]
fn test_last_output_arriving_after_the_exit_wait_is_still_in_the_run_output() {
    let play = format!(
        r#"[{}, (0, "exited", {{"exitCode": 101}}), (3, "output", {{"category": "stdout", "output": "failures:\n\ntest result: FAILED. 1 passed; 1 failed\n"}}), (0, "terminated", {{}})]"#,
        output("running 2 tests\ntest a ... ok\ntest b ... FAILED\n"),
    );
    let (mut session, dir) = session("late", &play);
    let (end, _) = session.run_reaching(&[], Duration::from_secs(30)).unwrap();
    let seen = session.take_output();
    let _ = std::fs::remove_dir_all(dir);
    assert_eq!(end, RunEnd::Exited(101));
    assert!(
        seen.contains("test result: FAILED"),
        "the run ended with its last output still on the way: {seen:?}"
    );
}

/// GDB never says `terminated`: the output cannot be known complete, so the
/// run is an error that says so, not an exit whose output is trusted.
#[test]
fn test_a_run_whose_output_never_ends_is_an_error_naming_the_output() {
    let play = format!(
        r#"[{}, (0, "exited", {{"exitCode": 0}})]"#,
        output("running 1 test\ntest a ... ok\n"),
    );
    let (mut session, dir) = session("never", &play);
    session.set_last_output_wait(Duration::from_millis(500));
    let started = std::time::Instant::now();
    let ended = session.run_reaching(&[], Duration::from_secs(30));
    let waited = started.elapsed();
    let _ = std::fs::remove_dir_all(dir);
    let why = ended
        .expect_err("an exit with incomplete output must not be a verdict")
        .to_string();
    assert!(
        waited < Duration::from_secs(10),
        "the set bound was ignored: {waited:?}"
    );
    assert!(why.contains("output"), "{why}");
}

const SYSCALL_EXIT: &str = r#"(0, "stopped", {"reason": "syscall 'exit_group'", "threadId": 1})"#;

/// The program is caught at its exit: its output so far has libtest's
/// header and results but not the summary, which GDB forwards 0.5 s later,
/// after the session's idle wait.
#[test]
fn test_summary_arriving_after_the_caught_exit_settles_is_still_in_the_run_output() {
    let play = format!(
        r#"[{}, {SYSCALL_EXIT}, (0.5, "output", {{"category": "stdout", "output": "\ntest result: FAILED. 1 passed; 1 failed\n"}})]"#,
        output("running 2 tests\ntest a ... ok\ntest b ... FAILED\n"),
    );
    let (mut session, dir) = session("syscall-late", &play);
    let (end, _) = session.run_reaching(&[], Duration::from_secs(30)).unwrap();
    let seen = session.take_output();
    let _ = std::fs::remove_dir_all(dir);
    assert_eq!(end, RunEnd::Exited(101));
    assert!(
        seen.contains("test result: FAILED"),
        "the exit was caught with its summary still on the way: {seen:?}"
    );
}

/// A libtest run caught at its exit whose summary never comes is an error
/// that names the output.
#[test]
fn test_a_caught_exit_whose_summary_never_comes_is_an_error_naming_the_output() {
    let play = format!(
        r#"[{}, {SYSCALL_EXIT}]"#,
        output("running 2 tests\ntest a ... ok\n"),
    );
    let (mut session, dir) = session("syscall-never", &play);
    session.set_last_output_wait(Duration::from_millis(500));
    let started = std::time::Instant::now();
    let ended = session.run_reaching(&[], Duration::from_secs(30));
    let waited = started.elapsed();
    let _ = std::fs::remove_dir_all(dir);
    assert!(
        waited < Duration::from_secs(10),
        "the bound that was set is not the one waited: {waited:?}"
    );
    let why = ended
        .expect_err("a summary-less libtest exit has no verdict")
        .to_string();
    assert!(why.contains("output"), "{why}");
}

/// A program that is not libtest has no summary to wait for: its caught
/// exit settles when its output goes quiet, as before.
#[test]
fn test_a_caught_exit_of_a_program_without_libtest_output_is_an_exit() {
    let play = format!(r#"[{}, {SYSCALL_EXIT}]"#, output("hello\n"));
    let (mut session, dir) = session("syscall-plain", &play);
    session.set_last_output_wait(Duration::from_millis(300));
    let (end, _) = session.run_reaching(&[], Duration::from_secs(30)).unwrap();
    let _ = std::fs::remove_dir_all(dir);
    assert_eq!(end, RunEnd::Exited(101));
}

/// Output that merely resembles libtest's header is not libtest's: no
/// summary is waited for, whichever half of the header is there.
#[test]
fn test_a_caught_exit_whose_output_only_half_resembles_libtests_is_an_exit() {
    for (name, text) in [
        ("half-running", "running fast\n"),
        ("half-tests", "all tests\n"),
    ] {
        let play = format!(r#"[{}, {SYSCALL_EXIT}]"#, output(text));
        let (mut session, dir) = session(name, &play);
        session.set_last_output_wait(Duration::from_millis(300));
        let (end, _) = session.run_reaching(&[], Duration::from_secs(30)).unwrap();
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(end, RunEnd::Exited(101), "{text:?}");
    }
}

/// A libtest run whose summary is already in when the exit is caught ends
/// at once, with that output.
#[test]
fn test_a_caught_exit_with_libtests_summary_already_in_is_an_exit_with_its_output() {
    let play = format!(
        r#"[{}, {SYSCALL_EXIT}]"#,
        output("running 1 test\ntest a ... ok\n\ntest result: ok. 1 passed\n"),
    );
    let (mut session, dir) = session("syscall-complete", &play);
    session.set_last_output_wait(Duration::from_millis(300));
    let (end, _) = session.run_reaching(&[], Duration::from_secs(30)).unwrap();
    let seen = session.take_output();
    let _ = std::fs::remove_dir_all(dir);
    assert_eq!(end, RunEnd::Exited(101));
    assert!(seen.contains("test result: ok"), "{seen:?}");
}

// --- Cancelling a session from another thread.

use std::sync::Arc;

use cherenkov_exec_dap::cancel::Watch;
use cherenkov_run_ports::executor::Cancel;

/// Where a hung fake GDB stops answering.
enum Hangs {
    /// `continue` is never answered: the program never ends.
    OnContinue,
    /// `configurationDone` is never answered: the launch never ends.
    OnLaunch,
}

/// The fake GDB of [`FAKE_GDB`] with a `sleep 600` child, and `hangs`. It
/// writes its own pid and the child's to `pids`.
fn hung_gdb(hangs: &Hangs, pids: &Path) -> String {
    let mut script = FAKE_GDB.replace("__PLAY__", "[]").replace(
        "launch_req = None\nwhile True:",
        &format!(
            "import subprocess\nchild = subprocess.Popen([\"sleep\", \"600\"])\nopen({:?}, \"w\").write(\"%d %d\" % (os.getpid(), child.pid))\nlaunch_req = None\nwhile True:",
            pids.display().to_string()
        ),
    );
    script = match hangs {
        Hangs::OnContinue => script.replace(
            "        respond(req, {\"allThreadsContinued\": True})\n        threading.Thread(target=play, daemon=True).start()\n",
            "        pass\n",
        ),
        Hangs::OnLaunch => script.replace(
            "    elif cmd == \"configurationDone\":\n        respond(req)\n",
            "    elif cmd == \"configurationDone\":\n        pass\n        continue\n",
        ),
    };
    script
}

fn hung_config(name: &str, hangs: &Hangs) -> (GdbConfig, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "cherenkov-gdb-cancel-{}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let (gdb, pids) = (dir.join("gdb"), dir.join("pids"));
    std::fs::write(&gdb, hung_gdb(hangs, &pids)).unwrap();
    std::fs::set_permissions(&gdb, std::fs::Permissions::from_mode(0o755)).unwrap();
    let config = GdbConfig {
        gdb,
        test_binary: Path::new("/nonexistent/tests").to_path_buf(),
        test_args: Vec::new(),
        cwd: None,
        env: Vec::new(),
        source_root: None,
    };
    (config, pids, dir)
}

/// The fake's pid and its child's, once it has written them.
fn pids_of(path: &Path) -> (u32, u32) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Some((a, b)) = text.split_once(' ')
            && let (Ok(a), Ok(b)) = (a.parse(), b.parse())
        {
            return (a, b);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the fake gdb wrote no pids");
}

/// Whether process `pid` is gone (or a zombie awaiting its parent's wait),
/// within five seconds.
fn gone(pid: u32) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Err(_) => return true,
            Ok(stat)
                if stat
                    .rsplit_once(')')
                    .is_some_and(|(_, r)| r.trim_start().starts_with('Z')) =>
            {
                return true;
            }
            Ok(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    false
}

fn wait_for<T>(rx: &std::sync::mpsc::Receiver<T>) -> Option<T> {
    rx.recv_timeout(Duration::from_secs(5)).ok()
}

#[test]
fn test_cancelling_a_run_in_flight_ends_the_debugger_and_its_child_and_the_run_errors() {
    let (config, pids, dir) = hung_config("run", &Hangs::OnContinue);
    let watch = Arc::new(Watch::default());
    let (done, result) = std::sync::mpsc::channel();
    let owner = watch.clone();
    let runner = std::thread::spawn(move || {
        let mut session = loop {
            match GdbSession::start_watched(&config, &owner) {
                Err(e) if e.to_string().contains("busy") => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                other => break other.unwrap(),
            }
        };
        let ended = session.run_to_exit(Duration::from_secs(600));
        let _ = done.send(ended.is_err());
        session
    });
    let (gdb, child) = pids_of(&pids);
    // The run is under way once gdb has the continue it never answers.
    std::thread::sleep(Duration::from_millis(500));
    watch.cancel();
    let errored = wait_for(&result);
    let (gdb_gone, child_gone) = (gone(gdb), gone(child));
    let session = runner.join().unwrap();
    drop(session);
    let _ = std::fs::remove_dir_all(dir);
    assert_eq!(
        errored,
        Some(true),
        "the blocked run did not return an error"
    );
    assert!(gdb_gone, "gdb {gdb} outlived the cancel");
    assert!(child_gone, "gdb's child {child} outlived the cancel");
}

#[test]
fn test_cancelling_while_the_debugger_launches_fails_the_start_and_ends_the_debugger() {
    let (config, pids, dir) = hung_config("launch", &Hangs::OnLaunch);
    let watch = Arc::new(Watch::default());
    let (done, result) = std::sync::mpsc::channel();
    let owner = watch.clone();
    let starter = std::thread::spawn(move || {
        let started = loop {
            match GdbSession::start_watched(&config, &owner) {
                Err(e) if e.to_string().contains("busy") => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                other => break other,
            }
        };
        let _ = done.send(started.is_err());
    });
    let (gdb, child) = pids_of(&pids);
    std::thread::sleep(Duration::from_millis(500));
    watch.cancel();
    let errored = wait_for(&result);
    let (gdb_gone, child_gone) = (gone(gdb), gone(child));
    starter.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
    assert_eq!(
        errored,
        Some(true),
        "the blocked start did not return an error"
    );
    assert!(gdb_gone, "gdb {gdb} outlived the cancel");
    assert!(child_gone, "gdb's child {child} outlived the cancel");
}
