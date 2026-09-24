//! Keep a project's coding-agent guidance current, with no install step.
//!
//! ## The journey this replaces
//!
//! Before: open the launcher, notice a hint, click "Connect your AI agent",
//! approve a Touch ID prompt for a `/usr/local/bin` symlink, and receive a copy
//! of the skill in `~/.claude/skills/moss/` — machine-scoped, so it drifts from
//! the binary the moment either updates, and invisible to any agent that has no
//! skill mechanism. Now: nothing. Open the folder in whatever agent you use and
//! it is already oriented.
//!
//! ## Where things go, and why there
//!
//! Every path here is either the agent's own config directory or moss's. None
//! of it is the author's prose — the site folder is their writing, and moss
//! keeps its state in `.moss/` precisely so a folder of markdown stays a folder
//! of markdown. moss no longer writes anything into the author's folder itself
//! — the root `AGENTS.md` pointer this module used to write there was retired
//! in favor of a one-line human prompt pointing an agent at
//! `.moss/agents/SKILL.md` directly (see `docs/authoring/agent-prompts.md`).
//!
//! | target | when | why there |
//! |---|---|---|
//! | `.claude/skills/moss/SKILL.md` | `.claude/` here or in `$HOME` | Claude Code's native project-skill mechanism: auto-discovered, loaded on demand rather than burning context every turn |
//! | `.cursor/rules/moss.mdc` | `.cursor/` here or in `$HOME` | Cursor's native mechanism |
//! | `.moss/agents/SKILL.md` | always | for tools with no config directory of their own to write into; lives in moss's own directory, not the author's, so there is no footprint question to gate |
//!
//! ## What goes in them: a pointer, not the prose
//!
//! Each of those files names `moss guide` and stops. The guidance itself ships
//! inside the binary and is printed on demand, so nothing on disk can describe
//! a moss other than the one that wrote it. See
//! [`skill_package::render_pointer`] for what that buys and what it costs.
//!
//! The stamped copy this replaces was already refreshed on every full build, so
//! it drifted only in the window between a moss upgrade and the next build —
//! but a copy that [`write_managed`] declines to refresh is frozen permanently
//! and announces that to nobody, and every file written by a moss older than
//! the stamp itself is in exactly that state.
//!
//! Gemini CLI is **not** covered: its default context filename is `GEMINI.md`,
//! and moss does not write one. Adding it would mean a second file in the
//! author's folder under a switch whose text promises one — a footprint
//! decision, not an oversight. `build::scan::classify` already knows the name,
//! so a `GEMINI.md` an author writes themselves is handled correctly.
//!
//! `.moss/AGENTS.md` — the *site's* conventions, as opposed to moss's — is a
//! separate concern with its own merge contract; see [`super::merge_template`].
//!
//! ## None of it belongs in a commit
//!
//! Every file above interpolates the absolute path of the moss binary that
//! wrote it, so it is machine-local by construction — see [`ensure_ignored`],
//! which adds the pattern to a `.gitignore` inside each directory moss writes
//! into. Never the project's root `.gitignore`: that one is the author's.
//!
//! ## Why a version stamp
//!
//! [`sync_project`] runs on every full build (see `SYNC_TRIGGERS` below), so it
//! has to cost nothing when nothing changed.
//! Every managed file carries `<!-- moss:agent-guidance vN h:… -->` near the
//! top. If the file is byte-identical to what moss would write, it is left
//! untouched — no write, no mtime change, no watcher event, no iCloud resync.
//! A write happens at most once per upgrade.
//!
//! The stamp is also the consent boundary, and `h:` is the half that carries
//! it: a hash of the rest of the file. Presence alone was not enough. Appending
//! your own rules underneath moss's block is the most likely edit anyone makes
//! to a managed file — coding agents do it unprompted — and it leaves the
//! stamp line untouched, so a presence check read the whole file as moss's and
//! the next build silently replaced it. Hashing the body means any edit,
//! anywhere, hands the file to the user.
//!
//! ## Nothing here deletes
//!
//! This module has never removed a file, and that still matters even though it
//! no longer writes into the author's folder: an `AGENTS.md` moss wrote at the
//! root of a site folder under the old, now-retired setting (shipped in
//! v0.8.0) is left exactly where it is. moss has no way to know whether an
//! agent in that folder still depends on it, and `remove_file` has no undo —
//! no trash, no backup, no diff to review. Every data-loss finding across two
//! independent reviews of this module was a removal, and each fix was another
//! rule about when deleting is safe. Not deleting has no such rules.

use std::path::{Path, PathBuf};

use super::skill_package;

/// Bumped whenever the *shape* of what moss writes changes — a new pointer
/// target, a different managed-block format. Not bumped for edits to the skill
/// text itself: those change file content, which the content comparison
/// catches on its own.
///
/// Nothing compares it. It is a human-readable marker and a grep target, not a
/// gate: the hash beside it already decides everything the version could, and
/// per-version gating would only add a way for two moss builds to disagree
/// about a file they both wrote.
pub const GUIDANCE_VERSION: u32 = 1;

/// How far into a file [`split_stamp`] looks for the marker.
///
/// Not line 1, because [`stamped_with`] places the stamp *after* any YAML
/// frontmatter. Generous enough for real frontmatter and far too small to find
/// a `<!-- moss:agent-guidance -->` an author quoted in prose halfway down a
/// file they now own. `every_managed_file_stamps_inside_the_window` pins that
/// every file moss actually writes lands inside it.
const WINDOW: usize = 16;

/// The marker moss writes near the top of every file it manages.
///
/// `rest` is the entire file *except* this line — see [`split_stamp`], which
/// reconstructs exactly that to verify the hash. xxh3 is here to notice an
/// accident, not to withstand a forgery; the only thing forging it costs you
/// is your own file.
fn stamp_for(rest: &str) -> String {
    format!(
        "<!-- moss:agent-guidance v{GUIDANCE_VERSION} h:{:016x} -->",
        xxhash_rust::xxh3::xxh3_64(rest.as_bytes())
    )
}

/// When [`sync_project`] should run. Documented here rather than at each call
/// site so the whole policy is visible in one place.
///
/// - **Any full build** — one gate in `build::run_pipeline`, on
///   `BuildTrigger::Full`. That single condition already covers every trigger
///   worth having: opening a folder in the GUI builds it (so "I installed
///   Cursor since I last opened this folder" is caught), and `moss build` /
///   `moss preview` cover headless use, where there is no open event at all.
///   It has a pleasing property too — a coding agent's own first `moss`
///   command in a folder writes the guidance for its next turn.
/// **Not** on watch rebuilds as a rule: they fire on every save. But
/// `rebuild_pending` — any save that lands while a rebuild is in flight, which
/// is ordinary during editing — passes `BuildTrigger::Full`, so in practice a
/// sync does run on some watch rebuilds. It is bounded: the content comparison
/// makes it a no-op, and `watch::evaluate_gate` suppresses the root agent files
/// outright so moss's own write can never schedule a rebuild of its own.
pub const SYNC_TRIGGERS: &str = "any full build";

/// What a sync did, for logging and for the tests.
///
/// There is no `removed`. Nothing here deletes — see the module header.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Paths written or refreshed, project-relative.
    pub written: Vec<String>,
    /// Paths left alone because they are not moss's to write.
    pub user_owned: Vec<String>,
}

impl SyncReport {
    pub fn is_noop(&self) -> bool {
        self.written.is_empty()
    }
}

/// Which agents to write for.
///
/// Detection reads **both** the project and `$HOME`: a home marker means the
/// user has that agent and will plausibly use it here, and a false positive
/// costs one file inside a directory the agent itself owns. Writing only ever
/// happens **into the project** — never into `$HOME`, which is how the old
/// global `~/.claude/skills/moss/` copy came to contradict the binary.
fn detected(project: &Path, home: Option<&Path>) -> (bool, bool) {
    let present = |name: &str| {
        project.join(name).is_dir() || home.map(|h| h.join(name).is_dir()).unwrap_or(false)
    };
    (present(".claude"), present(".cursor"))
}

/// The line that makes every `moss …` command in the guidance runnable.
///
/// The guidance says `moss describe --json` and `moss build` some two dozen
/// times, as bare words. Nothing on this branch puts `moss` on `PATH` — the
/// symlink step was retired precisely because it could not reach an agent
/// launched from Finder — so without this the whole package reads as
/// instructions for a command that does not exist.
///
/// [`bundled_cli_path`] explains why an absolute path is the right answer and
/// where it can still go stale, hence the fallback clause: a person who does
/// have `moss` on `PATH` is told they may use it, and an agent holding a stale
/// path is told what to try instead of giving up.
fn cli_binding(cli: &Path) -> String {
    format!(
        "> Run moss as `{}`.\n\
         > Every `moss …` command below means that binary; moss is usually not on\n\
         > `PATH`. If that path no longer exists, moss has moved — use `moss` if\n\
         > your shell resolves it, and reopen this folder in moss to refresh this file.\n",
        cli.display()
    )
}

/// Prefix `body` with the stamp, without displacing a leading frontmatter block.
///
/// A stamp on line 1 would be correct for plain markdown and wrong for the two
/// files that matter most. Claude Code reads `SKILL.md`'s name and description
/// from YAML frontmatter that must open the file; Cursor's `.mdc` is the same.
/// Push their `---` down by one line and the frontmatter stops parsing — the
/// skill is still on disk and no longer discoverable, which is the worst of
/// both. So the stamp goes on the line after the closing delimiter, and
/// [`split_stamp`] looks past the block rather than only at line 1.
/// Prefix `body` with the stamp, placing `extra` on the lines after it.
///
/// Used for the entry-point files to carry [`cli_binding`]. The reference files
/// pass an empty `extra`: an agent reaches them from an entry point that
/// already said what `moss` means, and repeating it six times is noise in the
/// context window it is trying to save.
fn stamped_with(body: &str, extra: &str) -> String {
    // Split off a leading frontmatter block so the stamp lands under it rather
    // than above it. Where the block ends is `frontmatter_span`'s call — an
    // opening delimiter with no closing one is not frontmatter, and neither is a
    // body that never opened with one.
    let (front, after) = match moss_core::frontmatter::frontmatter_span(body) {
        // Char-aligned: `body` is a line-boundary offset from the splitter.
        #[allow(clippy::string_slice)]
        Some(span) => (body[..span.body].to_string(), &body[span.body..]),
        None => (String::new(), body),
    };
    let tail = if extra.is_empty() {
        after.to_string()
    } else {
        format!("{extra}\n{after}")
    };
    // The hash covers everything the file will contain except the stamp line
    // itself, so splicing the stamp back out reproduces it byte for byte. Keep
    // this insertion and [`split_stamp`]'s removal symmetrical: any drift
    // between them makes every managed file read as user-owned, and moss stops
    // refreshing its own guidance for good.
    let stamp = stamp_for(&format!("{front}{tail}"));
    format!("{front}{stamp}\n{tail}")
}

/// The stamp and the rest of the file with the stamp line spliced out —
/// exactly the string [`stamped_with`] hashed.
///
/// Returns `None` when no stamp appears within [`WINDOW`].
fn split_stamp(content: &str) -> Option<(&str, String)> {
    // Accumulate the lines already passed over, then hand the iterator's
    // remainder straight to `rest` — the same string byte-index slicing would
    // produce, without a byte index anywhere (`clippy::string_slice`, the
    // panic-free contract in lib.rs).
    let mut lines = content.split_inclusive('\n');
    let mut rest = String::with_capacity(content.len());
    let mut scanned = 0usize;
    while let Some(line) = lines.next() {
        if line.trim_end().starts_with("<!-- moss:agent-guidance v") {
            rest.extend(lines);
            return Some((line.trim_end(), rest));
        }
        rest.push_str(line);
        scanned += 1;
        if scanned == WINDOW {
            return None;
        }
    }
    None
}

/// Whether this file is still exactly what moss wrote.
///
/// A stamp whose hash matches the rest of the file is moss's to refresh.
/// Anything else — no stamp, or a hash that no longer matches — is the user's,
/// and silently replacing it is the wrong move.
fn is_managed_by_moss(content: &str) -> bool {
    split_stamp(content).is_some_and(|(stamp, rest)| stamp == stamp_for(&rest))
}

/// Whether `path` really lands inside `root`, following every symlink on the way.
///
/// `.claude` symlinked into a dotfiles repo is a common setup, and
/// `create_dir_all` + `fs::write` follow symlinks without comment. Unguarded,
/// a project-relative `.claude/skills/moss/SKILL.md` wrote into
/// `~/dotfiles/.claude/skills/moss/` — recreating the machine-scoped copy this
/// module exists to abolish, now owned by whichever project built last, and
/// logged under a project-relative path that named none of it.
///
/// `path` normally does not exist yet, so the deepest *existing* ancestor is
/// what gets canonicalized: that is the last thing a symlink can redirect.
/// When `path` does exist, it is canonicalized itself, which catches a
/// symlinked `AGENTS.md` pointing out of the folder.
fn is_inside(root: &Path, path: &Path) -> bool {
    let mut cur = path;
    loop {
        if let Ok(real) = cur.canonicalize() {
            return real.starts_with(root);
        }
        match cur.parent() {
            Some(parent) => cur = parent,
            None => return false,
        }
    }
}

/// Keep what moss writes here out of the author's commits, by adding `pattern`
/// to a `.gitignore` inside `dir`.
///
/// Everything moss writes is machine-local: each entry point interpolates the
/// absolute path of the binary that wrote it ([`cli_binding`]), so committing
/// one commits a path that exists on exactly one machine, and a second checkout
/// gets guidance pointing at a moss that was never there. Un-ignored, `git
/// status` also gained six untracked files nobody created, which `git add -A`
/// swept straight in.
///
/// moss never touches the project's root `.gitignore` — a folder of markdown
/// may not even be a repo, and if it is, that file is the author's. It writes
/// only inside directories it is already writing into, and only ever adds. Same
/// contract as `.moss/.gitignore` (`infra::moss_paths::ensure_moss_gitignore`).
///
/// `pattern` is `*` where moss owns the whole directory, which self-ignores:
/// `*` matches the `.gitignore` too, so nothing shows up at all. Where moss
/// owns one file in a directory that is the user's, it is that filename, and
/// the `.gitignore` itself stays visible — correctly, since committing *it* is
/// the right thing for everyone else on the repo.
/// Whether `existing` already keeps `pattern` out of commits.
///
/// Same flaw as `.moss/.gitignore` had (`infra::moss_paths::already_excluded`):
/// exact string equality does not recognize the user's own, broader spelling of
/// the rule, so moss appended a redundant line to a file the author maintains —
/// `.cursor/rules/.gitignore` is theirs, not moss's. A bare `*` already ignores
/// everything here, and a leading `/` is the same pattern anchored.
///
/// Comments and `!` lines are skipped. Wildcards beyond a bare `*` are not
/// matched — moss carries no glob matcher, and the conservative answer appends
/// a redundant line rather than skipping a needed one.
fn already_ignored(existing: &str, pattern: &str) -> bool {
    existing.lines().map(str::trim).any(|l| {
        !l.is_empty()
            && !l.starts_with('#')
            && !l.starts_with('!')
            && (l == "*" || l == pattern || l.trim_start_matches('/') == pattern)
    })
}

fn ensure_ignored(root: &Path, dir: &Path, pattern: &str) -> Result<bool, String> {
    let path = dir.join(".gitignore");
    if !is_inside(root, &path) {
        return Ok(false);
    }
    let existing = match std::fs::read_to_string(&path) {
        Ok(existing) => existing,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        // Present but unreadable. Not moss's to repair, and not worth failing a
        // build over — the guidance write itself will decline for the same
        // reason if it hits the same trouble.
        Err(_) => return Ok(false),
    };
    if already_ignored(&existing, pattern) {
        return Ok(false);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let sep = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    // allow:raw_write agent-guidance files in the vault are the user's documents, not build output
    std::fs::write(&path, format!("{existing}{sep}{pattern}\n"))
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(true)
}

/// Write `content` to `path` unless it is byte-identical or user-owned.
///
/// Returns `Some(true)` when written, `Some(false)` when already current, and
/// `None` when the file belongs to the user.
fn write_managed(root: &Path, path: &Path, content: &str) -> Result<Option<bool>, String> {
    if !is_inside(root, path) {
        return Ok(None);
    }
    match std::fs::read_to_string(path) {
        // Already exactly this. The common case on every build after the first,
        // and the whole reason a sync costs nothing when nothing changed.
        Ok(existing) if existing == content => return Ok(Some(false)),
        Ok(existing) if !is_managed_by_moss(&existing) => return Ok(None),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        // Unreadable — a directory in the file's place, or permissions. Not
        // ours to replace.
        Err(_) => return Ok(None),
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    // allow:raw_write agent-guidance files in the vault are the user's documents, not build output
    std::fs::write(path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(Some(true))
}

/// Absolute path to the running moss binary.
///
/// This is the whole of what the retired "install the CLI" step bought, minus
/// the step. moss is a single argv-dispatching binary that already ships inside
/// the app bundle, so `/Applications/moss.app/Contents/MacOS/moss describe
/// --json` works on a machine with nothing on `PATH` and no auth prompt.
///
/// The `/usr/local/bin/moss` symlink bought only the bare word `moss` in an
/// interactive shell, at the cost of a Touch ID dialog — and every directory on
/// the default macOS `PATH` is root-owned, so there was no cleverer location to
/// use. Worse, `PATH` does not reach the consumer: an agent launched from
/// Finder or the Dock inherits launchd's environment, not the user's shell, so
/// a perfect symlink can still leave `moss` unresolvable in exactly the context
/// this exists for. An absolute path has no such failure mode.
///
/// Falls back to the bare name when `current_exe()` cannot name a path that
/// still exists — which is not only the error case. On Linux `current_exe()`
/// reads `/proc/self/exe`, and if the binary was unlinked while running it
/// returns `Ok` with a literal ` (deleted)` appended rather than `Err`. moss
/// replaces its own binary during an auto-update, so believing that `Ok` writes
/// `/…/moss (deleted)` into every project's pointer file — a path an agent
/// cannot execute, embedded in the one file whose entire job is naming a path
/// an agent can execute. Checking `exists()` covers both spellings of gone
/// without having to enumerate them. Found by an agent-surface audit,
/// 2026-08-06.
pub fn bundled_cli_path() -> PathBuf {
    usable_cli_path(std::env::current_exe().ok())
}

/// The decision [`bundled_cli_path`] makes, minus the syscall — so the
/// `(deleted)` case can be tested without unlinking a running binary.
pub(crate) fn usable_cli_path(current_exe: Option<PathBuf>) -> PathBuf {
    match current_exe {
        Some(p) if p.exists() => p,
        _ => PathBuf::from("moss"),
    }
}

/// The file a coding agent should read to orient itself in `project`, if one
/// is on disk.
///
/// This is the other half of the cold-start problem [`sync_project`] solves.
/// Writing the guidance is useless if nothing ever *says* it is there: an agent
/// told only "run moss build" has no reason to go looking in a dot directory it
/// did not create. So the CLI prints this path after every build, which is what
/// lets one sentence — the binary's path plus `build .` — bootstrap an agent
/// that knew nothing about moss a moment earlier.
///
/// Deliberately reports on **every** build, not only the one that wrote the
/// files. A second `moss build` in an already-oriented folder is exactly the
/// case where a fresh agent session needs the pointer most, and by then the
/// content comparison has made the write a no-op.
///
/// Ordered by how much use an agent gets from each: Claude Code's skill
/// directory is loaded on demand and is the richest, so it wins when present;
/// `.moss/agents/SKILL.md` is written unconditionally and is the fallback.
/// Cursor's `.mdc` is not offered — Cursor loads its rules itself, so naming
/// the file would tell that agent to read something it already has.
pub fn guidance_pointer(project: &Path) -> Option<PathBuf> {
    [".claude/skills/moss/SKILL.md", ".moss/agents/SKILL.md"]
        .iter()
        .map(|rel| project.join(rel))
        .find(|p| p.is_file())
}

/// Tell a CLI caller where [`guidance_pointer`] found the guidance, if anywhere.
///
/// Lives here rather than at the call site in `lib.rs` so the rationale sits
/// beside the function whose output it announces.
pub fn print_guidance_pointer(project: &Path) {
    if let Some(skill) = guidance_pointer(project) {
        crate::build::cli_output::cli_eprintln!(
            "🤖 Coding agent: read {} for how to author and theme this site.",
            skill.display()
        );
    }
}

/// The build pipeline's entry into [`sync_project`] — see [`SYNC_TRIGGERS`].
///
/// Takes `is_full_build` rather than reading the trigger itself so the policy
/// lives beside the constant that documents it, and so this is testable
/// without a `PipelineConfig`.
///
/// Never returns an error. The user asked moss to build their site, not to
/// orient their agent; a read-only checkout or a permissions oddity in a dot
/// directory must not cost them the site.
pub fn sync_after_build(project: &Path, is_full_build: bool) {
    if !is_full_build {
        return;
    }
    if let Err(e) = sync_project(project, &bundled_cli_path()) {
        log::warn!(target: "build", "agent guidance sync skipped: {e}");
    }
}

/// Bring `project`'s agent guidance up to date.
///
/// `cli` is the absolute path to a moss binary — see [`bundled_cli_path`]. It
/// is interpolated into what agents read, so it must be a path that works when
/// launched from Finder rather than the bare word `moss`.
///
/// Takes the project path as an argument and must never read
/// `std::env::current_dir()`. That was the defect in the Connect flow this
/// replaces: it detected `.cursor/` against the *process* cwd, so moss
/// launched from Finder wrote a moss pointer into whatever unrelated directory
/// launchd happened to hand it.
pub fn sync_project(project: &Path, cli: &Path) -> Result<SyncReport, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from).or_else(dirs::home_dir);
    sync_with_home(project, home.as_deref(), cli)
}

/// [`sync_project`] with the home directory passed in rather than read.
///
/// The whole seam exists for the tests. `$HOME` is process-global, so a test
/// that points it at a fixture corrupts every *other* test running in
/// parallel — `vault::paths` has one that resolves the real home and fails
/// outright when it moves. Threading it through costs one argument and makes
/// the ambient read happen in exactly one place.
fn sync_with_home(project: &Path, home: Option<&Path>, cli: &Path) -> Result<SyncReport, String> {
    let mut report = SyncReport::default();
    if !project.join(".moss").is_dir() {
        // Not a moss project. Nothing has been established here to describe.
        return Ok(report);
    }
    // Resolved once, up front, and every destination is checked against it —
    // see [`is_inside`]. Resolving the root itself is what lets a symlinked
    // project folder work at all, which is the ordinary case on a Mac with the
    // site living in iCloud Drive.
    let real_root = project
        .canonicalize()
        .map_err(|e| format!("resolve {}: {e}", project.display()))?;

    let (has_claude, has_cursor) = detected(project, home);

    let mut record = |path: &Path, outcome: Option<bool>| {
        // `/`-separated on every platform — these are display strings, and
        // Windows would otherwise mix separators (".claude/skills\SKILL.md").
        let rel = moss_core::slug::normalize_separators(
            &path.strip_prefix(project).unwrap_or(path).to_string_lossy(),
        );
        match outcome {
            Some(true) => report.written.push(rel),
            Some(false) => {}
            None => report.user_owned.push(rel),
        }
    };

    if has_claude {
        // Claude Code's project-skill mechanism. Every file written here is
        // stamped, stubs included: the stamp is what marks a file as moss's to
        // refresh, so an unstamped one is frozen at whatever the first build
        // wrote and never updates again.
        let dest = project.join(".claude/skills/moss");
        // First, so the files below are never briefly visible to git. This
        // whole directory is moss's, so `*` — which ignores the `.gitignore`
        // itself, leaving nothing for `git status` to report.
        record(&dest.join(".gitignore"), Some(ensure_ignored(&real_root, &dest, "*")?));
        let binding = cli_binding(cli);
        let skill = dest.join("SKILL.md");
        let body = stamped_with(&skill_package::render_pointer_with_frontmatter(), &binding);
        record(&skill, write_managed(&real_root, &skill, &body)?);
        // The references are stubs now, and are written only where a previous
        // moss already put a file. Writing them unconditionally would create
        // six files whose entire content is "read the command in the file next
        // to me" — noise in a fresh folder, and useful only where the old
        // paths might still be held.
        for topic in skill_package::topics() {
            let path = dest.join(format!("references/{topic}.md"));
            if !path.is_file() {
                continue;
            }
            let stub = stamped_with(&skill_package::render_reference_stub(&topic), &binding);
            record(&path, write_managed(&real_root, &path, &stub)?);
        }
    }

    if has_cursor {
        // `rules/` is the user's — their own rules live beside ours — so this
        // names the one file rather than sweeping the directory.
        let rules = project.join(".cursor/rules");
        record(
            &rules.join(".gitignore"),
            Some(ensure_ignored(&real_root, &rules, "moss.mdc")?),
        );
        let path = rules.join("moss.mdc");
        let body = stamped_with(&skill_package::render_cursor_mdc(), &cli_binding(cli));
        record(&path, write_managed(&real_root, &path, &body)?);
    }

    // Unconditional: this lives inside `.moss/`, moss's own directory, not the
    // author's folder, so there is no footprint question to gate. It is also
    // the only guidance at all for an agent-neutral tool (Codex, Gemini CLI,
    // aider) with no `.claude`/`.cursor` directory to detect.
    let neutral = project.join(".moss/agents/SKILL.md");
    let body = stamped_with(&skill_package::render_pointer(), &cli_binding(cli));
    record(&neutral, write_managed(&real_root, &neutral, &body)?);

    if !report.is_noop() {
        log::info!("agent guidance synced: wrote {:?}", report.written);
    }
    // One line per build, not one per skipped file. "moss left your file alone"
    // is the expected outcome on every build of every project that has one, so
    // the per-file form is pure repetition — it fires unchanged forever and
    // pushes real diagnostics out of an uploaded log. The paths still matter
    // when you are debugging guidance sync itself, so they stay at DEBUG.
    if !report.user_owned.is_empty() {
        log::info!(
            "agent guidance: {} user-owned file(s) left alone",
            report.user_owned.len()
        );
        for path in &report.user_owned {
            log::debug!("agent guidance: {path} is not moss's to write — left alone");
        }
    }

    Ok(report)
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
