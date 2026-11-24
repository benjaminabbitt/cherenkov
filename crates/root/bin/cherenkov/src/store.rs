//! The store a run is kept in, as the commands see it: the lock a run and a
//! prune take of it (`cherenkov-run-store`'s `lock`), `cherenkov store
//! prune` and `cherenkov store unlock`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cherenkov_cli::error::CliError;
use cherenkov_run_application::prune::Pruned;
use cherenkov_run_store::lock::{self, Holder, LockError, StoreLock, Unlocked};
use clap::{Args, Subcommand};

/// What `cherenkov store` is asked.
#[derive(Args)]
pub struct StoreArgs {
    #[command(subcommand)]
    command: StoreCommand,
}

#[derive(Subcommand)]
enum StoreCommand {
    /// Remove the older finished runs of the store, keeping the last N
    /// (`cherenkov run` keeps every run). An unfinished run is never
    /// removed, and a mutant's history is removed only when no run that
    /// stays plans the mutant. Refused while a run holds the store.
    Prune(PruneArgs),
    /// Clear what a run that was killed left of its hold on the store. The
    /// operating system gives a lock up when its process ends, so a store a
    /// process still holds is refused: stop that process first.
    Unlock(WhereArgs),
}

/// Which store a command is about.
#[derive(Args)]
struct WhereArgs {
    /// The project whose store it is (default: the current directory).
    #[arg(default_value = ".")]
    dir: PathBuf,
    /// The store file, as `cherenkov run --store` names it. Default:
    /// `.cherenkov/store/runs.sqlite` under DIR.
    #[arg(long, value_name = "FILE")]
    store: Option<PathBuf>,
}

#[derive(Args)]
struct PruneArgs {
    #[command(flatten)]
    at: WhereArgs,
    /// How many finished runs to keep: the newest.
    #[arg(long, value_name = "N")]
    keep: usize,
    /// Say what would be removed, and remove nothing.
    #[arg(long = "dry-run")]
    dry_run: bool,
}

impl WhereArgs {
    /// The store file: the one named, else the project's.
    fn path(&self) -> PathBuf {
        match &self.store {
            Some(file) => file.clone(),
            None => cherenkov_run_store::project_store(&self.dir),
        }
    }
}

/// `cherenkov store`.
pub fn command(args: StoreArgs) -> Result<(), CliError> {
    match args.command {
        StoreCommand::Prune(args) => prune(&args),
        StoreCommand::Unlock(args) => unlock(&args.path()),
    }
}

/// Take the lock of the store at `path`, or be refused at once naming the
/// process that holds it. Held until the guard is dropped.
pub fn lock(path: &Path) -> Result<StoreLock, CliError> {
    lock::acquire(path).map_err(|e| refusal(path, e, true))
}

/// What `error` of locking the store at `path` is, to the command line.
fn refusal(path: &Path, error: LockError, taking: bool) -> CliError {
    let store = path.display().to_string();
    match error {
        LockError::Held { holder, .. } => {
            let holder = describe(holder.as_ref(), now());
            match taking {
                true => CliError::StoreLocked {
                    path: store,
                    holder,
                },
                false => CliError::StoreHeld {
                    path: store,
                    holder,
                },
            }
        }
        other => CliError::Unreadable {
            path: store,
            reason: other.to_string(),
        },
    }
}

/// Seconds since the epoch.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Who holds a store, as an error says it.
fn describe(holder: Option<&Holder>, now: u64) -> String {
    match holder {
        Some(holder) => format!(
            "pid {}, held for {}, `{}`",
            holder.pid,
            elapsed(holder.held_for(now).as_secs()),
            holder.command
        ),
        None => "a process that left no record of itself".to_string(),
    }
}

/// `seconds` as `1h 2m`, `3m 12s` or `5s`.
fn elapsed(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

/// `cherenkov store unlock`.
fn unlock(path: &Path) -> Result<(), CliError> {
    match lock::unlock(path).map_err(|e| refusal(path, e, false))? {
        Unlocked::Free => println!("{}: not locked", path.display()),
        Unlocked::Cleared(left) => println!(
            "{}: nothing holds it; cleared what {} left",
            path.display(),
            left.as_ref().map_or_else(
                || "an earlier run".to_string(),
                |h| format!("pid {} (`{}`)", h.pid, h.command)
            )
        ),
    }
    Ok(())
}

/// `cherenkov store prune`.
fn prune(args: &PruneArgs) -> Result<(), CliError> {
    let path = args.at.path();
    if !path.is_file() {
        return Err(CliError::NoStore {
            path: path.display().to_string(),
        });
    }
    let _held = lock(&path)?;
    let pruned = cherenkov_wiring::prune_in_file(&path, args.keep, args.dry_run)
        .map_err(|e| CliError::run(&e))?;
    print!("{}", report(&pruned, args.dry_run));
    Ok(())
}

/// What a prune says: what it removed, or would, and what stays.
fn report(pruned: &Pruned, dry_run: bool) -> String {
    let stays = format!(
        "kept {} finished {}, {} unfinished",
        pruned.kept_finished,
        plural(pruned.kept_finished, "run"),
        pruned.kept_unfinished
    );
    if pruned.runs.is_empty() {
        return format!("nothing to remove; {stays}\n");
    }
    let verb = if dry_run { "would remove" } else { "removed" };
    let mut text = format!(
        "{verb} {} {}, {} mutant {}, {} events; {stays}\n",
        pruned.runs.len(),
        plural(pruned.runs.len(), "run"),
        pruned.mutants,
        plural(pruned.mutants, "stream"),
        pruned.events,
    );
    for run in &pruned.runs {
        text.push_str(&format!("  run {run}\n"));
    }
    text
}

fn plural(n: usize, word: &str) -> String {
    match n {
        1 => word.to_string(),
        _ => format!("{word}s"),
    }
}

#[cfg(test)]
#[path = "store.test.rs"]
mod tests;
