//! Bounded subprocess execution: drained pipes, a hard deadline, and a
//! process-group stop for every child moss runs.
//!
//! Deliberately generic — nothing OnionPress-specific belongs here. Callers
//! own their deadlines and pass them in as plain [`std::time::Duration`]s;
//! the operation-specific numbers stay with the callers (see `stack_install`'s
//! `mod timeouts`). Extracted verbatim from `stack_install`, where the
//! "no unbounded child" invariant was earned.

/// What a bounded run produced. Same three fields as [`std::process::Output`],
/// but the streams arrive already decoded because every caller here
/// immediately lossy-decodes them anyway.
#[derive(Debug)]
pub struct BoundedOutput {
    pub status: std::process::ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Grace granted twice when a child has to be stopped: first to the launcher's
/// own EXIT trap, between the SIGTERM and the SIGKILL that follows it, and then
/// to the drain threads, to hand over what they read. Both are the same
/// judgement — the orderly path gets a moment, and then moss stops waiting.
pub const STOP_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

/// Drain a child pipe on its own thread, handing bytes over in chunks rather
/// than as one `read_to_end` at the end: the read may never reach EOF (see
/// [`run_bounded_with_tick`]), and chunks are what survives that.
fn drain_pipe<R: std::io::Read + Send + 'static>(
    pipe: Option<R>,
) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Some(mut pipe) = pipe else { return };
        let mut buf = [0u8; 8192];
        // Ends on EOF, on a read error, and on the receiver going away — the
        // last is what lets an abandoned thread exit instead of leaking.
        while let Ok(n) = pipe.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                return;
            }
        }
    });
    rx
}

/// What a drain thread has handed over, waiting at most [`STOP_GRACE`] for it
/// to finish. Deliberately lossy, and deliberately NOT a `join`: the bytes that
/// arrived are worth more than the ones still owed by a grandchild that may
/// never let go, so the thread is abandoned to die with the process. See
/// [`run_bounded_with_tick`].
fn collect_drained(rx: &std::sync::mpsc::Receiver<Vec<u8>>) -> String {
    let deadline = std::time::Instant::now() + STOP_GRACE;
    let mut buf = Vec::new();
    while let Ok(chunk) =
        rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
    {
        buf.extend_from_slice(&chunk);
        // A grandchild that keeps WRITING must not extend the wait either.
        if std::time::Instant::now() >= deadline {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// One member of a process group, as the kernel reports it.
///
/// `started_us` is microseconds since the epoch. The resolution matters: the
/// comparison in [`members_to_signal`] separates processes spawned milliseconds
/// apart, so anything coarser than microseconds would make it guesswork.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GroupMember {
    pid: i32,
    started_us: u64,
}

/// Which members of a process group moss is entitled to stop.
///
/// **A descendant of the child moss spawned cannot have started before it.** So
/// a member older than the group's leader was never part of the subtree moss
/// created, and signalling it is signalling a stranger.
///
/// Groups acquire strangers by pid recycling, which is not theoretical here.
/// `run_bounded_with_tick` reaps its child on the SUCCESS path, freeing that
/// pid and with it the process-group id — while live members can remain in the
/// now-leaderless group, because a vendor daemon backgrounded with plain
/// `nohup … &` stays in whatever group it was born into (nohup ignores SIGHUP;
/// it does not detach a session). A later child spawned with `process_group(0)`
/// that lands on the recycled pid becomes leader of that same group, and a
/// deadline on THAT child would then reach the stranger — with SIGKILL, so the
/// stranger never gets to clean up. Measured on a Mac 2026-08-18 while chasing
/// the OnionPress menu-bar SIGTERM: pids ran 99230 → 231 → 4063 inside four
/// minutes, i.e. the whole pid space wrapped twice over between two ordinary
/// installs.
///
/// The rule costs nothing on the path it has to protect: everything the wedged
/// child started is younger than the child, so a genuine subtree is still
/// stopped in full.
#[cfg(unix)]
fn members_to_signal(members: &[GroupMember], leader_started_us: u64) -> Vec<i32> {
    members
        .iter()
        .filter(|m| m.started_us >= leader_started_us)
        .map(|m| m.pid)
        .collect()
}

/// When `pid` started, in microseconds since the epoch, or `None` if the kernel
/// will not say (an exited or unreadable process).
#[cfg(target_os = "macos")]
fn process_started_us(pid: i32) -> Option<u64> {
    // SAFETY: `proc_bsdinfo` is a plain C struct of integers and fixed arrays,
    // so all-zero is a valid inhabitant. It is overwritten below before it is read.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: `proc_pidinfo` fills at most `size` bytes into `info`, which is a
    // correctly sized, zeroed `proc_bsdinfo`. It returns the bytes written.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    if n != size {
        return None;
    }
    Some(info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

/// Every live member of process group `pgid`, with its start time.
///
/// `proc_listpgrppids` returns a **count of pids**, not a byte count — unlike
/// `proc_listpids`, whose otherwise identical shape returns bytes. Dividing the
/// return value by `size_of::<i32>()` therefore yields 0 for any group smaller
/// than four processes, which reads as "the group is empty" and stops nothing.
/// Caught by the two wedged-child tests below, which is what they are for.
#[cfg(target_os = "macos")]
fn group_members(pgid: i32) -> Vec<GroupMember> {
    // SAFETY: a null buffer asks only for an upper bound on the count.
    let upper = unsafe { libc::proc_listpgrppids(pgid, std::ptr::null_mut(), 0) };
    if upper <= 0 {
        return Vec::new();
    }
    // Slack for members that appear between the sizing call and the read.
    let mut pids = vec![0i32; upper as usize + 16];
    let capacity_bytes = (pids.len() * std::mem::size_of::<i32>()) as i32;
    // SAFETY: `pids` owns exactly `capacity_bytes` writable bytes, and that is
    // what is offered.
    let count = unsafe {
        libc::proc_listpgrppids(pgid, pids.as_mut_ptr().cast(), capacity_bytes)
    };
    if count <= 0 {
        return Vec::new();
    }
    pids.truncate(count as usize);
    pids.into_iter()
        .filter(|p| *p > 0)
        .filter_map(|pid| {
            process_started_us(pid).map(|started_us| GroupMember { pid, started_us })
        })
        .collect()
}

/// Signal the part of `pgid` that moss started.
///
/// Falls back to signalling the WHOLE group whenever the refinement cannot be
/// applied — the leader's start time is unreadable, or the enumeration came
/// back empty (a failed query and a group that has already died look the same
/// from here). A subtree that outlives its deadline is the worse failure, and
/// it is the one this module was built around; the age filter is a refinement
/// of that rule, never a replacement for it.
///
/// The fallback cannot itself hit a stranger: the leader is deliberately left
/// unreaped until after the SIGKILL, so its pid — and with it this group id —
/// is not available for recycling while either pass runs.
#[cfg(unix)]
fn signal_group(pgid: i32, leader_started_us: Option<u64>, sig: i32) {
    #[cfg(target_os = "macos")]
    if let Some(started) = leader_started_us {
        // Re-enumerated per pass, so anything the group gained during
        // STOP_GRACE is caught by the SIGKILL.
        let targets = members_to_signal(&group_members(pgid), started);
        if !targets.is_empty() {
            for pid in targets {
                // SAFETY: a positive pid signals exactly that one process.
                unsafe { libc::kill(pid, sig) };
            }
            return;
        }
    }
    let _ = leader_started_us;
    // SAFETY: `kill` with a negative pid signals the process GROUP. The group
    // is one moss created via `process_group(0)` and still owns, because the
    // child below it has not been reaped yet.
    unsafe { libc::kill(-pgid, sig) };
}

/// Stop a child AND everything it started, as forcefully as it takes.
///
/// `Child::kill` sends SIGKILL to the DIRECT child only, and SIGKILL is the one
/// signal the launcher's `trap '… kill $(jobs -p) …' EXIT INT TERM HUP` never
/// sees — the script dies and its background children live on holding the
/// pipes. So the child is spawned into its own process group and the GROUP is
/// signalled: SIGTERM first, which that trap DOES see, so the launcher reaps
/// its own children the clean way, then SIGKILL for whatever ignored it. The
/// child is left unreaped until after the SIGKILL — while it is a zombie its
/// pid, and with it the process-group id, cannot be recycled onto someone else.
///
/// What the group may NOT reach is a process older than the child: see
/// [`members_to_signal`]. The leader's start time is read once, before the
/// SIGTERM, and reused — by SIGKILL time the leader may be a zombie the kernel
/// will no longer describe.
fn stop_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pgid = child.id() as i32;
        #[cfg(target_os = "macos")]
        let leader_started_us = process_started_us(pgid);
        #[cfg(not(target_os = "macos"))]
        let leader_started_us = None;
        signal_group(pgid, leader_started_us, libc::SIGTERM);
        std::thread::sleep(STOP_GRACE);
        signal_group(pgid, leader_started_us, libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = child.kill();
    let _ = child.wait();
}

/// Run a child to completion, STOPPING it if it outlives `timeout`, and calling
/// `tick` roughly every 100 ms while it runs.
///
/// Three failures this shape exists to prevent, all observed on the live path:
///
/// 1. **Pipe deadlock.** Piping stdout+stderr and then only polling `try_wait`
///    reads nothing until after the child exits. A child that fills the ~64 KB
///    pipe buffer blocks in `write` and can never exit, so the parent's
///    deadline measures BUFFER PRESSURE rather than elapsed work and reports a
///    timeout for a command that answered. Both pipes are therefore drained on
///    their own threads for the whole wait.
/// 2. **Unbounded waits.** `.output()` and a bare `try_wait` loop have no
///    deadline at all, so one wedged child (a `quit` that never returns) hangs
///    the Tauri command that called it with nothing left to cancel it.
/// 3. **The bound that wasn't.** Killing the child and then JOINING the drain
///    threads put the unbounded wait straight back one level down: the launcher
///    starts background children (`bundled_python … ensure-archive-s3-keys &`)
///    that inherit both pipes and outlive it, so `read_to_end` waits on the
///    GRANDCHILD. Measured at 8 s past a successful exit and forever past a
///    SIGKILLed one — all of it holding [`install_lock`], which left every later
///    install and every publish blocked until moss restarted. The child and the
///    collection of its output are now bounded independently
///    ([`stop_process_group`], [`collect_drained`]).
///
/// stdin is `/dev/null`: every subcommand moss drives here is non-interactive,
/// and a CLI that decides to prompt must hit EOF and fail rather than block
/// forever on a terminal that does not exist.
pub fn run_bounded_with_tick(
    cmd: &mut std::process::Command,
    timeout: std::time::Duration,
    what: &str,
    mut tick: impl FnMut(),
) -> Result<BoundedOutput, String> {
    #[cfg(unix)]
    {
        // Its own process group, so a deadline can reach the whole subtree
        // rather than only the direct child. See `stop_process_group`.
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run {what}: {e}"))?;

    let out = drain_pipe(child.stdout.take());
    let err = drain_pipe(child.stderr.take());

    let deadline = std::time::Instant::now() + timeout;
    let stopped = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            // A `try_wait` that errors leaves a child nobody is waiting on;
            // falling straight out of here used to leak the whole subtree and
            // both drain threads with it.
            Err(e) => break Err(format!("waiting on {what} failed: {e}")),
            Ok(None) if std::time::Instant::now() >= deadline => {
                break Err(format!(
                    "{what} timed out after {}s and was stopped",
                    timeout.as_secs()
                ))
            }
            Ok(None) => {
                tick();
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    };

    match stopped {
        // The child is gone, but its pipes may not be: collection is bounded
        // on the success path too, because a background grandchild holding
        // them open is the ordinary case, not the failure case.
        Ok(status) => Ok(BoundedOutput {
            status,
            stdout: collect_drained(&out),
            stderr: collect_drained(&err),
        }),
        Err(msg) => {
            stop_process_group(&mut child);
            // The receivers drop with this frame, which is what ends the drain
            // threads if anything is still holding a pipe open.
            Err(msg)
        }
    }
}

/// [`run_bounded_with_tick`] with nothing to do while waiting.
pub fn run_bounded(
    cmd: &mut std::process::Command,
    timeout: std::time::Duration,
    what: &str,
) -> Result<BoundedOutput, String> {
    run_bounded_with_tick(cmd, timeout, what, || {})
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    #[cfg(unix)]
    fn a_child_that_outtalks_the_pipe_buffer_still_finishes_inside_its_deadline() {
        // The deadlock this guards: piping stdout+stderr and only polling
        // `try_wait` reads nothing until the child exits, so a child that fills
        // the ~64 KB pipe buffer blocks in `write` and can never exit. The
        // deadline then measures buffer pressure and reports a timeout for a
        // command that had already done its work. 1 MB is well past any pipe
        // buffer; a generous 60 s deadline means a failure here is the
        // deadlock, not a slow machine.
        //
        // Written with `dd` and NO pipe. The obvious `yes | head -c N` form is
        // nondeterministic here: `head` exiting hands `yes` a SIGPIPE and the
        // SHELL reports the broken pipeline on its own stderr — the very
        // stream this test counts — so stderr intermittently measured 200_116
        // instead of 200_000 (~1 run in 5 under full-module parallelism).
        // Silencing the writers' stderr does not help; the diagnostic is the
        // shell's. No pipe, no SIGPIPE, no diagnostic.
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg(
            "dd if=/dev/zero bs=1000 count=1000 2>/dev/null; \
             dd if=/dev/zero bs=1000 count=200 >&2 2>/dev/null; \
             exit 3",
        );
        let out = run_bounded(&mut cmd, std::time::Duration::from_secs(60), "chatty child")
            .expect("must not time out on a full pipe");

        assert_eq!(out.stdout.len(), 1_000_000, "stdout drained in full");
        assert_eq!(out.stderr.len(), 200_000, "stderr drained too");
        assert_eq!(out.status.code(), Some(3), "the real exit status survives");
    }

    #[test]
    #[cfg(unix)]
    fn a_wedged_child_is_killed_at_its_deadline_rather_than_waited_on_forever() {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg("sleep 120");
        let started = std::time::Instant::now();
        let err = run_bounded(&mut cmd, std::time::Duration::from_millis(400), "sleeper")
            .expect_err("a child past its deadline is a failure, not a hang");

        assert!(err.contains("timed out"), "says what happened: {err}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "and returns promptly instead of waiting out the child"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_wedged_child_cannot_hide_behind_a_grandchild_holding_the_pipes() {
        // The bound that wasn't. `sh -c "sleep 120"` is a single command bash
        // EXECS, so there is no grandchild and killing the child closed the
        // pipes — which is why the test above passed while the live path hung.
        // The staged launcher is nothing like that: it starts background
        // children that inherit both pipes and are reaped by a `trap … EXIT`
        // SIGKILL never triggers. Measured: a 400 ms deadline returning after
        // 300 s, all of it holding the install lock.
        let tmp = TempDir::new().unwrap();
        let marker = tmp.path().join("the-grandchild-lived");
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg(format!(
            "(sleep 2; touch {}) & sleep 60",
            marker.display()
        ));
        let started = std::time::Instant::now();
        let err = run_bounded(&mut cmd, std::time::Duration::from_millis(400), "sleeper")
            .expect_err("a child past its deadline is a failure, not a hang");

        assert!(err.contains("timed out"), "says what happened: {err}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(15),
            "and returns instead of waiting on the grandchild (took {:?})",
            started.elapsed()
        );
        // The grandchild would touch its marker 2 s in. Wait past that moment
        // before concluding it never came — killing only the direct child
        // returns FASTER, not slower, so a bare check here proves nothing.
        std::thread::sleep(
            std::time::Duration::from_secs(4).saturating_sub(started.elapsed()),
        );
        assert!(
            !marker.exists(),
            "the WHOLE process group is stopped — a grandchild that outlives the deadline keeps \
             both pipes open and holds the install lock with them"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_successful_child_is_not_held_open_by_what_it_left_running() {
        // The same pipe, the other path: the child exits 0 having started
        // something that outlives it, so the drain threads never see EOF and
        // joining them waits on the GRANDCHILD. Measured at 8 s past a
        // successful exit; here the grandchild would hold it for 30.
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg("echo done; sleep 30 & exit 0");
        let started = std::time::Instant::now();
        let out = run_bounded(&mut cmd, std::time::Duration::from_secs(60), "backgrounder")
            .expect("a child that exited is a result, not a wait");

        assert!(out.status.success(), "its own exit status is what is reported");
        assert_eq!(out.stdout.trim(), "done", "and what it managed to say survives");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(15),
            "output collection is bounded too (took {:?})",
            started.elapsed()
        );
    }

    #[test]
    #[cfg(unix)]
    fn everything_the_child_started_is_still_in_the_stop_set() {
        // The invariant the group stop was earned for: a wedged subtree goes
        // in full. Every descendant is younger than the leader, so the age
        // filter never subtracts from it.
        let members = [
            GroupMember { pid: 100, started_us: 1_000_000 },
            GroupMember { pid: 101, started_us: 1_000_001 },
            GroupMember { pid: 102, started_us: 9_000_000 },
        ];
        assert_eq!(members_to_signal(&members, 1_000_000), vec![100, 101, 102]);
    }

    #[test]
    #[cfg(unix)]
    fn a_group_member_older_than_the_leader_is_not_moss_s_to_stop() {
        // The recycled-pgid stranger: it was in this group before moss's child
        // existed, so it cannot be anything the child started.
        let members = [
            GroupMember { pid: 90958, started_us: 500_000 },
            GroupMember { pid: 100, started_us: 1_000_000 },
            GroupMember { pid: 101, started_us: 1_000_400 },
        ];
        assert_eq!(members_to_signal(&members, 1_000_000), vec![100, 101]);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_process_that_predates_the_leader_survives_a_group_stop() {
        // The recycled-pgid shape, built by hand. Under real recycling the
        // stranger joins because the kernel handed its group id to a new
        // leader; here perl joins it deliberately, because what the rule reads
        // is the ORDER, not how the membership came about. The stranger is the
        // OnionPress menu bar app in miniature: alive, healthy, and in moss's
        // group only because `nohup … &` never left it.
        let tmp = TempDir::new().unwrap();
        let handoff = tmp.path().join("pgid");
        let mut stranger = std::process::Command::new("perl")
            .arg("-e")
            .arg(
                "my $f = $ARGV[0]; \
                 until (-s $f) { select(undef, undef, undef, 0.02) } \
                 open my $h, '<', $f; chomp(my $p = <$h>); \
                 setpgrp(0, $p); sleep 30",
            )
            .arg(&handoff)
            .stdin(std::process::Stdio::null())
            .spawn()
            .expect("perl ships with macOS");
        // Strictly older than the leader. Start times are microseconds, so this
        // is far more separation than the comparison needs.
        std::thread::sleep(std::time::Duration::from_millis(300));

        let mut child = {
            use std::os::unix::process::CommandExt;
            let mut cmd = std::process::Command::new("sh");
            cmd.arg("-c").arg("sleep 30").process_group(0);
            cmd.spawn().expect("spawn the group leader")
        };
        let pgid = child.id() as i32;
        std::fs::write(&handoff, format!("{pgid}\n")).unwrap();

        let waited = std::time::Instant::now();
        // SAFETY: `getpgid` is a read-only query.
        while unsafe { libc::getpgid(stranger.id() as i32) } != pgid {
            assert!(
                waited.elapsed() < std::time::Duration::from_secs(5),
                "the stranger never joined the group — the test proves nothing"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        stop_process_group(&mut child);

        // SAFETY: signal 0 delivers nothing; it only asks whether the process
        // is still there to be signalled.
        let alive = unsafe { libc::kill(stranger.id() as i32, 0) } == 0;
        let _ = stranger.kill();
        let _ = stranger.wait();
        assert!(
            alive,
            "a healthy process older than the group leader is not part of the \
             subtree moss started, and a moss deadline must never reach it"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_child_that_prompts_reads_eof_instead_of_blocking_on_a_terminal() {
        // `onionpress setup` prompts interactively when it does not like its
        // arguments. With an inherited stdin under moss that is an unkillable
        // wait; with /dev/null it is an immediate EOF.
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg("read line; echo \"got:[$line]\"");
        let out = run_bounded(&mut cmd, std::time::Duration::from_secs(30), "prompting child")
            .expect("must not hang waiting for input");
        assert_eq!(out.stdout.trim(), "got:[]");
    }
}
