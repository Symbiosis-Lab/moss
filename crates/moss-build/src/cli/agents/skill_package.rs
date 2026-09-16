//! The authored moss agent skill, and the two shapes it is delivered in.
//!
//! Source only — nothing here decides *where* anything is written. That is
//! [`super::sync`]'s job, and keeping the split means one module owns the
//! content and another owns consent and placement.
//!
//! This used to be `install_skill`, the engine behind `moss agents
//! install-skill` and the launcher's "Connect your AI agent" button. Both are
//! gone: they wrote a machine-scoped copy into `$HOME`, which drifted from the
//! binary that wrote it the moment either updated, and reached only the one
//! agent that happened to have a skill mechanism.

use include_dir::{include_dir, Dir};

/// The authored, tool-neutral skill package, embedded at compile time.
/// Precedent: `plugins::bundled` embeds `bundled-plugins/` the same way.
static SKILL_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/assets/skills/moss");

/// Strip a leading YAML frontmatter block (`---\n...\n---\n`) if present.
fn strip_frontmatter(md: &str) -> &str {
    match moss_core::frontmatter::frontmatter_span(md) {
        // Char-aligned: `body` is a line-boundary offset from the splitter.
        #[allow(clippy::string_slice)]
        Some(span) => md[span.body..].trim_start_matches('\n'),
        None => md,
    }
}

fn skill_file(path: &str) -> &'static str {
    SKILL_DIR
        .get_file(path)
        .and_then(|f| f.contents_utf8())
        .unwrap_or("")
}

/// Every file in the embedded skill package, as `(relative path, contents)`.
///
/// Relative paths use `/` and are joined onto the destination by the caller, so
/// `references/authoring.md` lands under `references/`. Exposed for
/// [`super::sync`], which writes the package into a project rather than into
/// `$HOME`.
pub(crate) fn package_files() -> Vec<(String, String)> {
    fn walk(dir: &Dir, prefix: &str, out: &mut Vec<(String, String)>) {
        for file in dir.files() {
            let Some(name) = file.path().file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(contents) = file.contents_utf8() else {
                continue;
            };
            let rel = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}/{name}")
            };
            out.push((rel, contents.to_string()));
        }
        for sub in dir.dirs() {
            let Some(name) = sub.path().file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let next = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}/{name}")
            };
            walk(sub, &next, out);
        }
    }
    let mut out = Vec::new();
    walk(&SKILL_DIR, "", &mut out);
    out
}

/// SKILL.md + **every** reference, flattened into one document, no frontmatter.
/// Pure transform of the embedded source — there is no second copy of the
/// content to maintain.
///
/// This is the shape for an agent with no config directory of its own: one file
/// to read, nothing tool-specific in it. [`render_cursor_mdc`] is this plus the
/// frontmatter Cursor requires.
///
/// Two things this must keep doing, both learned the hard way:
///
/// - **Enumerate the references, never list them.** A hardcoded list silently
///   dropped `example.md` (the canonical folder shape to copy) and
///   `debugging.md`, so the one agent that gets *only* this file got the
///   smallest version of the guidance.
/// - **Say that a `moss guide <topic>` cross-reference is already here.** The
///   prose sends the reader to `moss guide authoring` and friends; in this
///   rendering those are sections below, so an agent that follows them literally
///   re-runs moss to reread text it is holding. (Until 2026-08-06 this note
///   warned about relative `references/<name>.md` links instead — the links the
///   pointer rewrite replaced. It described a convention that no longer existed,
///   which is the failure mode a comment about other text always has.)
pub(crate) fn render_flat() -> String {
    let mut out = String::from(
        "<!-- This is moss's complete agent guidance in one file. Where the text \
         below says to run `moss guide <topic>`, that topic is a section of THIS \
         file, below — you are already holding it. -->\n\n",
    );
    out.push_str(&strip_frontmatter(skill_file("SKILL.md")));
    for (name, body) in package_files() {
        if name.starts_with("references/") {
            out.push_str("\n\n---\n\n");
            out.push_str(&body);
        }
    }
    out.push('\n');
    out
}

/// The `description:` line of `SKILL.md`'s frontmatter.
///
/// Both Claude Code and Cursor load this package **on demand, by matching this
/// description** — Cursor's rule is `alwaysApply: false` with no `globs`. So the
/// description is not a label; it is the trigger, and a use it fails to name is
/// a use the agent handles from memory instead. Styling is most of what this
/// skill is (four rungs, tokens, dark mode) and went unnamed for a while, which
/// is exactly the request most likely to arrive.
///
/// Read from SKILL.md rather than repeated here. Cursor's copy used to be a
/// second hardcoded string, which is the same two-hand-maintained-lists shape
/// that let `--help` and `cli_commands` drift apart.
fn skill_description() -> &'static str {
    skill_file("SKILL.md")
        .lines()
        .find_map(|l| l.strip_prefix("description: "))
        .unwrap_or("Author, style, extend, or migrate a moss static site the canonical way.")
}

/// [`render_flat`] with the frontmatter Cursor's rule format requires.
pub(crate) fn render_cursor_mdc() -> String {
    format!(
        "---\ndescription: {}\nglobs:\nalwaysApply: false\n---\n\n{}",
        skill_description(),
        render_pointer()
    )
}

/// The topics `moss guide <topic>` accepts, derived from the reference files.
///
/// Derived, never listed: [`render_flat`] once shipped a hardcoded list that
/// silently dropped two references, and a hand-kept topic table would fail the
/// same way — except worse, because a topic that is missing from the table is
/// unreachable rather than merely unmentioned.
pub fn topics() -> Vec<String> {
    let mut out: Vec<String> = package_files()
        .into_iter()
        .filter_map(|(name, _)| {
            name.strip_prefix("references/")
                .and_then(|n| n.strip_suffix(".md"))
                .map(str::to_string)
        })
        .collect();
    out.sort();
    out
}

/// The text `moss guide [topic]` prints.
///
/// `None` is the entry point (SKILL.md, frontmatter stripped — it is Claude
/// Code's discovery metadata, not prose anyone reads). `Some(topic)` is one
/// reference. An unknown topic returns `None` so the caller can name the ones
/// that exist rather than printing nothing.
pub fn guide_text(topic: Option<&str>) -> Option<String> {
    match topic {
        None => Some(strip_frontmatter(skill_file("SKILL.md")).to_string()),
        Some("all") => Some(render_flat()),
        Some(t) if topics().iter().any(|k| k == t) => {
            Some(skill_file(&format!("references/{t}.md")).to_string())
        }
        Some(_) => None,
    }
}

/// What moss writes to disk: a pointer at the binary, not a copy of the prose.
///
/// The prose ships inside the binary and is printed by `moss guide`. A file on
/// disk cannot say the same thing without becoming a second source of truth —
/// and this one is refreshed only when a build runs, so a folder that has not
/// been built since the last moss upgrade holds guidance from the older one.
/// That was survivable while the copy was merely *old*; it stopped being
/// survivable once `write_managed` learned to leave edited files alone, because
/// a file moss declines to refresh is frozen for good and says so to nobody.
///
/// So the copy is gone and what remains names its source. The cost is real and
/// deliberate: on a machine where the interpolated path no longer resolves,
/// this file teaches nothing, where a stale copy would still have taught
/// something mostly-right. [`super::sync::cli_binding`] is what keeps that
/// narrow — it prints the absolute path of the binary that wrote the file, and
/// tells the reader what to try when the path has moved.
pub fn render_pointer() -> String {
    let mut out = String::from(
        "# Working in moss\n\n\
         This file is a pointer, deliberately. moss's guidance lives in the binary\n\
         named above and is printed on demand, so there is no copy here to fall\n\
         behind the moss you are actually running.\n\n\
         ```\n\
         moss guide              # start here: conventions, styling, hard rules\n\
         moss guide --list       # the topics below, as this binary defines them\n\
         moss guide all          # everything, one document\n\
         ```\n\n\
         Topics (`moss guide <topic>`):\n\n",
    );
    for topic in topics() {
        out.push_str(&format!("- `{topic}`\n"));
    }
    out.push_str(
        "\nFor names rather than prose — every `--moss-*` token, `moss-*` class,\n\
         frontmatter field, language code, plugin hook and CLI command this binary\n\
         knows — run `moss describe --json`. Neither command needs a network.\n\n",
    );
    out.push_str(NO_BINARY);
    out
}

/// What to do when the interpolated path does not resolve.
///
/// The cost of pointing rather than copying: this folder can be opened on a
/// machine where moss is not installed — synced from another Mac, cloned from a
/// repo, handed to a collaborator — and then every command above fails. A copy
/// would still have taught something. So the file has to close its own loop and
/// say where moss comes from, or the reader is stuck with a path and no way to
/// act on it.
///
/// The link below deliberately names the repository the current release
/// repo becomes at the F6 flip, ahead of that rename — see the F6 runbook,
/// `docs/archive/2026-09-13-f6-flip-day-runbook.md`.
const NO_BINARY: &str = "\
## If that path does not resolve

moss is not installed on this machine, or has moved. Both commands above live in
the moss binary, so install it and re-run them:

- **Download:** <https://github.com/Symbiosis-Lab/moss/releases/latest>
  — the macOS `.dmg`. Open it and drag moss to Applications.
- **The CLI is the same binary**, inside the app bundle. After installing:

  ```
  /Applications/moss.app/Contents/MacOS/moss guide
  ```

  There is no separate CLI package and no `npm`/`brew` install. moss is usually
  not on `PATH` — use the absolute path above, or the one at the top of this
  file if it still exists.
- **Nothing else is needed to read a site.** `guide` and `describe` answer from
  the binary alone — no account, no network, no config. `build` needs no account
  or config either, but it is not strictly offline: a page with a link-preview
  card fetches that link's metadata (cached, so it is a once-per-link cost), and
  converting a video can pull down an encoder. Offline, the build still finishes
  — you pay a timeout and get a card without its title, or a video left
  unconverted.

Until then, this folder is still plain markdown: the `.md` files are the
content and `.moss/` is moss's own state. Nothing here requires moss to *read*.
";

/// [`render_pointer`] carrying SKILL.md's own frontmatter, for Claude Code.
///
/// The frontmatter is the discovery trigger (see [`skill_description`]), so it
/// has to survive verbatim even though nothing else does.
pub(crate) fn render_pointer_with_frontmatter() -> String {
    let src = skill_file("SKILL.md");
    // Delimiters included: the block has to survive verbatim. Where it ends is
    // `frontmatter_span`'s call, like every other split in the tree.
    let front = moss_core::frontmatter::frontmatter_span(src)
        // Char-aligned: `body` is a line-boundary offset from the splitter.
        .and_then(|span| src.get(..span.body))
        .unwrap_or_default();
    format!("{front}{}", render_pointer())
}

/// The stub left where a reference file used to be written.
///
/// Nothing here deletes (see [`super::sync`]'s header), so the references moss
/// wrote before it became a pointer are still on disk — stamped, and therefore
/// still moss's to refresh. Rewriting them as stubs is how their stale content
/// goes away without a `remove_file`: an agent that still holds the old path
/// lands on the command instead of on last release's prose.
pub(crate) fn render_reference_stub(topic: &str) -> String {
    format!(
        "# {topic}\n\n\
         This material is no longer copied to disk — it lives in the moss binary\n\
         named above, so it always matches the moss you are running:\n\n\
         ```\n\
         moss guide {topic}\n\
         ```\n\n\
         `moss guide --list` names every topic; `moss guide` is the entry point.\n\
         If no moss binary resolves, `../SKILL.md` says where to get one.\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_skill_text() -> String {
        let mut out = String::new();
        fn walk(dir: &Dir, out: &mut String) {
            for f in dir.files() {
                out.push_str(f.contents_utf8().unwrap_or(""));
            }
            for d in dir.dirs() {
                walk(d, out);
            }
        }
        walk(&SKILL_DIR, &mut out);
        out
    }

    /// The description is the trigger, not a label: Claude Code and Cursor both
    /// load this package on demand by matching it. Styling is most of what the
    /// skill contains and was once absent from the description entirely, so a
    /// request like "make the headings warmer" matched nothing and the agent
    /// styled from memory instead — hardcoded token names, `!important`, a
    /// nested `@layer`. These are the words that request actually uses.
    #[test]
    fn description_names_styling_so_a_restyle_request_matches_it() {
        let d = skill_description().to_lowercase();
        for word in ["style", "theme", "css", "color", "typography", "layout"] {
            assert!(d.contains(word), "SKILL.md description never says `{word}`: {d}");
        }
    }

    /// Cursor's frontmatter is derived from SKILL.md rather than repeated, so
    /// the two cannot drift the way `--help` and `cli_commands` did.
    #[test]
    fn cursor_frontmatter_reuses_the_skill_description() {
        assert!(render_cursor_mdc().contains(skill_description()));
    }

    /// The pointer has to close its own loop: a reader on a machine with no
    /// moss cannot act on a path alone. This is the whole cost of pointing
    /// rather than copying, so it is the one paragraph that must never be
    /// dropped as "boilerplate".
    #[test]
    fn the_pointer_says_where_to_get_moss() {
        let p = render_pointer();
        assert!(
            p.contains("moss/releases/latest"),
            "the pointer never says where to download moss"
        );
        assert!(
            p.contains("/Applications/moss.app/Contents/MacOS/moss"),
            "the pointer never says where the CLI lives once installed"
        );
    }

    /// The pointer is prose that ships to users, but it lives in Rust rather
    /// than in a markdown file — so `skill_package_paths_test`, which scans
    /// `assets/skills/moss/` for repo-relative paths, cannot see it. That test
    /// exists because the shipped guidance once opened with three moss-repo
    /// paths that do not exist in a user's site; moving text into a `const`
    /// must not move it out from under the guard.
    #[test]
    fn the_pointer_cites_no_moss_repo_paths() {
        const REPO_DIRS: [&str; 8] = [
            "crates/", "src-tauri/", "frontend/", "scripts/",
            "packages/", "plugins/", "tests/", ".githooks/",
        ];
        for (name, text) in [
            ("pointer", render_pointer()),
            ("cursor rule", render_cursor_mdc()),
            ("reference stub", render_reference_stub("authoring")),
        ] {
            for dir in REPO_DIRS {
                assert!(
                    !text.contains(dir),
                    "{name} cites the moss repo path `{dir}`, which does not exist \
                     in a user's site"
                );
            }
        }
    }

    /// A download link naming the private repo ships a 404 to every user.
    /// Keyed to the private repo's post-flip name (`Symbiosis-Lab/moss-desktop`,
    /// per `docs/archive/2026-09-13-f6-flip-day-runbook.md`) rather than its
    /// pre-flip name, since the pre-flip name becomes the PUBLIC repo at F6.
    #[test]
    fn no_shipped_text_links_the_private_repo() {
        for (name, text) in [
            ("pointer", render_pointer()),
            ("cursor", render_cursor_mdc()),
            ("flat", render_flat()),
            ("skill source", all_skill_text()),
        ] {
            for line in text.lines() {
                assert!(
                    !line.contains("github.com/Symbiosis-Lab/moss-desktop/")
                        && !line.trim_end().ends_with("github.com/Symbiosis-Lab/moss-desktop"),
                    "{name} links the PRIVATE source repo: {line}"
                );
            }
        }
    }

    #[test]
    fn skill_package_has_required_files() {
        assert!(SKILL_DIR.get_file("SKILL.md").is_some(), "SKILL.md missing");
        assert!(SKILL_DIR.get_file("references/authoring.md").is_some());
        assert!(SKILL_DIR.get_file("references/plugins.md").is_some());
        assert!(SKILL_DIR.get_file("references/importing.md").is_some());
    }

    /// Strip code spans from text to leave only scannable prose:
    /// - Triple-backtick fenced code blocks (``` ... ```) are replaced line-by-line
    ///   with blank lines so line counts are preserved.
    /// - Inline code spans (`` `...` `` and ` ``...`` `) are replaced with a single
    ///   space so the surrounding sentence still tokenises correctly.
    /// Only the remaining prose is returned.
    fn strip_code_spans(text: &str) -> String {
        // Phase 1: strip fenced blocks line-by-line.
        let mut after_fences = String::with_capacity(text.len());
        let mut in_fence = false;
        for line in text.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") {
                in_fence = !in_fence;
                after_fences.push('\n'); // keep fence line as blank (no token names in it)
            } else if in_fence {
                after_fences.push('\n'); // blank out fenced content
            } else {
                after_fences.push_str(line);
                after_fences.push('\n');
            }
        }

        // Phase 2: strip inline code spans (`...` and ``...``) from each prose line.
        // Replace the whole span (including backtick delimiters) with a space so
        // the preceding/following prose context doesn't merge across the gap.
        let re = regex::Regex::new(r"``[^`]+``|`[^`]+`").unwrap();
        re.replace_all(&after_fences, " ").into_owned()
    }

    /// Scan for hardcoded live contract names — bare classes (`moss-foo`) OR
    /// tokens (`--moss-foo`). Enforces the prose/reference split (D3): the skill
    /// points at `moss describe --json`, never bakes in versioned names.
    /// NOT flagged: namespace placeholders guarded by a preceding backtick or
    /// slash, or whose `moss-` is not followed by a lowercase letter (`` `moss-*` ``,
    /// `--moss-...`, `.moss/`), plus the package names `moss-api` / `moss-plugin`.
    /// Content inside triple-backtick fenced code blocks and inline code spans is
    /// also exempt — CSS examples legitimately show live token names for pedagogy.
    fn contract_name_leaks(text: &str) -> Vec<String> {
        let prose = strip_code_spans(text);
        // `-{0,2}` so BARE classes (zero dashes) are caught as well as tokens;
        // `.` is intentionally NOT in the exclusion set so a `.moss-foo` CSS
        // selector is caught too.
        let re = regex::Regex::new(r"(?:^|[^*`/])(-{0,2}moss-[a-z][a-z-]+)").unwrap();
        re.captures_iter(&prose)
            .map(|c| c.get(1).unwrap().as_str().to_string())
            .filter(|s| !s.starts_with("moss-api") && !s.starts_with("moss-plugin"))
            .collect()
    }

    #[test]
    fn skill_text_has_no_live_contract_names() {
        let leaks = contract_name_leaks(&all_skill_text());
        assert!(leaks.is_empty(), "live contract names leaked into skill: {leaks:?}");
    }

    #[test]
    fn guard_catches_bare_class_and_token_names() {
        // Regression guard for the earlier dash-only regex, which silently
        // missed bare class names. Both halves must be flagged.
        let leaks = contract_name_leaks("use moss-hero, .moss-grid {}, and --moss-color-accent");
        assert!(leaks.iter().any(|s| s == "moss-hero"), "must flag bare class: {leaks:?}");
        assert!(leaks.iter().any(|s| s == "moss-grid"), "must flag selector class: {leaks:?}");
        assert!(leaks.iter().any(|s| s == "--moss-color-accent"), "must flag token: {leaks:?}");
        // Package names are allowlisted, not leaks.
        assert!(contract_name_leaks("see moss-api and moss-plugin packages").is_empty());
    }

    #[test]
    fn guard_exempts_code_spans_but_catches_bare_prose() {
        // (a) Token names inside a ```css fenced block must NOT trip the guard.
        let fenced = "Some prose with no tokens here.\n\
```css\n\
:root {\n\
  --moss-color-bg: #fff;\n\
  --moss-color-text: #000;\n\
}\n\
```\n\
More prose, still no tokens.\n";
        assert!(
            contract_name_leaks(fenced).is_empty(),
            "token names inside a fenced block must not be flagged"
        );

        // (a2) Token names inside inline backtick code spans must also NOT trip it.
        // This covers patterns like `--moss-color-ui-accent: var(--moss-color-text)`.
        let inline = "Use `--moss-color-ui-accent: var(--moss-color-text)` in your override.";
        assert!(
            contract_name_leaks(inline).is_empty(),
            "token names inside inline code spans must not be flagged"
        );

        // (b) The same token name in bare prose (outside any code span) MUST trip the guard.
        let prose = "You should set --moss-color-bg directly in your CSS.";
        let leaks = contract_name_leaks(prose);
        assert!(
            leaks.iter().any(|s| s == "--moss-color-bg"),
            "token name in bare prose must be flagged: {leaks:?}"
        );
    }

    /// Cursor gets the pointer, wrapped in the frontmatter its rule format
    /// requires — not the prose. Cursor loads a rule by matching its
    /// `description:` (`alwaysApply: false`, no `globs`), so the frontmatter is
    /// the whole discovery mechanism and has to survive even though the body
    /// no longer carries the guidance itself.
    #[test]
    fn cursor_mdc_wraps_the_pointer_in_its_own_frontmatter() {
        let mdc = render_cursor_mdc();
        assert!(mdc.starts_with("---\n"), "must start with mdc frontmatter");
        assert!(mdc.contains("alwaysApply: false"));
        assert!(mdc.contains(skill_description()), "the discovery trigger is missing");
        assert!(mdc.contains("moss guide"), "Cursor is told nothing it can run");
        assert!(
            mdc.contains("moss/releases/latest"),
            "Cursor's copy must close its own loop too"
        );
        // SKILL.md's own YAML must not appear a second time inside the body —
        // two frontmatter blocks in one `.mdc` is a parse error, not a nuisance.
        assert_eq!(
            mdc.matches("name: moss").count(),
            0,
            "the skill's own frontmatter leaked into the body"
        );
    }

}
