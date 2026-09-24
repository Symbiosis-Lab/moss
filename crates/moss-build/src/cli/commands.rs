//! The CLI command table — one list, read by everything that describes the
//! command surface.
//!
//! moss used to keep this in four places: this table (for `moss describe`),
//! `run_mode.rs::print_help()`'s prose, `run_mode.rs::dispatch()`'s match arms,
//! and `help_for`'s own copy. Only one pair was ever pinned against another, so
//! the rest drifted silently — and a second binary (`moss-cli`) would have made
//! it five. The table lives here, below both binaries, so neither can hold a
//! private idea of what moss answers.
//!
//! What is NOT here: the dispatch arms. Which subcommands a given binary can
//! actually run differs between the two by design — moss-cli has no window
//! system, so `preview` and `edit` hand off to moss desktop instead of
//! running one (E2') — so the table describes the surface and each host
//! decides what it answers.

use moss_core::contract::describe::CliCommandInfo;

/// CLI command table.
///
/// Both `--help` screens and `moss describe` render from here, so the prose
/// cannot drift from the table. The one thing the table does not generate is
/// the dispatch arms — `src-tauri/src/startup/run_mode.rs` and
/// `crates/moss-cli/src/main.rs` — so a command added here with no arm is
/// advertised by both binaries and answered by neither.
pub fn cli_commands() -> Vec<CliCommandInfo> {
    vec![
        CliCommandInfo {
            name: "build",
            args: "<folder> [--serve] [--watch] [--no-plugins] [--allow-plugins] [--wait-plugins] [--strict] [--site-url=<url>]",
            description: "Build the site with plugins. --serve starts a local preview server when the build finishes, and --watch rebuilds on every file change. Use --no-plugins for fast CI/CD builds. A plugin that came with the folder and was never allowed in the moss app is refused unless --allow-plugins is passed. --strict exits 1 if the build reported any problems (they are printed to stderr and summarized as `moss: N problems …`); without it a build with warnings still exits 0.",
        },
        CliCommandInfo {
            name: "deploy",
            args: "<folder> [--prebuilt=<dir>] [--site-id=<name>] [--allow-plugins] [--overwrite-newer]",
            description: "Build and deploy the site. Where it goes is the folder's to say: a prebuilt directory wins, then a `[hooks] deploy` plugin, otherwise moss hosting. A folder publishing for the first time needs `moss env <staging|production|local>` first, and then registers a site named after the folder (or --site-id) — a site ID cannot be un-minted, so moss will not pick one for a folder that never named an environment. A plugin that came with the folder and was never allowed in the moss app is refused unless --allow-plugins is passed, exactly as in `moss build`. A publish to moss hosting, --prebuilt included, is refused when the live site was published from another copy after this folder last published — deploying would undo that publish. Bring this folder up to date with that copy (for example, git pull) and deploy again, or pass --overwrite-newer to replace the live site anyway.",
        },
        CliCommandInfo {
            name: "preview",
            args: "<path>",
            description: "Open a GUI preview window, in moss desktop (which `moss desktop install` provides). <path> may be a folder or a .md/.markdown file.",
        },
        CliCommandInfo {
            name: "edit",
            args: "<path>",
            description: "Open GUI preview in moss desktop (which `moss desktop install` provides), then open <path> directly in the editor (agent-friendly scriptable entrypoint).",
        },
        CliCommandInfo {
            name: "desktop",
            args: "install",
            description: "Install moss desktop, the GUI editor and preview app. Downloads the signed release from the same update channel the app itself uses and installs it into /Applications. macOS only for now; elsewhere it says where to get moss desktop. It asks before downloading, so it refuses when stdin is not a terminal.",
        },
        CliCommandInfo {
            name: "domain",
            args: "<list|link> <folder> [<domain>]",
            description: "List or link custom domains for a project's site.",
        },
        CliCommandInfo {
            name: "env",
            args: "<staging|production|local> <folder>  |  <folder>",
            description: "Set or show the hosting environment (staging / production / local) for a project.",
        },
        CliCommandInfo {
            name: "describe",
            args: "[--json] [--css <selector>]",
            description: "Print the moss contract surface (tokens, components, custom properties, frontmatter, plugin hooks, slots, CLI). --json for machine-readable output. --css <selector> prints the default CSS rules mentioning that selector or custom property, with their enclosing media queries and the comments explaining them — what a theme override has to displace.",
        },
        CliCommandInfo {
            name: "import",
            args: "<url> [<folder>] [-r|--recursive]  |  --list <file> [<folder>] [-r|--recursive]",
            description: "Import a URL (or batch of URLs) into a project folder as markdown.",
        },
        CliCommandInfo {
            name: "agents",
            args: "init",
            description: "Seed .moss/AGENTS.md with this site's own conventions, for a coding agent to read. moss's own guidance is `moss guide`.",
        },
        CliCommandInfo {
            name: "guide",
            args: "[<topic>|all|--list]",
            description: "Print moss's agent guidance: project conventions, the four styling rungs, the hard rules, and the checks that tell you whether a change worked. No argument prints the entry point; `--list` names the topics (authoring, plugins, debugging, importing, example); `all` prints every topic as one document. Reads nothing but the binary, so it needs neither a project nor a network — and unlike a copy written into a project folder, it cannot describe a different moss than the one running it.",
        },
        CliCommandInfo {
            name: "rename",
            args: "<old_path> <new_path>",
            description: "Rename a file or folder and rewrite every [[wikilink]] and [text](link) that points at it, project-wide. Scriptable, so it is the safe way to bulk-rename (e.g. localizing filenames) without breaking links. `mv` is an alias.",
        },
        CliCommandInfo {
            name: "doctor",
            args: "--math [<folder>]",
            description: "Audit a vault without changing it. --math reports every $-span moss would parse as math (these render as their LaTeX source; moss does not typeset math yet), so an imported corpus can be checked before publishing. Escape false positives as \\$.",
        },
        CliCommandInfo {
            name: "list",
            args: "[<folder>] [--json]",
            description: "List every document the last build parsed: source file, the URL it publishes at, the BCP-47 tag it will carry in `<html lang>`, kind, date, title, and — in the HIDDEN column — why it is absent from generated listings (draft, unlisted, nav-item, slot). The LANG column is how you confirm a new language tree registered: an unrecognized folder name is treated as ordinary content and the build succeeds either way (`moss describe --json` lists the recognized codes under `languages`). Reports the last build, so run `moss build` first. Use --json to filter the same rows programmatically.",
        },
        CliCommandInfo {
            name: "history",
            args: "[<path>] [--json] | --save [<name>] | <path> --restore --at <id> [--copy] | --restore --at <id> --yes",
            description: "moss's own version history, kept outside the site folder (not git): every landed publish gets a snapshot, and `--save` takes one on demand. With no path, lists the site's timeline newest first: when, whether it was a publish or a named save, a `live` marker on the version currently published, and what changed. With a path, lists that one page's timeline instead, noting when its content was not kept (over the size ceiling, or unreadable at publish time). `--save [<name>]` builds the site and saves a version of it right now. `--restore --at <id>` restores that version — a path restores just that page (`--copy` writes it beside the current file instead of overwriting it); with no path it restores the whole site, which needs `--yes` since it can move files to the Trash. `<id>` is a version's id from the timeline, or an unambiguous prefix of one. This command always operates on the site containing the current directory — it takes no folder argument.",
        },
    ]
}

/// The subcommands that parse `--help` themselves; [`help_for`] declines
/// them so their per-mode usage stays authoritative.
const OWN_HELP: [&str; 7] = ["list", "doctor", "rename", "domain", "env", "import", "history"];

/// Render `moss <cmd> --help` from the table `moss describe` already publishes.
///
/// Six subcommands took their argument positionally and so read `--help` as
/// that argument: `build`/`deploy` reported `Folder does not exist: …/--help`,
/// `preview`/`edit` the same for a path, `describe` ignored the flag and dumped
/// the whole contract, and `agents` said `unknown agents subcommand: --help`.
/// Every one of those reads as "your argument is wrong" rather than "that flag
/// landed in an argument slot" — and `<cmd> --help` is the first thing anyone
/// types at a CLI they do not know, an agent most of all. Observed in the
/// 2026-08-05 trial (docs/archive/2026-08-05-agent-surface-vs-hugo.md).
///
/// Drawing the text from [`cli_commands`] means a command cannot describe
/// itself one way in `moss describe --json` and another way at `--help`.
///
/// Returns `None` for the subcommands that already parse `--help` themselves
/// (`list`, `doctor`, `rename`, `domain`, `env`, `import`) — each prints
/// per-mode usage this one-line table cannot carry, so it stays authoritative.
pub fn help_for(cmd: &str) -> Option<String> {
    if OWN_HELP.contains(&cmd) {
        return None;
    }
    let info = cli_commands().into_iter().find(|c| c.name == cmd)?;
    Some(format!(
        "Usage: moss {} {}\n\n{}\n\nRun `moss --help` for all commands, or `moss describe` for the\nfull contract surface (tokens, components, frontmatter fields).",
        info.name, info.args, info.description
    ))
}

/// The examples section of `moss --help`, each line tagged with the command it
/// demonstrates so a binary that cannot answer that command does not advertise
/// it. Not rendered from [`cli_commands`] because an example is a second thing
/// to know about a command, not a restatement of its grammar.
const EXAMPLE_LINES: &[(&str, &str)] = &[
    ("build", "moss build docs/public/              # Build with plugins"),
    ("build", "moss build ~/blog/ --serve           # Build with plugins and serve"),
    ("build", "moss build ~/blog/ --no-plugins      # Fast build for CI/CD"),
    ("build", "moss build ~/blog/ --wait-plugins    # Build and wait for plugins (e2e testing)"),
    ("build", "moss build ~/blog/ --strict          # Fail (exit 1) if the build reported problems"),
    ("list", "moss list ~/blog/                    # Inventory every page: kind, URL, title, lang"),
    ("list", "moss list ~/blog/ --json             # Same inventory, machine-readable"),
    ("deploy", "moss deploy ~/blog/                  # Build and publish the site"),
    ("deploy", "moss deploy ~/site/ --prebuilt=_site # Upload _site/ as the site, skipping the build"),
    ("deploy", "moss deploy ~/blog/ --site-id=my-blog  # First publish: register my-blog.mosspub.com"),
    ("preview", "moss preview ~/blog/                 # Open a preview window"),
    ("preview", "moss preview ~/blog/posts/article.md # Preview and navigate to the article"),
    ("edit", "moss edit ~/blog/posts/article.md    # Preview, then open the article in the editor"),
    // The binary that can open a preview window is the binary that opens one
    // with no arguments at all.
    ("preview", "moss                                 # Launch the app window"),
    ("domain", "moss domain list ~/blog/             # Show domains linked to this project's site"),
    ("domain", "moss domain link ~/blog/ example.com # Link a custom domain (same path as the UI)"),
    ("env", "moss env staging ~/blog/             # Set the hosting environment to staging"),
    ("env", "moss env ~/blog/                     # Show the hosting environment"),
    ("import", "moss import https://example.com/     # Mirror a site into the current directory"),
    ("import", "moss import https://blog.example.com/post/ ~/Sites/me/articles/"),
    ("import", "moss import --list urls.txt ~/Sites/me/articles/"),
    ("doctor", "moss doctor --math ~/blog/            # Every $-span moss would parse as math"),
];

/// The only two flags that belong to no command. Everything else a command
/// takes is in its `args` and explained in full by `moss <cmd> --help`, which
/// renders from the same table — listing them here again was a seventh copy of
/// the fact this slice exists to have one of.
const OPTION_LINES: &str = "    -h, --help                     Show this help message
    -V, --version                  Print the version and exit";

/// The one environment variable that belongs to no command. `build`, `deploy`
/// and `history` all log through `build::cli_output::install_headless_logger`,
/// which reads `MOSS_LOG_LEVEL` and nothing else, so `RUST_LOG` is silently
/// ignored; without this line the only way to find the switch that turns on the
/// timing lines was to read that source (2026-09-19, a build-speed investigation
/// by an agent).
///
/// The level split is read off the `log::info!` / `log::debug!` call sites
/// (2026-09-20, after the first wording put the `[render]` timings at `info`).
/// At `info`: `[slots]`, `[search]` and two `[render]` summaries (all
/// `target: "timing"`), one `[cache]` line and the `build.summary` line. Every
/// other `[render]` line and all the `[reduce]`, `[scan]`, `[pipeline]` and
/// `[build]` ones are `debug`. Change a call's level and this text moves with it.
const ENVIRONMENT_LINES: &str = "    MOSS_LOG_LEVEL=<level>         Log verbosity on stderr: error, warn
                                   (default), info or debug. `info` adds the
                                   build.summary line (per-phase ms) and the
                                   [slots], [search], [cache] and two [render]
                                   summary lines; `debug` adds the per-step
                                   [render], [reduce], [scan] and [build]
                                   timings. RUST_LOG is not read.";

/// The first sentence of a description — what a command is, without the
/// paragraph of consequences `<cmd> --help` and `moss describe` print in full.
fn summary(description: &str) -> &str {
    match description.find(". ") {
        Some(end) => &description[..=end],
        None => description,
    }
}

/// Greedy wrap at 78 columns, every line after the first carrying `indent`.
fn wrap(text: &str, indent: &str) -> String {
    let mut out = String::new();
    let mut column = indent.len();
    for word in text.split_whitespace() {
        let width = word.chars().count();
        if column + width + 1 > 78 && column > indent.len() {
            out.push('\n');
            out.push_str(indent);
            column = indent.len();
        } else if !out.is_empty() {
            out.push(' ');
            column += 1;
        }
        out.push_str(word);
        column += width;
    }
    out
}

/// The cold-start paragraph, printed at the end of `moss --help` by both
/// binaries.
///
/// moss is not on `PATH` — the `/usr/local/bin/moss` symlink was retired
/// deliberately (see `cli::agents::sync::bundled_cli_path` for why a symlink
/// does not even reach an agent launched from Finder). That leaves an absolute
/// path as the only reliable way to invoke moss, and nothing on a fresh machine
/// tells anyone what it is. `--help` is the one place guaranteed to be read by
/// someone who has found the binary but does not yet know what it can do, so it
/// is where the path belongs.
///
/// The sentence is printed ready to paste into a coding agent because that is
/// the actual first step of the journey: a human tells an agent to use moss.
/// Everything after it is self-bootstrapping — `moss build` writes the skill
/// files and prints where they landed (see `sync::guidance_pointer`), so this
/// one line is the whole handoff.
fn agent_footer() -> String {
    let cli = crate::cli::agents::sync::bundled_cli_path();
    format!(
        "
USING MOSS FROM A CODING AGENT:
    First time in a folder — paste this to your agent:

    Build a website from this folder: run {0} build .

    That points your agent at the authoring and theming guide. Afterwards —
    including after previewing the folder in the moss app, which does the
    same — say what you want and nothing more:

    Restyle this site: warmer palette, serif headings, quiet nav.

    Codex and other agents with no skill mechanism read nothing on their own,
    so name the guide in the prompt:

    Restyle this site per `{0} guide`: warmer palette, serif headings.

    `moss describe --json` is the live contract — token, component and
    frontmatter names change between releases, so read it rather than relying
    on memory.",
        cli.display()
    )
}

/// What `moss compile` gets. Renamed to `moss build` on 2026-04-24 (#554),
/// but shell history and old docs still carry it, and both binaries have to
/// say the same thing — the open binary is the one an old script finds first.
pub fn renamed_compile_hint(folder: Option<&str>) -> String {
    format!(
        "error: `moss compile` has been renamed to `moss build`.\n       Try: moss build {}",
        folder.unwrap_or("<folder>")
    )
}

/// `moss --help`, rendered from the same table `moss describe` publishes.
///
/// `answers` is what this binary can actually run: the open binary has no
/// window, so `preview` and `edit` are named at the end as needing the app
/// rather than listed as commands and demonstrated in examples. Everything
/// else is identical between the two binaries, and the parity test compares
/// the whole text.
///
/// Rendering rather than printing prose ended the sixth copy of the command
/// list: this section was hand-kept in `run_mode.rs` and had drifted from the
/// table it claimed to mirror (2026-09-09, slice C5a).
pub fn top_level_help(version: &str, answers: &dyn Fn(&str) -> bool) -> String {
    let table = cli_commands();
    let mut text = format!(
        "moss {version} — build and publish a website from a folder\n\nUSAGE:\n    moss <command> [options]\n\nCOMMANDS:\n"
    );
    for cmd in table.iter().filter(|cmd| answers(cmd.name)) {
        text.push_str(&format!(
            "    {}\n",
            wrap(&format!("{} {}", cmd.name, cmd.args), "        ")
        ));
        text.push_str(&format!("        {}\n\n", wrap(summary(cmd.description), "        ")));
    }
    text.push_str("OPTIONS:\n");
    text.push_str(OPTION_LINES);
    text.push_str("\n\nENVIRONMENT:\n");
    text.push_str(ENVIRONMENT_LINES);
    text.push_str("\n\nEXAMPLES:\n");
    for (_, line) in EXAMPLE_LINES.iter().filter(|(verb, _)| answers(verb)) {
        text.push_str(&format!("    {line}\n"));
    }
    let app_only: Vec<&str> = table
        .iter()
        .map(|cmd| cmd.name)
        .filter(|name| !answers(name))
        .collect();
    if !app_only.is_empty() {
        text.push_str(&format!(
            "\nCommands that need the moss app: {}.\n",
            app_only.join(", ")
        ));
    }
    text.push_str(&agent_footer());
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every documented command must answer `--help` by one route or the other:
    /// either it parses the flag itself (and is on `help_for`'s decline list) or
    /// `help_for` renders it from this table. A command that is on neither is
    /// one whose `--help` falls into a positional slot — the exact failure this
    /// table was wired up to end, and the one an agent hits first.
    #[test]
    fn every_command_answers_help() {
        for cmd in cli_commands() {
            let self_handled = OWN_HELP.contains(&cmd.name);
            assert!(
                self_handled || help_for(cmd.name).is_some(),
                "`moss {} --help` is answered by nobody: not on help_for's decline \
                 list, and help_for could not render it",
                cmd.name
            );
        }
    }

    /// An example tagged with a name no command has is dropped from the open
    /// binary's help and kept in the app's, so nothing goes red — the one
    /// drift the two-screen comparison cannot see.
    #[test]
    fn every_example_names_a_real_command() {
        let names: Vec<&str> = cli_commands().into_iter().map(|c| c.name).collect();
        for (verb, line) in EXAMPLE_LINES {
            assert!(names.contains(verb), "example tagged `{verb}`, which is no command: {line}");
        }
    }

    /// The COMMANDS entry is one sentence, not the paragraph `<cmd> --help`
    /// prints — and the sentence has to end where a sentence ends.
    #[test]
    fn a_summary_stops_at_the_first_sentence() {
        assert_eq!(summary("One. Two. Three."), "One.");
        assert_eq!(summary("No full stop at all"), "No full stop at all");
        assert_eq!(summary("Trailing only."), "Trailing only.");
        for cmd in cli_commands() {
            let line = summary(cmd.description);
            assert!(
                line.len() < 260,
                "`{}`'s first sentence is a paragraph: {line}",
                cmd.name
            );
        }
    }

    /// A word longer than the column limit is emitted long rather than retried,
    /// which is the difference between a wide line and an infinite loop. The
    /// wrap counts characters, not bytes, because these descriptions carry em
    /// dashes and would otherwise break several columns early.
    #[test]
    fn wrapping_terminates_and_measures_characters() {
        let long = "x".repeat(120);
        assert_eq!(wrap(&long, "  "), long);
        assert_eq!(wrap("", "  "), "");
        let dashes = "— ".repeat(30);
        assert!(
            wrap(dashes.trim(), "").lines().all(|l| l.chars().count() <= 78),
            "wrapped by bytes, not characters"
        );
    }

    /// `MOSS_LOG_LEVEL` is the only way to see the timing lines and `RUST_LOG` is
    /// ignored, so `--help` has to say both, and which level shows which: at
    /// `info` a build prints its `build.summary` and a handful of `[slots]` /
    /// `[render]` / `[cache]` summary lines, and the per-step `[render]`,
    /// `[reduce]`, `[scan]` and `[build]` breakdown only appears at `debug`. An
    /// earlier wording promised the `[render]` timings at `info`, where only two
    /// `[render]` summary lines print.
    #[test]
    fn top_level_help_says_how_to_see_timing_output() {
        let text = top_level_help("0.0.0", &|_| true);
        // The block is wrapped for the terminal; compare it as words.
        let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(text.contains("\nENVIRONMENT:\n"), "no ENVIRONMENT section:\n{text}");
        assert!(text.contains("MOSS_LOG_LEVEL"), "MOSS_LOG_LEVEL not named:\n{text}");
        assert!(
            words.contains("`info` adds the build.summary line (per-phase ms) and the [slots], [search], [cache] and two [render] summary lines;"),
            "what `info` shows is not stated:\n{text}"
        );
        assert!(
            words.contains("`debug` adds the per-step [render], [reduce], [scan] and [build] timings."),
            "what only `debug` shows is not stated:\n{text}"
        );
        assert!(text.contains("RUST_LOG is not read"), "RUST_LOG's silence not stated:\n{text}");
    }

    /// The rendered text has to carry the argument grammar, or it is a slogan
    /// rather than help — the caller still cannot tell where the folder goes.
    #[test]
    fn rendered_help_carries_the_argument_grammar() {
        let text = help_for("build").expect("build is not self-handled");
        assert!(text.starts_with("Usage: moss build "), "no usage line: {text}");
        assert!(text.contains("--strict"), "flags missing from usage: {text}");
    }

    /// `every_command_answers_help` cannot see a verb that declines rendering
    /// (an `OWN_HELP` entry) and then answers nothing — it only asserts that
    /// SOME route exists, not that the declared one does. `desktop` renders
    /// from the table, so this pins the actual route rather than the "some
    /// route" that test would also accept from a bare `OWN_HELP` entry.
    #[test]
    fn desktop_install_help_renders_from_the_table() {
        let text = help_for("desktop").expect("desktop should render from the table");
        assert!(text.contains("install"), "grammar missing from usage: {text}");
    }
}
