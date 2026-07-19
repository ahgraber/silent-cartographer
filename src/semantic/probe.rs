//! A bounded probe of a short-lived external tool: spawn it, drain its output into capped buffers,
//! and give up after a deadline so a hung or wedged child can never block the caller forever.
//!
//! Every readiness/version probe of an indexer or interpreter routes through [`run_bounded`]. The
//! long-running real indexing work (`rust-analyzer scip`, `scip-python index`) does not: that is
//! unbounded by design, so those call sites keep a plain blocking invocation.

use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The deadline every production probe passes to [`run_bounded`]: a tool that has not answered a
/// version/readiness probe within this window is treated as unresponsive rather than waited on
/// indefinitely. Test call sites pass a sub-second deadline so the suite stays fast; only the
/// process-level tests that must observe a real timeout pay this full duration.
pub const PROBE_DEADLINE: Duration = Duration::from_secs(10);

/// The per-stream capture cap: at most this many bytes of stdout (and of stderr) are retained. The
/// drain threads keep reading past the cap and discard the excess, so a tool that floods a pipe
/// never blocks on a full pipe, yet the caller never buffers unbounded output.
const CAPTURE_CAP: usize = 64 * 1024;

/// How often the poll loop wakes to check whether the child has exited before the deadline.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// How long the success path waits for the drain threads to reach EOF after the child exits. Both
/// pipes are normally at EOF the moment the child exits, so the wait is momentary — the grace only
/// binds when a surviving descendant inherited a pipe write-end, in which case the readers are
/// abandoned (as on the timeout path) and the capture-so-far is returned.
const JOIN_GRACE: Duration = Duration::from_secs(1);

/// What a completed probe captured: the (capped) stdout and stderr bytes and the child's exit status.
///
/// The capture is best-effort: when a descendant of the exited child kept a pipe open past the join
/// grace, a stream holds whatever had arrived by then rather than a guaranteed-complete read.
#[derive(Debug)]
pub struct ProbeCapture {
    /// The child's standard output, truncated to [`CAPTURE_CAP`] bytes.
    pub stdout: Vec<u8>,
    /// The child's standard error, truncated to [`CAPTURE_CAP`] bytes.
    pub stderr: Vec<u8>,
    /// The child's exit status.
    pub status: ExitStatus,
}

/// The outcome of a bounded probe: the child either completed within the deadline, or the deadline
/// expired and the child was killed.
#[derive(Debug)]
pub enum ProbeOutcome {
    /// The child exited on its own before the deadline.
    Completed(ProbeCapture),
    /// The deadline expired; the child was killed and reaped. No output is returned — a wedged tool's
    /// partial output is not trustworthy, and the reader threads were abandoned rather than joined.
    TimedOut,
}

/// Spawn `command`, drain its stdout and stderr into capped buffers, and wait up to `deadline` for it
/// to exit. On exit, returns [`ProbeOutcome::Completed`] with the captured output and status. If the
/// deadline expires first, kills and reaps the direct child and returns [`ProbeOutcome::TimedOut`].
///
/// The caller passes a fully configured `command` (program, args, `current_dir`, `env`); this
/// function sets its stdout/stderr to piped and spawns it. A spawn failure surfaces as the returned
/// `io::Error`.
///
/// # Why the reader threads are abandoned rather than joined unboundedly
///
/// Two threads drain the pipes so the child can never block writing into a full pipe while we wait.
/// Neither path joins them without a bound, because a reader can be un-joinable: killing (or
/// outliving) the direct child does not close its descendants' handles, and a surviving grandchild
/// can inherit and hold a pipe write-end open. A reader blocked on such a pipe never reaches EOF, so
/// an unbounded join would block this thread forever — re-creating the exact hang this helper exists
/// to remove.
///
/// - **Timeout path:** the direct child is killed and reaped, and the readers are abandoned outright.
/// - **Success path:** the child has exited, so both pipes are normally at EOF and the readers finish
///   momentarily; they are waited on only up to a short grace ([`JOIN_GRACE`]), and a reader still
///   running past it — a descendant of the cleanly-exited child holding the pipe — is abandoned the
///   same way, returning the capture accumulated so far (best-effort by contract).
///
/// Abandoned threads are harmless detached threads, reaped by the OS when the (short-lived) process
/// exits.
///
/// The `wait-timeout` crate was declined: it bounds the wait on the child, but the drain-thread
/// machinery that dominates this helper — the two capped readers and the abandon rules — would still
/// be needed on top of it, so it removes no complexity here.
pub fn run_bounded(mut command: Command, deadline: Duration) -> std::io::Result<ProbeOutcome> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;

    // Take the pipe ends and hand each to a drain thread; unwrap is safe because both were just set
    // to piped above.
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let stdout_reader = spawn_drain(stdout);
    let stderr_reader = spawn_drain(stderr);

    let start = Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) => {
                // The child exited: collect each capture under the bounded join grace — see the
                // doc comment for why even the success-path wait must be bounded.
                let stdout = stdout_reader.collect(JOIN_GRACE);
                let stderr = stderr_reader.collect(JOIN_GRACE);
                return Ok(ProbeOutcome::Completed(ProbeCapture { stdout, stderr, status }));
            }
            None => {
                if start.elapsed() >= deadline {
                    // Kill and reap the direct child, then abandon the reader threads — see the
                    // doc comment: a descendant may hold a pipe open, so a join could never return.
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(ProbeOutcome::TimedOut);
                }
                thread::sleep(POLL_INTERVAL);
            }
        }
    }
}

/// A running drain thread paired with the shared buffer it appends into, so the capture-so-far is
/// readable even when the thread must be abandoned rather than joined.
struct DrainHandle {
    /// The capped capture the drain thread appends into as bytes arrive.
    buffer: Arc<Mutex<Vec<u8>>>,
    /// The drain thread itself.
    handle: JoinHandle<()>,
}

impl DrainHandle {
    /// The captured bytes, waiting up to `grace` for the drain thread to reach EOF. A thread still
    /// running past the grace — a descendant holding the pipe write-end open — is abandoned, and the
    /// capture accumulated so far is returned.
    fn collect(self, grace: Duration) -> Vec<u8> {
        let start = Instant::now();
        while !self.handle.is_finished() {
            if start.elapsed() >= grace {
                // Abandon the pipe-blocked reader; snapshot whatever it has captured. The abandoned
                // thread may append more afterwards, but this clone is the answer.
                return self.buffer.lock().map(|b| b.clone()).unwrap_or_default();
            }
            thread::sleep(POLL_INTERVAL);
        }
        let _ = self.handle.join();
        self.buffer.lock().map(|b| b.clone()).unwrap_or_default()
    }
}

/// Spawn a thread that drains `reader` to EOF, retaining at most [`CAPTURE_CAP`] bytes in the shared
/// buffer and discarding the rest so the child never blocks on a full pipe.
fn spawn_drain<R: Read + Send + 'static>(mut reader: R) -> DrainHandle {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::clone(&buffer);
    let handle = thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let Ok(mut captured) = shared.lock() else {
                        break;
                    };
                    if captured.len() < CAPTURE_CAP {
                        let room = CAPTURE_CAP - captured.len();
                        captured.extend_from_slice(&chunk[..n.min(room)]);
                    }
                    // Past the cap, keep reading and discard: draining the pipe is what stops the
                    // child from blocking on a full pipe.
                }
                Err(_) => break,
            }
        }
    });
    DrainHandle { buffer, handle }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    // A child that exits on its own is reported completed, with its stdout captured and a success
    // status.
    #[test]
    fn completed_child_returns_output_and_status() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("printf hello");
        let outcome = run_bounded(cmd, Duration::from_secs(5)).expect("spawns");
        match outcome {
            ProbeOutcome::Completed(capture) => {
                assert!(capture.status.success(), "the child exited zero");
                assert_eq!(capture.stdout, b"hello", "stdout is captured");
            }
            ProbeOutcome::TimedOut => panic!("a fast child must not time out"),
        }
    }

    // A child that outlives the deadline is killed and reported timed out — and the call returns close
    // to the deadline, not after the child's own long sleep.
    #[test]
    fn hung_child_times_out_promptly() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 30");
        let start = Instant::now();
        let outcome = run_bounded(cmd, Duration::from_millis(150)).expect("spawns");
        let elapsed = start.elapsed();
        assert!(matches!(outcome, ProbeOutcome::TimedOut), "the hung child timed out");
        assert!(
            elapsed < Duration::from_secs(5),
            "the call returns near the deadline, not after the child's sleep: {elapsed:?}"
        );
    }

    // A child that exits 0 while a backgrounded grandchild inherits and holds its stdout open: the
    // success path must not join the pipe-blocked reader unboundedly — it waits only the join grace,
    // abandons the reader, and returns Completed with the output captured up to the exit.
    #[test]
    fn clean_exit_with_a_descendant_holding_the_pipe_returns_promptly_with_the_capture() {
        let mut cmd = Command::new("sh");
        // The grandchild inherits stdout and outlives the parent, which exits 0 immediately.
        cmd.arg("-c").arg("printf hello\nsleep 30 &\nexit 0");
        let start = Instant::now();
        let outcome = run_bounded(cmd, Duration::from_secs(10)).expect("spawns");
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "the success-path join is bounded, not blocked by the held pipe: {elapsed:?}"
        );
        match outcome {
            ProbeOutcome::Completed(capture) => {
                assert!(capture.status.success(), "the child exited zero");
                assert_eq!(capture.stdout, b"hello", "the capture up to the exit is returned");
            }
            ProbeOutcome::TimedOut => panic!("a cleanly-exited child must report completed"),
        }
    }

    // A grandchild that inherits stdout and outlives its killed parent holds the pipe open; the probe
    // must still return (the reader threads are abandoned, not joined) rather than hang.
    #[test]
    fn descendant_holding_the_pipe_does_not_block_the_probe() {
        let mut cmd = Command::new("sh");
        // Background a grandchild that inherits stdout, then the parent waits on it: killing the
        // parent leaves the grandchild holding the stdout write-end open.
        cmd.arg("-c").arg("sleep 30 & wait");
        let start = Instant::now();
        let outcome = run_bounded(cmd, Duration::from_millis(150)).expect("spawns");
        let elapsed = start.elapsed();
        assert!(
            matches!(outcome, ProbeOutcome::TimedOut),
            "timed out despite the held pipe"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "the probe returns without joining the pipe-blocked reader: {elapsed:?}"
        );
    }

    // A child that floods stdout well past the cap is drained (it never blocks) and its capture is
    // bounded to the cap.
    #[test]
    fn flooded_output_is_capped_not_unbounded() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("head -c 200000 /dev/zero");
        let outcome = run_bounded(cmd, Duration::from_secs(10)).expect("spawns");
        match outcome {
            ProbeOutcome::Completed(capture) => {
                assert!(capture.status.success(), "the flooding child still exits zero");
                assert!(
                    capture.stdout.len() <= CAPTURE_CAP,
                    "the capture is bounded to the cap: {}",
                    capture.stdout.len()
                );
            }
            ProbeOutcome::TimedOut => panic!("a child that exits after flooding must complete"),
        }
    }
}
