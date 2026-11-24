//! The ground truth: a mutant compiled from source, in a scratch copy of the
//! project (the working tree is never written), and its tests run.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cherenkov_exec_dap::toolchain::test_binaries_from_cargo_json;

use crate::judge::Run;
use cherenkov_exec_dap::libtest;

/// A copy of a Cargo project, rebuilt once per mutant into its own target
/// directory (incremental, so only the mutated crate recompiles).
pub struct ScratchProject {
    dir: PathBuf,
    cargo: PathBuf,
}

impl ScratchProject {
    /// Copy `root` (without `target/` or `.git/`) into a fresh directory.
    pub fn copy(root: &Path, cargo: &Path) -> std::io::Result<ScratchProject> {
        // Unique per copy: several may exist at once in one process.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("cherenkov-fidelity-{}-{n}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        for entry in walkdir::WalkDir::new(root).into_iter().filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            e.depth() == 0 || !(e.file_type().is_dir() && (name == "target" || name == ".git"))
        }) {
            let entry = entry.map_err(std::io::Error::other)?;
            let rel = entry
                .path()
                .strip_prefix(root)
                .map_err(std::io::Error::other)?;
            let to = dir.join(rel);
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&to)?;
            } else if entry.file_type().is_file() {
                std::fs::copy(entry.path(), &to)?;
            }
        }
        Ok(ScratchProject {
            dir,
            cargo: cargo.to_path_buf(),
        })
    }

    /// Build with `rel` replaced by `contents` (restored afterwards), run
    /// every test binary with `test_args`, each within `timeout`.
    pub fn run_mutant(
        &self,
        rel: &Path,
        contents: &str,
        test_args: &[String],
        timeout: Duration,
    ) -> std::io::Result<Run> {
        let path = self.dir.join(rel);
        let original = std::fs::read_to_string(&path)?;
        std::fs::write(&path, contents)?;
        let run = self.build_and_test(test_args, timeout);
        std::fs::write(&path, original)?;
        run
    }

    /// The unmutated project, to time a baseline.
    pub fn run_baseline(&self, test_args: &[String], timeout: Duration) -> std::io::Result<Run> {
        self.build_and_test(test_args, timeout)
    }

    fn build_and_test(&self, test_args: &[String], timeout: Duration) -> std::io::Result<Run> {
        let build = Command::new(&self.cargo)
            .args(["test", "--no-run", "--message-format=json"])
            .current_dir(&self.dir)
            .stderr(Stdio::null())
            .output()?;
        if !build.status.success() {
            return Ok(Run::DoesNotCompile);
        }
        let mut binaries = test_binaries_from_cargo_json(&String::from_utf8_lossy(&build.stdout));
        binaries.sort();
        let mut tests = BTreeMap::new();
        for binary in binaries {
            match run_binary(&binary, test_args, &self.dir, timeout)? {
                Run::Tests(t) => tests.extend(t),
                other => return Ok(other),
            }
        }
        Ok(Run::Tests(tests))
    }
}

impl Drop for ScratchProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Run one test binary, killing it at `timeout`.
fn run_binary(
    binary: &Path,
    args: &[String],
    dir: &Path,
    timeout: Duration,
) -> std::io::Result<Run> {
    let mut child = Command::new(binary)
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdout = child.stdout.take().expect("piped");
    // Read concurrently, so a chatty test cannot fill the pipe and block.
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let out = reader.join().unwrap_or_default();
    Ok(match status {
        None => Run::TimedOut,
        Some(s) if s.code().is_none() => Run::Crashed(format!("{s}")),
        Some(_) => Run::Tests(libtest::parse(&out)),
    })
}
