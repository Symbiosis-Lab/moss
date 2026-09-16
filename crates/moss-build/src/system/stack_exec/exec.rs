//! `run`: drive one of a stack's declared verbs through
//! [`crate::system::bounded_command`]. The declaration is the only source of
//! argv — a verb it does not declare is a typed refusal, never a guessed
//! default, because inventing `["uninstall"]` for a stack that named no
//! uninstall argv would run something moss's own review never saw.

use std::path::Path;
use std::time::Duration;

use crate::system::bounded_command::{run_bounded, run_bounded_with_tick, BoundedOutput};

use super::{StackExecError, StackHome};

/// A tick callback for [`RunOpts`], the same shape
/// [`crate::system::bounded_command::run_bounded_with_tick`] already takes.
pub type TickFn<'a> = dyn FnMut() + 'a;

/// Which of a stack's declared argv lists to run. `Stop` carries no variant
/// here — S5 owns the typed `StopReason` — so the app's existing `quit_stack`
/// argv keeps going through `Raw` until then. `Raw` also skips the
/// declaration entirely: its argv is the caller's own, not read off `start`
/// or `uninstall`.
pub enum Verb {
    Start,
    Uninstall,
    Raw(Vec<String>),
}

/// Everything the run needs beyond the verb itself. `cwd: None` leaves the
/// child's working directory unset (inherits the caller's own, matching
/// today's `stack_install` behavior); `env` is added to, never replacing,
/// the child's inherited environment.
pub struct RunOpts<'a> {
    pub timeout: Duration,
    pub env: &'a [(String, String)],
    pub cwd: Option<&'a Path>,
    pub tick: Option<&'a mut TickFn<'a>>,
}

/// Run one declared verb's argv through the resolved binary
/// ([`super::layout::binary_path`]). `Verb::Start`/`Verb::Uninstall` read
/// their argv off the declaration and refuse — never invent a default — when
/// the stack declares none; `Verb::Raw` runs exactly the argv it is given.
pub fn run(home: &StackHome<'_>, verb: Verb, mut opts: RunOpts<'_>) -> Result<BoundedOutput, StackExecError> {
    let (argv, verb_name) = match &verb {
        Verb::Start => (&home.stack.start, "start"),
        Verb::Uninstall => (&home.stack.uninstall, "uninstall"),
        Verb::Raw(argv) => (argv, "raw"),
    };
    if argv.is_empty() && !matches!(verb, Verb::Raw(_)) {
        return Err(StackExecError::Run(format!(
            "stack '{}' declares no {verb_name} argv",
            home.stack.id
        )));
    }

    let bin = super::layout::binary_path(home.root, home.stack)?;
    let mut cmd = std::process::Command::new(&bin);
    cmd.args(argv);
    if let Some(cwd) = opts.cwd {
        cmd.current_dir(cwd);
    }
    for (k, v) in opts.env {
        cmd.env(k, v);
    }

    let what = format!("stack '{}' {verb_name}", home.stack.id);
    let result = match opts.tick.take() {
        Some(tick) => run_bounded_with_tick(&mut cmd, opts.timeout, &what, tick),
        None => run_bounded(&mut cmd, opts.timeout, &what),
    };
    result.map_err(StackExecError::Run)
}

#[cfg(test)]
#[path = "exec_tests.rs"]
mod exec_tests;
