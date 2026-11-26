//! What every native debugger driver provides for tier P, and the one
//! executor written over it. Drivers (Delve, GDB) differ only in how they
//! start a session and implement these primitives.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::cancel::Watch;
use crate::error::{NotRecorded, Session, Toolchain};
use cherenkov_mutation::edit::Edit;
use cherenkov_mutation::execution::{Budget, Observed, RawOutcome, TestReport, Tier};
use cherenkov_mutation::plan::{Action, MutantPlan};
use cherenkov_mutation::syntax::Pos;
use cherenkov_mutation::token::Token;
use cherenkov_run_ports::executor::{
    Cancel, Driven, ExecError, MutantExecutor, Recording, Selection,
};
use coded_diagnostic::Report;

use crate::isa::Isa;
use crate::reason::Reason;
use crate::tierp::{Insn, Patch, Unpatchable, derive, derive_returning};
use crate::unbuildable::Unbuildable;

/// One function's instructions, with the path its debug info reports for
/// the requested source file (instructions from other files are inlined code).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionCode {
    pub file: String,
    pub insns: Vec<Insn>,
}

/// How a run under a debugger ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEnd {
    Exited(i32),
    /// The debugger stopped the process on a crash (Go: unrecovered panic
    /// or fatal error; native: a fatal signal) before it could exit.
    Crashed {
        kind: String,
        message: String,
    },
    TimedOut,
}

/// What a tier B run does each time execution reaches the mutant's stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Evaluate `value`, store it in `target`, and resume after the statement.
    Assign { target: String, value: String },
    /// Evaluate `value` and return it from the current function.
    Return { value: String },
    /// Return this value as the current function's result: in the integer
    /// return registers, or through the caller's result pointer. Its heap
    /// blocks are allocated in the program first.
    ReturnBuilt {
        value: crate::layouts::Built,
        in_registers: bool,
    },
    /// Return the value the support library's constructor for `request`
    /// makes: resolved (the library built, its layout of the type checked
    /// against `layout`, the program's) into [`Effect::ReturnBuilt`]
    /// before the run.
    ReturnConstructed {
        request: crate::support_lib::Request,
        layout: crate::layouts::Layout,
        in_registers: bool,
        /// The value built from its layout, for when the library cannot
        /// make it.
        fallback: Option<Box<crate::layouts::Built>>,
    },
    /// Flip the boolean in `al` (a call's result) at every hit.
    FlipResult,
    /// At each jump out of a swapped `&&`/`||`'s left operand, read from
    /// the flags which successor the program takes and continue where the
    /// mutant goes from it (storing a constant first when the operator's
    /// result is a value). Nothing is evaluated.
    Steer { steers: Vec<crate::tierp::Steer> },
    /// Evaluate `condition` and continue at `if_true` or `if_false`, the
    /// exits of the condition's code (its own code is skipped).
    Branch {
        condition: String,
        if_true: u64,
        if_false: u64,
    },
    /// `inner`, unless the mutated operation overflows its integer type:
    /// then the program goes to the compiled operation's overflow panic, as
    /// the mutated source's checked operation would (the debugger's own
    /// arithmetic wraps).
    Guarded {
        guard: OverflowGuard,
        inner: Box<Effect>,
    },
    /// The copies of a generic function each have their own stops and
    /// their own effect (a layout, a jump's exits): at a stop, the effect
    /// of the copy it belongs to.
    PerStop(Vec<(Vec<u64>, Effect)>),
}

/// A mutated checked operation: its operands' source text, the operator,
/// and where the compiled operation panics on overflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverflowGuard {
    pub lhs: String,
    pub rhs: String,
    pub op: &'static str,
    pub panic: u64,
}

/// How a tier B run ended: as a run does, whether the stop was reached, and
/// the evaluator's complaint if it could not apply the effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluated {
    pub end: RunEnd,
    pub reached: bool,
    pub refused: Option<Reason>,
}

/// How a recording run ended, and who called each function entry it hit.
/// A recording run: how it ended, which tests called each entry hit, and
/// which reach addresses any code executed.
pub type Recorded = (RunEnd, Vec<(u64, Driven)>, Vec<u64>);

/// A debuggee stopped before running any test code.
pub trait NativeSession {
    /// Replace the program with a fresh process in this session, ready for
    /// the next run. `false` when the debugger cannot: start a new session.
    fn restart(&mut self) -> Result<bool, ExecError> {
        Ok(false)
    }

    /// Tier B: run with a breakpoint at each of `stops`, applying `effect`
    /// at every hit. `None` when the debugger has no evaluator for it.
    fn run_evaluating(
        &mut self,
        _stops: &[u64],
        _effect: &Effect,
        _timeout: Duration,
    ) -> Result<Option<Evaluated>, ExecError> {
        Ok(None)
    }

    /// The instructions of the function containing `file:line`, or `None`
    /// when this program has no code for that line.
    fn function_code(&mut self, file: &Path, line: u32) -> Result<Option<FunctionCode>, ExecError>;
    /// Every function with code on the line, first [`NativeSession::function_code`]'s
    /// (a closure's body shares its line with the function around it).
    /// `line` may resolve to a later line up to `last` when it has no code
    /// of its own (the mutated site's last line).
    fn function_codes(
        &mut self,
        file: &Path,
        line: u32,
        _last: u32,
    ) -> Result<Vec<FunctionCode>, ExecError> {
        Ok(self.function_code(file, line)?.into_iter().collect())
    }
    fn read(&mut self, addr: u64, len: usize) -> Result<Vec<u8>, ExecError>;
    fn write(&mut self, addr: u64, bytes: &[u8]) -> Result<(), ExecError>;
    /// Let the program run until it exits, crashes, or `timeout` passes.
    fn run_to_exit(&mut self, timeout: Duration) -> Result<RunEnd, ExecError> {
        self.run_reaching(&[], timeout).map(|(end, _)| end)
    }

    /// As [`NativeSession::run_to_exit`], also telling whether execution
    /// reached any of the instructions at `reach` (the mutated code): an
    /// instruction breakpoint on each, cleared at the first hit.
    fn run_reaching(
        &mut self,
        reach: &[u64],
        timeout: Duration,
    ) -> Result<(RunEnd, bool), ExecError>;

    /// Run to exit recording, for each of `entries` (function entries),
    /// which of `tests` (the binary's test names) call it: the code the
    /// selected tests drive. Entries never hit are left out. Also which of
    /// `reach` (mutated code) anything executes. `None` when this debugger
    /// cannot tell.
    fn run_recording(
        &mut self,
        _entries: &[u64],
        _reach: &[u64],
        _tests: &[String],
        _timeout: Duration,
    ) -> Result<Option<Recorded>, ExecError> {
        Ok(None)
    }

    /// Run only these tests from the next restart on (`None`: the
    /// configured ones). `false` when this debugger cannot select.
    fn select_tests(&mut self, _tests: Option<&[String]>) -> Result<bool, ExecError> {
        Ok(false)
    }

    /// How much of the last run's wall time went to waiting for a CPU
    /// (machine load), when the debugger can tell. A run is judged on its
    /// wall time less this.
    fn lost_to_load(&self) -> Duration {
        Duration::ZERO
    }

    /// The program's standard output so far, when the debugger forwards it.
    fn take_output(&mut self) -> String {
        String::new()
    }

    /// What the program printed since the last [`NativeSession::take_output`],
    /// as a person reads it to diagnose a failure (every stream the
    /// debugger forwards, in arrival order); empty when the session has
    /// nothing beyond what `take_output` gave. Nothing decides from it.
    fn take_diagnostic_output(&mut self) -> String {
        String::new()
    }

    /// Apply a patch. Fails, writing nothing, unless memory still holds
    /// every region's `original`.
    fn arm(&mut self, patch: &Patch) -> Result<(), ExecError> {
        for r in &patch.regions {
            let current = self.read(r.addr, r.original.len())?;
            if current != r.original {
                return Err(ExecError::of(&Session::PatchMismatch {
                    expected: format!("{:02x?}", r.original),
                    addr: format!("{:#x}", r.addr),
                    found: format!("{current:02x?}"),
                }));
            }
        }
        for r in &patch.regions {
            write_verified(self, r.addr, &r.replacement)?;
        }
        Ok(())
    }

    /// Restore every region's `original` and verify it reads back byte-identical.
    fn disarm(&mut self, patch: &Patch) -> Result<(), ExecError> {
        for r in &patch.regions {
            write_verified(self, r.addr, &r.original)?;
        }
        Ok(())
    }
}

fn write_verified<S: NativeSession + ?Sized>(
    session: &mut S,
    addr: u64,
    bytes: &[u8],
) -> Result<(), ExecError> {
    session.write(addr, bytes)?;
    let back = session.read(addr, bytes.len())?;
    if back != bytes {
        return Err(ExecError::of(&Session::WriteMismatch {
            wrote: format!("{bytes:02x?}"),
            addr: format!("{addr:#x}"),
            back: format!("{back:02x?}"),
        }));
    }
    Ok(())
}

/// Kill a debuggee if it still runs. A debugger's own teardown can release
/// a running debuggee without killing it; a timed-out mutant would then loop
/// on with no parent.
pub(crate) fn kill_debuggee(pid: Option<u32>) {
    let pid = pid
        .and_then(|p| i32::try_from(p).ok())
        .and_then(rustix::process::Pid::from_raw);
    if let Some(pid) = pid {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
    }
}

/// Starts a fresh session for each run.
pub trait Launcher {
    type Session: NativeSession;
    /// `watch` is told the debugger's process, so that a cancel can end it.
    fn start(&self, watch: &Arc<Watch>) -> Result<Self::Session, ExecError>;
    /// The calling convention of the programs it runs, a key of the ISA
    /// table's `return_stub`.
    fn abi(&self) -> &'static str;
    /// The program it runs, whose DWARF line table gives instructions their
    /// columns.
    fn binary(&self) -> Option<&Path> {
        None
    }
    /// The tests the program's output reports, with how each failed.
    /// None when the launcher cannot read its program's output.
    fn tests_of(&self, _output: &str) -> Vec<TestReport> {
        Vec::new()
    }
}

/// The facts of a tier P run. A run past the budget is a timeout even if
/// its end was already queued when the budget was checked (design section 7).
pub fn outcome_of(end: RunEnd, wall: Duration, budget: Budget) -> RawOutcome {
    outcome_in(Tier::P, end, wall, budget)
}

/// [`outcome_of`] for a run under `tier`.
pub fn outcome_in(tier: Tier, end: RunEnd, wall: Duration, budget: Budget) -> RawOutcome {
    outcome_judged(tier, end, wall, wall, budget)
}

/// [`outcome_in`] for a run that took `wall` and is judged on `judged`: the
/// wall time less what machine load took from it, so that a busy machine
/// makes no mutant a timeout.
pub fn outcome_judged(
    tier: Tier,
    end: RunEnd,
    wall: Duration,
    judged: Duration,
    budget: Budget,
) -> RawOutcome {
    if spent(judged, budget) {
        return RawOutcome::TimedOut { tier, wall };
    }
    match end {
        RunEnd::Exited(code) => RawOutcome::Exited { code, tier, wall },
        RunEnd::Crashed { kind, message } => RawOutcome::Crashed {
            kind,
            message,
            tier,
            wall,
        },
        RunEnd::TimedOut => RawOutcome::TimedOut { tier, wall },
    }
}

/// True once a run judged on `judged` has used more than its budget.
fn spent(judged: Duration, budget: Budget) -> bool {
    judged > budget.timeout
}

/// A tier B run the evaluator refused: unsupported, unless the run had used
/// its whole budget by then, when it is a timeout like any run that ran out
/// of time (a hot site's stop per hit cannot finish), never a refusal.
fn refusal_outcome(why: Report, wall: Duration, judged: Duration, budget: Budget) -> RawOutcome {
    if spent(judged, budget) {
        RawOutcome::TimedOut {
            tier: Tier::B,
            wall,
        }
    } else {
        RawOutcome::Unsupported { why }
    }
}

/// A mutant refused at tier B, for `cause`, with no tier P patch to apply
/// instead, for `tier_p`.
fn unappliable(cause: Report, tier_p: &Unpatchable) -> RawOutcome {
    RawOutcome::Unsupported {
        why: Report::of(&Reason::Unappliable {
            cause,
            tier_p: Report::of(tier_p),
        }),
    }
}

/// The mutated code never ran: the selected tests do not execute it.
fn never_executed() -> RawOutcome {
    RawOutcome::NotReached {
        why: Report::of(&Reason::NeverExecuted),
    }
}

/// A generic function's copies, one of which has no patch for `cause`.
fn generic_copies(cause: &Unpatchable) -> Report {
    Report::of(&Reason::GenericCopies {
        cause: Report::of(cause),
    })
}

/// A run's outcome, judged on its wall time less what the session says
/// machine load took from it.
fn judge<S: NativeSession>(
    tier: Tier,
    end: RunEnd,
    wall: Duration,
    session: &S,
    budget: Budget,
) -> RawOutcome {
    let judged = wall.saturating_sub(session.lost_to_load());
    outcome_judged(tier, end, wall, judged, budget)
}

/// Whether a result is an integer wider than one register (`i128`).
fn is_wide(result: Option<&crate::layouts::Layout>) -> bool {
    matches!(result, Some(crate::layouts::Layout::Int { size, .. }) if *size > 8)
}

/// Mutants applied as tier P patches, one fresh debuggee per run.
pub struct TierPExecutor<L: Launcher> {
    launcher: L,
    isa: Isa,
    /// The binary's line-table rows, read once.
    rows: std::sync::OnceLock<Vec<crate::lines::Row>>,
    /// The program's load offset: run-time address minus file address.
    bias: std::sync::OnceLock<u64>,
    /// The binary's result types, read once (when a value must be built).
    layouts: std::sync::OnceLock<Option<crate::layouts::Layouts>>,
    /// Where the plans' root lies below the build's root.
    source_prefix: PathBuf,
    /// The session kept between runs, restarted with a fresh process each
    /// time (GDB and the program's symbols load once).
    session: Option<L::Session>,
    /// The tests the next runs are narrowed to; `None` runs the configured
    /// ones.
    selected: Option<Vec<String>>,
    /// What the project's source says about its types' defaults.
    default_facts: cherenkov_mutation::types::DefaultFacts,
    /// The rustup toolchain the program is built with, for the support
    /// library; `None` builds it with whatever `cargo` is.
    toolchain: Option<String>,
    /// The directory the plans' paths are relative to, where their source
    /// can be read to check a replacement value's names; `None` checks none.
    scope_root: Option<PathBuf>,
    /// Which names of a value, at a byte of a source text, do not resolve.
    unresolved: Option<UnresolvedNames>,
    /// Shared with the sessions and the canceller: ends them from outside.
    watch: Arc<Watch>,
}

/// The syntax layer's check of a replacement value's names: the source, the
/// byte of the mutated function, the value; the names that do not resolve.
pub type UnresolvedNames = fn(&str, usize, &str) -> Vec<String>;

impl<L: Launcher> TierPExecutor<L> {
    /// Uses `isa`'s return stub for the launcher's calling convention; if
    /// the table has none, body replacements are reported unpatchable.
    pub fn new(launcher: L, isa: Isa) -> Self {
        let isa = isa.with_abi(launcher.abi()).unwrap_or(isa);
        TierPExecutor {
            launcher,
            isa,
            rows: std::sync::OnceLock::new(),
            bias: std::sync::OnceLock::new(),
            layouts: std::sync::OnceLock::new(),
            source_prefix: PathBuf::new(),
            session: None,
            selected: None,
            default_facts: Default::default(),
            toolchain: None,
            scope_root: None,
            unresolved: None,
            watch: Arc::default(),
        }
    }

    fn timed_run(session: &mut L::Session, budget: Budget) -> Result<RawOutcome, ExecError> {
        let start = Instant::now();
        let end = session.run_to_exit(budget.timeout)?;
        // The baseline sets every mutant's budget: its time is the time it
        // ran, not the time a busy machine kept it waiting.
        let ran = start.elapsed().saturating_sub(session.lost_to_load());
        Ok(outcome_of(end, ran, budget))
    }

    /// The plan's path as the compiler recorded it: relative to the build's
    /// root (a Cargo workspace root, when the plan's root is a member below
    /// it). Debuggers match it by trailing components, so it still finds code
    /// in a binary whose debug info names another checkout of the same tree
    /// (copied target directories, CI caches); an absolute path would not.
    fn source(&self, plan: &MutantPlan) -> PathBuf {
        self.source_prefix.join(&plan.file)
    }

    /// Plans' paths are relative to `prefix` below the build's root.
    pub fn with_source_prefix(mut self, prefix: impl Into<PathBuf>) -> Self {
        self.source_prefix = prefix.into();
        self
    }

    /// The project's source facts about its types' defaults, for values
    /// the binary has no code to make.
    pub fn with_default_facts(mut self, facts: cherenkov_mutation::types::DefaultFacts) -> Self {
        self.default_facts = facts;
        self
    }

    /// Read the plans' sources below `root` (the directory their paths are
    /// relative to), so a replacement value naming something the source
    /// cannot resolve is decided unviable, without a debugger.
    pub fn with_scope_check(mut self, root: impl Into<PathBuf>, check: UnresolvedNames) -> Self {
        self.scope_root = Some(root.into());
        self.unresolved = Some(check);
        self
    }

    /// The unresolvable names of a body replacement's value, when the
    /// mutated source would not compile with them.
    fn unresolved_value(&self, plan: &MutantPlan) -> Option<Reason> {
        let Action::ReturnDefault {
            value_text: Some(v),
            ..
        } = &plan.action
        else {
            return None;
        };
        let source = std::fs::read_to_string(self.scope_root.as_ref()?.join(&plan.file)).ok()?;
        let names = self.unresolved?(&source, plan.site.start.byte, v);
        (!names.is_empty()).then(|| Reason::UnresolvedInSource {
            value: v.clone(),
            names: names
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", "),
        })
    }

    /// Build the support library with this rustup toolchain, the
    /// program's own.
    pub fn with_toolchain(mut self, toolchain: Option<String>) -> Self {
        self.toolchain = toolchain;
        self
    }
}

/// What tier B knows of the program under test.
struct Program<'a> {
    isa: &'a Isa,
    /// Its result types, when its debug info could be read.
    layouts: Option<&'a crate::layouts::Layouts>,
    /// Its load offset (run-time minus file address).
    bias: u64,
    /// What the project's source says about its types' defaults.
    facts: &'a cherenkov_mutation::types::DefaultFacts,
    /// Its rustup toolchain, which builds the support library.
    toolchain: Option<&'a str>,
}

/// The tier B effect of a plan, when its action is one the evaluator can
/// carry out: an assignment, or a value to return.
pub fn effect_of(plan: &MutantPlan) -> Option<Effect> {
    action_effect(plan).or_else(|| crate::tierp::negatable(plan).then_some(Effect::FlipResult))
}

/// Whether any part of a built value is made by the program's own code.
fn calls_program(built: &crate::layouts::Built) -> bool {
    !built.calls.is_empty() || built.heap.iter().any(|(_, b)| calls_program(b))
}

/// A body replacement in an `async fn`.
fn is_async_fn(plan: &MutantPlan) -> bool {
    matches!(plan.action, Action::ReturnDefault { async_fn: true, .. })
}

/// The effect a plan's action asks for, apart from negating a call's result.
fn action_effect(plan: &MutantPlan) -> Option<Effect> {
    match &plan.action {
        Action::EvalThenJump { target, value, .. } => Some(Effect::Assign {
            target: target.clone(),
            value: value.clone(),
        }),
        Action::EvalReturn { value_text } => Some(Effect::Return {
            value: value_text.clone(),
        }),
        Action::ReturnDefault {
            value_text: Some(v),
            ..
        } => Some(Effect::Return { value: v.clone() }),
        // The exits are found in the function's code (tier_b).
        Action::EvalBranch { cond_text, .. } => Some(Effect::Branch {
            condition: cond_text.clone(),
            if_true: 0,
            if_false: 0,
        }),
        _ => None,
    }
}

/// The stops and effect a mutant starts from: a negated boolean from a
/// call is flipped where the call returns (so nothing is evaluated and how
/// the value is used does not matter); any other is its action's effect.
fn initial_effect(
    plan: &MutantPlan,
    code: &FunctionCode,
    tier_p: &Unpatchable,
) -> Result<(Vec<u64>, Effect), RawOutcome> {
    let flip = crate::tierp::negatable(plan)
        .then(|| crate::tierp::flip_after_call(plan, &code.file, &code.insns).ok())
        .flatten();
    match (flip, action_effect(plan)) {
        (Some(at), _) => Ok((vec![at], Effect::FlipResult)),
        (None, Some(e)) => Ok((Vec::new(), e)),
        (None, None) => Err(unappliable(Report::of(&Reason::NegatedNotFromCall), tier_p)),
    }
}

/// A body replaced by a value of a compound type: built from its layout,
/// since GDB cannot return those; or, where that needs the program's code
/// or heap blocks, made by the support library's compiled constructor (a
/// std type, named by its public path). `None` for any other mutant, and
/// for a pointer no constructor makes (GDB evaluates it).
fn constructed_effect(
    plan: &MutantPlan,
    code: &FunctionCode,
    program: &Program<'_>,
    tier_p: &Unpatchable,
) -> Result<Option<Effect>, RawOutcome> {
    let (
        Action::ReturnDefault {
            value_text: Some(v),
            ..
        },
        Some(layouts),
        Some(entry),
    ) = (
        &plan.action,
        program.layouts,
        code.insns.iter().map(|i| i.pc).min(),
    )
    else {
        return Ok(None);
    };
    let Some(layout) = layouts.result_of(entry.wrapping_sub(program.bias)) else {
        return Ok(None);
    };
    if !matches!(
        layout,
        crate::layouts::Layout::Struct { .. }
            | crate::layouts::Layout::Enum { .. }
            | crate::layouts::Layout::Int { size: 16, .. }
            | crate::layouts::Layout::Pointer { .. }
    ) {
        return Ok(None);
    }
    // A small value holding a float may come back in xmm registers under
    // Rust's ABI, which this builder does not write.
    if layout.has_float() && layout.size() <= 16 {
        return Err(unappliable(
            Report::of(&Reason::FloatResult {
                bytes: layout.size(),
            }),
            tier_p,
        ));
    }
    let entry = entry.wrapping_sub(program.bias);
    // An async fn's body runs in a future's poll: the mutated fn's future
    // is ready, with the value, on its first poll.
    let v = if is_async_fn(plan) && layouts.is_future_poll(entry) {
        format!("Poll::Ready({v})")
    } else {
        v.clone()
    };
    compound_effect(layouts, layout, &v, entry, program, tier_p)
}

/// The effect that returns `v` of the compound `layout` from the function
/// at `entry` (a file address): built bytes, or a compiled constructor.
fn compound_effect(
    layouts: &crate::layouts::Layouts,
    layout: crate::layouts::Layout,
    v: &str,
    entry: u64,
    program: &Program<'_>,
    tier_p: &Unpatchable,
) -> Result<Option<Effect>, RawOutcome> {
    use crate::layouts::{Layout, Refusal};
    let bias = program.bias;
    let pointer = matches!(layout, Layout::Pointer { .. });
    let built = if pointer {
        Err(Refusal::unsupported(Unbuildable::NeedsProgramCode {
            value: v.to_string(),
            name: "a pointer".to_string(),
        }))
    } else {
        crate::layouts::build_value_with(&layout, v, program.facts)
    };
    let needs_code = match &built {
        Ok(b) => !b.heap.is_empty() || !b.calls.is_empty(),
        Err(Refusal::Unsupported(_)) => true,
        Err(Refusal::Unviable(_)) => false,
    };
    let named = needs_code
        .then(|| layouts.result_type_name(entry))
        .flatten()
        .map(|n| crate::support_lib::request_in(&n, v, layouts, program.facts))
        // Parts only the program's own code makes (a hand-written
        // Default) stay its code's: a mirror has no code of its own.
        .filter(|r| match (r, &built) {
            (Ok(request), Ok(b)) => request.mirrors.is_empty() || !calls_program(b),
            _ => true,
        });
    match (named, built) {
        (Some(Ok(request)), built) => {
            crate::support_lib::request(program.toolchain, &request);
            Ok(Some(Effect::ReturnConstructed {
                request,
                in_registers: layout.in_registers(),
                layout,
                fallback: built.ok().map(|b| Box::new(b.rebased(bias))),
            }))
        }
        // A pointer no constructor makes: as GDB evaluates it.
        (_, Err(_)) if pointer => Ok(None),
        (_, Ok(value)) => Ok(Some(Effect::ReturnBuilt {
            in_registers: layout.in_registers(),
            value: value.rebased(bias),
        })),
        (_, Err(Refusal::Unviable(why))) => Err(RawOutcome::Unviable { why }),
        (Some(Err(why)), Err(e)) => Err(unappliable(
            Report::of(&Reason::NoConstructor {
                built: e.report().clone(),
                constructor: why.report().clone(),
            }),
            tier_p,
        )),
        (None, Err(why)) => Err(unappliable(why.report().clone(), tier_p)),
    }
}

/// A swapped `&&` or `||`, in any context: steered at the program's own
/// jumps on the left operand, so operands that call methods need no
/// evaluation under the debugger. `None` when `effect` is not one it
/// steers.
fn logical_steering(
    plan: &MutantPlan,
    code: &FunctionCode,
    effect: &Effect,
) -> Option<Result<Vec<crate::tierp::Steer>, Unpatchable>> {
    (crate::tierp::is_logical_swap(plan)
        && matches!(
            effect,
            Effect::Branch { .. } | Effect::Assign { .. } | Effect::Return { .. }
        ))
    .then(|| crate::tierp::swap_logical(plan, &code.file, &code.insns))
}

/// The arithmetic operator a swap puts in, when rustc checks it.
fn checked_operator(edit: &Edit) -> Option<&'static str> {
    match edit {
        Edit::ReplaceToken {
            with: Token::Add, ..
        } => Some("+"),
        Edit::ReplaceToken {
            with: Token::Sub, ..
        } => Some("-"),
        Edit::ReplaceToken {
            with: Token::Mul, ..
        } => Some("*"),
        _ => None,
    }
}

/// A swapped arithmetic operator that rustc checks: guarded, so an
/// overflow panics as the mutated source would.
fn overflow_guard(
    plan: &MutantPlan,
    code: &FunctionCode,
    effect: &Effect,
) -> Option<OverflowGuard> {
    let (Some(op), [lhs, rhs]) = (checked_operator(&plan.edit), &plan.operand_texts[..]) else {
        return None;
    };
    if !matches!(
        effect,
        Effect::Assign { .. } | Effect::Return { .. } | Effect::Branch { .. }
    ) {
        return None;
    }
    let panic = crate::tierp::overflow_panic(plan, &code.file, &code.insns)?;
    Some(OverflowGuard {
        lhs: lhs.clone(),
        rhs: rhs.clone(),
        op,
        panic,
    })
}

/// Where a mutant that no other stop serves stops. A body replacement
/// stops at the function's true entry (so the frame is the function's own,
/// not an inlined callee's); the others at the first instruction of their
/// statement: positioned at or after its column, since a line can hold more
/// than one statement (`if f { s = s + 2; }`). The line's first only without
/// columns.
fn statement_stop(action: &Action, stop_at: Pos, insns: &[Insn]) -> Option<u64> {
    let at = (stop_at.line, stop_at.col);
    let on_line = || insns.iter().filter(|i| i.line == at.0);
    match action {
        Action::ReturnDefault { .. } => insns.iter().map(|i| i.pc).min(),
        _ if on_line().any(|i| i.column != 0) => on_line()
            .filter(|i| i.column != 0 && (i.line, i.column) >= at)
            .map(|i| i.pc)
            .min(),
        _ => on_line().map(|i| i.pc).min(),
    }
}

impl<L: Launcher> TierPExecutor<L> {
    /// The binary's line-table rows: read once they have been read, and
    /// asked for again after a read that failed (a busy machine fails reads
    /// a quiet one does not, and no column is a verdict on the mutant).
    fn line_rows(&self) -> &[crate::lines::Row] {
        if let Some(rows) = self.rows.get() {
            return rows;
        }
        match self.launcher.binary().map(crate::lines::rows) {
            Some(Ok(rows)) => self.rows.get_or_init(|| rows),
            _ => &[],
        }
    }

    /// Give each instruction the column of the line-table row it falls in.
    /// Rows hold file addresses; a position-independent program runs at a
    /// load offset, found by aligning the function's instructions with rows
    /// of the same lines. Without a readable binary or a consistent offset
    /// the columns stay 0 (whole lines).
    fn with_columns(&self, mut code: FunctionCode) -> FunctionCode {
        let rows = self.line_rows();
        let row_at = |addr: u64| {
            let at = rows.partition_point(|r| r.addr <= addr);
            at.checked_sub(1).map(|k| &rows[k])
        };
        let Some(first) = code.insns.iter().min_by_key(|i| i.pc) else {
            return code;
        };
        // Rows of the function's own file: inlined code from other files has
        // rows of its own, under lines that are not GDB's for that address.
        let tail = |path: &str| {
            let mut parts = path.rsplit('/');
            (
                parts.next().unwrap_or("").to_string(),
                parts.next().unwrap_or("").to_string(),
            )
        };
        let own = tail(&code.file);
        let ours = |r: &crate::lines::Row| tail(&r.file) == own;
        let agrees = |bias: u64| {
            let (mut yes, mut all) = (0usize, 0usize);
            for i in &code.insns {
                if let Some(r) = row_at(i.pc.wrapping_sub(bias)).filter(|r| ours(r)) {
                    all += 1;
                    // Line 0 rows are compiler-generated code with no source
                    // position, which GDB reports under the line before.
                    if r.line == i.line || r.line == 0 {
                        yes += 1;
                    }
                }
            }
            (yes, all)
        };
        // One load offset for the whole program: found once, by the first
        // instruction's rows at the same address within a page.
        let bias = match self.bias.get() {
            Some(&b) => b,
            None => {
                let best = rows
                    .iter()
                    .filter(|r| {
                        r.line == first.line && r.addr & 0xfff == first.pc & 0xfff && ours(r)
                    })
                    .map(|r| first.pc.wrapping_sub(r.addr))
                    .max_by_key(|&b| agrees(b).0);
                let Some(b) = best.filter(|&b| {
                    let (yes, all) = agrees(b);
                    all >= 3 && yes * 10 >= all * 9
                }) else {
                    return code;
                };
                let _ = self.bias.set(b);
                b
            }
        };
        for insn in &mut code.insns {
            if let Some(row) = row_at(insn.pc.wrapping_sub(bias)).filter(|r| ours(r))
                && row.line == insn.line
            {
                insn.column = row.column;
            }
        }
        code
    }

    /// Tier B: stop where the plan says, apply its effect through the
    /// debugger's evaluator, run on. `tier_p` is why no patch exists.
    /// Tier B's plan for a mutant: where it stops, what it does there, and
    /// why its `&&`/`||` swap was not steered (if so); or the outcome when
    /// it cannot apply. `tier_p` is why no patch exists.
    fn tier_b_plan(
        plan: &MutantPlan,
        code: &FunctionCode,
        program: &Program<'_>,
        tier_p: &Unpatchable,
    ) -> Result<TierBPlan, RawOutcome> {
        let (mut stops, mut effect) = initial_effect(plan, code, tier_p)?;
        if let Some(constructed) = constructed_effect(plan, code, program, tier_p)? {
            effect = constructed;
        }
        // Why a swap was not steered, when it is one: the evaluator's
        // fallback hides it.
        let mut unsteered = None;
        match logical_steering(plan, code, &effect) {
            Some(Ok(steers)) if !steers.is_empty() => {
                stops = steers.iter().map(|s| s.pc).collect();
                effect = Effect::Steer { steers };
            }
            Some(Err(e)) => unsteered = Some(e),
            _ => {}
        }
        if let Some(guard) = overflow_guard(plan, code, &effect) {
            effect = Effect::Guarded {
                guard,
                inner: Box::new(effect),
            };
        }
        if let Effect::Branch {
            if_true, if_false, ..
        } = &mut effect
        {
            match crate::tierp::branch_exits(plan, &code.file, &code.insns, program.isa) {
                Ok(exits) => {
                    *if_true = exits.if_true;
                    *if_false = exits.if_false;
                    stops = vec![exits.stop];
                }
                Err(e) => return Err(unappliable(Report::of(&e), tier_p)),
            }
        }
        if stops.is_empty() {
            stops.extend(statement_stop(&plan.action, plan.stop_at, &code.insns));
        }
        if stops.is_empty() {
            return Err(unappliable(
                Report::of(&Reason::NoCodeForLine {
                    line: plan.stop_at.line,
                }),
                tier_p,
            ));
        }
        Ok((stops, effect, unsteered))
    }

    /// An effect with its constructors resolved to libraries.
    fn resolved(
        effect: Effect,
        program: &Program<'_>,
        tier_p: &Unpatchable,
    ) -> Result<Effect, RawOutcome> {
        let Effect::ReturnConstructed {
            request,
            layout,
            in_registers,
            fallback,
        } = &effect
        else {
            return Ok(effect);
        };
        match crate::support_lib::resolve(program.toolchain, request, layout) {
            Ok((library, symbol)) => Ok(Effect::ReturnBuilt {
                value: crate::layouts::Built {
                    bytes: vec![0; layout.size() as usize],
                    constructs: vec![crate::layouts::Construct {
                        at: 0,
                        size: layout.size(),
                        library,
                        symbol,
                    }],
                    ..Default::default()
                },
                in_registers: *in_registers,
            }),
            Err(crate::layouts::Refusal::Unviable(why)) => Err(RawOutcome::Unviable { why }),
            Err(_) if fallback.is_some() => Ok(Effect::ReturnBuilt {
                value: fallback.as_deref().cloned().unwrap_or_default(),
                in_registers: *in_registers,
            }),
            Err(why) => Err(unappliable(why.report().clone(), tier_p)),
        }
    }

    /// The stops and the effect of tier B over every copy of the function:
    /// one effect where the copies agree, else each copy's own at its stops.
    fn tier_b_all(
        plan: &MutantPlan,
        copies: &[FunctionCode],
        program: &Program<'_>,
        tier_p: &Unpatchable,
    ) -> Result<TierBPlan, RawOutcome> {
        let mut stops = Vec::new();
        let mut effects: Vec<(Vec<u64>, Effect)> = Vec::new();
        let mut unsteered = None;
        for code in copies {
            let (s, e, u) = Self::tier_b_plan(plan, code, program, tier_p)?;
            let e = Self::resolved(e, program, tier_p)?;
            stops.extend(s.iter().copied());
            unsteered = unsteered.or(u);
            match effects.iter_mut().find(|(_, known)| *known == e) {
                Some((at, _)) => at.extend(s),
                None => effects.push((s, e)),
            }
        }
        let effect = match effects.len() {
            1 => effects.remove(0).1,
            _ => Effect::PerStop(effects),
        };
        Ok((stops, effect, unsteered))
    }

    fn tier_b(
        session: &mut L::Session,
        plan: &MutantPlan,
        copies: &[FunctionCode],
        program: &Program<'_>,
        budget: Budget,
        tier_p: Unpatchable,
    ) -> Result<(RawOutcome, String), ExecError> {
        let (stops, effect, unsteered) = match Self::tier_b_all(plan, copies, program, &tier_p) {
            Ok(p) => p,
            Err(outcome) => return Ok((outcome, String::new())),
        };
        let budget = budget.evaluating();
        let start = Instant::now();
        let Some(run) = session.run_evaluating(&stops, &effect, budget.timeout)? else {
            let refused = unappliable(Report::of(&Reason::NoTierBEvaluator), &tier_p);
            return Ok((refused, String::new()));
        };
        let output = session.take_output();
        if let Some(why) = run.refused {
            let refused = Report::of(&Reason::EvaluatorRefused {
                cause: Report::of(&why),
            });
            let cause = match unsteered {
                Some(e) => Report::of(&Reason::Unsteered {
                    cause: refused,
                    unsteered: Report::of(&e),
                }),
                None => refused,
            };
            let why = Report::of(&Reason::Unappliable {
                cause,
                tier_p: Report::of(&tier_p),
            });
            let wall = start.elapsed();
            let judged = wall.saturating_sub(session.lost_to_load());
            return Ok((refusal_outcome(why, wall, judged, budget), output));
        }
        if !run.reached && matches!(run.end, RunEnd::Exited(_)) {
            return Ok((never_executed(), output));
        }
        Ok((
            judge(Tier::B, run.end, start.elapsed(), session, budget),
            output,
        ))
    }

    /// As [`MutantExecutor::run`], also returning the program's standard
    /// output (empty when the debugger does not forward it).
    pub fn run_capturing(
        &mut self,
        plan: &MutantPlan,
        budget: Budget,
    ) -> Result<(RawOutcome, String), ExecError> {
        if let Some(reason) = self.unresolved_value(plan) {
            let why = Report::of(&reason);
            return Ok((RawOutcome::Unviable { why }, String::new()));
        }
        // Mutants neither tier can run are rejected before a debugger starts.
        if let Err(e @ Unpatchable::UnsupportedEdit(_)) = derive(plan, "", &[], &self.isa)
            && effect_of(plan).is_none()
            && !crate::tierp::negatable(plan)
        {
            return Ok((
                RawOutcome::Unsupported {
                    why: Report::of(&e),
                },
                String::new(),
            ));
        }
        let mut session = self.acquire()?;
        let result = self.run_in(&mut session, plan, budget);
        // Kept for the next run, unless the run failed (its state is unknown).
        if result.is_ok() {
            self.session = Some(session);
        }
        result
    }

    /// A session ready for a run: the kept one with a fresh process, or a new
    /// one when there is none or it cannot restart.
    /// Every copy of the plan's function, with columns: of the functions
    /// with code on its line, those with the most code inside the mutated
    /// expression (one per instance of a generic).
    fn locate_all(
        &self,
        session: &mut L::Session,
        plan: &MutantPlan,
    ) -> Result<Vec<FunctionCode>, ExecError> {
        // An async fn's signature has only the code that makes its future;
        // its body's first line has the poll's.
        let line = if is_async_fn(plan) {
            plan.site.start.line
        } else {
            plan.stop_at.line
        };
        let mut candidates: Vec<FunctionCode> = session
            .function_codes(&self.source(plan), line, plan.site.end.line)?
            .into_iter()
            .map(|c| self.with_columns(c))
            .collect();
        // The body of an async fn runs in a future's poll, not in a closure
        // that shares its first line.
        if is_async_fn(plan) && candidates.iter().any(|c| self.future_poll(c)) {
            candidates.retain(|c| self.future_poll(c));
        }
        let mut span = plan.site;
        for o in &plan.operands {
            span.start = span.start.min(o.start);
            span.end = span.end.max(o.end);
        }
        let insns: Vec<&Vec<Insn>> = candidates.iter().map(|c| &c.insns).collect();
        let keep = crate::tierp::at_site_all(&insns, &span);
        let mut k = 0;
        candidates.retain(|_| {
            k += 1;
            keep.contains(&(k - 1))
        });
        Ok(candidates)
    }

    /// Whether the function is a future's poll.
    fn future_poll(&self, code: &FunctionCode) -> bool {
        let program = self.program();
        let entry = code.insns.iter().map(|i| i.pc).min();
        matches!(
            (program.layouts, entry),
            (Some(l), Some(e)) if l.is_future_poll(e.wrapping_sub(program.bias))
        )
    }

    /// The layout of the function's result, from the program's DWARF.
    fn result_layout(&self, code: &FunctionCode) -> Option<crate::layouts::Layout> {
        let layouts = self
            .layouts
            .get_or_init(|| {
                self.launcher
                    .binary()
                    .and_then(|b| crate::layouts::Layouts::read(b).ok())
            })
            .as_ref()?;
        let bias = self.bias.get().copied().unwrap_or(0);
        let entry = code.insns.iter().map(|i| i.pc).min()?;
        layouts.result_of(entry.wrapping_sub(bias))
    }

    /// The tier P patch of `plan` over one copy of its function. A body
    /// replaced by a trivial value is a stub from the shapes table, chosen
    /// by the layout of the copy's result.
    fn derive_copy(&self, plan: &MutantPlan, code: &FunctionCode) -> Result<Patch, Unpatchable> {
        let result = match plan.action {
            Action::ReturnDefault { .. } => self.result_layout(code),
            _ => None,
        };
        derive_returning(plan, &code.file, &code.insns, &self.isa, result.as_ref())
    }

    fn acquire(&mut self) -> Result<L::Session, ExecError> {
        self.cancelled_or(())?;
        if let Some(mut s) = self.session.take()
            && s.select_tests(self.selected.as_deref()).is_ok()
            && s.restart().unwrap_or(false)
        {
            return Ok(s);
        }
        // Without a line table nothing can be located, and every mutant
        // would quietly count as never called or unreached.
        if let Some(binary) = self.launcher.binary()
            && !crate::lines::has_line_table(binary)
        {
            return Err(ExecError::of(&Toolchain::NoLineTable {
                binary: binary.display().to_string(),
            }));
        }
        let mut s = self.launcher.start(&self.watch)?;
        // A fresh launch runs the configured tests: narrow it, then start
        // over with the narrowed ones.
        if self.selected.is_some() && s.select_tests(self.selected.as_deref())? && !s.restart()? {
            return Err(ExecError::of(&Session::NoRestart));
        }
        Ok(s)
    }

    /// `answer`, unless the executor was cancelled meanwhile: then its
    /// session is dropped and the answer is the cancellation, whatever the
    /// call produced (a debugger killed under a baseline reads as a crash).
    fn cancelled_or<T>(&mut self, answer: T) -> Result<T, ExecError> {
        if self.watch.cancelled() {
            self.session = None;
            return Err(ExecError::of(&Session::Cancelled));
        }
        Ok(answer)
    }

    /// What tier B needs of the program: its ISA, layouts and load offset.
    fn program(&self) -> Program<'_> {
        let layouts = self
            .layouts
            .get_or_init(|| {
                self.launcher
                    .binary()
                    .and_then(|b| crate::layouts::Layouts::read(b).ok())
            })
            .as_ref();
        Program {
            isa: &self.isa,
            layouts,
            bias: self.bias.get().copied().unwrap_or(0),
            facts: &self.default_facts,
            toolchain: self.toolchain.as_deref(),
        }
    }

    /// The addresses whose execution means the mutated code ran: a tier P
    /// patch's regions, or tier B's stops (the ones a run of the mutant
    /// watches). None when the plan cannot apply here.
    fn reach_of(&self, plan: &MutantPlan, copies: &[FunctionCode]) -> Option<Vec<u64>> {
        match self.derive_all(plan, copies) {
            Derived::Patch(p) => Some(p.regions.iter().map(|r| r.addr).collect()),
            Derived::TierB(e) => Self::tier_b_all(plan, copies, &self.program(), &e)
                .ok()
                .map(|(stops, _, _)| stops),
            Derived::Refused(_) => None,
        }
    }

    /// The tier P patch of `plan` over every copy of its function (the
    /// first is the one tier B would stop in), or why there is none.
    fn derive_all(&self, plan: &MutantPlan, copies: &[FunctionCode]) -> Derived {
        let Some((code, rest)) = copies.split_first() else {
            return Derived::Refused(Report::of(&Reason::NoCode));
        };
        // The other copies of a generic function: the mutant changes each.
        // Tier P patches them all; tier B applies to one stop's frame, so a
        // mutant it would need is refused rather than applied to one copy.
        let mut others = Vec::new();
        for copy in rest {
            match self.derive_copy(plan, copy) {
                Ok(p) => others.extend(p.regions),
                Err(e) if effect_of(plan).is_some() => return Derived::TierB(e),
                Err(e) => return Derived::Refused(generic_copies(&e)),
            }
        }
        let derived = match self.derive_copy(plan, code) {
            // The return stub clears only the first result register: a
            // wider integer result (i128, u128) would keep its upper half.
            Ok(_)
                if matches!(plan.action, Action::ReturnDefault { .. })
                    && is_wide(self.result_layout(code).as_ref()) =>
            {
                Err(Unpatchable::UnsupportedEdit(
                    "a return stub for a result wider than a register".into(),
                ))
            }
            // An async fn's body returns `Poll<T>`, which zeroed registers do
            // not make from a zero `T`: tier B builds `Poll::Ready(value)`.
            Ok(_) if is_async_fn(plan) && self.future_poll(code) => Err(
                Unpatchable::UnsupportedEdit("a return stub for an async fn's poll".into()),
            ),
            d => d,
        };
        match derived {
            Ok(mut p) => {
                p.regions.extend(others);
                Derived::Patch(p)
            }
            Err(e) if !rest.is_empty() && effect_of(plan).is_none() => {
                Derived::Refused(generic_copies(&e))
            }
            Err(e) if effect_of(plan).is_some() => Derived::TierB(e),
            Err(e) => Derived::Refused(Report::of(&e)),
        }
    }

    fn run_in(
        &self,
        session: &mut L::Session,
        plan: &MutantPlan,
        budget: Budget,
    ) -> Result<(RawOutcome, String), ExecError> {
        let copies = match self.locate_all(session, plan) {
            Ok(c) if !c.is_empty() => c,
            Ok(_) => {
                let why = Report::of(&Reason::NotInProgram {
                    file: plan.file.display().to_string(),
                    line: plan.stop_at.line,
                });
                return Ok((RawOutcome::NotReached { why }, String::new()));
            }
            Err(e) => {
                return Ok((RawOutcome::Unsupported { why: e.cause }, String::new()));
            }
        };
        let patch = match self.derive_all(plan, &copies) {
            Derived::Patch(p) => p,
            Derived::Refused(why) => {
                return Ok((RawOutcome::Unsupported { why }, String::new()));
            }
            Derived::TierB(e) => {
                return Self::tier_b(session, plan, &copies, &self.program(), budget, e);
            }
        };
        session.arm(&patch)?;
        // After arming, so a breakpoint saves the patched bytes as its own.
        let reach: Vec<u64> = patch.regions.iter().map(|r| r.addr).collect();
        let start = Instant::now();
        let (end, reached) = session.run_reaching(&reach, budget.timeout)?;
        let output = session.take_output();
        if !reached && matches!(end, RunEnd::Exited(_)) {
            return Ok((never_executed(), output));
        }
        Ok((
            judge(Tier::P, end, start.elapsed(), session, budget),
            output,
        ))
    }

    /// `outcome`, with the tests the launcher reads from `output`.
    fn observed(&self, outcome: RawOutcome, output: &str) -> Observed {
        Observed {
            outcome,
            tests: self.launcher.tests_of(output),
            output_tail: None,
        }
    }

    /// The unmutated run, also returning the program's output.
    pub fn baseline_capturing(
        &mut self,
        budget: Budget,
    ) -> Result<(RawOutcome, String), ExecError> {
        self.baseline_printing(budget)
            .map(|(raw, output, _)| (raw, output))
    }

    /// [`Self::baseline_capturing`], also returning what the program printed
    /// as a person reads it (the session's diagnostic output, else the
    /// output itself): for the tail only, never for a verdict.
    fn baseline_printing(
        &mut self,
        budget: Budget,
    ) -> Result<(RawOutcome, String, String), ExecError> {
        let mut session = self.acquire()?;
        let raw = Self::timed_run(&mut session, budget)?;
        let output = session.take_output();
        let diagnostic = match session.take_diagnostic_output() {
            more if more.is_empty() => output.clone(),
            more => more,
        };
        self.session = Some(session);
        Ok((raw, output, diagnostic))
    }
}

impl<L: Launcher> TierPExecutor<L> {
    /// [`MutantExecutor::recording`], before a cancel is looked for.
    fn record(&mut self, plans: &[MutantPlan], budget: Budget) -> Result<Recording, ExecError> {
        let every_test = |tests: Option<usize>, why: NotRecorded| Recording {
            tests,
            selection: Selection::EveryTest {
                why: Report::of(&why),
            },
            driven: None,
        };
        // The binary's tests by the runner's names, to tell them on the stack.
        let Some(tests) = self.launcher.binary().and_then(crate::libtest::list_tests) else {
            return Ok(every_test(None, NotRecorded::NoTestList));
        };
        let count = Some(tests.len());
        self.selected = None;
        let mut session = self.acquire()?;
        // Each plan's function by its entry: the first instruction of the
        // outermost function block holding the plan's line.
        let mut entries = Vec::with_capacity(plans.len());
        // A plan whose function could not be located (an error, not "not
        // in this program") is run with every test rather than dropped.
        let mut unlocated = Vec::with_capacity(plans.len());
        // Each tier P plan's patched addresses, the ones a run watches to
        // tell whether the mutated code executed: a plan no test executes
        // needs no run.
        let mut reach = Vec::with_capacity(plans.len());
        for plan in plans {
            // Every copy's entry: a generic's instances are called by
            // different tests.
            let copies = self.locate_all(&mut session, plan);
            unlocated.push(copies.is_err());
            let copies = copies.unwrap_or_default();
            let starts: Vec<u64> = copies
                .iter()
                .filter_map(|c| c.insns.iter().map(|i| i.pc).min())
                .collect();
            entries.push(starts);
            reach.push(self.reach_of(plan, &copies));
        }
        let mut watch: Vec<u64> = entries.iter().flatten().copied().collect();
        watch.sort_unstable();
        watch.dedup();
        let mut watch_reach: Vec<u64> = reach.iter().flatten().flatten().copied().collect();
        watch_reach.sort_unstable();
        watch_reach.dedup();
        let recorded = session.run_recording(
            &watch,
            &watch_reach,
            &tests,
            recording_timeout(budget.timeout),
        )?;
        self.session = Some(session);
        let Some((end, calls, hit)) = recorded else {
            return Ok(every_test(count, NotRecorded::NothingRecorded));
        };
        let hit: std::collections::HashSet<u64> = hit.into_iter().collect();
        let reached: Vec<Option<bool>> = reach
            .iter()
            .map(|r| {
                r.as_ref()
                    .map(|addrs| addrs.iter().any(|a| hit.contains(a)))
            })
            .collect();
        if end != RunEnd::Exited(0) {
            let output = self
                .session
                .as_mut()
                .map(|s| s.take_output())
                .unwrap_or_default();
            let failing: Vec<String> = crate::libtest::failures(&output).into_keys().collect();
            // A test that fails only under the recording's breakpoints (a
            // timing-sensitive one, slowed down) may have stopped before
            // calling what it would have; the others ran whole, each on its
            // own thread. libtest's failure exit names them: the failed
            // ones run every mutant. A crash or a timeout leaves the whole
            // record in doubt: every test runs every mutant.
            if end == RunEnd::Exited(101) && !failing.is_empty() {
                let calls: std::collections::HashMap<u64, Driven> = calls.into_iter().collect();
                return Ok(Recording {
                    tests: count,
                    driven: Some(selections(&entries, &unlocated, &reached, &calls, &failing)),
                    selection: Selection::RecordedWithFailures {
                        failed_tests: failing,
                    },
                });
            }
            let end = format!("{end:?}");
            let why = match failing.is_empty() {
                true => NotRecorded::TestsFailed { end },
                false => NotRecorded::NamedTestsFailed {
                    end,
                    failing: failing.join(", "),
                },
            };
            return Ok(every_test(count, why));
        }
        let calls: std::collections::HashMap<u64, Driven> = calls.into_iter().collect();
        Ok(Recording {
            tests: count,
            selection: Selection::Recorded,
            driven: Some(selections(&entries, &unlocated, &reached, &calls, &[])),
        })
    }
}

impl<L: Launcher> MutantExecutor for TierPExecutor<L> {
    fn run(&mut self, plan: &MutantPlan, budget: Budget) -> Result<Observed, ExecError> {
        let ran = self.run_capturing(plan, budget);
        let (outcome, output) = self.cancelled_or(ran)??;
        Ok(self.observed(outcome, &output))
    }

    fn baseline(&mut self, budget: Budget) -> Result<Observed, ExecError> {
        let ran = self.baseline_printing(budget);
        let (outcome, output, diagnostic) = self.cancelled_or(ran)??;
        Ok(self.observed(outcome, &output).with_output(&diagnostic))
    }

    fn driven(
        &mut self,
        plans: &[MutantPlan],
        budget: Budget,
    ) -> Result<Option<Vec<Driven>>, ExecError> {
        self.recording(plans, budget).map(|r| r.driven)
    }

    fn recording(&mut self, plans: &[MutantPlan], budget: Budget) -> Result<Recording, ExecError> {
        let recorded = self.record(plans, budget);
        self.cancelled_or(recorded)?
    }

    fn select_tests(&mut self, tests: Option<&[String]>) {
        self.selected = tests.map(<[String]>::to_vec);
    }

    fn describe(&self) -> Option<String> {
        self.launcher.binary().map(|b| b.display().to_string())
    }

    fn close_session(&mut self) {
        // Dropping the session ends GDB and the program under it.
        self.session = None;
    }

    fn canceller(&self) -> Arc<dyn Cancel> {
        self.watch.clone()
    }
}

/// Tier B's stops, its effect at them, and why a logical swap was not
/// steered.
type TierBPlan = (Vec<u64>, Effect, Option<Unpatchable>);

/// How a plan applies to the program's code.
enum Derived {
    /// A tier P patch.
    Patch(Patch),
    /// Unsupported, and why.
    Refused(Report),
    /// Through the debugger at a stop: tier P cannot, for this reason.
    TierB(Unpatchable),
}

/// How long the recording run may take, given a mutant run's timeout.
pub(crate) fn recording_timeout(run: Duration) -> Duration {
    run * RECORDING_SLOWDOWN
}

/// How many mutant runs' time the one recording run may take.
const RECORDING_SLOWDOWN: u32 = 20;

/// Which tests drive each plan, from the recorded calls: by each plan's
/// function entries (one per generic copy), or `unlocated` when its
/// function could not be located. `failed` are tests that failed while
/// recording: they may have stopped before calling what they would have.
pub(crate) fn selections(
    entries: &[Vec<u64>],
    unlocated: &[bool],
    reached: &[Option<bool>],
    calls: &std::collections::HashMap<u64, Driven>,
    failed: &[String],
) -> Vec<Driven> {
    entries
        .iter()
        .zip(unlocated)
        .zip(reached)
        .map(|((starts, &unlocated), &reached)| {
            if unlocated {
                return Driven::Every;
            }
            // The copies' callers together.
            let mut tests = failed.to_vec();
            // Called, but no test executed the mutated code: only a test
            // that failed while recording might have.
            let starts: &[u64] = if reached == Some(false) { &[] } else { starts };
            for pc in starts {
                match calls.get(pc) {
                    Some(Driven::Every) => return Driven::Every,
                    Some(Driven::Tests(t)) => tests.extend(t.iter().cloned()),
                    _ => {}
                }
            }
            tests.sort();
            tests.dedup();
            if tests.is_empty() {
                Driven::NotCalled
            } else {
                Driven::Tests(tests)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    fn insn(pc: u64, line: u32, column: u32) -> Insn {
        Insn {
            pc,
            bytes: vec![0x90],
            file: "a.rs".into(),
            line,
            column,
        }
    }

    fn pos(line: u32, col: u32) -> Pos {
        Pos { byte: 0, line, col }
    }

    #[test]
    fn test_a_statement_stops_at_its_columns_first_instruction_on_the_line() {
        let insns = [
            insn(30, 4, 5),
            insn(10, 4, 9),
            insn(20, 4, 17),
            insn(5, 3, 1),
        ];
        let nothing = Action::JumpOnly {
            resume: cherenkov_mutation::plan::Target::AfterStatement(
                cherenkov_mutation::syntax::Span {
                    start: pos(1, 1),
                    end: pos(1, 2),
                },
            ),
        };
        // At or after the statement's column; an earlier one on the line is another statement's.
        assert_eq!(statement_stop(&nothing, pos(4, 9), &insns), Some(10));
        assert_eq!(statement_stop(&nothing, pos(4, 10), &insns), Some(20));
        assert_eq!(statement_stop(&nothing, pos(4, 18), &insns), None);
    }

    #[test]
    fn test_a_line_without_columns_stops_at_its_first_instruction() {
        let insns = [insn(30, 4, 0), insn(10, 4, 0), insn(5, 3, 0)];
        let nothing = Action::JumpOnly {
            resume: cherenkov_mutation::plan::Target::AfterStatement(
                cherenkov_mutation::syntax::Span {
                    start: pos(1, 1),
                    end: pos(1, 2),
                },
            ),
        };
        assert_eq!(statement_stop(&nothing, pos(4, 1), &insns), Some(10));
        assert_eq!(statement_stop(&nothing, pos(9, 1), &insns), None);
    }

    #[test]
    fn test_a_body_replacement_stops_at_the_functions_lowest_instruction() {
        let insns = [insn(30, 4, 5), insn(10, 4, 9), insn(5, 3, 1)];
        let body = Action::ReturnDefault {
            value_text: None,
            zero_bits: false,
            async_fn: false,
        };
        assert_eq!(statement_stop(&body, pos(4, 1), &insns), Some(5));
    }

    #[test]
    fn test_only_add_sub_and_mul_swaps_are_overflow_guarded() {
        let swap = |with| Edit::ReplaceToken {
            capture: cherenkov_mutation::concept::Capture::Op,
            from: Token::Div,
            with,
        };
        assert_eq!(checked_operator(&swap(Token::Add)), Some("+"));
        assert_eq!(checked_operator(&swap(Token::Sub)), Some("-"));
        assert_eq!(checked_operator(&swap(Token::Mul)), Some("*"));
        assert_eq!(checked_operator(&swap(Token::Div)), None);
        assert_eq!(checked_operator(&Edit::DeleteStatement), None);
    }

    fn budget_of(secs: u64) -> Budget {
        Budget {
            timeout: Duration::from_secs(secs),
        }
    }

    #[test]
    fn test_a_run_slow_only_for_want_of_a_cpu_is_judged_on_the_time_it_ran() {
        let wall = Duration::from_secs(9);
        let judged = Duration::from_secs(4);
        assert_eq!(
            outcome_judged(Tier::P, RunEnd::Exited(0), wall, judged, budget_of(5)),
            RawOutcome::Exited {
                code: 0,
                tier: Tier::P,
                wall
            }
        );
    }

    #[test]
    fn test_a_run_past_its_budget_without_the_wait_is_timed_out() {
        let wall = Duration::from_secs(9);
        assert_eq!(
            outcome_judged(Tier::B, RunEnd::Exited(0), wall, wall, budget_of(5)),
            RawOutcome::TimedOut {
                tier: Tier::B,
                wall
            }
        );
    }

    #[test]
    fn test_a_refusal_after_the_whole_budget_is_spent_is_a_timeout_not_unsupported() {
        let wall = Duration::from_secs(9);
        assert_eq!(
            refusal_outcome(Report::of(&Reason::NoCode), wall, wall, budget_of(5)),
            RawOutcome::TimedOut {
                tier: Tier::B,
                wall
            }
        );
    }

    #[test]
    fn test_a_refusal_with_budget_left_stays_unsupported() {
        let wall = Duration::from_secs(5);
        assert_eq!(
            refusal_outcome(Report::of(&Reason::NoCode), wall, wall, budget_of(5)),
            RawOutcome::Unsupported {
                why: Report::of(&Reason::NoCode)
            }
        );
        // Time lost to a busy machine is not the program's.
        assert_eq!(
            refusal_outcome(
                Report::of(&Reason::NoCode),
                Duration::from_secs(9),
                Duration::from_secs(4),
                budget_of(5)
            ),
            RawOutcome::Unsupported {
                why: Report::of(&Reason::NoCode)
            }
        );
    }

    #[test]
    fn test_a_run_the_executor_timed_out_stays_timed_out_whatever_it_waited() {
        let wall = Duration::from_secs(1);
        assert_eq!(
            outcome_judged(
                Tier::P,
                RunEnd::TimedOut,
                wall,
                Duration::ZERO,
                budget_of(5)
            ),
            RawOutcome::TimedOut {
                tier: Tier::P,
                wall
            }
        );
    }

    #[test]
    fn test_selections_of_a_mutant_no_test_executes_are_no_test() {
        // Its function is called, but no test reaches the mutated code (a
        // branch never taken): running it would only show it unreached.
        let calls = HashMap::from([(1, Driven::Tests(names(&["a"])))]);
        let got = selections(
            &[vec![1], vec![1], vec![1]],
            &[false; 3],
            &[Some(false), Some(true), None],
            &calls,
            &[],
        );
        assert_eq!(
            got,
            vec![
                Driven::NotCalled,
                Driven::Tests(names(&["a"])),
                Driven::Tests(names(&["a"])),
            ]
        );
    }

    #[test]
    fn test_selections_of_an_unexecuted_mutant_keep_the_tests_that_failed_recording() {
        // A test that failed may have stopped short of the mutated code.
        let calls = HashMap::from([(1, Driven::Tests(names(&["a"])))]);
        let failed = names(&["slow"]);
        let got = selections(&[vec![1]], &[false], &[Some(false)], &calls, &failed);
        assert_eq!(got, vec![Driven::Tests(names(&["slow"]))]);
    }

    #[test]
    fn test_recording_gets_far_longer_than_a_mutant_run() {
        // The recording walks the stack in Python at each test's first call
        // of each function; running out of time means every test runs every
        // mutant, so the one recording gets a much larger allowance.
        let run = Duration::from_secs(15);
        assert!(recording_timeout(run) >= run * 20);
    }

    #[test]
    fn test_selections_join_the_callers_of_every_copy() {
        let calls = HashMap::from([
            (1, Driven::Tests(names(&["b"]))),
            (2, Driven::Tests(names(&["a", "b"]))),
        ]);
        let got = selections(
            &[vec![1, 2], vec![3]],
            &[false, false],
            &[None, None],
            &calls,
            &[],
        );
        assert_eq!(
            got,
            vec![Driven::Tests(names(&["a", "b"])), Driven::NotCalled]
        );
    }

    #[test]
    fn test_selections_of_an_unlocated_or_everyones_function_are_every_test() {
        let calls = HashMap::from([(1, Driven::Every)]);
        let got = selections(
            &[vec![1], vec![]],
            &[false, true],
            &[None, None],
            &calls,
            &[],
        );
        assert_eq!(got, vec![Driven::Every, Driven::Every]);
    }

    #[test]
    fn test_selections_add_the_tests_that_failed_while_recording_to_every_plan() {
        // A test that failed under the recording's breakpoints may have
        // stopped before calling the function: it runs every mutant. The
        // tests that passed were recorded whole (each on its own thread).
        let calls = HashMap::from([(1, Driven::Tests(names(&["a"])))]);
        let failed = names(&["slow"]);
        let got = selections(
            &[vec![1], vec![2]],
            &[false, false],
            &[None, None],
            &calls,
            &failed,
        );
        assert_eq!(
            got,
            vec![
                Driven::Tests(names(&["a", "slow"])),
                Driven::Tests(names(&["slow"])),
            ]
        );
    }

    /// A session that exits 101 having printed `stdout` and, for a
    /// person, `diagnostic`.
    struct Printing {
        stdout: &'static str,
        diagnostic: &'static str,
    }

    impl NativeSession for Printing {
        fn function_code(&mut self, _: &Path, _: u32) -> Result<Option<FunctionCode>, ExecError> {
            unreachable!()
        }
        fn read(&mut self, _: u64, _: usize) -> Result<Vec<u8>, ExecError> {
            unreachable!()
        }
        fn write(&mut self, _: u64, _: &[u8]) -> Result<(), ExecError> {
            unreachable!()
        }
        fn run_reaching(&mut self, _: &[u64], _: Duration) -> Result<(RunEnd, bool), ExecError> {
            Ok((RunEnd::Exited(101), true))
        }
        fn take_output(&mut self) -> String {
            self.stdout.to_string()
        }
        fn take_diagnostic_output(&mut self) -> String {
            self.diagnostic.to_string()
        }
    }

    /// Starts [`Printing`] sessions and keeps what `tests_of` was given.
    struct Reading {
        stdout: &'static str,
        diagnostic: &'static str,
        read: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Launcher for Reading {
        type Session = Printing;
        fn start(&self, _: &Arc<Watch>) -> Result<Printing, ExecError> {
            Ok(Printing {
                stdout: self.stdout,
                diagnostic: self.diagnostic,
            })
        }
        fn abi(&self) -> &'static str {
            "sysv"
        }
        fn tests_of(&self, output: &str) -> Vec<TestReport> {
            self.read.lock().unwrap().push(output.to_string());
            Vec::new()
        }
    }

    fn baseline_of(stdout: &'static str, diagnostic: &'static str) -> (Observed, Vec<String>) {
        let read = Arc::new(std::sync::Mutex::new(Vec::new()));
        let launcher = Reading {
            stdout,
            diagnostic,
            read: read.clone(),
        };
        let mut executor = TierPExecutor::new(launcher, Isa::amd64());
        let observed = executor
            .baseline(Budget::from_baseline(Duration::from_secs(1)))
            .unwrap();
        let seen = read.lock().unwrap().clone();
        (observed, seen)
    }

    #[test]
    fn test_a_baselines_output_tail_is_the_diagnostic_output_and_tests_are_read_from_stdout_only() {
        let (with, read_with) = baseline_of("out\n", "out\nerr\n");
        let (without, read_without) = baseline_of("out\n", "");
        assert_eq!(with.output_tail.as_deref(), Some("out\nerr\n"));
        assert_eq!(
            without.output_tail.as_deref(),
            Some("out\n"),
            "a session with nothing more is tailed by its stdout"
        );
        assert_eq!(read_with, ["out\n"], "parsing sees what it always saw");
        assert_eq!(read_with, read_without);
        let code = |o: &Observed| match o.outcome {
            RawOutcome::Exited { code, .. } => Some(code),
            _ => None,
        };
        assert_eq!(code(&with), Some(101));
        assert_eq!(
            (code(&with), &with.tests),
            (code(&without), &without.tests),
            "verdict facts are identical with and without the tail"
        );
    }

    /// A launcher that never starts a session: only its binary is asked
    /// about, and the starts are counted.
    struct BinaryOnly(PathBuf, std::sync::atomic::AtomicUsize);

    impl BinaryOnly {
        fn new(binary: PathBuf) -> BinaryOnly {
            BinaryOnly(binary, std::sync::atomic::AtomicUsize::new(0))
        }
    }

    struct NoSession;

    impl NativeSession for NoSession {
        fn function_code(&mut self, _: &Path, _: u32) -> Result<Option<FunctionCode>, ExecError> {
            unreachable!()
        }
        fn read(&mut self, _: u64, _: usize) -> Result<Vec<u8>, ExecError> {
            unreachable!()
        }
        fn write(&mut self, _: u64, _: &[u8]) -> Result<(), ExecError> {
            unreachable!()
        }
        fn run_reaching(&mut self, _: &[u64], _: Duration) -> Result<(RunEnd, bool), ExecError> {
            unreachable!()
        }
    }

    impl Launcher for BinaryOnly {
        type Session = NoSession;
        fn start(&self, _: &Arc<Watch>) -> Result<NoSession, ExecError> {
            self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(ExecError::of(&Session::NoRestart))
        }
        fn abi(&self) -> &'static str {
            "sysv"
        }
        fn binary(&self) -> Option<&Path> {
            Some(&self.0)
        }
    }

    /// Instructions at the first rows of this crate's `native.rs` in the
    /// running test binary (load offset 0), without columns.
    fn own_code(rows: &[crate::lines::Row]) -> FunctionCode {
        let ours: Vec<&crate::lines::Row> = rows
            .iter()
            .filter(|r| r.file.ends_with("cherenkov-exec-dap/src/native.rs") && r.line != 0)
            .take(40)
            .collect();
        FunctionCode {
            file: ours[0].file.clone(),
            insns: ours
                .iter()
                .map(|r| Insn {
                    pc: r.addr,
                    bytes: vec![0x90],
                    file: r.file.clone(),
                    line: r.line,
                    column: 0,
                })
                .collect(),
        }
    }

    #[test]
    fn test_columns_found_after_the_binary_could_not_be_read_once() {
        let exe = std::env::current_exe().unwrap();
        let rows = crate::lines::rows(&exe).unwrap();
        let code = own_code(&rows);
        let dir = std::env::temp_dir().join(format!("cherenkov-columns-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let binary = dir.join("program");
        let executor = TierPExecutor::new(BinaryOnly::new(binary.clone()), Isa::amd64());
        // The first look finds no binary (a rebuild, a descriptor limit, a
        // busy disk): no columns now...
        assert!(
            executor
                .with_columns(code.clone())
                .insns
                .iter()
                .all(|i| i.column == 0)
        );
        // ...but not for the rest of the run: once it reads, they come.
        std::fs::copy(&exe, &binary).unwrap();
        let columned = executor.with_columns(code);
        assert!(
            columned.insns.iter().any(|i| i.column != 0),
            "a failed read of the line table was remembered for good"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[inline(never)]
    fn probe_u32_result() -> u32 {
        std::hint::black_box(7)
    }

    #[inline(never)]
    fn probe_u128_result() -> u128 {
        std::hint::black_box(7)
    }

    #[inline(never)]
    fn probe_option_result() -> Option<u32> {
        std::hint::black_box(Some(7))
    }

    /// The file address of the test binary's function whose symbol holds `name`.
    fn entry_of(name: &str) -> u64 {
        use object::{Object, ObjectSymbol};
        let exe = std::env::current_exe().unwrap();
        let data = std::fs::read(exe).unwrap();
        let file = object::File::parse(&*data).unwrap();
        file.symbols()
            .find(|s| s.name().is_ok_and(|n| n.contains(name)))
            .map(|s| s.address())
            .unwrap_or_else(|| panic!("no symbol {name}"))
    }

    /// A function's code in the test binary: one instruction at its entry.
    fn code_at(name: &str) -> FunctionCode {
        // Keep the probes linked in.
        std::hint::black_box((
            probe_u32_result(),
            probe_u128_result(),
            probe_option_result(),
        ));
        FunctionCode {
            file: "src/lib.rs".into(),
            insns: vec![Insn {
                pc: entry_of(name),
                bytes: vec![0x90; 8],
                file: "src/lib.rs".into(),
                line: 1,
                column: 0,
            }],
        }
    }

    fn starts(executor: &TierPExecutor<BinaryOnly>) -> usize {
        executor
            .launcher
            .1
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    const BUDGET: Budget = Budget {
        timeout: Duration::from_secs(1),
    };

    #[test]
    fn test_a_cancelled_executor_fails_its_baseline_without_starting_a_session() {
        let mut executor = own_executor();
        executor.canceller().cancel();
        let error = executor.baseline(BUDGET).unwrap_err();
        assert_eq!(error, ExecError::of(&Session::Cancelled));
        assert_eq!(starts(&executor), 0);
    }

    #[test]
    fn test_a_cancelled_executor_fails_its_recording_without_starting_a_session() {
        let mut executor = own_executor();
        executor.canceller().cancel();
        let error = executor.recording(&[], BUDGET).unwrap_err();
        assert_eq!(error, ExecError::of(&Session::Cancelled));
        assert_eq!(starts(&executor), 0);
    }

    #[test]
    fn test_a_cancelled_executor_fails_a_mutants_run_without_starting_a_session() {
        let mut executor = own_executor();
        executor.canceller().cancel();
        let plan = return_plan("pub fn f(a: u32) -> u32 { a + 2 }\n", "1");
        let error = executor.run(&plan, BUDGET).unwrap_err();
        assert_eq!(error, ExecError::of(&Session::Cancelled));
        assert_eq!(starts(&executor), 0);
    }

    /// The canceller of a executor is the one its sessions are watched by:
    /// cancelling after a start is seen by the watch the launcher got.
    #[test]
    fn test_the_canceller_is_the_watch_the_launcher_starts_sessions_with() {
        let mut executor = own_executor();
        // The launcher's start fails: the call's own error is the answer
        // when nothing was cancelled.
        assert_eq!(
            executor.baseline(BUDGET).unwrap_err(),
            ExecError::of(&Session::NoRestart)
        );
        assert_eq!(starts(&executor), 1);
        executor.canceller().cancel();
        assert!(executor.watch.cancelled());
    }

    fn own_executor() -> TierPExecutor<BinaryOnly> {
        let exe = std::env::current_exe().unwrap();
        TierPExecutor::new(BinaryOnly::new(exe), Isa::amd64())
    }

    #[test]
    fn test_the_result_layout_of_a_function_is_read_from_the_binarys_dwarf() {
        use crate::layouts::Layout;
        let executor = own_executor();
        assert!(matches!(
            executor.result_layout(&code_at("probe_u32_result")),
            Some(Layout::Int {
                size: 4,
                signed: false
            })
        ));
        assert!(matches!(
            executor.result_layout(&code_at("probe_u128_result")),
            Some(Layout::Int { size: 16, .. })
        ));
        assert!(matches!(
            executor.result_layout(&code_at("probe_option_result")),
            Some(Layout::Enum { .. })
        ));
    }

    #[test]
    fn test_only_an_integer_wider_than_a_register_is_wide() {
        use crate::layouts::Layout;
        let int = |size| Layout::Int {
            size,
            signed: false,
        };
        assert!(is_wide(Some(&int(16))));
        assert!(!is_wide(Some(&int(8))));
        assert!(!is_wide(Some(&Layout::Float { size: 16 })));
        assert!(!is_wide(None));
    }

    fn return_plan(source: &str, value: &str) -> MutantPlan {
        use cherenkov_run_ports::syntax::SyntaxPort;
        let syntax = cherenkov_syntax::TreeSitterSyntax::builtin().unwrap();
        let rust = cherenkov_mutation::syntax::LanguageId::new("rust");
        let profile = syntax
            .profiles()
            .into_iter()
            .find(|p| p.lang == rust)
            .unwrap();
        let file = syntax
            .analyze(&rust, Path::new("src/lib.rs"), std::sync::Arc::from(source))
            .unwrap();
        let set = cherenkov_mutation::operator_set::named("cargo-mutants").unwrap();
        cherenkov_mutation::plan::plan_file(&file, &profile, &set.operators())
            .plans
            .into_iter()
            .find(|p| {
                matches!(&p.action, Action::ReturnDefault { value_text: Some(v), .. } if v == value)
            })
            .unwrap()
    }

    #[test]
    fn test_a_body_replaced_by_a_trivial_value_is_patched_with_the_stub_of_its_results_layout() {
        let executor = own_executor();
        let one = return_plan("pub fn f(a: u32) -> u32 { a + 2 }\n", "1");
        let patch = executor
            .derive_copy(&one, &code_at("probe_u32_result"))
            .unwrap();
        assert_eq!(patch.regions[0].replacement, [0xb8, 1, 0, 0, 0, 0xc3]);
        let none = return_plan("pub fn f(a: u32) -> Option<u32> { Some(a) }\n", "None");
        let patch = executor
            .derive_copy(&none, &code_at("probe_option_result"))
            .unwrap();
        assert_eq!(patch.regions[0].replacement, [0x31, 0xc0, 0xc3]);
    }

    #[test]
    fn test_a_zero_body_takes_the_zero_stub_whatever_the_result_layout() {
        let executor = own_executor();
        let zero = return_plan("pub fn f(a: u32) -> u32 { a + 2 }\n", "0");
        // A zero body takes the zero stub whatever the layout.
        let patch = executor
            .derive_copy(&zero, &code_at("probe_u32_result"))
            .unwrap();
        assert_eq!(patch.regions[0].replacement.last(), Some(&0xc3));
    }

    #[test]
    fn test_a_zero_stub_is_not_written_for_a_result_wider_than_a_register() {
        let executor = own_executor();
        let zero = return_plan("pub fn f(a: u32) -> u128 { a as u128 }\n", "0");
        let wide = executor.derive_all(&zero, &[code_at("probe_u128_result")]);
        assert!(matches!(wide, Derived::TierB(_)), "wide result");
        let narrow = executor.derive_all(&zero, &[code_at("probe_u32_result")]);
        assert!(matches!(narrow, Derived::Patch(_)), "narrow result");
    }
}
