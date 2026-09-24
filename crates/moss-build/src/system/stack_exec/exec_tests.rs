use super::*;
use crate::plugins::contributions::stack::{StackContribution, StackHooks, StackSource};
use tempfile::TempDir;

fn stack_with_argv(start: Vec<&str>, uninstall: Vec<&str>) -> StackContribution {
    StackContribution {
        id: "widget".to_string(),
        display_name: None,
        version: "1.0.0".to_string(),
        sources: vec![StackSource::Path { platform: None, binary: "bin/widget".to_string() }],
        start: start.into_iter().map(str::to_string).collect(),
        stop: vec![],
        uninstall: uninstall.into_iter().map(str::to_string).collect(),
        recover: vec![],
        self_heal_grace_secs: None,
        preserve_on_uninstall: vec![],
        hooks: StackHooks::default(),
    }
}

/// A stand-in for a stack's binary: a `#!/bin/sh` script, in the same shape
/// as `fake_onionpress` in the desktop app's stack installer tests,
/// lifted here stack-agnostic. It prints its own argv, working directory and
/// one env var, one per line, so a test can pin all three from
/// [`super::run`]'s `BoundedOutput::stdout` without a real subprocess
/// framework.
#[cfg(unix)]
fn fake_stack_binary(home: &Path, script: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = home.join("bin/widget");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

#[test]
#[cfg(unix)]
fn the_declared_start_argv_is_what_runs() {
    let home_dir = TempDir::new().unwrap();
    fake_stack_binary(
        home_dir.path(),
        r#"echo "ARGV:$@"; echo "CWD:$(pwd)"; echo "ENV:$WIDGET_TEST_VAR""#,
    );

    let stack = stack_with_argv(vec!["start", "--managed"], vec![]);
    let home = StackHome { root: home_dir.path(), stack: &stack };
    let env = [("WIDGET_TEST_VAR".to_string(), "hello".to_string())];
    let opts = RunOpts {
        timeout: Duration::from_secs(5),
        env: &env,
        cwd: Some(home_dir.path()),
        tick: None,
    };

    let out = run(&home, Verb::Start, opts).expect("the declared start argv runs the fake binary");
    assert!(out.status.success());
    assert!(out.stdout.contains("ARGV:start --managed"), "got: {}", out.stdout);
    // Canonicalize: on macOS `TempDir::path()` comes back under `/var`, a
    // symlink to `/private/var`, and the subprocess's own `pwd` resolves it —
    // comparing the raw path against a real `getcwd()` fails on every macOS
    // run regardless of what cwd the executor actually passed.
    let expected_cwd = std::fs::canonicalize(home_dir.path()).unwrap();
    assert!(
        out.stdout.contains(&format!("CWD:{}", expected_cwd.display())),
        "got: {}",
        out.stdout
    );
    assert!(out.stdout.contains("ENV:hello"), "got: {}", out.stdout);
}

#[test]
#[cfg(unix)]
fn a_verb_the_stack_does_not_declare_is_not_invented() {
    let home_dir = TempDir::new().unwrap();
    // The binary succeeds unconditionally — an invented `["uninstall"]`
    // default would come back `Ok`, so the refusal below can only be the
    // typed one, caught before the binary is ever reached.
    fake_stack_binary(home_dir.path(), "exit 0");

    let stack = stack_with_argv(vec!["start"], vec![]);
    let home = StackHome { root: home_dir.path(), stack: &stack };
    let opts = RunOpts { timeout: Duration::from_secs(5), env: &[], cwd: None, tick: None };

    let err = run(&home, Verb::Uninstall, opts).expect_err("an undeclared verb must be refused");
    match err {
        StackExecError::Run(msg) => {
            assert!(msg.contains("widget"), "the refusal names the stack: {msg}");
            assert!(msg.contains("uninstall"), "the refusal names the verb: {msg}");
        }
        other => panic!("expected StackExecError::Run, got {other:?}"),
    }
}
