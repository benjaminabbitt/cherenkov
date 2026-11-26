//! Why the debugger adapter's executor failed: coded, with named data, never
//! English (the messages are `locales/en-US/messages.ftl`'s). What a tool or
//! the operating system said is carried as `detail`, text as it was said.

use coded_diagnostic::{Arg, Diagnostic, Report, code};

use crate::dap::DapError;
use crate::rpc::RpcError;

/// Why a tool the executor runs, or a file it reads, failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Toolchain {
    /// A tool did not start.
    Spawn { tool: String, detail: String },
    /// A tool ran and failed; `detail` is what it printed.
    Failed {
        tool: String,
        dir: String,
        detail: String,
    },
    /// A tool's output is not what it should be.
    Output { what: String, detail: String },
    /// A path could not be read or resolved.
    Path { path: String, detail: String },
    /// A binary could not be read or parsed.
    Binary {
        binary: String,
        step: String,
        detail: String,
    },
    /// The viability check could not run.
    Viability { detail: String },
    /// The unmutated project does not pass `cargo check`, so no mutant of
    /// it can be judged by it.
    BaselineUnchecked { detail: String },
    /// A test binary has no line table.
    NoLineTable { binary: String },
}

impl std::fmt::Display for Toolchain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl std::error::Error for Toolchain {}

impl Diagnostic for Toolchain {
    fn code(&self) -> &str {
        match self {
            Toolchain::Spawn { .. } => code!("toolchain-spawn"),
            Toolchain::Failed { .. } => code!("toolchain-failed"),
            Toolchain::Output { .. } => code!("toolchain-output"),
            Toolchain::Path { .. } => code!("toolchain-path"),
            Toolchain::Binary { .. } => code!("toolchain-binary"),
            Toolchain::Viability { .. } => code!("toolchain-viability"),
            Toolchain::BaselineUnchecked { .. } => code!("toolchain-baseline-unchecked"),
            Toolchain::NoLineTable { .. } => code!("toolchain-no-line-table"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            Toolchain::Spawn { tool, detail } => {
                vec![
                    ("tool", tool.as_str().into()),
                    ("detail", detail.as_str().into()),
                ]
            }
            Toolchain::Failed { tool, dir, detail } => vec![
                ("tool", tool.as_str().into()),
                ("dir", dir.as_str().into()),
                ("detail", detail.as_str().into()),
            ],
            Toolchain::Output { what, detail } => {
                vec![
                    ("what", what.as_str().into()),
                    ("detail", detail.as_str().into()),
                ]
            }
            Toolchain::Path { path, detail } => {
                vec![
                    ("path", path.as_str().into()),
                    ("detail", detail.as_str().into()),
                ]
            }
            Toolchain::Binary {
                binary,
                step,
                detail,
            } => vec![
                ("binary", binary.as_str().into()),
                ("step", step.as_str().into()),
                ("detail", detail.as_str().into()),
            ],
            Toolchain::Viability { detail } | Toolchain::BaselineUnchecked { detail } => {
                vec![("detail", detail.as_str().into())]
            }
            Toolchain::NoLineTable { binary } => vec![("binary", binary.as_str().into())],
        }
    }
}

/// Why a debugger session failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Session {
    /// The debugger did not start.
    Spawn { tool: String, detail: String },
    /// The debugger started but a pipe it should have is missing.
    NoPipe { pipe: String },
    /// The debugger did not announce its listener; `detail` is its stderr.
    NotListening { detail: String },
    /// A connection to the debugger failed.
    Connect {
        what: String,
        addr: String,
        detail: String,
    },
    /// A step of the session failed, for `cause`.
    Step { step: String, cause: Report },
    /// Waiting for the program's exit failed.
    Wait { detail: String },
    /// The debugger knows no code at the location.
    NoCode { at: String },
    /// The debugger knows no function at the location.
    NoFunction { at: String },
    /// The program exited but its output has no verdict after `waited`:
    /// the run's output is incomplete.
    OutputIncomplete { what: String, waited: String },
    /// The program exited but its output did not end within `waited`.
    OutputUnended { waited: String, cause: Report },
    /// A constructor of a return value is not resolved to a library.
    Unresolved { value: String, ty: String },
    /// The memory a patch replaces is not what it expected.
    PatchMismatch {
        expected: String,
        addr: String,
        found: String,
    },
    /// Memory written does not read back as written.
    WriteMismatch {
        wrote: String,
        addr: String,
        back: String,
    },
    /// The debugger cannot restart with the tests selected.
    NoRestart,
    /// The session was ended from outside (the run failed elsewhere).
    Cancelled,
}

impl std::fmt::Display for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl std::error::Error for Session {}

impl Diagnostic for Session {
    fn code(&self) -> &str {
        match self {
            Session::Spawn { .. } => code!("session-spawn"),
            Session::NoPipe { .. } => code!("session-no-pipe"),
            Session::NotListening { .. } => code!("session-not-listening"),
            Session::Connect { .. } => code!("session-connect"),
            Session::Step { .. } => code!("session-step"),
            Session::Wait { .. } => code!("session-wait"),
            Session::NoCode { .. } => code!("session-no-code"),
            Session::NoFunction { .. } => code!("session-no-function"),
            Session::OutputIncomplete { .. } => code!("session-output-incomplete"),
            Session::OutputUnended { .. } => code!("session-output-unended"),
            Session::Unresolved { .. } => code!("session-unresolved"),
            Session::PatchMismatch { .. } => code!("session-patch-mismatch"),
            Session::WriteMismatch { .. } => code!("session-write-mismatch"),
            Session::NoRestart => code!("session-no-restart"),
            Session::Cancelled => code!("session-cancelled"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        let text = |name, value: &str| (name, Arg::from(value));
        match self {
            Session::Spawn { tool, detail } => vec![text("tool", tool), text("detail", detail)],
            Session::NoPipe { pipe } => vec![text("pipe", pipe)],
            Session::NotListening { detail } | Session::Wait { detail } => {
                vec![text("detail", detail)]
            }
            Session::Connect { what, addr, detail } => {
                vec![
                    text("what", what),
                    text("addr", addr),
                    text("detail", detail),
                ]
            }
            Session::Step { step, cause } => {
                vec![text("step", step), ("cause", Arg::nested(cause))]
            }
            Session::NoCode { at } | Session::NoFunction { at } => vec![text("at", at)],
            Session::OutputIncomplete { what, waited } => {
                vec![text("what", what), text("waited", waited)]
            }
            Session::OutputUnended { waited, cause } => {
                vec![text("waited", waited), ("cause", Arg::nested(cause))]
            }
            Session::Unresolved { value, ty } => vec![text("value", value), text("ty", ty)],
            Session::PatchMismatch {
                expected,
                addr,
                found,
            } => vec![
                text("expected", expected),
                text("addr", addr),
                text("found", found),
            ],
            Session::WriteMismatch { wrote, addr, back } => {
                vec![text("wrote", wrote), text("addr", addr), text("back", back)]
            }
            Session::NoRestart | Session::Cancelled => Vec::new(),
        }
    }
}

/// Why a recording run could not choose each mutant's tests, so every
/// mutant runs every test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotRecorded {
    /// The test program does not list its tests.
    NoTestList,
    /// The recording run recorded nothing.
    NothingRecorded,
    /// The tests did not pass while recording what they call.
    TestsFailed { end: String },
    /// [`NotRecorded::TestsFailed`], and the failing tests are known.
    NamedTestsFailed { end: String, failing: String },
}

impl std::fmt::Display for NotRecorded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl Diagnostic for NotRecorded {
    fn code(&self) -> &str {
        match self {
            NotRecorded::NoTestList => code!("not-recorded-no-test-list"),
            NotRecorded::NothingRecorded => code!("not-recorded-nothing"),
            NotRecorded::TestsFailed { .. } => code!("not-recorded-tests-failed"),
            NotRecorded::NamedTestsFailed { .. } => code!("not-recorded-named-tests-failed"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            NotRecorded::NoTestList | NotRecorded::NothingRecorded => Vec::new(),
            NotRecorded::TestsFailed { end } => vec![("end", end.as_str().into())],
            NotRecorded::NamedTestsFailed { end, failing } => {
                vec![
                    ("end", end.as_str().into()),
                    ("failing", failing.as_str().into()),
                ]
            }
        }
    }
}

/// A failure the executor holds as the cause of a step: a diagnostic of the
/// adapter's own, or what a library said.
pub trait Cause {
    fn report(&self) -> Report;
}

/// A failure of a library that has no code of its own: what it said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct External {
    pub detail: String,
}

impl std::fmt::Display for External {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl Diagnostic for External {
    fn code(&self) -> &str {
        code!("external")
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        vec![("detail", self.detail.as_str().into())]
    }
}

macro_rules! external_cause {
    ($($t:ty),*) => {
        $(impl Cause for $t {
            fn report(&self) -> Report {
                Report::of(&External { detail: self.to_string() })
            }
        })*
    };
}

external_cause!(
    serde_json::Error,
    std::num::ParseIntError,
    std::io::Error,
    base64::DecodeError
);

impl Cause for DapError {
    fn report(&self) -> Report {
        Report::of(self)
    }
}

impl Cause for RpcError {
    fn report(&self) -> Report {
        Report::of(self)
    }
}

/// One value of every error, for the catalog test.
#[cfg(test)]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let s = || "s".to_string();
    let cause = || Report::of(&DapError::Closed);
    vec![
        Box::new(Toolchain::Spawn {
            tool: s(),
            detail: s(),
        }),
        Box::new(Toolchain::Failed {
            tool: s(),
            dir: s(),
            detail: s(),
        }),
        Box::new(Toolchain::Output {
            what: s(),
            detail: s(),
        }),
        Box::new(Toolchain::Path {
            path: s(),
            detail: s(),
        }),
        Box::new(Toolchain::Binary {
            binary: s(),
            step: s(),
            detail: s(),
        }),
        Box::new(Toolchain::Viability { detail: s() }),
        Box::new(Toolchain::BaselineUnchecked { detail: s() }),
        Box::new(Toolchain::NoLineTable { binary: s() }),
        Box::new(Session::Spawn {
            tool: s(),
            detail: s(),
        }),
        Box::new(Session::NoPipe { pipe: s() }),
        Box::new(Session::NotListening { detail: s() }),
        Box::new(Session::Connect {
            what: s(),
            addr: s(),
            detail: s(),
        }),
        Box::new(Session::Step {
            step: s(),
            cause: cause(),
        }),
        Box::new(Session::Wait { detail: s() }),
        Box::new(Session::NoCode { at: s() }),
        Box::new(Session::NoFunction { at: s() }),
        Box::new(Session::OutputIncomplete {
            what: s(),
            waited: s(),
        }),
        Box::new(Session::OutputUnended {
            waited: s(),
            cause: cause(),
        }),
        Box::new(Session::Unresolved {
            value: s(),
            ty: s(),
        }),
        Box::new(Session::PatchMismatch {
            expected: s(),
            addr: s(),
            found: s(),
        }),
        Box::new(Session::WriteMismatch {
            wrote: s(),
            addr: s(),
            back: s(),
        }),
        Box::new(Session::NoRestart),
        Box::new(Session::Cancelled),
        Box::new(NotRecorded::NoTestList),
        Box::new(NotRecorded::NothingRecorded),
        Box::new(NotRecorded::TestsFailed { end: s() }),
        Box::new(NotRecorded::NamedTestsFailed {
            end: s(),
            failing: s(),
        }),
        Box::new(External { detail: s() }),
        Box::new(DapError::Io(s())),
        Box::new(DapError::Protocol(s())),
        Box::new(DapError::Failed {
            command: s(),
            message: s(),
        }),
        Box::new(DapError::Timeout(s())),
        Box::new(DapError::Closed),
        Box::new(RpcError::Io(s())),
        Box::new(RpcError::Protocol(s())),
        Box::new(RpcError::Call {
            method: s(),
            message: s(),
        }),
    ]
}
