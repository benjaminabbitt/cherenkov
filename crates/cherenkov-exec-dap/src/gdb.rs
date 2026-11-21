//! The GDB driver: GDB's built-in DAP server (`gdb -i=dap`) on stdio.
//! Code memory and disassembly are plain DAP; locating a source line's
//! function uses GDB's Python API through the repl (docs/rust-gdb-findings.md).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine;
use cherenkov_core::execution::{Driven, ExecError};
use cherenkov_dap_types::{
    Capabilities, ExitedEventBody, InitializeRequestArguments, StoppedEventBody,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::dap::{DapClient, DapError, EventQueue};
use crate::load::{Allowance, Scheduling};
use crate::native::{
    Effect, Evaluated, FunctionCode, Launcher, NativeSession, Recorded, RunEnd, TierPExecutor,
    kill_debuggee,
};
use crate::tierp::Insn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GdbConfig {
    pub gdb: PathBuf,
    /// A debug-profile test binary.
    pub test_binary: PathBuf,
    /// Arguments for the test binary.
    pub test_args: Vec<String>,
    /// The directory it runs in: its package's, as `cargo test` runs it.
    /// `None` keeps the debugger's own.
    pub cwd: Option<PathBuf>,
    /// Variables added to its environment (cargo's, for a Rust test).
    pub env: Vec<(String, String)>,
    /// The project's root: a source file is the project's only under it (a
    /// dependency may record a file of the same relative name). `None`
    /// accepts any file outside cargo's stores and the toolchain.
    pub source_root: Option<PathBuf>,
}

const REQUEST: Duration = Duration::from_secs(60);

fn err<E: std::fmt::Display>(context: &str) -> impl Fn(E) -> ExecError + '_ {
    move |e| ExecError(format!("{context}: {e}"))
}

/// A program under GDB, stopped at the start of `main`.
pub struct GdbSession {
    child: Child,
    dap: DapClient,
    thread: i64,
    inferior: Option<u32>,
    /// The last run timed out: the program is still running.
    running: bool,
    /// What the last run lost to waiting for a CPU, in its wall time.
    lost: Duration,
    /// GDB is inside a call into the program that did not return (an
    /// evaluation waiting on a thread it stopped): it cannot be reused.
    stuck: bool,
    /// The test binary's arguments from the configuration (the selected
    /// tests), used when no narrower selection is made.
    test_args: Vec<String>,
    /// The project's root as a Python string literal ("" when unknown):
    /// only files under it are the project's.
    source_root_json: String,
    /// The test binary, whose line table says which lines have code.
    binary: PathBuf,
    /// Per file of the line table, the lines with code; `Some(None)` when
    /// the table cannot be read. Read once, on the first locate.
    code_lines: Option<Option<HashMap<String, BTreeSet<u32>>>>,
    /// What each (file, line, last line) located to: the binary does not
    /// change within a session, nor its addresses (GDB turns off address
    /// randomisation), and several mutants share a line.
    located: HashMap<(PathBuf, u32, u32), Vec<FunctionCode>>,
    /// Disassembly requests made, for [`GdbSession::disassemblies`].
    disassembled: u64,
    /// Each function's instructions by its start and the requested file:
    /// a large function has many mutated lines.
    functions: HashMap<(u64, String), FunctionCode>,
}

impl GdbSession {
    pub fn start(config: &GdbConfig) -> Result<GdbSession, ExecError> {
        let mut child = Command::new(&config.gdb)
            .args(["-q", "-nx", "-i=dap"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| ExecError(format!("starting {}: {e}", config.gdb.display())))?;
        let (Some(stdout), Some(stdin)) = (child.stdout.take(), child.stdin.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ExecError("gdb has no stdio pipes".into()));
        };
        let mut session = GdbSession {
            child,
            dap: DapClient::over(stdout, stdin, REQUEST),
            thread: 1,
            inferior: None,
            lost: Duration::ZERO,
            running: false,
            stuck: false,
            test_args: config.test_args.clone(),
            source_root_json: serde_json::to_string(
                &config
                    .source_root
                    .as_ref()
                    .map(|r| r.display().to_string())
                    .unwrap_or_default(),
            )
            .map_err(err("source root"))?,
            binary: config.test_binary.clone(),
            code_lines: None,
            located: HashMap::new(),
            disassembled: 0,
            functions: HashMap::new(),
        };
        session.launch(config)?;
        // An exit is caught alive, so the session stays stopped and can start
        // the next run (DAP refuses requests once the program has exited).
        session.repl("set confirm off")?;
        session.repl("catch syscall exit_group")?;
        session.record_inferior();
        Ok(session)
    }

    /// What stopped the program, naming the signal when it was one (GDB's
    /// stop event says only "signal").
    fn stop_kind(&mut self, body: &StoppedEventBody) -> String {
        let kind = body
            .description
            .clone()
            .unwrap_or_else(|| body.reason.clone());
        if !kind.contains("signal") {
            return kind;
        }
        let name = self
            .repl("python import signal; print(signal.Signals(int(gdb.parse_and_eval('$_siginfo.si_signo'))).name)")
            .ok()
            .map(|o| o.trim().to_string())
            .filter(|n| n.starts_with("SIG"));
        match name {
            Some(n) => format!("signal {n}"),
            None => kind,
        }
    }

    /// A variable of the environment the program runs with, as GDB has it.
    pub fn environment_variable(&mut self, name: &str) -> Result<Option<String>, ExecError> {
        let out = self.repl(&format!("show environment {name}"))?;
        // `NAME = value`, or "Environment variable "NAME" not defined."
        Ok(out
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{name} = ")))
            .map(|v| v.trim_end().to_string()))
    }

    /// The program's process id, for teardown; read while it is stopped.
    fn record_inferior(&mut self) {
        self.inferior = self
            .repl("python print(gdb.selected_inferior().pid)")
            .ok()
            .and_then(|o| o.trim().parse().ok());
        for name in PASSED_SIGNALS {
            let _ = self.repl(&format!("handle {name} nostop noprint pass"));
        }
    }

    /// A caught exit's status, from the `exit_group` argument.
    fn exit_status(&mut self) -> Result<i32, ExecError> {
        let out = self.repl("python print(int(gdb.parse_and_eval('$rdi')) & 0xff)")?;
        out.trim()
            .parse()
            .map_err(|e| ExecError(format!("exit status {out:?}: {e}")))
    }

    /// How a process GDB reports as exited ended: DAP gives a death by
    /// signal (SIGKILL, or one passed to the program) as exit code 0, so
    /// GDB's `$_exitsignal` decides.
    fn exit_end(&mut self, code: i32) -> RunEnd {
        let signal = self
            .repl(
                "python v = gdb.parse_and_eval('$_exitsignal'); \
                 print('none' if v.type.code == gdb.TYPE_CODE_VOID else int(v))",
            )
            .ok()
            .and_then(|o| o.trim().parse::<usize>().ok());
        match signal {
            Some(n) => RunEnd::Crashed {
                kind: SIGNALS
                    .get(n.wrapping_sub(1))
                    .map_or_else(|| format!("signal {n}"), |s| (*s).to_string()),
                message: "the process was killed by a signal".into(),
            },
            None => RunEnd::Exited(code),
        }
    }

    /// The continue-and-evaluate loop of [`GdbSession::run_evaluating`].
    fn evaluate_loop(
        &mut self,
        allowance: &mut Allowance,
        call: &str,
    ) -> Result<Evaluated, ExecError> {
        let mut reached = false;
        let mut resume = true;
        loop {
            if resume {
                self.dap
                    .request("continue", json!({"threadId": self.thread}))
                    .map_err(err("continue"))?;
            }
            resume = true;
            match self.next_step(allowance, call, &mut reached)? {
                Step::Resume => {}
                Step::Wait => resume = false,
                Step::Done(evaluated) => return Ok(evaluated),
                Step::Ended(end) => {
                    return Ok(Evaluated {
                        end,
                        reached,
                        refused: None,
                    });
                }
            }
        }
    }

    /// The continue-and-wait loop of [`GdbSession::run_reaching`].
    fn reach_loop(
        &mut self,
        allowance: &mut Allowance,
        watching: &mut bool,
        reached: &mut bool,
    ) -> Result<RunEnd, ExecError> {
        loop {
            self.dap
                .request("continue", json!({"threadId": self.thread}))
                .map_err(err("continue"))?;
            match self.wait_end(allowance) {
                Ok(event) if event[VANISHED] == true => return Ok(self.vanished_end()),
                Ok(event) if event["event"] == "exited" => {
                    let body: ExitedEventBody = serde_json::from_value(event["body"].clone())
                        .map_err(err("exited event"))?;
                    self.await_last_output();
                    return Ok(self.exit_end(body.exit_code));
                }
                Ok(event) => {
                    let body: StoppedEventBody = serde_json::from_value(event["body"].clone())
                        .map_err(err("stopped event"))?;
                    // The only breakpoints after `main` are the reach ones;
                    // any other stop is a crash (a fatal signal), caught
                    // before it kills the process.
                    if body.reason.contains("syscall") {
                        let code = self.exit_status()?;
                        self.settle_output();
                        return Ok(RunEnd::Exited(code));
                    }
                    if *watching && body.reason.contains("breakpoint") {
                        *reached = true;
                        *watching = false;
                        self.instruction_breakpoints(&[])?;
                        continue;
                    }
                    let kind = self.stop_kind(&body);
                    return Ok(RunEnd::Crashed {
                        kind,
                        message: body.text.unwrap_or_default(),
                    });
                }
                Err(DapError::Timeout(_)) => {
                    self.running = true;
                    return Ok(RunEnd::TimedOut);
                }
                Err(e) => return Err(ExecError(format!("waiting for exit: {e}"))),
            }
        }
    }

    /// The time allowed one run of `timeout`, stretched by the time machine
    /// load takes from it.
    fn allowance(&self, timeout: Duration) -> Allowance {
        Allowance::new(
            timeout,
            Box::new(Scheduling::new(self.pid(), self.debuggee_pid())),
        )
    }

    /// The next `exited` or `stopped` event within the `allowance`. A program killed
    /// outright (SIGKILL) sometimes ends with no event from GDB at all; the
    /// wait checks, once a second, that the program still exists, and when
    /// it does not, gives GDB a moment more and then reports it gone (an
    /// event marked [`VANISHED`]) rather than waiting out the budget.
    fn wait_end(&mut self, allowance: &mut Allowance) -> Result<Value, DapError> {
        let mut gone_since: Option<Instant> = None;
        loop {
            let left = allowance.left();
            match self
                .dap
                .wait_any_event(&["exited", "stopped"], left.min(Duration::from_secs(1)))
            {
                Err(DapError::Timeout(what)) => {
                    if left.is_zero() {
                        return Err(DapError::Timeout(what));
                    }
                    if self.inferior.is_some_and(|pid| !alive(pid)) {
                        let since = *gone_since.get_or_insert_with(Instant::now);
                        if since.elapsed() >= Duration::from_secs(1) {
                            return Ok(json!({ VANISHED: true }));
                        }
                    }
                }
                other => return other,
            }
        }
    }

    /// How a program that disappeared without an event ended: by the signal
    /// GDB recorded, if any, else as killed. Never an exit: nothing says it
    /// ran to completion.
    fn vanished_end(&mut self) -> RunEnd {
        match self.exit_end(0) {
            RunEnd::Exited(_) => RunEnd::Crashed {
                kind: "killed".into(),
                message: "the process ended without GDB reporting how".into(),
            },
            end => end,
        }
    }

    /// The program is stopped at its caught exit: its output is all written,
    /// but GDB may still be forwarding it.
    fn settle_output(&mut self) {
        // libtest's summary is its last line; without one (a crash, another
        // runner), wait until GDB has gone quiet.
        let summary = |events: &EventQueue| {
            events.named("output").any(|e| {
                e["body"]["output"]
                    .as_str()
                    .is_some_and(|o| o.contains("test result:"))
            })
        };
        if !summary(self.dap.queued()) {
            self.dap.settle_until(Duration::from_millis(30), summary);
        }
    }

    fn launch(&mut self, config: &GdbConfig) -> Result<(), ExecError> {
        let args = json!({"clientID": "cherenkov", "adapterID": "gdb", "linesStartAt1": true, "columnsStartAt1": true});
        let args: InitializeRequestArguments =
            serde_json::from_value(args).map_err(err("initialize arguments"))?;
        let body = self
            .dap
            .request(
                "initialize",
                serde_json::to_value(&args).map_err(err("initialize"))?,
            )
            .map_err(err("initialize"))?;
        let _caps: Capabilities =
            serde_json::from_value(body).map_err(err("initialize response"))?;
        // GDB answers `launch` only after `configurationDone`.
        let launch = self
            .dap
            .send("launch", {
                let mut launch = json!({
                    "program": config.test_binary,
                    "args": config.test_args,
                    "stopAtBeginningOfMainSubprogram": true,
                });
                if let Some(cwd) = &config.cwd {
                    launch["cwd"] = json!(cwd);
                }
                if !config.env.is_empty() {
                    // GDB's DAP `launch` replaces the program's whole
                    // environment with this one (`clear_env`): pass ours,
                    // with the configured variables over it, or the program
                    // runs without PATH or HOME.
                    let mut env: serde_json::Map<String, Value> =
                        std::env::vars().map(|(k, v)| (k, json!(v))).collect();
                    for (k, v) in &config.env {
                        env.insert(k.clone(), json!(v));
                    }
                    launch["env"] = Value::Object(env);
                }
                launch
            })
            .map_err(err("launch"))?;
        self.dap
            .request("configurationDone", Value::Null)
            .map_err(err("configurationDone"))?;
        self.dap.response(launch, REQUEST).map_err(err("launch"))?;
        let stopped = self
            .dap
            .wait_event("stopped", REQUEST)
            .map_err(err("stop at main"))?;
        let body: StoppedEventBody =
            serde_json::from_value(stopped["body"].clone()).map_err(err("stopped event"))?;
        self.thread = body.thread_id.map_or(1, i64::from);
        Ok(())
    }

    /// [`GdbSession::repl`], waiting at most `limit` for GDB's answer.
    fn repl_within(&mut self, command: &str, limit: Duration) -> Result<String, DapError> {
        let seq = self.dap.send(
            "evaluate",
            json!({"expression": command, "context": "repl"}),
        )?;
        let body = self.dap.response(seq, limit)?;
        Ok(body["result"].as_str().unwrap_or_default().to_string())
    }

    /// Run a GDB CLI command through the repl and return its output.
    fn repl(&mut self, command: &str) -> Result<String, ExecError> {
        let body = self
            .dap
            .request(
                "evaluate",
                json!({"expression": command, "context": "repl"}),
            )
            .map_err(err("evaluate"))?;
        Ok(body["result"].as_str().unwrap_or_default().to_string())
    }

    /// How many source lines this session has asked GDB to decode while
    /// locating code: what locating costs, in the large binaries of a
    /// workspace.
    pub fn line_lookups(&mut self) -> Result<u64, ExecError> {
        let out = self.repl("python print(globals().get('_cherenkov_lookups', 0))")?;
        out.trim()
            .parse()
            .map_err(|e| ExecError(format!("line lookups {out:?}: {e}")))
    }

    /// How many functions this session has asked GDB to disassemble.
    pub fn disassemblies(&self) -> u64 {
        self.disassembled
    }

    /// Process id of the program under test.
    pub fn debuggee_pid(&self) -> u32 {
        self.inferior.unwrap_or(0)
    }

    /// Process id of GDB.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}

/// The bounds of the function holding a source line, from GDB's Python API.
#[derive(Deserialize)]
struct Bounds {
    start: u64,
    end: u64,
    file: String,
}

/// Python run inside GDB: print the function bounds of `FILE:LINE` as
/// JSON, or `{}` when the program has no code there.
const LOCATE: &str = "import gdb, json
try:
    # A relative path matches by trailing components, so `src/lib.rs:N` can
    # also name a line of the toolchain's own sources (libtest is
    # `library/test/src/lib.rs`). Prefer the file recorded under exactly that
    # name; never a toolchain source.
    path = LOCATION.rsplit(':', 1)[0]
    def ours(f):
        # The same file recorded or requested under an absolute prefix (a
        # binary built in another checkout) counts; a relative prefix, as
        # libtest's `library/test/src/lib.rs`, or a toolchain path does not.
        # Nor does a dependency's source in cargo's stores (getopts'
        # `.../registry/.../src/lib.rs`): in a binary without the project
        # file it would be the only match, and be patched instead.
        if f.startswith('/rustc/') or '/registry/' in f or '/git/checkouts/' in f:
            return False
        return (f == path
                or (f.startswith('/') and f.endswith('/' + path))
                or (path.startswith('/') and path.endswith('/' + f)))
    def ours_symtab(t):
        # By its full path too: a dependency can record its file under the
        # same relative name (getopts' `src/lib.rs`, under libtest; a path
        # dependency's `src/lib.rs`). Never one in cargo's stores or the
        # toolchain's, and only under the project's root when it is known.
        if t is None or not ours(t.filename):
            return False
        full = t.fullname() or ''
        if full.startswith('/rustc/') or '/registry/' in full or '/git/checkouts/' in full:
            return False
        root = SOURCE_ROOT
        return not root or full.startswith(root.rstrip('/') + '/')
    # A line with no row of its own has its code under the lines after it
    # (`Pair {` in a struct literal): the next line with code in our file,
    # asked for exactly (GDB would rather match the line in another file of
    # the same name, libtest's lib.rs, than move on in ours). Its function
    # must start at or before the requested line: otherwise the line's own
    # function was never compiled into this binary, and the code found is
    # another function's.
    line = int(LOCATION.rsplit(':', 1)[1])
    def starts_by(s):
        # The outermost function around the code (an inlined callee's
        # block sits inside it).
        fn = None
        k = gdb.block_for_pc(s.pc)
        while k is not None:
            if k.function is not None:
                fn = k.function
            k = k.superblock
        return fn is not None and fn.line <= line
    # A file in none of the binary's line tables has no code on any line:
    # one lookup says so, and it is remembered for the session (a workspace
    # asks every test program about every planned file).
    absent = globals().setdefault('_cherenkov_absent', set())
    sals = []
    for ln in CANDIDATES:
        if path in absent:
            break
        try:
            globals()['_cherenkov_lookups'] = globals().get('_cherenkov_lookups', 0) + 1
            found = gdb.decode_line(f'{path}:{ln}')[1] or []
        except gdb.error as e:
            if 'No source file' in str(e):
                absent.add(path)
                break
            continue
        sals = [s for s in found if ours_symtab(s.symtab) and s.line == ln]
        if sals:
            sals = [s for s in sals if starts_by(s)]
            break
    sals.sort(key=lambda s: s.symtab.filename != path)
    # Every function with code on the line (a closure's body shares its
    # line with the function around it), each by its outermost function
    # block: code inlined into a function (a `vec!` expansion, `Box::new`)
    # has blocks of its own nested inside it.
    found = []
    for sal in sals:
        b = None
        k = gdb.block_for_pc(sal.pc)
        while k is not None:
            if k.function is not None:
                b = k
            k = k.superblock
        if b is not None and all(f['start'] != b.start for f in found):
            found.append({'start': b.start, 'end': b.end, 'file': sal.symtab.filename})
except gdb.error:
    found = []
print(json.dumps(found))";

/// Marks the event [`GdbSession::wait_end`] makes for a program that
/// disappeared without one.
const VANISHED: &str = "cherenkov_vanished";

/// Whether process `pid` is still running. A program GDB has not noticed
/// dying is also not reaped: it stays a zombie, which still exists but runs
/// no more, so the process's state decides, as the OS reports it (sysinfo,
/// the same on every OS).
fn alive(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};
    let pid = Pid::from_u32(pid);
    // Without the OS's process list, say yes: the wait falls back to GDB's
    // own event rather than calling a running program gone.
    let Ok(mut system) = System::new() else {
        return true;
    };
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    system
        .process(pid)
        .is_some_and(|p| !matches!(p.status(), ProcessStatus::Zombie | ProcessStatus::Dead))
}

/// Each address's source line as GDB places it (`find_pc_line`, as DAP's
/// disassembly does): `[line, symtab file name]`, or null for none.
const LINES: &str = "import gdb, json
out = []
for pc in PCS:
    sal = gdb.find_pc_line(pc)
    if sal.symtab is None or sal.line == 0 or sal.symtab.filename is None:
        out.append(None)
    else:
        out.append([sal.line, sal.symtab.filename])
print(json.dumps(out))";

/// How far past a line with no code its code is looked for.
const LOCATE_WINDOW: u32 = 64;

/// Whether `recorded`, a file of the line table, is the requested file
/// `path`: the same name, or one under an absolute prefix of the other
/// (a binary built in another checkout). Never a toolchain source or a
/// dependency's in cargo's stores, as the locate script decides.
fn same_source(recorded: &str, path: &str) -> bool {
    if recorded.starts_with("/rustc/")
        || recorded.contains("/registry/")
        || recorded.contains("/git/checkouts/")
    {
        return false;
    }
    recorded == path
        || (recorded.starts_with('/') && recorded.ends_with(&format!("/{path}")))
        || (path.starts_with('/') && path.ends_with(&format!("/{recorded}")))
}

/// GDB-side tier B helpers (`cherenkov_assign`, `cherenkov_return`).
const TIER_B: &str = include_str!("gdb_tierb.py");

/// GDB-side recording of which tests call which functions.
const RECORD: &str = include_str!("gdb_record.py");

/// How long one tier B evaluation at a stop may take: building a value or
/// evaluating an expression takes milliseconds; longer means it is waiting.
const EVALUATION: Duration = Duration::from_secs(5);

/// Hits of one function's entry before it counts as called by every test.
const RECORD_HITS: u32 = 64;

impl NativeSession for GdbSession {
    /// A fresh process of the same program in this session: GDB, the
    /// program's symbols and the helpers stay loaded.
    fn restart(&mut self) -> Result<bool, ExecError> {
        if self.stuck {
            return Ok(false);
        }
        if self.running {
            self.dap
                .request("pause", json!({"threadId": self.thread}))
                .map_err(err("pause"))?;
            self.dap
                .wait_event("stopped", REQUEST)
                .map_err(err("pause"))?;
            self.running = false;
        }
        self.instruction_breakpoints(&[])?;
        let _ = self.dap.take_events("stopped");
        let _ = self.dap.take_events("output");
        // Start the new process and run it to `main`, the state a launch
        // leaves it in (the program loaded and relocated). Each step waits
        // for its own stop; notifications left over are discarded so the
        // next run does not take one for its own end.
        self.repl("starti")?;
        self.repl("tbreak main")?;
        self.dap
            .request("continue", json!({"threadId": self.thread}))
            .map_err(err("continue to main"))?;
        let body = self.wait_stop(|b| b.reason.contains("breakpoint"))?;
        self.settle_stops();
        self.thread = body.thread_id.map_or(self.thread, i64::from);
        self.record_inferior();
        Ok(true)
    }

    fn run_evaluating(
        &mut self,
        stops: &[u64],
        effect: &Effect,
        timeout: Duration,
    ) -> Result<Option<Evaluated>, ExecError> {
        let script = serde_json::to_string(TIER_B).map_err(err("tier B script"))?;
        self.repl(&format!("python exec({script})"))?;
        // `return` asks for confirmation unless told not to.
        self.repl("set confirm off")?;
        let call = format!("python {}", effect_call(effect)?);
        self.instruction_breakpoints(stops)?;
        if let Some(refused) = self.load_libraries(effect)? {
            return Ok(Some(refused));
        }
        let mut allowance = self.allowance(timeout);
        let evaluated = self.evaluate_loop(&mut allowance, &call);
        self.lost = allowance.finish();
        evaluated.map(Some)
    }

    fn lost_to_load(&self) -> Duration {
        self.lost
    }

    fn take_output(&mut self) -> String {
        self.dap
            .take_events("output")
            .iter()
            .filter(|e| e["body"]["category"].as_str().is_none_or(|c| c == "stdout"))
            .filter_map(|e| e["body"]["output"].as_str())
            .collect()
    }

    fn function_code(&mut self, file: &Path, line: u32) -> Result<Option<FunctionCode>, ExecError> {
        Ok(self.function_codes(file, line, line)?.into_iter().next())
    }

    fn function_codes(
        &mut self,
        file: &Path,
        line: u32,
        last: u32,
    ) -> Result<Vec<FunctionCode>, ExecError> {
        let key = (file.to_path_buf(), line, last);
        if let Some(found) = self.located.get(&key) {
            return Ok(found.clone());
        }
        let found = self.locate_uncached(file, line, last)?;
        self.located.insert(key, found.clone());
        Ok(found)
    }

    fn read(&mut self, addr: u64, len: usize) -> Result<Vec<u8>, ExecError> {
        let body = self
            .dap
            .request(
                "readMemory",
                json!({"memoryReference": format!("{addr:#x}"), "count": len}),
            )
            .map_err(err("read memory"))?;
        base64::engine::general_purpose::STANDARD
            .decode(body["data"].as_str().unwrap_or_default())
            .map_err(err("read memory data"))
    }

    fn write(&mut self, addr: u64, bytes: &[u8]) -> Result<(), ExecError> {
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        self.dap
            .request(
                "writeMemory",
                json!({"memoryReference": format!("{addr:#x}"), "data": data}),
            )
            .map_err(err("write memory"))?;
        Ok(())
    }

    fn run_reaching(
        &mut self,
        reach: &[u64],
        timeout: Duration,
    ) -> Result<(RunEnd, bool), ExecError> {
        let mut watching = !reach.is_empty();
        if watching {
            self.instruction_breakpoints(reach)?;
        }
        let mut reached = false;
        let mut allowance = self.allowance(timeout);
        let ended = self.reach_loop(&mut allowance, &mut watching, &mut reached);
        self.lost = allowance.finish();
        ended.map(|end| (end, reached))
    }

    fn run_recording(
        &mut self,
        entries: &[u64],
        reach: &[u64],
        tests: &[String],
        timeout: Duration,
    ) -> Result<Option<Recorded>, ExecError> {
        let script = serde_json::to_string(RECORD).map_err(err("record script"))?;
        self.repl(&format!("python exec({script})"))?;
        let addrs = serde_json::to_string(entries).map_err(err("entries"))?;
        let reach = serde_json::to_string(reach).map_err(err("reach"))?;
        let names = serde_json::to_string(tests).map_err(err("tests"))?;
        self.repl(&format!(
            "python cherenkov_record({}, {}, {}, {RECORD_HITS})",
            py_str(&addrs)?,
            py_str(&reach)?,
            py_str(&names)?
        ))?;
        // The recording breakpoints never stop the program: this is a plain
        // run to exit.
        let (end, _) = self.run_reaching(&[], timeout)?;
        if end == RunEnd::TimedOut {
            return Ok(Some((end, Vec::new(), Vec::new())));
        }
        let out = self.repl("python cherenkov_recorded()")?;
        let json_line = out
            .lines()
            .rev()
            .find(|l| l.trim_start().starts_with('{'))
            .unwrap_or("{}");
        let recorded: Value = serde_json::from_str(json_line).map_err(err("recorded"))?;
        let reached: Vec<u64> = recorded["reached"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect();
        let mut calls = Vec::new();
        for (addr, r) in recorded["calls"].as_object().into_iter().flatten() {
            let addr: u64 = addr.parse().map_err(err("recorded address"))?;
            let driven = if r["every"] == true {
                Driven::Every
            } else {
                let names = r["tests"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|t| t.as_str().map(String::from))
                    .collect();
                Driven::Tests(names)
            };
            calls.push((addr, driven));
        }
        Ok(Some((end, calls, reached)))
    }

    fn select_tests(&mut self, tests: Option<&[String]>) -> Result<bool, ExecError> {
        // libtest: exact names, any number of them.
        let args: Vec<String> = match tests {
            Some(names) => names
                .iter()
                .cloned()
                .chain(std::iter::once("--exact".to_string()))
                .collect(),
            None => self.test_args.clone(),
        };
        let quoted: Vec<String> = args
            .iter()
            .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
            .collect();
        self.repl(&format!("set args {}", quoted.join(" ")))?;
        Ok(true)
    }
}

impl GdbSession {
    /// [`NativeSession::function_codes`], asking GDB.
    fn locate_uncached(
        &mut self,
        file: &Path,
        line: u32,
        last: u32,
    ) -> Result<Vec<FunctionCode>, ExecError> {
        let location = serde_json::to_string(&format!("{}:{line}", file.display()))
            .map_err(err("location"))?;
        // Only lines the line table has code for are asked about: each
        // lookup costs milliseconds in a large binary, and the search used
        // to walk every line in between.
        let candidates = match self.candidate_lines(file, line) {
            Some(lines) if lines.is_empty() => return Ok(Vec::new()),
            Some(lines) => format!("{lines:?}"),
            None => format!("range({line}, {})", line + LOCATE_WINDOW),
        };
        let script = serde_json::to_string(
            &LOCATE
                .replace("LOCATION", &location)
                .replace("LAST", &last.max(line).to_string())
                .replace("SOURCE_ROOT", &self.source_root_json)
                .replace("CANDIDATES", &candidates),
        )
        .map_err(err("script"))?;
        let out = self.repl(&format!("python exec({script})"))?;
        let json_line = out
            .lines()
            .rev()
            .find(|l| l.trim_start().starts_with('['))
            .unwrap_or("[]");
        let found: Vec<Bounds> = serde_json::from_str(json_line).map_err(err("locate output"))?;
        found
            .into_iter()
            .map(|bounds| self.code_in(bounds))
            .collect()
    }

    /// The lines from `line` on, within [`LOCATE_WINDOW`], that the line
    /// table has code for in `file`; None when the table cannot be read.
    fn candidate_lines(&mut self, file: &Path, line: u32) -> Option<Vec<u32>> {
        let binary = &self.binary;
        let table = self.code_lines.get_or_insert_with(|| {
            let rows = crate::lines::rows(binary).ok()?;
            let mut by_file: HashMap<String, BTreeSet<u32>> = HashMap::new();
            // GDB places a line only at rows that begin a statement.
            for r in rows.into_iter().filter(|r| r.line > 0 && r.is_stmt) {
                by_file.entry(r.file).or_default().insert(r.line);
            }
            Some(by_file)
        });
        let table = table.as_ref()?;
        let path = file.display().to_string();
        let mut lines: BTreeSet<u32> = BTreeSet::new();
        for (recorded, code) in table {
            if same_source(recorded, &path) {
                lines.extend(code.range(line..line + LOCATE_WINDOW));
            }
        }
        Some(lines.into_iter().collect())
    }

    /// The instructions of the function `bounds` spans.
    fn code_in(&mut self, bounds: Bounds) -> Result<FunctionCode, ExecError> {
        let key = (bounds.start, bounds.file.clone());
        if let Some(code) = self.functions.get(&key) {
            return Ok(code.clone());
        }
        let code = self.disassemble(bounds)?;
        self.functions.insert(key, code.clone());
        Ok(code)
    }

    /// The instructions of the function `bounds` spans, asking GDB.
    /// The function's instructions, decoded here from one read of its
    /// bytes, each with the source line GDB places it on (as DAP's
    /// `disassemble` reports it, without its per-instruction symbol and
    /// source lookups or its count running past the function's end).
    fn disassemble(&mut self, bounds: Bounds) -> Result<FunctionCode, ExecError> {
        self.disassembled += 1;
        let len = bounds.end.saturating_sub(bounds.start) as usize;
        let bytes = NativeSession::read(self, bounds.start, len)?;
        let mut decoder =
            iced_x86::Decoder::with_ip(64, &bytes, bounds.start, iced_x86::DecoderOptions::NONE);
        let mut spans = Vec::new();
        while decoder.can_decode() {
            let at = decoder.position();
            let insn = decoder.decode();
            spans.push((insn.ip(), bytes[at..at + insn.len()].to_vec()));
        }
        let pcs: Vec<u64> = spans.iter().map(|(pc, _)| *pc).collect();
        let pcs = serde_json::to_string(&pcs).map_err(err("pcs"))?;
        let script =
            serde_json::to_string(&LINES.replace("PCS", &pcs)).map_err(err("lines script"))?;
        let out = self.repl(&format!("python exec({script})"))?;
        let json_line = out
            .lines()
            .rev()
            .find(|l| l.trim_start().starts_with('['))
            .unwrap_or("[]");
        let placed: Vec<Option<(u32, String)>> =
            serde_json::from_str(json_line).map_err(err("lines output"))?;
        let insns: Vec<Insn> = spans
            .into_iter()
            .zip(placed)
            .filter_map(|((pc, bytes), at)| {
                let (line, file) = at?;
                Some(Insn {
                    pc,
                    bytes,
                    file,
                    line,
                    column: 0,
                })
            })
            .collect();
        // Instructions report the symtab's path; match the requested file to it.
        let reported = insns
            .iter()
            .map(|i| i.file.clone())
            .find(|f| f.ends_with(&bounds.file))
            .unwrap_or(bounds.file);
        Ok(FunctionCode {
            file: reported,
            insns,
        })
    }

    /// The next stop `wanted` accepts; other stops before it are dropped.
    fn wait_stop(
        &mut self,
        wanted: impl Fn(&StoppedEventBody) -> bool,
    ) -> Result<StoppedEventBody, ExecError> {
        loop {
            let stopped = self
                .dap
                .wait_event("stopped", REQUEST)
                .map_err(err("waiting for a stop"))?;
            let body: StoppedEventBody =
                serde_json::from_value(stopped["body"].clone()).map_err(err("stopped event"))?;
            if wanted(&body) {
                return Ok(body);
            }
        }
    }

    /// Drop the notifications already received: the stops and continues of
    /// getting the new process to `main`.
    fn settle_stops(&mut self) {
        self.dap.settle(Duration::ZERO);
        for name in ["stopped", "continued", "exited", "output"] {
            let _ = self.dap.take_events(name);
        }
    }

    /// GDB can forward the program's last output after `exited`; it is all
    /// in once `terminated` arrives.
    fn await_last_output(&mut self) {
        let _ = self.dap.wait_event("terminated", Duration::from_secs(1));
    }

    /// Replace the instruction breakpoints with one at each address.
    fn instruction_breakpoints(&mut self, addrs: &[u64]) -> Result<(), ExecError> {
        let bps: Vec<Value> = addrs
            .iter()
            .map(|a| json!({"instructionReference": format!("{a:#x}")}))
            .collect();
        self.dap
            .request("setInstructionBreakpoints", json!({"breakpoints": bps}))
            .map_err(err("setInstructionBreakpoints"))?;
        Ok(())
    }
}

/// The GDB Python call that applies `effect` at a stop.
fn effect_call(effect: &Effect) -> Result<String, ExecError> {
    Ok(match effect {
        Effect::Assign { target, value } => {
            format!("cherenkov_assign({}, {})", py_str(target)?, py_str(value)?)
        }
        Effect::Return { value } => format!("cherenkov_return({})", py_str(value)?),
        Effect::ReturnBuilt {
            value,
            in_registers,
        } => {
            let regs = if *in_registers { "True" } else { "False" };
            let spec = serde_json::to_string(&built_json(value)).map_err(err("value"))?;
            format!("cherenkov_return_built({}, {regs})", py_str(&spec)?)
        }
        Effect::ReturnConstructed { request, .. } => {
            return Err(ExecError(format!(
                "{} for {}: a constructor not resolved to a library",
                request.value, request.ty
            )));
        }
        Effect::FlipResult => "cherenkov_flip()".to_string(),
        Effect::Steer { steers } => {
            let spec = serde_json::to_string(&steer_json(steers)).map_err(err("steers"))?;
            format!("cherenkov_steer({})", py_str(&spec)?)
        }
        Effect::Branch {
            condition,
            if_true,
            if_false,
        } => format!(
            "cherenkov_branch({}, {if_true:#x}, {if_false:#x})",
            py_str(condition)?
        ),
        Effect::PerStop(copies) => {
            let mut by_pc = serde_json::Map::new();
            for (stops, effect) in copies {
                let call = effect_call(effect)?;
                for pc in stops {
                    by_pc.insert(pc.to_string(), Value::String(call.clone()));
                }
            }
            let spec = serde_json::to_string(&by_pc).map_err(err("copies"))?;
            format!("cherenkov_by_pc({})", py_str(&spec)?)
        }
        Effect::Guarded { guard, inner } => {
            let spec = json!({
                "lhs": guard.lhs,
                "rhs": guard.rhs,
                "op": guard.op,
                "panic": guard.panic,
            });
            let spec = serde_json::to_string(&spec).map_err(err("guard"))?;
            format!(
                "cherenkov_guarded({}, {})",
                py_str(&spec)?,
                py_str(&effect_call(inner)?)?
            )
        }
    })
}

/// Steers for `cherenkov_steer`, keyed by the jump's address in decimal.
fn steer_json(steers: &[crate::tierp::Steer]) -> Value {
    let goto = |g: &crate::tierp::Goto| {
        json!({
            "to": g.to,
            "store": g.store.map(|s| json!({"disp": s.disp, "size": s.size, "value": s.value})),
        })
    };
    let by_pc: serde_json::Map<String, Value> = steers
        .iter()
        .map(|s| {
            let entry = json!({"cc": s.cc, "taken": goto(&s.taken), "fall": goto(&s.fall)});
            (s.pc.to_string(), entry)
        })
        .collect();
    Value::Object(by_pc)
}

/// The libraries an effect's constructors are in.
fn libraries(effect: &Effect) -> Vec<PathBuf> {
    fn of(b: &crate::layouts::Built, out: &mut Vec<PathBuf>) {
        for c in &b.constructs {
            if !out.contains(&c.library) {
                out.push(c.library.clone());
            }
        }
        for (_, h) in &b.heap {
            of(h, out);
        }
    }
    let mut out = Vec::new();
    match effect {
        Effect::ReturnBuilt { value, .. } => of(value, &mut out),
        Effect::Guarded { inner, .. } => out = libraries(inner),
        Effect::PerStop(copies) => {
            for (_, effect) in copies {
                for library in libraries(effect) {
                    if !out.contains(&library) {
                        out.push(library);
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// A built value as the GDB helper reads it: hex bytes and heap blocks.
fn built_json(b: &crate::layouts::Built) -> Value {
    let hex: String = b.bytes.iter().map(|x| format!("{x:02x}")).collect();
    let heap: Vec<Value> = b
        .heap
        .iter()
        .map(|(at, v)| json!({"at": at, "value": built_json(v)}))
        .collect();
    let calls: Vec<Value> = b
        .calls
        .iter()
        .map(|(at, addr)| json!({"at": at, "addr": addr}))
        .collect();
    let constructs: Vec<Value> = b
        .constructs
        .iter()
        .map(|c| {
            json!({"at": c.at, "size": c.size, "library": c.library.display().to_string(), "symbol": c.symbol})
        })
        .collect();
    json!({"bytes": hex, "heap": heap, "calls": calls, "constructs": constructs})
}

/// A Python string literal for `s` (JSON's escaping is valid Python).
fn py_str(s: &str) -> Result<String, ExecError> {
    serde_json::to_string(s).map_err(err("python string"))
}

impl Drop for GdbSession {
    fn drop(&mut self) {
        // Ask GDB to end the inferior, then make sure: GDB killed with SIGKILL
        // before handling the request would leave a running inferior behind.
        let _ = self
            .dap
            .send("disconnect", json!({"terminateDebuggee": true}));
        kill_debuggee(self.inferior);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Launcher for GdbConfig {
    type Session = GdbSession;

    fn start(&self) -> Result<GdbSession, ExecError> {
        GdbSession::start(self)
    }

    fn abi(&self) -> &'static str {
        "sysv"
    }

    fn binary(&self) -> Option<&Path> {
        Some(&self.test_binary)
    }
}

/// Native mutants under GDB, applied as tier P patches.
pub type GdbExecutor = TierPExecutor<GdbConfig>;

/// Linux x86-64 signal names by number (1..=31).
const SIGNALS: [&str; 31] = [
    "SIGHUP",
    "SIGINT",
    "SIGQUIT",
    "SIGILL",
    "SIGTRAP",
    "SIGABRT",
    "SIGBUS",
    "SIGFPE",
    "SIGKILL",
    "SIGUSR1",
    "SIGSEGV",
    "SIGUSR2",
    "SIGPIPE",
    "SIGALRM",
    "SIGTERM",
    "SIGSTKFLT",
    "SIGCHLD",
    "SIGCONT",
    "SIGSTOP",
    "SIGTSTP",
    "SIGTTIN",
    "SIGTTOU",
    "SIGURG",
    "SIGXCPU",
    "SIGXFSZ",
    "SIGVTALRM",
    "SIGPROF",
    "SIGWINCH",
    "SIGIO",
    "SIGPWR",
    "SIGSYS",
];

/// The signals passed straight to the program, never a stop: its own
/// signal use (Rust ignores SIGPIPE; tests use SIGCHLD, SIGTERM, timers) is
/// its business. One that kills it is still a crash, seen at its exit
/// (`$_exitsignal`, or the program found gone). Left out: faults, always a
/// crash here (Rust catches SIGSEGV and SIGBUS only to report a stack
/// overflow; passed on, a mutant's real fault would read as a pass), and the
/// debugger's own (SIGINT, SIGTRAP) and the uncatchable (SIGKILL, SIGSTOP).
/// GDB names signals the same on every host.
pub const PASSED_SIGNALS: [&str; 20] = [
    "SIGHUP",
    "SIGQUIT",
    "SIGUSR1",
    "SIGUSR2",
    "SIGPIPE",
    "SIGALRM",
    "SIGTERM",
    "SIGCHLD",
    "SIGCONT",
    "SIGTSTP",
    "SIGTTIN",
    "SIGTTOU",
    "SIGURG",
    "SIGXCPU",
    "SIGXFSZ",
    "SIGVTALRM",
    "SIGPROF",
    "SIGWINCH",
    "SIGIO",
    "SIGSYS",
];

impl GdbSession {
    /// Load the libraries the effect calls into, at `main`, while the
    /// program has one thread (after the breakpoints: DAP sets those only
    /// in a stop it has reported, and the load's calls are not). The
    /// refusal when one does not load.
    fn load_libraries(&mut self, effect: &Effect) -> Result<Option<Evaluated>, ExecError> {
        for library in libraries(effect) {
            let out = self.repl(&format!(
                "python cherenkov_load({})",
                py_str(&library.display().to_string())?
            ))?;
            let answer = evaluator_answer(&out);
            let _ = self.dap.take_events("stopped");
            if let Some(why) = answer["error"].as_str() {
                return Ok(Some(Evaluated {
                    end: RunEnd::TimedOut,
                    reached: false,
                    refused: Some(why.to_string()),
                }));
            }
        }
        Ok(None)
    }

    /// Wait for the program's next event within `left` and say what the run
    /// does about it.
    fn next_step(
        &mut self,
        allowance: &mut Allowance,
        call: &str,
        reached: &mut bool,
    ) -> Result<Step, ExecError> {
        match self.wait_end(allowance) {
            Ok(event) if event[VANISHED] == true => Ok(Step::Ended(self.vanished_end())),
            Ok(event) if event["event"] == "exited" => {
                let body: ExitedEventBody =
                    serde_json::from_value(event["body"].clone()).map_err(err("exited event"))?;
                self.await_last_output();
                Ok(Step::Ended(self.exit_end(body.exit_code)))
            }
            Ok(event) => {
                let body: StoppedEventBody =
                    serde_json::from_value(event["body"].clone()).map_err(err("stopped event"))?;
                self.on_stop(body, allowance, call, reached)
            }
            Err(DapError::Timeout(_)) => {
                self.running = true;
                Ok(Step::Ended(RunEnd::TimedOut))
            }
            Err(e) => Err(ExecError(format!("waiting for exit: {e}"))),
        }
    }

    /// What the run does at a stop: let GDB's own calls pass, end at a
    /// syscall exit, evaluate the effect at a breakpoint, else the program
    /// crashed.
    fn on_stop(
        &mut self,
        body: StoppedEventBody,
        allowance: &mut Allowance,
        call: &str,
        reached: &mut bool,
    ) -> Result<Step, ExecError> {
        // GDB reports the calls it makes for the evaluator itself (a
        // mutated value that calls a function) as stops; they are ours, not
        // the program's.
        if body.reason == "function call" || body.description.as_deref() == Some("function call") {
            return Ok(Step::Wait);
        }
        if body.reason.contains("syscall") {
            let code = self.exit_status()?;
            self.settle_output();
            return Ok(Step::Done(Evaluated {
                end: RunEnd::Exited(code),
                reached: *reached,
                refused: None,
            }));
        }
        if body.reason.contains("breakpoint") {
            *reached = true;
            return self.evaluate_at_breakpoint(allowance, call);
        }
        let kind = self.stop_kind(&body);
        Ok(Step::Ended(RunEnd::Crashed {
            kind,
            message: body.text.unwrap_or_default(),
        }))
    }

    /// Apply the effect at a breakpoint, within what is left of the run's
    /// budget; the run goes on unless the evaluation is refused or the
    /// budget is spent.
    fn evaluate_at_breakpoint(
        &mut self,
        allowance: &mut Allowance,
        call: &str,
    ) -> Result<Step, ExecError> {
        let done = |end, refused| {
            Ok(Step::Done(Evaluated {
                end,
                reached: true,
                refused,
            }))
        };
        // The run's budget covers its evaluations too.
        let left = allowance.left();
        if left.is_zero() {
            return done(RunEnd::TimedOut, None);
        }
        let limit = EVALUATION.min(left);
        let out = match self.repl_within(call, limit) {
            Ok(out) => out,
            // Evaluations run with the program's other threads stopped: one
            // waiting on them never returns. The mutated program would not
            // hang, so the mutant is refused, and GDB, stuck in the call, is
            // done.
            Err(DapError::Timeout(_)) => {
                self.stuck = true;
                return done(
                    RunEnd::TimedOut,
                    Some(format!(
                        "the evaluation did not finish within {} s with the \
                         program's other threads stopped",
                        limit.as_secs()
                    )),
                );
            }
            Err(e) => return Err(ExecError(format!("evaluate: {e}"))),
        };
        // Stops from the evaluator's own calls are already queued.
        let _ = self.dap.take_events("stopped");
        match evaluator_answer(&out)["error"].as_str() {
            Some(why) => done(RunEnd::TimedOut, Some(why.to_string())),
            None => Ok(Step::Resume),
        }
    }
}

/// What a run does after an event.
enum Step {
    /// Resume the program.
    Resume,
    /// Wait for the next event without resuming: the stop was GDB's own call.
    Wait,
    /// The run is over, with this result.
    Done(Evaluated),
    /// The program ended this way, without a refusal.
    Ended(RunEnd),
}

/// The evaluator's answer in its output: the last line that is JSON, or the
/// output as an error.
fn evaluator_answer(out: &str) -> Value {
    out.lines()
        .rev()
        .find_map(|l| serde_json::from_str(l.trim()).ok())
        .unwrap_or_else(|| json!({"error": out.trim()}))
}

#[cfg(test)]
mod tests {
    use super::alive;

    #[test]
    fn test_this_process_is_alive() {
        assert!(alive(std::process::id()));
    }

    #[test]
    fn test_an_exited_and_reaped_child_is_gone() {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!alive(pid));
    }

    #[test]
    fn test_an_exited_child_not_yet_reaped_is_gone() {
        // A program GDB has not noticed dying is not reaped either: it stays
        // a zombie, which still exists but runs no more.
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while alive(pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!alive(pid), "a zombie counts as gone");
        child.wait().unwrap();
    }
}
