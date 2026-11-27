//! What a running cargo says of itself: each crate it finishes compiling,
//! reported as it happens so that a long cold compile is visibly alive.

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// Crates cargo has finished compiling: how many, and the latest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiling {
    pub done: usize,
    pub last: String,
}

/// The least time between two notices: a compile of many small crates
/// would otherwise say one line each. The first crate is always said.
pub const NOTICE_INTERVAL: Duration = Duration::from_secs(1);

/// What `cargo` printed, as `Command::output` would give it, but with
/// `compiling` told of each crate as its `compiler-artifact` line arrives
/// (at most one notice per `interval`, the first always). The stdout and
/// stderr returned are exactly what the process wrote.
pub(crate) fn output_streaming(
    cargo: &mut Command,
    interval: Duration,
    compiling: &mut dyn FnMut(Compiling),
) -> std::io::Result<Output> {
    let mut child = cargo
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stderr = child.stderr.take().expect("stderr is piped");
    // Drained beside stdout, or a full pipe would stall cargo.
    let errors = std::thread::spawn(move || {
        let mut text = Vec::new();
        stderr.read_to_end(&mut text).map(|_| text)
    });
    let stdout = read_lines(
        child.stdout.take().expect("stdout is piped"),
        interval,
        compiling,
    );
    let status = child.wait()?;
    let stderr = errors.join().expect("the stderr reader does not panic")?;
    Ok(Output {
        status,
        stdout: stdout?,
        stderr,
    })
}

/// Everything `source` yields, its artifact lines told to `compiling`.
fn read_lines(
    source: impl Read,
    interval: Duration,
    compiling: &mut dyn FnMut(Compiling),
) -> std::io::Result<Vec<u8>> {
    let mut lines = BufReader::new(source);
    let mut all = Vec::new();
    let mut seen = Counter::new(interval);
    let mut line = Vec::new();
    loop {
        line.clear();
        if lines.read_until(b'\n', &mut line)? == 0 {
            return Ok(all);
        }
        let crate_done = artifact_of(&line).and_then(|name| seen.crate_done(name, Instant::now()));
        if let Some(said) = crate_done {
            compiling(said);
        }
        all.extend_from_slice(&line);
    }
}

/// The crate a `compiler-artifact` line of cargo's JSON messages is for.
fn artifact_of(line: &[u8]) -> Option<String> {
    let message: Value = serde_json::from_slice(line).ok()?;
    (message["reason"] == "compiler-artifact").then(|| {
        message["target"]["name"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    })
}

/// The running count of crates, and when one was last said.
struct Counter {
    interval: Duration,
    done: usize,
    said_at: Option<Instant>,
}

impl Counter {
    fn new(interval: Duration) -> Counter {
        Counter {
            interval,
            done: 0,
            said_at: None,
        }
    }

    /// Count `name` as compiled at `now`; what to say of it, if anything.
    fn crate_done(&mut self, name: String, now: Instant) -> Option<Compiling> {
        self.done += 1;
        let due = self
            .said_at
            .is_none_or(|at| now.saturating_duration_since(at) >= self.interval);
        due.then(|| {
            self.said_at = Some(now);
            Compiling {
                done: self.done,
                last: name,
            }
        })
    }
}

#[cfg(test)]
#[path = "progress.test.rs"]
mod tests;
