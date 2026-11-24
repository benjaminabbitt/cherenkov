//! The Delve driver. One headless Delve per run, serving both a DAP client
//! (session lifecycle: attach, configurationDone, continue, exit) and a
//! JSON-RPC client (code memory, disassembly, locations) on one listener.

use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use cherenkov_dap_types::{
    Capabilities, ExitedEventBody, InitializeRequestArguments, StoppedEventBody,
};
use cherenkov_mutation::execution::TestReport;
use cherenkov_run_ports::executor::ExecError;
use serde_json::{Value, json};

use crate::dap::{DapClient, DapError};
use crate::native::{Launcher, NativeSession, TierPExecutor, kill_debuggee};
use crate::rpc::DelveRpc;
use crate::tierp::Insn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelveConfig {
    pub dlv: PathBuf,
    /// A test binary built with `-gcflags=all=-N -l`.
    pub test_binary: PathBuf,
    /// Arguments for the test binary, e.g. `-test.run ^TestX$`.
    pub test_args: Vec<String>,
}

pub use crate::native::{FunctionCode, RunEnd, outcome_of};
pub use crate::toolchain::build_go_test_binary;

const STARTUP: Duration = Duration::from_secs(20);
const REQUEST: Duration = Duration::from_secs(30);
/// The most a run's output is waited for after the run has ended.
const OUTPUT_BOUND: Duration = Duration::from_secs(2);

fn err<E: std::fmt::Display>(context: &str) -> impl Fn(E) -> ExecError + '_ {
    move |e| ExecError(format!("{context}: {e}"))
}

/// A running headless Delve, stopped at entry until [`DelveSession::run_to_exit`].
pub struct DelveSession {
    child: Child,
    dap: DapClient,
    rpc: DelveRpc,
    debuggee: Option<u32>,
    /// What the program has printed, read off Delve's stdout.
    printed: Arc<Printed>,
}

impl DelveSession {
    pub fn start(config: &DelveConfig) -> Result<DelveSession, ExecError> {
        let mut child = Command::new(&config.dlv)
            .arg("exec")
            .arg(&config.test_binary)
            .args([
                "--headless",
                "--accept-multiclient",
                "--api-version=2",
                "--listen=127.0.0.1:0",
                "--",
            ])
            .args(&config.test_args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ExecError(format!("starting {}: {e}", config.dlv.display())))?;
        let (addr, printed) = match listening_address(&mut child) {
            Ok(found) => found,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        // From here, dropping the session tears the process down.
        let connect = |what| {
            TcpStream::connect(&addr)
                .map_err(|e| ExecError(format!("connecting {what} to {addr}: {e}")))
        };
        let dap_stream = connect("dap");
        let rpc_stream = connect("json-rpc");
        let (dap_stream, rpc_stream) = match (dap_stream, rpc_stream) {
            (Ok(d), Ok(r)) => (d, r),
            (Err(e), _) | (_, Err(e)) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        rpc_stream
            .set_read_timeout(Some(REQUEST))
            .map_err(err("json-rpc"))?;
        let rpc = DelveRpc::new(rpc_stream).map_err(err("json-rpc"))?;
        let mut session = DelveSession {
            child,
            dap: DapClient::new(dap_stream, REQUEST),
            rpc,
            debuggee: None,
            printed,
        };
        session.initialize()?;
        // Recorded while the target is stopped and Delve responsive, for teardown.
        session.debuggee = session
            .rpc
            .call("ProcessPid", json!({}))
            .ok()
            .and_then(|r| r["Pid"].as_u64())
            .and_then(|p| u32::try_from(p).ok());
        Ok(session)
    }

    fn initialize(&mut self) -> Result<(), ExecError> {
        let args = json!({
            "clientID": "cherenkov",
            "adapterID": "go",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "pathFormat": "path",
        });
        // Validate against the generated protocol types before sending.
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
        // Delve-specific attach arguments: the process is already running
        // under the headless server, stopped at entry.
        self.dap
            .request("attach", json!({"mode": "remote", "stopOnEntry": true}))
            .map_err(err("attach"))?;
        // Delve's default breakpoints on unrecovered panics and fatal throws
        // stay set: a crashing mutant stops under the debugger and is
        // reported with its message instead of taking the process down.
        Ok(())
    }

    /// Process id of the program under test.
    pub fn debuggee_pid(&self) -> u32 {
        self.debuggee.unwrap_or(0)
    }

    /// Process id of the Delve server.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl NativeSession for DelveSession {
    fn function_code(&mut self, file: &Path, line: u32) -> Result<Option<FunctionCode>, ExecError> {
        let loc = format!("{}:{line}", file.display());
        let locs = self.rpc.find_location(&loc).map_err(err("find location"))?;
        let first = locs
            .first()
            .ok_or_else(|| ExecError(format!("no code at {loc}")))?;
        let entry = first
            .function
            .as_ref()
            .ok_or_else(|| ExecError(format!("no function at {loc}")))?
            .value;
        let reported_file = first.file.clone();
        let insns = self
            .rpc
            .disassemble_function(entry)
            .map_err(err("disassemble"))?;
        let insns = insns
            .into_iter()
            .map(|i| Insn {
                pc: i.loc.pc,
                bytes: i.bytes,
                file: i.loc.file,
                line: i.loc.line,
                column: 0,
            })
            .collect();
        Ok(Some(FunctionCode {
            file: reported_file,
            insns,
        }))
    }

    fn take_output(&mut self) -> String {
        self.printed.settled(OUTPUT_BOUND)
    }

    fn read(&mut self, addr: u64, len: usize) -> Result<Vec<u8>, ExecError> {
        self.rpc.read_memory(addr, len).map_err(err("read memory"))
    }

    fn write(&mut self, addr: u64, bytes: &[u8]) -> Result<(), ExecError> {
        self.rpc
            .write_memory(addr, bytes)
            .map_err(err("write memory"))
    }

    /// Finish configuration, continue, and wait until the process exits,
    /// crashes under the debugger, or `timeout` passes; with `reach`, also
    /// tell whether execution got to any of those instructions.
    fn run_reaching(
        &mut self,
        reach: &[u64],
        timeout: Duration,
    ) -> Result<(RunEnd, bool), ExecError> {
        let mut watching = !reach.is_empty();
        if watching {
            self.instruction_breakpoints(reach)?;
        }
        self.dap
            .request("configurationDone", Value::Null)
            .map_err(err("configurationDone"))?;
        self.dap
            .request("continue", json!({"threadId": 1}))
            .map_err(err("continue"))?;
        let mut reached = false;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = match self.dap.wait_any_event(&["exited", "stopped"], left) {
                Ok(e) => e,
                Err(DapError::Timeout(_)) => return Ok((RunEnd::TimedOut, reached)),
                Err(e) => return Err(ExecError(format!("waiting for exit: {e}"))),
            };
            if event["event"] == "exited" {
                let body: ExitedEventBody =
                    serde_json::from_value(event["body"].clone()).map_err(err("exited event"))?;
                return Ok((RunEnd::Exited(body.exit_code), reached));
            }
            let body: StoppedEventBody =
                serde_json::from_value(event["body"].clone()).map_err(err("stopped event"))?;
            // A crash stops the run. The `entry` stop from attaching is still
            // queued and is skipped.
            if body.reason == "exception" {
                let kind = body
                    .description
                    .clone()
                    .unwrap_or_else(|| "exception".into());
                let end = RunEnd::Crashed {
                    kind,
                    message: self.crash_message(&body),
                };
                return Ok((end, reached));
            }
            if watching && body.reason.contains("breakpoint") {
                reached = true;
                watching = false;
                self.instruction_breakpoints(&[])?;
                let thread = body.thread_id.unwrap_or(1);
                self.dap
                    .request("continue", json!({"threadId": thread}))
                    .map_err(err("continue"))?;
            }
        }
    }
}

impl DelveSession {
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

    /// What the crash says about itself: Delve's message when it has one
    /// (`exceptionInfo`, else the stopped event's text), otherwise its
    /// goroutine stack, which locates the crash and the test that hit it.
    /// Delve cannot extract a message from Go runtime errors such as an
    /// index out of range; the stack is still reported.
    fn crash_message(&mut self, stopped: &StoppedEventBody) -> String {
        // Read fields leniently: Delve sends `"breakMode": ""`, which the
        // protocol's ExceptionBreakMode does not allow, so the generated
        // ExceptionInfoResponseBody rejects the whole body.
        let info = stopped
            .thread_id
            .and_then(|t| {
                self.dap
                    .request("exceptionInfo", json!({"threadId": t}))
                    .ok()
            })
            .unwrap_or(Value::Null);
        let text = |v: &Value| v.as_str().map(String::from);
        let description = text(&info["description"]);
        let stack = text(&info["details"]["stackTrace"]);
        [description, stopped.text.clone(), stack]
            .into_iter()
            .flatten()
            .find(|s| !s.trim().is_empty())
            .unwrap_or_default()
    }
}

/// What the program under test prints, as Delve's stdout carries it.
#[derive(Default)]
struct Printed {
    text: Mutex<String>,
    grew: Condvar,
}

/// Output going quiet for this long, with no verdict printed, is all the
/// run printed (a crash stops the program before its verdict).
const QUIET: Duration = Duration::from_millis(100);

impl Printed {
    fn lock(&self) -> MutexGuard<'_, String> {
        self.text.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&self, line: &str) {
        self.lock().push_str(line);
        self.grew.notify_all();
    }

    /// What the program printed. The run has ended, but its last lines may
    /// not have crossed the pipe yet: wait, for at most `bound`, until the
    /// verdict line (`PASS` or `FAIL`, which Go prints last) is in, or the
    /// output has been quiet for [`QUIET`]. What is returned is taken.
    fn settled(&self, bound: Duration) -> String {
        let deadline = Instant::now() + bound;
        let mut text = self.lock();
        while !has_verdict(&text) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let before = text.len();
            let (guard, _) = self
                .grew
                .wait_timeout(text, QUIET.min(left))
                .unwrap_or_else(PoisonError::into_inner);
            text = guard;
            if text.len() == before {
                break;
            }
        }
        std::mem::take(&mut *text)
    }
}

/// Whether the last line is the verdict line of a Go test binary.
fn has_verdict(text: &str) -> bool {
    matches!(text.lines().next_back(), Some("PASS" | "FAIL")) && text.ends_with('\n')
}

/// The lines of Delve's stdout, each with its newline: the banner's
/// address goes to `listening`, and what follows the banner, which is the
/// program's, to `printed`. What came before the banner is Delve's own.
fn split_stdout(
    mut reader: impl BufRead,
    mut listening: impl FnMut(String),
    mut printed: impl FnMut(&str),
) {
    let mut announced = false;
    let mut raw = Vec::new();
    loop {
        raw.clear();
        if !matches!(reader.read_until(b'\n', &mut raw), Ok(n) if n > 0) {
            return;
        }
        let line = String::from_utf8_lossy(&raw);
        match line.strip_prefix(BANNER) {
            Some(addr) if !announced => {
                announced = true;
                listening(addr.trim().to_string());
            }
            _ if announced => printed(&line),
            _ => {}
        }
    }
}

const BANNER: &str = "API server listening at: ";

/// Read Delve's stdout until it announces its listener; keep draining both
/// pipes afterwards so the test process never blocks on a full pipe. The
/// program's output after the banner is kept for [`Printed::settled`].
fn listening_address(child: &mut Child) -> Result<(String, Arc<Printed>), ExecError> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ExecError("no stdout pipe".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ExecError("no stderr pipe".into()))?;
    let (tx, rx) = mpsc::channel();
    let printed = Arc::new(Printed::default());
    let program = Arc::clone(&printed);
    thread::spawn(move || {
        let mut sent = false;
        split_stdout(
            BufReader::new(stdout),
            |addr| {
                sent = true;
                let _ = tx.send(Ok(addr));
            },
            |line| program.push(line),
        );
        if !sent {
            let _ = tx.send(Err(()));
        }
    });
    let stderr_text = Arc::new(Mutex::new(String::new()));
    let sink = Arc::clone(&stderr_text);
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Ok(mut s) = sink.lock() {
                s.push_str(&line);
                s.push('\n');
            }
        }
    });
    match rx.recv_timeout(STARTUP) {
        Ok(Ok(addr)) => Ok((addr, printed)),
        Ok(Err(())) | Err(_) => {
            thread::sleep(Duration::from_millis(100));
            let stderr = stderr_text
                .lock()
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            Err(ExecError(format!(
                "delve did not start listening: {stderr}"
            )))
        }
    }
}

impl Drop for DelveSession {
    fn drop(&mut self) {
        // Kill the debuggee through Delve first: killing only Delve would
        // detach ptrace and leave a looping mutant running. While DAP holds
        // the target in `continue` (a timed-out mutant), JSON-RPC calls,
        // `halt` included, block until the RPC timeout; a DAP `pause`
        // stops the target at once (measured), after which `Detach` returns.
        let _ = self.dap.request("pause", json!({"threadId": 1}));
        let _ = self.rpc.call("Detach", json!({"Kill": true}));
        kill_debuggee(self.debuggee);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Launcher for DelveConfig {
    type Session = DelveSession;

    fn start(&self) -> Result<DelveSession, ExecError> {
        DelveSession::start(self)
    }

    fn abi(&self) -> &'static str {
        "go"
    }

    fn binary(&self) -> Option<&Path> {
        Some(&self.test_binary)
    }

    fn tests_of(&self, output: &str) -> Vec<TestReport> {
        crate::gotest::reports(output)
    }
}

/// Go mutants under unmodified Delve, applied as tier P patches.
pub type DelveExecutor = TierPExecutor<DelveConfig>;

#[cfg(test)]
#[path = "delve.test.rs"]
mod tests;
