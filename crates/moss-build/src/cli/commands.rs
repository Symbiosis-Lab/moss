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
/// the dispatch arms — the desktop app's startup dispatcher and
/// `crates/moss-cli/src/main.rs` — so a command added here with no arm is
/// advertised by both binaries and answered by neither.
pub fn cli_commands() -> Vec<CliCommandInfo> {
    vec![
        CliCommandInfo {
            name: "build",
            args: "[<folder>] [--serve [--watch]] [--no-plugins] [--allow-plugins] [--wait-plugins] [--strict] [--site-url=<url>]",
            description: "Build the site with plugins. With no folder, builds the site containing the current directory, as `moss history` finds it. --serve starts a local preview server when the build finishes, on port 8080 or the next free port (there is no --port flag; set MOSS_PREVIEW_PORT_BASE=<port> to start the search elsewhere), and --watch, which only works with --serve, rebuilds on every file change and refreshes the preview. Use --no-plugins for fast CI/CD builds. A plugin that came with the folder and was never allowed in the moss app is refused unless --allow-plugins is passed. --strict exits 1 if the build reported any problems (they are printed to stderr and summarized as `moss: N problems …`); without it a build with warnings still exits 0.",
        },
        CliCommandInfo {
            name: "deploy",
            args: "<folder> [--prebuilt=<dir>] [--site-id=<name>] [--email <address>] [--allow-plugins] [--overwrite-newer] [--dry-run [--json]] [--accept-removals] [--accept-stale-content] [--retry-copies=<channel>]",
            description: "Build the folder and publish it: a prebuilt directory if one is set (--prebuilt=<dir>), else through a `[hooks] deploy` plugin, else to the moss host. A first publish to the moss host: `moss env <staging|production|local> <folder>`, then `moss deploy <folder> --site-id=<name> --email <address>`, which emails a link that signs the folder in, waits up to 15 minutes for it to be opened, and publishes the site as <name>. A site name can't be given back, and no email is sent without --email. A folder that isn't signed in is refused with the command that signs it in. Other refusals and their ways out: a plugin that came with the folder and was never allowed on this computer (--allow-plugins); a live site published from another copy of this folder since this copy last published (bring this folder up to date, or --overwrite-newer); an address the site would stop serving that you didn't delete (add the printed line under [redirects] in .moss/config.toml, or --accept-removals); changed pages older than what is published (restore them, or --accept-stale-content); two sources with one address (no override: give each its own). --dry-run builds as a deploy does, sends and records nothing, prints what would change and whether a deploy would be refused (exit 1), and asks the server once whether this folder's key may publish its site and whether another copy published since (no answer refuses nothing and says why). --dry-run --json prints the whole report: `refusal` is {kind, text}, kind one of unresolved_inputs, missing_references, drafts_linked, links_in_cloud, links_in_flight, pending_derivations, removals, stale_content, output_collision, no_environment, foreign_inputs, not_signed_in, not_owner, stale_copy; `owner` is {state: answered, site, access: authorized, not_owner, not_signed_in or not_found} or {state: unchecked, reason}; `pending_removals` lists each refused address with its cause (none with --prebuilt, whose refused files are `transfer.removed_output_paths`); `redirects_to_add` holds the [redirects] lines; a failure prints {\"error\"}. An `output_path` is a file of the build output, an `address` or `moved_to` is what a visitor asks for, a `source_path` is a file in the folder. After a publish, moss posts each new page to the channels its `syndicate:` names (at most ten per channel per run, never twice) and adds each copy's address to the page's `syndicated:`; --retry-copies=<channel> resends copies whose send was cut off.",
        },
        CliCommandInfo {
            name: "preview",
            args: "<path>",
            description: "Open a GUI preview window, in moss desktop (which `moss desktop install` provides). <path> may be a folder or a .md/.markdown file.",
        },
        CliCommandInfo {
            name: "edit",
            args: "[folder] [--serve] [--no-browser] [--port <n>] [--public-url <https origin> [--public-preview-port <n>]]",
            description: "Open the editor in moss desktop when installed and stdin is a terminal. With --serve, --no-browser or --port, without a terminal, or without desktop, build, watch and serve the editor and preview. A terminal opens a sign-in link in the default browser; --no-browser or piped stdin prints a link and QR. --port selects the UI port, and the preview takes the next one. --public-url names the HTTPS origin a reverse proxy serves the UI at, for another device; it needs --port, and the proxy serves the preview at the same host on the next port or on --public-preview-port, forwarding each to the port moss edit prints. The folder defaults to the current directory. A second edit requests a fresh sign-in link from the live owner.",
        },
        CliCommandInfo {
            name: "sessions",
            args: "[revoke <id> | revoke --all]",
            description: "List browser sessions (id, created, last seen, user agent), revoke one session by id, or revoke all sessions on this machine.",
        },
        CliCommandInfo {
            name: "desktop",
            args: "install",
            description: "Install moss desktop, the GUI editor and preview app. Downloads the signed release from the same update channel the app itself uses and installs it into /Applications. macOS only for now; elsewhere it says where to get moss desktop. It asks before downloading, so it refuses when stdin is not a terminal.",
        },
        CliCommandInfo {
            name: "domain",
            args: "<list|link|status> <folder> [<domain>] [--json]",
            description: "List, link or check custom domains for a project's site; link prints the DNS records to set.",
        },
        CliCommandInfo {
            name: "site",
            args: "transfer <folder> (--to <email> [--keep-publishing] [--dry-run] | --cancel) [--json]",
            description: "Hand a project's hosted site to another email address. The receiver gets one link by email and accepts from it within 72 hours; the address, files, subscribers and history stay with the site, and this folder's key can no longer publish it. With `--keep-publishing` the receiver may also take this folder's key, on its own button, so this folder keeps publishing the site as them; it is refused when the key has published another site of yours. `--dry-run` shows what would move and sends nothing; `--cancel` withdraws a pending transfer.",
        },
        CliCommandInfo {
            name: "env",
            args: "<staging|production|local> <folder>  |  <folder>",
            description: "Set or show which seta environment (staging / production / local) a project publishes to.",
        },
        CliCommandInfo {
            name: "describe",
            args: "[--json] [--css <selector>]",
            description: "Print the moss contract surface (tokens, components, custom properties, frontmatter, plugin hooks, slots, CLI). --json for machine-readable output. --css <selector> prints the default CSS rules mentioning that selector or custom property, with their enclosing media queries and the comments explaining them — what a theme override has to displace.",
        },
        CliCommandInfo {
            name: "render",
            args: "--json",
            description: "Render standalone Markdown documents from newline-delimited JSON on stdin. Each {id, source} request returns {id, html, css}; malformed requests return an error and do not stop later lines. Input is limited to 8 MiB per line. Raw HTML is preserved in the returned HTML and must be sanitized by the embedding application before displaying untrusted documents.",
        },
        CliCommandInfo {
            name: "tools",
            args: "download <ffmpeg|git>",
            description: "Download a command-line tool moss needs, into moss's own cache (~/.moss/bin). Running it is the consent: moss downloads nothing else without asking. The download is checked against a pinned SHA-256 before it is used. Exits 0 when the tool is available, 1 when the download or check fails.",
        },
        CliCommandInfo {
            name: "root",
            args: "<path> --json",
            description: "Print the moss site that owns a path, and the address it is live at, as {\"root\": string|null, \"address\": string|null}; address is null before a first publish and left out when it cannot be read (a settings file still in the cloud or from a newer moss). Uses the canonical .moss ownership check, stops at the home directory, and does not scan or change files.",
        },
        CliCommandInfo {
            name: "plugin",
            args: "hash <plugin-dir> [--json]  |  install <id> [<folder>]  |  secret set <plugin> <key> [<folder>]",
            description: "`install` puts a plugin into the existing site at <folder> and allows it. An explicit folder must be the site root with its own .moss directory; without a folder, moss finds the site containing the current directory. Plugins come from the plugin registry, or are copied out of moss when moss ships them. Typing the command is the consent, so it prints one line saying the plugin is approved for this user on this machine; the approval is kept in moss's per-user app data (the same store the moss app reads), never in the site folder. The registry catalog is fetched on every install so a withdrawn version is not installed; when it cannot be reached the last cached catalog is used and its age is printed, and with none cached the install fails naming the network error. Exits 1 on failure, 2 on bad arguments. `hash` prints the code hash of a plugin directory: the value moss records when a plugin is allowed and the registry pins per version. Needs no site. Prints the hex digest, or with --json {\"id\", \"version\", \"code_sha256\"}. Exits 1 when the directory has no readable manifest.json or entry file. `secret set` stores a credential the plugin's manifest asks the user for (an app password, a token), read from standard input, in moss's secret store outside every site folder, where the app's credential prompt keeps it too; empty input removes it. The key must be one the manifest declares.",
        },
        CliCommandInfo {
            name: "import",
            args: "<url> [<folder>] [-r|--recursive]  |  --list <file> [<folder>] [-r|--recursive]  |  <folder> [-r|--recursive] [--with <derivation-id>] [--allow-plugins]  |  --status [<site>] [--json] [--allow-plugins]",
            description: "Import a URL (or batch of URLs) into a project folder as markdown. Given an existing folder instead, show the files in it that an installed plugin can turn into pages (for example documents): records the choice in .moss/config.toml and writes a page beside each file, never overwriting one; `--with` picks a derivation when several accept the folder. `-r`/`--recursive` covers the folder and every folder beneath it (hidden folders and `.moss` skipped): each folder holding files the derivation accepts and with no decision yet is shown and gets its pages, a folder already decided (shown or kept, by its own entry or an ancestor's) keeps its decision, and a shown one still gets any pages it lacks, and one line per folder (folder, what was done, pages written) plus a total is printed. A plugin the site carries runs only if allowed, as in a build: without `--allow-plugins` a plugin not yet allowed in the app is refused with the same sentence a build prints, and nothing is written. `--status` lists every folder holding such files with its decision (undecided, show, keep), file count, pages still missing and files a plugin declined; `--json` prints {\"folders\": [{folder, derivation_id, noun_one, noun_other, count, decision, stubs_missing, skipped: [{file, reason, message}], failed: [{file, message}]}]}.",
        },
        CliCommandInfo {
            name: "agents",
            args: "sync | init",
            description: "`sync` writes the pointer to moss's guidance for the coding agents installed here or in your home folder (.claude/skills/moss/SKILL.md for Claude Code, .cursor/rules/moss.mdc for Cursor); a build never creates these, only keeps them current once written. `init` does the same and seeds .moss/AGENTS.md with this site's own conventions. moss's own guidance is `moss guide`.",
        },
        CliCommandInfo {
            name: "guide",
            args: "[<topic>|all|--list]",
            description: "Print moss's agent guidance: project conventions, the four styling rungs, the hard rules, and the checks that tell you whether a change worked. No argument prints the entry point; `--list` names each topic with the line saying when to read it; `all` prints every topic as one document. Reads nothing but the binary, so it needs neither a project nor a network — and unlike a copy written into a project folder, it cannot describe a different moss than the one running it.",
        },
        CliCommandInfo {
            name: "rename",
            args: "<old_path> <new_path>",
            description: "Rename a file or folder and rewrite every [[wikilink]] and [text](link) that points at it, project-wide. Scriptable, so it is the safe way to bulk-rename (e.g. localizing filenames) without breaking links. `mv` is an alias.",
        },
        CliCommandInfo {
            name: "new",
            args: "<folder> [--template <id>] [--lang <code>] [--title \"<title>\"] [--json]  |  --translation <page.md> --lang <code> [--json]  |  --list [--json]",
            description: "Make a page from a template, or a page's version in another language. <folder> is a folder in the site; with --template the page starts from that template and lands in the template's own folder when it names one, and --title names the file and sets its title. --translation writes the page's version in <code> at the mirrored path (work/x.md becomes zh-hans/work/x.md), copies its facts across, starts from the template for that language, and links the two with translationKey. --list prints the site's templates, language folders and default language; `--list --json` is the machine-readable form. Refuses rather than guesses: a template whose folder is gone creates nothing.",
        },
        CliCommandInfo {
            name: "expand",
            args: "<page.md|folder> [--dry-run] [--allow-plugins]",
            description: "Write embeds out as text. Each line holding only a `![[file.ext#fragment]]` embed of a file a plugin derives text from (once its text has been derived; missing text is derived first) or only a `![[note]]` / `![[note#heading]]` transclusion is replaced with the text it stands for, so the page can be edited as plain markdown. Embeds whose text is not available (skipped, unreadable, unresolved) are left as written, frontmatter is kept byte for byte, and a file is written only when something changed. --dry-run reports without writing. A plugin that derives the text must be allowed, as in a build: without `--allow-plugins` one not yet allowed in the app is refused with the same sentence a build prints.",
        },
        CliCommandInfo {
            name: "fetch",
            args: "<folder>",
            description: "Download every file in a folder that is still only in the cloud (Google Drive, iCloud Drive, Dropbox, OneDrive), so the folder can be copied or backed up without each read blocking on the provider. moss's own cache (`.moss/cache/`) is left in the cloud, since a build downloads what it needs again: exclude it from a copy (`rsync --exclude .moss/cache`). Reads and lists only: it writes nothing in the folder. Progress goes to stderr. Exits 0 when every file but the cache is on this device, and 1 when the provider stops delivering (nothing arrives for 90 seconds), naming the provider, how many files are still in the cloud with a short sample, and what to do. `moss build` makes the same check first and exits 1 the same way.",
        },
        CliCommandInfo {
            name: "doctor",
            args: "--math [<folder>] | --surface [<folder>] [--all] [--json]",
            description: "Audit a vault without changing it. --math reports every $-span moss would parse as math (these render as their LaTeX source; moss does not typeset math yet), so an imported corpus can be checked before publishing. Escape false positives as \\$. --surface lists markdown constructs a cleaner form could replace: classed divs repeated on three or more pages (a fact a frontmatter field could hold), theme rules that match no element on any page of the last build, theme selectors keyed on a page address (they break when the page is renamed), and the class names the theme invents that moss does not write, each marked as used in pages or nowhere (`theme_own_classes` in --json). --all lists every classed div, even one on a single page, to check that a migration left none. --json prints the surface report as JSON.",
        },
        CliCommandInfo {
            name: "migrate",
            args: "[<folder>] [--apply] [--json] | [<folder>] --lift <class> --as <field> --place <place> [--apply] [--json]",
            description: "Rewrite what `moss doctor --surface` finds. Without --apply it prints the change as a diff and writes nothing; with --apply it first saves a version of the site (undo with `moss history --restore`), then writes, changing only the frontmatter lines it names. With no --lift it removes a `translationKey` the page's path already implies, and only when a scratch copy of the site builds to the same output with and without it. --lift <class> --as <field> --place <place> moves each `::: {.class}` div holding one paragraph of plain text into its page's frontmatter as <field> and declares the field in .moss/config.toml; --place says where moss draws it instead of the div (eyebrow, subtitle, key-figure or facts), or none to keep it in the frontmatter only (it sets the field's place for the whole site, so none also removes a place the field already has); a page whose div holds more than plain text, holds it twice, or whose frontmatter already gives <field> a different value is left as written and listed. --json prints the changes, skipped pages and saved version as JSON.",
        },
        CliCommandInfo {
            name: "list",
            args: "[<folder>] [--json]",
            description: "List every document the last build parsed: source file, the URL it publishes at, the BCP-47 tag it will carry in `<html lang>`, kind, date, title, and — in the HIDDEN column — why it is absent from generated listings (draft, unlisted, nav-item, slot). The LANG column is how you confirm a new language tree registered: an unrecognized folder name is treated as ordinary content and the build succeeds either way (`moss describe --json` lists the recognized codes under `languages`). Reports the last build, so run `moss build` first. Use --json to filter the same rows programmatically.",
        },
        CliCommandInfo {
            name: "history",
            args: "[<folder>] [<path>] [--json] | [<folder>] --save [<name>] [--json] | [<folder>] <path> --restore --at <id> [--copy] | [<folder>] --restore --at <id> --yes",
            description: "moss's own version history, kept inside the site at `.moss/history` (not git — a git user already has their own history, and this store never adds commits): every landed publish gets a snapshot, and `--save` takes one on demand. With no path, lists the site's timeline newest first: when, whether it was a publish or a named save, a `live` marker on the version currently published, and what changed. With a path, lists that one page's timeline instead, noting when its content was not kept (over the size ceiling, or unreadable at publish time). A page's timeline also lists each outside edit kept when the editor saved the person's text over lines another writer had changed (`\"trigger\": \"displaced\"` in --json); restore one with `<path> --restore --at <id>`. `--save [<name>]` saves the files currently on disk without running a build, ending with one line naming the version saved; `--save --json` prints that one version as a single JSON object on stdout instead (diagnostics go to stderr), so a script can read the saved id directly. `--restore --at <id>` restores that version — a path restores just that page (`--copy` writes it beside the current file instead of overwriting it); with no path it restores the whole site, which needs `--yes` since it can move pages added since that version to the Trash. `<id>` is a version's id from the timeline, or an unambiguous prefix of one. With no folder, this command operates on the site containing the current directory; pass `<folder>` — a directory that already contains `.moss` — to act on a site from outside it, the same as `moss build <folder>`.",
        },
        CliCommandInfo {
            name: "map",
            args: "bbox [<folder>] [--json]",
            description: "Print the frame the site's places need, for a `[map.layers]` table. `bbox` prints `west,south,east,north` in degrees, computed from the places in .moss/places.toml that pages name in the place-typed kind's fields, plus 10% of each side (at least 0.01 degree), and a suggested `max_zoom` line: the zoom at which the frame's longer side spans about 1024 px, plus 3, kept within 10..19. `--json` prints {\"bbox\": [w, s, e, n], \"max_zoom\": n, \"places\": k}. Reads only the folder and writes nothing. With no folder, it acts on the site containing the current directory. Exits 1 when the site has no place-typed kind or no page names a place with coordinates.",
        },
        CliCommandInfo {
            name: "comments",
            args: "list [<folder>] [--json] | hide [<folder>] <id>... [--source <name>] [--json] | unhide [<folder>] <id>... [--source <name>] [--json] | adopt-key [<folder>] <pubkey> [--json]",
            description: "Review and hide a site's comments. `list` prints every comment, newest first: id, source, the page it is on (address and title once a build has run, otherwise the page uid), author, time, the first 60 characters of its text, and whether it is hidden; it needs no build. `hide` takes one or more ids from `list` and removes those comments, and the replies under them, from the site at the next publish; `unhide` reverses it. Nothing is erased and nothing is sent anywhere: a hide is a signed event in `.moss/data/social/moderation.jsonl`, signed with the site's own key in `.moss/identity`, which moss never creates for this (no key, no hide). A call validates every id first, so an unknown id changes nothing; an id that exists under two sources needs `--source <name>`; an already-hidden comment is left alone. Nothing reviews new comments before they go live, so list them before publishing and hide the spam. `--json` prints rows, or `{\"changed\": [...], \"unchanged\": [...]}` for hide and unhide, and failures as `{\"error\": ...}`. A hide keeps applying when the folder gets a new key: the key it replaced is remembered in `.moss/identity/previous-keys.jsonl`. When that file was lost with the key, `list` names the earlier key and `adopt-key <pubkey>` trusts it again; it refuses a key that signed nothing here or signed for another site. With no folder, this command operates on the site containing the current directory.",
        },
    ]
}

/// The subcommands that parse `--help` themselves; [`help_for`] declines
/// them so their per-mode usage stays authoritative.
const OWN_HELP: [&str; 11] = ["list", "doctor", "migrate", "rename", "new", "expand", "domain", "site", "env", "import", "history"];

/// Render `moss <cmd> --help` from the table `moss describe` already publishes.
///
/// Six subcommands took their argument positionally and so read `--help` as
/// that argument: `build`/`deploy` reported `Folder does not exist: …/--help`,
/// `preview`/`edit` the same for a path, `describe` ignored the flag and dumped
/// the whole contract, and `agents` said `unknown agents subcommand: --help`.
/// Every one of those reads as "your argument is wrong" rather than "that flag
/// landed in an argument slot" — and `<cmd> --help` is the first thing anyone
/// types at a CLI they do not know, an agent most of all. Observed in the
/// 2026-08-05 trial.
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
    ("root", "moss root ~/blog/posts/essay.md --json  # Find the site that owns this path"),
    ("render", "printf '%s\\n' '{\"id\":1,\"source\":\"# Hello\"}' | moss render --json"),
    ("build", "moss build docs/public/              # Build with plugins"),
    ("build", "moss build ~/blog/ --serve           # Build with plugins and serve"),
    ("build", "moss build ~/blog/ --no-plugins      # Fast build for CI/CD"),
    ("build", "moss build ~/blog/ --wait-plugins    # Build and wait for plugins (e2e testing)"),
    ("build", "moss build ~/blog/ --strict          # Fail (exit 1) if the build reported problems"),
    ("list", "moss list ~/blog/                    # Inventory every page: kind, URL, title, lang"),
    ("list", "moss list ~/blog/ --json             # Same inventory, machine-readable"),
    ("deploy", "moss deploy ~/blog/ --dry-run        # See what a deploy would change, send nothing"),
    ("deploy", "moss deploy ~/blog/                  # Build and publish the site"),
    ("deploy", "moss deploy ~/site/ --prebuilt=_site # Upload _site/ as the site, skipping the build"),
    ("deploy", "moss deploy ~/blog/ --site-id=my-blog --email you@example.com  # First publish: sign the folder in"),
    ("preview", "moss preview ~/blog/                 # Open a preview window"),
    ("preview", "moss preview ~/blog/posts/article.md # Preview and navigate to the article"),
    ("edit", "moss edit ~/blog/posts/article.md    # Preview, then open the article in the editor"),
    ("edit", "moss edit ~/blog/ --serve            # Serve the editor and sign in this browser"),
    ("edit", "moss edit --serve --no-browser       # Print a sign-in link and QR"),
    ("sessions", "moss sessions revoke --all          # Sign out all browser sessions"),
    // The binary that can open a preview window is the binary that opens one
    // with no arguments at all.
    ("preview", "moss                                 # Launch the app window"),
    ("domain", "moss domain list ~/blog/             # Show domains linked to this project's site"),
    ("domain", "moss domain link ~/blog/ example.com # Link a domain; prints the DNS records to set"),
    ("domain", "moss domain status ~/blog/ example.com # Check live DNS and certificate; exit 0 when live"),
    ("site", "moss site transfer ~/blog/ --to new@example.com --dry-run # What handing the site over would move"),
    ("env", "moss env staging ~/blog/             # Set the mosspub.com environment to staging"),
    ("env", "moss env ~/blog/                     # Show the mosspub.com environment"),
    ("tools", "moss tools download ffmpeg         # Fetch FFmpeg so videos are optimized"),
    ("import", "moss import https://example.com/     # Mirror a site into the current directory"),
    ("import", "moss import https://blog.example.com/post/ ~/Sites/me/articles/"),
    ("import", "moss import --list urls.txt ~/Sites/me/articles/"),
    ("fetch", "moss fetch ~/blog/                  # Download every cloud-only file; exit 1 if the provider is not delivering"),
    ("doctor", "moss doctor --math ~/blog/            # Every $-span moss would parse as math"),
    ("doctor", "moss doctor --surface ~/blog/ --json  # Repeated classed divs, theme rules that style nothing, address-keyed selectors, theme-invented classes"),
    ("doctor", "moss doctor --surface ~/blog/ --all   # Every classed div left, even on one page"),
    ("migrate", "moss migrate ~/blog/ --lift eyebrow --as series --place eyebrow  # Diff of moving ::: {.eyebrow} into frontmatter"),
    ("migrate", "moss migrate ~/blog/ --lift eyebrow --as series --place eyebrow --apply  # Save a version, then write it"),
    ("map", "moss map bbox ~/blog/                # The frame the places need, for [map.layers] bbox"),
    ("comments", "moss comments list ~/blog/          # Every comment, newest first, hidden ones marked"),
    ("comments", "moss comments hide ~/blog/ 12 13    # Keep comments 12 and 13 off the site at the next publish"),
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
/// `moss build` prints where the guidance pointer is (see
/// `sync::guidance_pointer`), so this one line orients the agent; for Claude
/// Code or Cursor to find it unprompted in later sessions, the author runs
/// `moss agents sync` once, since a build never adds their folders to a site.
fn agent_footer() -> String {
    let cli = crate::cli::agents::sync::bundled_cli_path();
    format!(
        "
USING MOSS FROM A CODING AGENT:
    First time in a folder — paste this to your agent:

    Build a website from this folder: run {0} build .

    That points your agent at the authoring and theming guide. For Claude
    Code or Cursor to find it on their own in later sessions, run once:

    {0} agents sync

    After that, say what you want and nothing more:

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

/// What `moss compile` gets. Renamed to `moss build` on 2026-04-24,
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

    /// `--watch` is refused without `--serve` (the refresh it drives goes out
    /// through the preview server), so the usage must not offer it alone.
    #[test]
    fn build_help_nests_watch_under_serve() {
        let text = help_for("build").expect("build is not self-handled");
        assert!(text.contains("[--serve [--watch]]"), "usage offers --watch on its own: {text}");
        assert!(text.contains("--watch, which only works with --serve"), "help does not say --watch needs --serve: {text}");
    }

    /// `--serve` has no port flag; the help must say how to move the start.
    #[test]
    fn build_help_says_how_to_choose_the_serve_port() {
        let text = help_for("build").expect("build is not self-handled");
        assert!(text.contains("MOSS_PREVIEW_PORT_BASE"), "build help does not name the port variable: {text}");
    }

    /// The store lives inside the site, at `.moss/history` — the store's own
    /// `store_dir()`, which `moss history`'s plain-text timeline prints at the
    /// end of its own output ("Versions are kept at …"). The help text used to
    /// say the opposite ("kept outside the site folder"), which sent an agent
    /// looking for a version store that was never there.
    #[test]
    fn history_help_says_versions_live_inside_the_site() {
        let cmd = cli_commands().into_iter().find(|c| c.name == "history").expect("history is a command");
        assert!(
            cmd.description.contains(".moss/history"),
            "history's --help does not say where versions actually live: {}",
            cmd.description
        );
        assert!(
            !cmd.description.contains("outside the site folder"),
            "history's --help still claims versions are kept outside the site: {}",
            cmd.description
        );
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
