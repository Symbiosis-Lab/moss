//! Tests for project agent-guidance sync.
//!
//! The load-bearing ones are the *negative* assertions: what sync must NOT
//! write. The site folder is the author's writing, and every regression in this
//! area has been moss putting something where it did not belong.

use super::*;

/// TempDir under the repo's gitignored `target/test-tmp/`, per project
/// convention (mirrors `moss_paths::tests::make_tmp`).
fn make_project() -> (tempfile::TempDir, PathBuf) {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = tempfile::TempDir::new_in(&base).expect("tempdir");
    let project = tmp.path().join("site");
    std::fs::create_dir_all(project.join(".moss")).expect("create .moss");
    (tmp, project)
}

fn cli() -> PathBuf {
    PathBuf::from("/Applications/moss.app/Contents/MacOS/moss")
}

/// A home directory with no agent markers.
///
/// Passed explicitly rather than exported as `$HOME`. `$HOME` is
/// process-global: a test that repoints it corrupts every other test running
/// in parallel, and `vault::paths` has one that resolves the real home and
/// fails outright when it moves. That is why `sync_with_home` exists.
fn empty_home() -> tempfile::TempDir {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create target/test-tmp");
    tempfile::TempDir::new_in(&base).expect("fake home")
}

/// `empty_home()` with `names` present, e.g. `home_with(&[".claude"])`.
fn home_with(names: &[&str]) -> tempfile::TempDir {
    let home = empty_home();
    for n in names {
        std::fs::create_dir_all(home.path().join(n)).expect("create marker");
    }
    home
}

#[test]
fn writes_the_claude_skill_when_claude_is_present() {
    let (_tmp, project) = make_project();
    std::fs::create_dir_all(project.join(".claude")).unwrap();

    let home = empty_home();
    let report = sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    let skill = project.join(".claude/skills/moss/SKILL.md");
    assert!(skill.is_file());
    assert!(!report.written.is_empty());

    // A pointer, not a copy: the prose lives in the binary and this names the
    // command that prints it. A fresh folder gets no `references/` directory at
    // all — those exist only as stubs, and only where an older moss left one.
    let text = std::fs::read_to_string(&skill).expect("skill");
    assert!(text.contains("moss guide"), "the skill never names `moss guide`: {text}");
    assert!(
        !project.join(".claude/skills/moss/references").exists(),
        "a fresh project should get no reference copies"
    );
}

/// The constraint that shapes this whole module. moss must not leave anything
/// in the author's folder — everything it writes lands inside an agent's own
/// dot directory, or inside `.moss/`.
#[test]
fn writes_nothing_into_the_authors_content() {
    let (_tmp, project) = make_project();
    std::fs::create_dir_all(project.join(".claude")).unwrap();

    let home = empty_home();
    sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    assert!(!project.join("AGENTS.md").exists(), "moss does not write into the author's folder");
    assert!(!project.join("CLAUDE.md").exists());
}

#[test]
fn does_not_create_a_directory_for_an_undetected_agent() {
    let (_tmp, project) = make_project();
    std::fs::create_dir_all(project.join(".claude")).unwrap();

    let home = empty_home();
    sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    assert!(!project.join(".cursor").exists(), "Cursor is not here; do not invent it");
}

#[test]
fn writes_the_cursor_rule_when_cursor_is_present() {
    let (_tmp, project) = make_project();
    std::fs::create_dir_all(project.join(".cursor")).unwrap();

    let home = empty_home();
    sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    let mdc = std::fs::read_to_string(project.join(".cursor/rules/moss.mdc")).expect("rule written");
    assert!(mdc.starts_with("---\n"), "Cursor needs its frontmatter on line 1");
    assert!(mdc.contains("moss:agent-guidance"), "must carry the stamp");
}

#[test]
fn a_folder_that_is_not_a_moss_project_is_left_entirely_alone() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::TempDir::new_in(&base).unwrap();
    let plain = tmp.path().join("not-a-site");
    std::fs::create_dir_all(plain.join(".claude")).unwrap();

    let home = empty_home();
    let report = sync_with_home(&plain, Some(home.path()), &cli()).expect("sync");

    assert!(report.is_noop());
    assert!(!plain.join(".claude/skills").exists());
}

/// Runs at every project open and every full build. A second run that rewrites
/// files would churn mtimes, wake the watcher, and trigger an iCloud resync.
#[test]
fn a_second_sync_writes_nothing() {
    let (_tmp, project) = make_project();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::create_dir_all(project.join(".cursor")).unwrap();

    let home = empty_home();
    let sync = || sync_with_home(&project, Some(home.path()), &cli());
    let first = sync().expect("first");
    assert!(!first.written.is_empty(), "first run writes");
    let second = sync().expect("second");
    assert!(second.is_noop(), "second run must be a no-op, wrote {:?}", second.written);
    // A file moss wrote one call ago cannot be the user's. This assertion is
    // the one that catches a stamp moss writes but cannot read back: without
    // it, "user-owned" and "already current" are indistinguishable here,
    // because `is_noop` counts both as nothing-to-do.
    assert!(
        second.user_owned.is_empty(),
        "moss must recognize its own writes, but disowned {:?}",
        second.user_owned
    );
}

/// The three managed files are three different shapes — plain markdown, YAML
/// frontmatter for Claude Code, YAML frontmatter for Cursor — and the stamp has
/// to survive a round trip through each. A stamp moss writes and cannot read
/// back looks exactly like consent: the file freezes at whatever the first
/// build wrote, forever, and never updates again.
#[test]
fn every_managed_file_is_refreshed_when_it_drifts() {
    let (_tmp, project) = make_project();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::create_dir_all(project.join(".cursor")).unwrap();

    let home = empty_home();
    let sync = || sync_with_home(&project, Some(home.path()), &cli());
    sync().expect("first");

    for rel in [
        ".claude/skills/moss/SKILL.md",
        ".cursor/rules/moss.mdc",
        ".moss/agents/SKILL.md",
    ] {
        let path = project.join(rel);
        let original = std::fs::read_to_string(&path).expect(rel);
        assert!(
            original
                .lines()
                .take(16)
                .any(|l| l.starts_with("<!-- moss:agent-guidance v")),
            "{rel} carries no stamp moss can find"
        );
        // Stand in for an older moss release: different content, but a stamp
        // that is valid *for that content*. Hand-corrupting the body instead
        // would test the opposite thing — that is a user edit, and the whole
        // point of hashing the body is that moss then keeps its hands off.
        std::fs::write(&path, stamped_with("stale\n", "")).unwrap();

        let report = sync().expect("refresh");
        assert!(
            report.written.contains(&rel.to_string()),
            "{rel} drifted and was not refreshed; report: {report:?}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original, "{rel} not restored");
    }
}

/// Claude Code and Cursor both read a file's name and description from YAML
/// frontmatter that must open the file. Push the `---` down by one line and the
/// frontmatter stops parsing: the guidance is still on disk and no longer
/// discoverable, which is worse than not writing it.
#[test]
fn frontmatter_still_opens_the_files_that_need_it() {
    let (_tmp, project) = make_project();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::create_dir_all(project.join(".cursor")).unwrap();

    let home = empty_home();
    sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    for rel in [".claude/skills/moss/SKILL.md", ".cursor/rules/moss.mdc"] {
        let text = std::fs::read_to_string(project.join(rel)).expect(rel);
        assert!(text.starts_with("---\n"), "{rel} must open with its frontmatter");
        let (_, after) = text[4..].split_once("\n---\n").unwrap_or_else(|| {
            panic!("{rel} has an opening delimiter with no closing one")
        });
        assert!(
            after.starts_with("<!-- moss:agent-guidance v"),
            "{rel}: the stamp belongs on the first line after the frontmatter"
        );
    }

    // The agent-neutral copy has no frontmatter to work around, and no reason
    // to carry Cursor's `globs:`/`alwaysApply:` keys.
    let neutral = std::fs::read_to_string(project.join(".moss/agents/SKILL.md")).expect("neutral");
    assert!(neutral.starts_with("<!-- moss:agent-guidance v"));
    assert!(!neutral.contains("alwaysApply"), "the neutral copy must not be Cursor's rendering");
}

/// The neutral copy lives in `.moss/`, moss's own directory, not the author's
/// folder, so it is written unconditionally. This is the only guidance a
/// config-directory-less agent (Codex, Gemini CLI, aider) ever gets, and it
/// must not depend on `.claude`/`.cursor` being detected.
#[test]
fn the_neutral_skill_is_always_written() {
    let (_tmp, project) = make_project();
    let home = empty_home();

    sync_with_home(&project, Some(home.path()), &cli()).expect("sync");
    assert!(
        project.join(".moss/agents/SKILL.md").is_file(),
        "the neutral copy is written even with no agent detected"
    );
    assert!(!project.join("AGENTS.md").exists(), "moss does not write into the author's folder");
}

/// The consent boundary. Once a user has edited a file, it is theirs — even at
/// a path moss created.
#[test]
fn a_user_edited_file_is_never_overwritten() {
    let (_tmp, project) = make_project();

    let home = empty_home();
    sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    // The user rewrites it, dropping moss's stamp.
    std::fs::write(project.join(".moss/agents/SKILL.md"), "# My own instructions\n").unwrap();

    let report = sync_with_home(&project, Some(home.path()), &cli()).expect("resync");
    assert_eq!(
        std::fs::read_to_string(project.join(".moss/agents/SKILL.md")).unwrap(),
        "# My own instructions\n",
        "moss must not clobber an edited file"
    );
    assert!(report.user_owned.contains(&".moss/agents/SKILL.md".to_string()));
}

/// Keeping the stamp line is not consent, and this is the edit people actually
/// make: append your house rules under moss's block and leave the top of the
/// file alone. Coding agents do it unprompted.
///
/// Two independent reviews found this the same way, because a stamp-presence
/// check reads the whole file as moss's: the next build restored moss's text
/// verbatim, reported it as `written` rather than `user_owned` so the
/// "left alone" log line never fired.
#[test]
fn an_edit_under_moss_s_block_is_still_the_users() {
    let (_tmp, project) = make_project();
    std::fs::create_dir_all(project.join(".claude")).unwrap();

    let home = empty_home();
    sync_with_home(&project, Some(home.path()), &cli()).expect("on");

    for rel in [".moss/agents/SKILL.md", ".claude/skills/moss/SKILL.md"] {
        let path = project.join(rel);
        let mine = format!(
            "{}\n## House rules\n\nNever touch `posts/2019/`.\n",
            std::fs::read_to_string(&path).expect(rel).trim_end()
        );
        std::fs::write(&path, &mine).unwrap();

        let report = sync_with_home(&project, Some(home.path()), &cli()).expect("resync");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), mine, "{rel} was clobbered");
        assert!(
            report.user_owned.contains(&rel.to_string()),
            "{rel} must be reported as the user's; report: {report:?}"
        );
    }
}

/// Quoting moss's own convention is not consent either. The stamp used to be
/// matched by prefix anywhere in the first 16 lines, so an authored file
/// documenting the marker — inside a fence, as prose about moss — read as
/// moss's, and the next build replaced it with moss's content.
#[test]
fn a_file_that_merely_quotes_the_stamp_is_not_moss_s() {
    let (_tmp, project) = make_project();
    let authored = "# House rules\n\n\
         moss stamps the files it manages like this:\n\n\
         ```\n<!-- moss:agent-guidance v1 h:0000000000000000 -->\n```\n\n\
         Never delete that line.\n";
    std::fs::create_dir_all(project.join(".moss/agents")).unwrap();
    std::fs::write(project.join(".moss/agents/SKILL.md"), authored).unwrap();

    let home = empty_home();
    let report = sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    assert!(
        report.user_owned.contains(&".moss/agents/SKILL.md".to_string()),
        "a file that merely mentions the marker is the author's: {report:?}"
    );
    assert_eq!(std::fs::read_to_string(project.join(".moss/agents/SKILL.md")).unwrap(), authored);
}

/// The stamp sits after any frontmatter, so [`split_stamp`] scans a window
/// rather than line 1 — and a file whose stamp falls outside that window is one
/// moss can write and never read back: permanently `user_owned`, and never
/// refreshed again. `references/example.md`
/// already contains `---` lines; the day one of them opens a file, the window
/// is what saves it.
#[test]
fn every_managed_file_stamps_inside_the_window() {
    let stubs = skill_package::topics()
        .into_iter()
        .map(|t| skill_package::render_reference_stub(&t));
    let bodies = [
        skill_package::render_pointer(),
        skill_package::render_pointer_with_frontmatter(),
        skill_package::render_cursor_mdc(),
    ]
    .into_iter()
    .chain(stubs);

    for body in bodies {
        for extra in ["", &cli_binding(&cli())] {
            let text = stamped_with(&body, extra);
            let line = text
                .lines()
                .position(|l| l.starts_with("<!-- moss:agent-guidance v"))
                .unwrap_or_else(|| panic!("no stamp in:\n{}", &text[..text.len().min(300)]));
            assert!(
                line < WINDOW,
                "stamp landed on line {line}, past the {WINDOW}-line window, in:\n{}",
                &text[..text.len().min(300)]
            );
        }
    }
}

/// A home marker is evidence the user has that agent. Writing for it costs one
/// file inside a directory the agent itself owns; not writing costs the whole
/// point of the feature for anyone who has not yet run the agent here.
#[test]
fn an_agent_installed_in_home_counts_as_detected() {
    let (_tmp, project) = make_project();
    let home = home_with(&[".claude"]);

    sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    assert!(
        project.join(".claude/skills/moss/SKILL.md").is_file(),
        "a home marker must reach the project"
    );
}

/// Writing into `$HOME` is what made the old global skill copy drift from the
/// binary that wrote it. Never again.
#[test]
fn nothing_is_ever_written_outside_the_project() {
    let (_tmp, project) = make_project();
    let home = home_with(&[".claude", ".cursor"]);

    sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    assert!(
        !home.path().join(".claude/skills").exists(),
        "sync must never write into the home directory"
    );
    assert!(!home.path().join(".cursor/rules").exists());
}

/// The guidance issues `moss …` commands as bare words some two dozen times,
/// and nothing on this branch puts `moss` on `PATH` — the symlink step was
/// retired because it could not reach a Finder-launched agent. Every file an
/// agent enters the package through has to say what `moss` means here, or the
/// whole package reads as instructions for a command that does not exist.
#[test]
fn every_entry_point_says_how_to_run_moss() {
    let (_tmp, project) = make_project();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::create_dir_all(project.join(".cursor")).unwrap();

    let home = empty_home();
    sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    let abs = cli().display().to_string();
    for rel in [
        ".claude/skills/moss/SKILL.md",
        ".cursor/rules/moss.mdc",
        ".moss/agents/SKILL.md",
    ] {
        let text = std::fs::read_to_string(project.join(rel)).expect(rel);
        assert!(text.contains(&abs), "{rel} never says where the moss binary is");
    }
}

/// A reference file an older moss wrote is replaced by a stub, not left holding
/// last release's prose and not deleted.
///
/// This is the one case the pointer switch could get wrong in a way that hurts.
/// The module deletes nothing (see its header), so the alternative to rewriting
/// these is leaving a `references/authoring.md` full of guidance from whichever
/// moss last copied it — sitting beside a SKILL.md that says the prose lives in
/// the binary. An agent reading the directory would find both and believe the
/// stale one, which is exactly the failure the pointer exists to end.
#[test]
fn a_reference_written_by_an_older_moss_becomes_a_stub() {
    let (_tmp, project) = make_project();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    let refs = project.join(".claude/skills/moss/references");
    std::fs::create_dir_all(&refs).unwrap();

    // Stand in for a copy an older moss wrote: real content, stamped for it.
    let old = refs.join("authoring.md");
    std::fs::write(&old, stamped_with("# Authoring\n\nlast release's prose\n", "")).unwrap();

    let home = empty_home();
    sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    let text = std::fs::read_to_string(&old).expect("authoring.md");
    assert!(old.is_file(), "the file must survive — this module deletes nothing");
    assert!(!text.contains("last release's prose"), "stale prose survived: {text}");
    assert!(text.contains("moss guide authoring"), "the stub names no command: {text}");
}

/// A reference file the *user* wrote is still theirs, stub or no stub.
#[test]
fn a_user_owned_reference_is_left_alone() {
    let (_tmp, project) = make_project();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    let refs = project.join(".claude/skills/moss/references");
    std::fs::create_dir_all(&refs).unwrap();

    let mine = refs.join("authoring.md");
    std::fs::write(&mine, "# My own notes\n").unwrap();

    let home = empty_home();
    let report = sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    assert_eq!(std::fs::read_to_string(&mine).unwrap(), "# My own notes\n");
    assert!(
        report.user_owned.iter().any(|p| p.ends_with("authoring.md")),
        "an unstamped reference must be reported as the user's: {report:?}"
    );
}

/// `moss guide all` is the one rendering that has to contain everything — it is
/// what an agent runs when it wants the whole thing in one read. A hardcoded
/// reference list silently dropped `example.md` (the folder shape to copy) and
/// `debugging.md` from it once, so this enumerates instead.
#[test]
fn the_flat_copy_carries_every_reference() {
    let flat = super::skill_package::render_flat();
    for (name, body) in super::skill_package::package_files() {
        let Some(_) = name.strip_prefix("references/") else { continue };
        let first_heading = body.lines().find(|l| l.starts_with("# ")).unwrap_or_else(|| {
            panic!("{name} has no `# ` heading to look for")
        });
        assert!(flat.contains(first_heading), "{name} is missing from the flat rendering");
    }
    // The prose cross-references itself as `moss guide <topic>`, and in this one
    // rendering those topics are already below. The disclaimer has to say so.
    assert!(
        flat.contains("moss guide authoring"),
        "the cross-references survive the flattening"
    );
    assert!(
        flat.contains("that topic is a section of THIS file"),
        "the flat copy must explain that a `moss guide <topic>` cross-reference is a section here"
    );
    // The disclaimer describes one linking convention, so only one may exist.
    // It spent a while describing the relative-path form the pointer rewrite had
    // already deleted — a comment about other text drifts silently, and nothing
    // caught it but an audit. If a `references/…` link comes back, the
    // disclaimer is wrong again and this is where that shows up.
    assert!(
        !flat.contains("references/"),
        "a relative `references/…` link is back in the prose, but `render_flat`'s \
         disclaimer only explains the `moss guide <topic>` form — update both"
    );
}

/// What moss writes into an agent's directory never reaches a commit.
///
/// Every one of these files carries the absolute path of the binary that wrote
/// it, so it is machine-local by construction: committing one hands a second
/// checkout a moss that does not exist there. Before this, a first build in any
/// folder belonging to anyone with Claude Code installed added six untracked
/// files nobody created — and `git add -A` swept them in. It did it to moss's
/// own test fixtures.
#[test]
fn nothing_written_into_an_agent_directory_can_reach_a_commit() {
    let (_tmp, project) = make_project();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::create_dir_all(project.join(".cursor/rules")).unwrap();
    // A rule of the user's own, which must keep its place in git.
    std::fs::write(project.join(".cursor/rules/mine.mdc"), "my rule\n").unwrap();

    let home = empty_home();
    sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    // moss owns the whole skill directory, so `*` — which covers the
    // `.gitignore` itself, leaving git nothing to report at all.
    assert_eq!(
        std::fs::read_to_string(project.join(".claude/skills/moss/.gitignore")).expect("written"),
        "*\n"
    );
    // `rules/` is shared, so only moss's own file is named.
    let cursor = std::fs::read_to_string(project.join(".cursor/rules/.gitignore")).expect("written");
    assert!(cursor.lines().any(|l| l == "moss.mdc"), "must ignore its own rule: {cursor:?}");
    assert!(!cursor.contains("mine.mdc"), "must not ignore the user's rule: {cursor:?}");

    // Only ever adds: a line the user put there survives, and a second sync
    // does not duplicate moss's.
    std::fs::write(project.join(".cursor/rules/.gitignore"), format!("{cursor}scratch.mdc\n"))
        .unwrap();
    sync_with_home(&project, Some(home.path()), &cli()).expect("resync");
    let after = std::fs::read_to_string(project.join(".cursor/rules/.gitignore")).unwrap();
    assert!(after.contains("scratch.mdc"), "the user's line survives: {after:?}");
    assert_eq!(after.matches("moss.mdc").count(), 1, "no duplicate: {after:?}");
}

/// A destination that resolves outside the project is never written, however
/// it got that way.
///
/// `.claude` symlinked into a dotfiles repo is an ordinary setup, and
/// `create_dir_all` + `fs::write` follow symlinks without comment. Unguarded,
/// the six-file package landed in `~/dotfiles/.claude/skills/moss/` — the
/// machine-scoped copy this module was written to abolish, owned by whichever
/// project built last, logged under a project-relative path that named none of
/// it. The pre-existing `nothing_is_ever_written_outside_the_project` could not
/// catch this: it only uses real directories.
#[cfg(unix)]
#[test]
fn a_symlinked_agent_directory_does_not_carry_writes_out_of_the_project() {
    let (_tmp, project) = make_project();
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(outside.path().join(".claude")).unwrap();
    std::os::unix::fs::symlink(outside.path().join(".claude"), project.join(".claude")).unwrap();

    let home = empty_home();
    let report = sync_with_home(&project, Some(home.path()), &cli()).expect("sync");

    assert!(
        !outside.path().join(".claude/skills").exists(),
        "a symlink must not carry the write out of the project"
    );
    assert!(
        report.written.iter().all(|p| !p.starts_with(".claude/")),
        "nothing under the symlink may be reported as written: {report:?}"
    );
}

/// A binary that no longer exists at the path `current_exe()` names must not be
/// written into the pointer files.
///
/// On Linux an unlinked binary makes `current_exe()` return `Ok` with a literal
/// ` (deleted)` suffix rather than `Err`, and moss replaces its own binary
/// during an auto-update. Trusting that `Ok` embeds an unexecutable path in the
/// one file whose entire job is naming an executable path.
#[test]
fn a_binary_that_is_gone_falls_back_to_the_bare_name() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("moss");
    std::fs::write(&real, b"#!/bin/sh\n").unwrap();

    assert_eq!(super::usable_cli_path(Some(real.clone())), real, "a live binary is used as-is");

    let bare = std::path::PathBuf::from("moss");
    assert_eq!(super::usable_cli_path(None), bare, "current_exe() failing falls back");
    assert_eq!(
        super::usable_cli_path(Some(tmp.path().join("moss (deleted)"))),
        bare,
        "the Linux `(deleted)` spelling names no real file, so it must fall back too"
    );

    std::fs::remove_file(&real).unwrap();
    assert_eq!(super::usable_cli_path(Some(real)), bare, "a since-removed path falls back");
}
