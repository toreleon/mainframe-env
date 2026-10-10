//! Private bounded two-process supervision; phase owner owns the actual source proof.
//! No receipt substitutes for the checked child exit and drained output.
//! This runner never accepts a receipt as a substitute for a checked child exit.

use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const CHILD: &str = "carddemo::terminal_process_tests::corpus_sqlite_terminal_process_child";
pub(super) const ROOT_ENV: &str = "MAINFRAME_ENV_CARDDEMO_TERMINAL_PROCESS_ROOT";
pub(super) const PHASE_ENV: &str = "MAINFRAME_ENV_CARDDEMO_TERMINAL_PROCESS_PHASE";
pub(super) const NONCE_ENV: &str = "MAINFRAME_ENV_CARDDEMO_TERMINAL_PROCESS_NONCE";
const MAX_OUTPUT: usize = 128 * 1024;
const PHASE_LIMIT: Duration = Duration::from_secs(120);
const REAP_LIMIT: Duration = Duration::from_secs(5);
const READER_LIMIT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(20);

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Phase {
    Prepare,
    Resume,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Prepare => "prepare",
            Self::Resume => "resume",
        }
    }
}

// These bytes may contain test failure diagnostics. Do not derive Debug or
// print them; a parent may retain them only inside its private owned fixture.
pub(super) struct CheckedExit {
    pub(super) pid: u32,
    pub(super) status: ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
}

struct Pipe {
    result: mpsc::Receiver<io::Result<Vec<u8>>>,
    worker: JoinHandle<()>,
}

fn pipe<R: Read + Send + 'static>(mut source: R, overflow: Arc<AtomicBool>) -> io::Result<Pipe> {
    let (sender, result) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("carddemo-child-output".into())
        .spawn(move || {
            let mut kept = Vec::new();
            let mut block = [0; 4096];
            let outcome = loop {
                match source.read(&mut block) {
                    Ok(0) => break Ok(kept),
                    Ok(count) => {
                        let remaining = MAX_OUTPUT.saturating_sub(kept.len());
                        kept.extend_from_slice(&block[..count.min(remaining)]);
                        if count > remaining {
                            // Continue draining until the exact child is killed/reaped;
                            // stop retaining bytes immediately at the frozen cap.
                            overflow.store(true, Ordering::Release);
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => break Err(error),
                }
            };
            let _ = sender.send(outcome);
        })?;
    Ok(Pipe { result, worker })
}

fn reap_after_kill(child: &mut Child) -> io::Result<ExitStatus> {
    // A race with a just-completed child is permitted only when try_wait below
    // confirms its exit. A failed kill never substitutes for a checked reap.
    let kill_problem = child.kill().err();
    let until = Instant::now() + REAP_LIMIT;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= until {
            return Err(kill_problem.unwrap_or_else(|| {
                io::Error::new(io::ErrorKind::TimedOut, "owned child could not be reaped")
            }));
        }
        thread::sleep(POLL);
    }
}

fn finish_pipe(pipe: Pipe) -> io::Result<Vec<u8>> {
    let until = Instant::now() + READER_LIMIT;
    while !pipe.worker.is_finished() {
        if Instant::now() >= until {
            // No cleanup/observation can follow an unjoined reader. The caller
            // must retain the exclusive fixture and report this infrastructure
            // refusal; it must not interpret a receipt as a successful drain.
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "child pipe did not drain",
            ));
        }
        thread::sleep(POLL);
    }
    pipe.worker
        .join()
        .map_err(|_| io::Error::other("child pipe reader panicked"))?;
    pipe.result
        .try_recv()
        .map_err(|_| io::Error::other("child pipe result missing"))?
}

// The exclusive root is retained by physical identity, not by a copied marker.
// This is a fixed owned-child check, not a concurrent descendant-closure claim.
pub(super) fn root_identity(root: &Path) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o777 != 0o700
        || fs::canonicalize(root)? != root
    {
        return Err(io::Error::other(
            "owned process root is not the admitted private directory",
        ));
    }
    Ok((metadata.dev(), metadata.ino()))
}

pub(super) fn require_root_identity(root: &Path, expected: (u64, u64)) -> io::Result<()> {
    if root_identity(root)? != expected {
        return Err(io::Error::other(
            "owned process root physical identity changed",
        ));
    }
    Ok(())
}

// The caller owns an exclusive canonical fixture and supplies its independently
// admitted executable identity and corpus. There is no shell or selectable child.
// This function has no effect on product deadlines, clocks, grants or budgets.
pub(super) fn run_phase(
    executable: &Path,
    root: &Path,
    corpus: &Path,
    nonce: &str,
    phase: Phase,
    owned_root: (u64, u64),
) -> io::Result<CheckedExit> {
    require_root_identity(root, owned_root)?;
    if executable != std::env::current_exe()?.as_path()
        || fs::canonicalize(root)? != root
        || fs::symlink_metadata(root)?.file_type().is_symlink()
        || !root.is_dir()
        || nonce.is_empty()
        || nonce.len() > 128
        || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid owned process inputs",
        ));
    }
    let mut child = Command::new(executable)
        .args([
            "--exact",
            CHILD,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROOT_ENV, root)
        .env(PHASE_ENV, phase.name())
        .env(NONCE_ENV, nonce)
        .env("CARDDEMO_CORPUS_DIR", corpus)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = child.id();
    let (stdout, stderr) = match (child.stdout.take(), child.stderr.take()) {
        (Some(stdout), Some(stderr)) => (stdout, stderr),
        _ => {
            reap_after_kill(&mut child)?;
            return Err(io::Error::other("owned child pipe unavailable"));
        }
    };
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout = match pipe(stdout, overflow.clone()) {
        Ok(pipe) => pipe,
        Err(error) => {
            reap_after_kill(&mut child)?;
            return Err(error);
        }
    };
    let stderr = match pipe(stderr, overflow.clone()) {
        Ok(pipe) => pipe,
        Err(error) => {
            let reaped = reap_after_kill(&mut child);
            let joined = finish_pipe(stdout);
            reaped?;
            joined?;
            return Err(error);
        }
    };
    let until = Instant::now() + PHASE_LIMIT;
    let mut first_problem = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(error) => {
                first_problem = Some(error);
                break reap_after_kill(&mut child);
            }
        }
        if overflow.load(Ordering::Acquire) || Instant::now() >= until {
            first_problem = Some(if overflow.load(Ordering::Acquire) {
                io::Error::other("child output exceeded the frozen bound")
            } else {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "child phase exceeded the frozen bound",
                )
            });
            break reap_after_kill(&mut child);
        }
        thread::sleep(POLL);
    };
    // Attempt both reader completions even when a child/status/pipe failed.
    let stdout = finish_pipe(stdout);
    let stderr = finish_pipe(stderr);
    // Reap and both joins must be confirmed before any caller may remove state.
    let status = status?;
    let stdout = stdout?;
    let stderr = stderr?;
    if overflow.load(Ordering::Acquire) && first_problem.is_none() {
        first_problem = Some(io::Error::other("child output exceeded the frozen bound"));
    }
    if let Some(error) = first_problem {
        return Err(error);
    }
    if pid == std::process::id() {
        return Err(io::Error::other("child reused the parent process identity"));
    }
    // Nonzero exit remains a real producer failure. The parent checks this
    // before receipt acceptance and retains the bounded raw logs privately.
    Ok(CheckedExit {
        pid,
        status,
        stdout,
        stderr,
    })
}

// Guard fixed private writes at the same retained-root refusal boundary.
// Observation before/after I/O is non-atomic: this is drift refusal for the owned
// serial fixture, not hostile concurrent containment or descendant closure.
pub(super) fn write_owned(
    root: &Path,
    path: &Path,
    bytes: &[u8],
    maximum: usize,
    owned_root: (u64, u64),
) -> io::Result<()> {
    require_root_identity(root, owned_root)?;
    if path.parent() != Some(root) {
        return Err(io::Error::other(
            "private write is not an exact owned-root entry",
        ));
    }
    write_private(path, bytes, maximum)?;
    require_root_identity(root, owned_root)
}

// Exact observed absence: a dangling link is an existing namespace entry and
// metadata errors are infrastructure refusals, never successful removal evidence.
pub(super) fn absence_result(result: io::Result<fs::Metadata>) -> io::Result<bool> {
    match result {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

pub(super) fn is_absent(path: &Path) -> io::Result<bool> {
    absence_result(fs::symlink_metadata(path))
}

// Exact private files are create_new, bounded before opening, and never printed.
pub(super) fn write_private(path: &Path, bytes: &[u8], maximum: usize) -> io::Result<()> {
    if bytes.len() > maximum {
        return Err(io::Error::other(
            "private process file exceeded the frozen bound",
        ));
    }
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub(super) fn read_private(path: &Path, maximum: usize) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(io::Error::other(
            "private process input is not an owned regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o777 != 0o600 {
            return Err(io::Error::other(
                "private process input has an invalid mode",
            ));
        }
    }
    let file = File::open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file()
        || opened.len() != metadata.len()
        || metadata.len() > maximum as u64
        || fs::canonicalize(path)? != path
    {
        return Err(io::Error::other(
            "private process input changed before read",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
            return Err(io::Error::other("private process input identity changed"));
        }
    }
    let mut bytes = Vec::new();
    let admitted = u64::try_from(maximum)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| io::Error::other("private process input bound overflow"))?;
    file.take(admitted).read_to_end(&mut bytes)?;
    let after = fs::symlink_metadata(path)?;
    if bytes.len() > maximum
        || bytes.len() as u64 != metadata.len()
        || after.file_type().is_symlink()
        || after.len() != metadata.len()
        || after.modified()? != metadata.modified()?
    {
        return Err(io::Error::other(
            "private process input exceeded the frozen bound",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if after.dev() != metadata.dev() || after.ino() != metadata.ino() {
            return Err(io::Error::other(
                "private process input identity changed after read",
            ));
        }
    }
    Ok(bytes)
}

// Stream the exact qualified executable: this bound concerns input identity,
// independently of the unchanged output and fixture receipt budgets.
pub(super) fn executable_digest(path: &Path) -> io::Result<String> {
    const MAX_EXECUTABLE: u64 = 1024 * 1024 * 1024;
    let before = fs::symlink_metadata(path)?;
    if !before.is_file()
        || before.file_type().is_symlink()
        || before.len() == 0
        || before.len() > MAX_EXECUTABLE
        || fs::canonicalize(path)? != path
    {
        return Err(io::Error::other("qualified executable input refused"));
    }
    let mut input = File::open(path)?;
    let mut digest = Sha256::new();
    let mut block = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let count = input.read(&mut block)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("executable bound overflow"))?;
        if total > MAX_EXECUTABLE {
            return Err(io::Error::other("executable input exceeded bound"));
        }
        digest.update(&block[..count]);
    }
    let after = fs::symlink_metadata(path)?;
    if total != before.len()
        || after.len() != before.len()
        || after.modified()? != before.modified()?
        || fs::canonicalize(path)? != path
    {
        return Err(io::Error::other("qualified executable changed while read"));
    }
    Ok(format!("{:x}", digest.finalize()))
}

// Implemented parent integration contract in terminal_process_tests.rs:
// 1. Fresh mode0700 root + create_new owner marker (parent PID/nonce/executable
//    digest); refuse reuse, canonical path drift, symlinks and unsafe cleanup.
// 2. Pin current_exe digest and corpus; child checks the same immutable identity.
// 3. run_phase Prepare; retain private output; require success and exact bounded
//    source/actual-suspension receipt. Reap/join precedes any Resume spawn.
// 4. run_phase Resume; require distinct child PIDs, success and exact retained
//    state + PF8 ordinary false + drain/disconnect receipt. No reinstallation.
// 5. Recheck root owner marker/path and executable digest; remove only the whole
//    exclusive owned fixture. Require actual removal before emitting observation.
// Failed/unjoined child output refuses credit and fixture deletion, rather than
// widening lifecycle contracts, forcing state, retrying or fabricating receipts.
