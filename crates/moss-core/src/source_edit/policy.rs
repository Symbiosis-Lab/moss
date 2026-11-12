//! Shared source visibility and filename roles.
use std::collections::HashSet;
use std::path::Path;

/// Return the wanted file name, or its first free `-2`, `-3`, … suffix.
/// The predicate receives each full path under `dir`; no I/O is performed here.
pub fn free_name(dir: &Path, name: &std::ffi::OsStr, taken: impl Fn(&Path) -> bool) -> std::ffi::OsString {
    if !taken(&dir.join(name)) {
        return name.to_os_string();
    }
    let path = Path::new(name);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = path.extension().map(|e| e.to_string_lossy());
    for i in 2u64.. {
        let candidate = match &ext {
            Some(e) => format!("{}-{}.{}", stem, i, e),
            None => format!("{}-{}", stem, i),
        };
        if !taken(&dir.join(&candidate)) {
            return candidate.into();
        }
    }
    unreachable!()
}

/// Agent-instruction filenames that conventionally live at a project root.
///
/// Every coding agent that reads a root-anchored instruction file uses one of
/// these exact names — `AGENTS.md` (the cross-tool convention, Codex and Gemini
/// CLI), `CLAUDE.md`, `GEMINI.md`. They are tooling, not prose.
///
/// Verified 2026-08-03 on a real build: a root `AGENTS.md` published to
/// `/agents/index.html` with an OG image and an `llms.txt` entry, because the
/// scanner excluded only dot-prefixed entries and the filename is the title.
/// Any moss user running a coding agent in their folder was publishing its
/// instructions as a page.
pub const ROOT_AGENT_CONFIG_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "GEMINI.md"];

/// Whether a *file name* is one of the agent-instruction conventions.
///
/// Exact case, deliberately. Every agent convention spells these in caps;
/// matching case-insensitively would silently unpublish an author's
/// `Agents.md` essay, which is a worse failure than publishing a stray config
/// file. The caller decides whether the file is at the root.
pub fn is_agent_config_name(name: &str) -> bool {
    ROOT_AGENT_CONFIG_FILES.contains(&name)
}

/// Folder names a site walk skips entirely, besides every dot-folder.
/// `assets`, `images` and `_`-prefixed folders are ordinary content: skipping
/// them once dropped media from the build.
pub const EXCLUDED_DIR_NAMES: &[&str] = &[
    "node_modules",
];

/// Whether a site walk skips a folder of this name. Folders only: a file named
/// like one (`_43A2045.jpg`, `node_modules`) is content.
pub fn is_excluded_dir_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    // Special case: "." and ".." are navigation, not hidden folders
    if lower == "." || lower == ".." {
        return false;
    }
    // Hidden directories: .git, .moss, .DS_Store-as-dir, etc.
    if lower.starts_with('.') {
        return true;
    }
    EXCLUDED_DIR_NAMES.iter().any(|&n| n.to_lowercase() == lower)
}

/// Why a walk of the site folder leaves an entry out of the site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftOut {
    /// A directory below the root whose name [`is_excluded_dir_name`] rejects.
    /// The root is never left out: its name is the author's choice, and a site
    /// living in `.mysite` would otherwise have its whole walk emptied.
    ExcludedDir,
    /// A directory below the root with its own `.moss/`: a different site,
    /// which the build goes around rather than absorbing its pages.
    NestedSite,
    /// An agent-instruction file directly in the site folder. Root only,
    /// because `posts/agents.md` is an ordinary article about agents; only the
    /// source root is a location tooling claims.
    AgentInstructions,
    /// A page whose name starts with a dot, at any depth: `posts/.draft.md`
    /// is the author's own marker that it is not for publishing, the same
    /// reading the file tree and the asset copy already give a dotfile. Only
    /// pages: a dot-prefixed placeholder such as `.keeper.md.icloud` stands
    /// for a page still in the cloud and must reach the scan.
    HiddenFile,
    /// A folder a `[map.layers.<name>]` table names as its `source`: the
    /// layer's raw plates, read by the build and never published.
    LayerSource,
}

/// The site walk's rule for one entry, `depth` levels below the site folder (a
/// file directly in it is at depth 1). `holds_site` says whether a folder has a
/// `.moss/` of its own; it is asked only for a folder every other rule kept.
pub fn left_out(name: &str, is_dir: bool, depth: usize, holds_site: impl FnOnce() -> bool) -> Option<LeftOut> {
    if !is_dir {
        if name.starts_with('.') && is_page_path(Path::new(name)) {
            return Some(LeftOut::HiddenFile);
        }
        return (depth == 1 && is_agent_config_name(name)).then_some(LeftOut::AgentInstructions);
    }
    if depth > 0 && is_excluded_dir_name(name) {
        return Some(LeftOut::ExcludedDir);
    }
    (depth > 0 && holds_site()).then_some(LeftOut::NestedSite)
}

/// Finder's custom-icon carrier: the literal name `Icon` followed by a carriage
/// return. moss writes one into every folder it publishes
/// (`platform::stamp_published_folder`), so unlike most OS metadata this is a
/// file *moss itself* leaves in the user's source tree.
pub const FINDER_ICON_MARKER: &str = "Icon\r";

/// Files the OS drops into a folder it has merely looked at: Finder's icon
/// marker (which moss itself writes on every published folder) and its
/// thumbnail cache, Windows's thumbnail cache. Not the author's, so neither
/// the build nor the editor's file tree may show them — the scan asks through
/// this predicate, the tree through [`is_hidden`], and a name added
/// here vanishes from both. Until 2026-09-11 the tree kept its own copy of
/// this list, which was missing the icon marker: after the first publish a
/// file called `Icon` sat at the top of every vault.
pub fn is_os_junk_file(name: &str) -> bool {
    name == ".DS_Store" || name == FINDER_ICON_MARKER || name.eq_ignore_ascii_case("thumbs.db")
}

/// Hidden entries that should not appear in the file tree.
pub const HIDDEN_ENTRIES: &[&str] = &[".git", "node_modules", "target"];

/// The `.moss/` paths the tree lists when `show_internal = true`, keyed the
/// way `is_hidden` sees them: project-root-relative, forward slashes. An
/// entry shows when it is one of these, sits below one, or is on the way to
/// one. Everything
/// else under `.moss/` (state.toml, build/, cache/, identity/, plugins/,
/// hashes.json, article-map.json, assets/) stays hidden.
///
/// Note: `assets/` is watcher-only (see `should_watch_moss_file` in
/// `build::watch::scope`, which reads `infra::moss_paths::MOSS_PATH_RULES`)
/// — it is build-managed media and not surfaced in the editor tree.
///
/// `data/` is listed for NOTHING. The 2026-09-04 writer audit asked of every
/// `.moss/` path who writes it and whether a hand edit survives, and only
/// `config.toml` and `theme/` answered "the user" then: `config.toml`, whose
/// every in-app writer goes through the format-preserving
/// `infra::toml_rewrite` precisely because the file is hand-written, and
/// `theme/`, which nothing but the user ever writes. `places.toml` (the
/// gazetteer, places build slice) joined them the same way — the app has no
/// writer for it yet, so every byte in it is the user's own.
/// `data/social/` was listed here until that audit and is not user-writable:
/// the background comment sync and the matters plugin re-serialize those files
/// wholesale, so an edit made in the editor survives only until the next sync,
/// and an autosave landing over a fresh sync is equally unguarded. The build
/// READING a directory is not the same question as the user writing it. Its
/// siblings were never listed for the same reason: `subscribers.csv` is a PII
/// list Settings edits with a real UI, `events*.json*` is the analytics stream,
/// and `email/drafts/` is auto-saved newsletter scratch.
pub const MOSS_INTERNAL_ALLOWLIST: &[&str] = &[".moss/config.toml", ".moss/theme", ".moss/places.toml"];

/// Why `is_hidden_reason` filtered an entry out of the file tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenReason {
    /// OS/VCS/tooling noise nobody authored: junk files, `HIDDEN_ENTRIES`,
    /// the root `.moss`/dotfile gate.
    OsOrVcsNoise,
    /// An entry inside `.moss/` not on `MOSS_INTERNAL_ALLOWLIST`.
    MossInternal,
}

impl HiddenReason {
    /// True for a reason that means "moss's curated view chose not to show
    /// real content", as opposed to OS/VCS noise. The one place a caller
    /// decides what counts toward a "something is filtered here" total —
    /// see `TreeNode::hidden`.
    pub fn is_curated(self) -> bool {
        matches!(self, HiddenReason::MossInternal)
    }
}

/// Return true when an entry should be filtered out of the file tree.
///
/// `parent_relative` is the path of the containing directory relative to the
/// project root ("" for root, ".moss" when filtering children of `.moss/`).
pub fn is_hidden(name: &str, parent_relative: &str, show_internal: bool) -> bool {
    is_hidden_reason(name, parent_relative, show_internal).is_some()
}

/// Same predicate as [`is_hidden`], naming *which* rule fired instead of a
/// bare bool — same branches, same order, no second list. Lets a caller (the
/// tree listing's hidden-count) tell "moss's own curated filter" apart from
/// OS/VCS noise, instead of merging every filtered entry into one number.
pub fn is_hidden_reason(name: &str, parent_relative: &str, show_internal: bool) -> Option<HiddenReason> {
    // Always hide these regardless of show_internal
    if HIDDEN_ENTRIES.contains(&name) || is_os_junk_file(name) {
        return Some(HiddenReason::OsOrVcsNoise);
    }

    // At the project root, .moss is surfaced only when show_internal is on
    if parent_relative.is_empty() {
        if name == ".moss" {
            return if show_internal { None } else { Some(HiddenReason::OsOrVcsNoise) };
        }
        // All other dotfiles are hidden at the project root
        if name.starts_with('.') {
            return Some(HiddenReason::OsOrVcsNoise);
        }
        // Root agent-instruction files (AGENTS.md, CLAUDE.md, GEMINI.md) are
        // listed: editing one is how an author takes it over from moss
        // (`cli::agents::sync::is_ours`), so the tree must reach it. They stay
        // unpublished (`left_out_of_site`); `special_file_kind` tells the editor so.
        return None;
    }

    // Inside .moss/ (only reached with show_internal on, since `.moss` itself
    // is hidden otherwise): listed, below a listed path, or an ancestor of one.
    if parent_relative == ".moss" || parent_relative.starts_with(".moss/") {
        let full = format!("{parent_relative}/{name}");
        let allowed = MOSS_INTERNAL_ALLOWLIST.iter().any(|allowed| {
            full == *allowed
                || full.starts_with(&format!("{allowed}/"))
                || allowed.starts_with(&format!("{full}/"))
        });
        return if allowed { None } else { Some(HiddenReason::MossInternal) };
    }

    None
}

/// A file's role on the site, beyond being a page or an asset. The editor
/// explains it before opening the file, so the tree says it rather than the
/// editor guessing from names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum SpecialFileKind {
    /// A root `AGENTS.md`/`CLAUDE.md`/`GEMINI.md`: read by coding agents, never published.
    AgentInstructions,
    /// The welcome email at a language-tree root: mail, not a page.
    WelcomeEmail,
    /// The reserved-name footer at a language-tree root, by name only: a
    /// `slot:` declaration in frontmatter does not make a file this kind.
    Footer,
    /// The site's `404.md`, shown by a host for an address that does not exist.
    NotFound,
    /// The `index.md` of a folder whose `index.html` is copied as-is: it holds
    /// that document's metadata and is not what visitors read.
    PageSettings,
}

/// The special role of the file at `rel_path` (project-relative, `/`
/// separators), if any. Every kind reuses the predicate the build decides it
/// with; `page_settings_dirs` is supplied from the host's configuration facts.
/// Nothing inside `.moss/` is special.
pub fn special_file_kind(rel_path: &str, page_settings_dirs: &HashSet<String>) -> Option<SpecialFileKind> {
    if rel_path == ".moss" || rel_path.starts_with(".moss/") {
        return None;
    }
    let (dir, name) = rel_path.rsplit_once('/').unwrap_or(("", rel_path));
    if dir.is_empty() && is_agent_config_name(name) {
        return Some(SpecialFileKind::AgentInstructions);
    }
    let (_, within_tree) = language_tree_split(rel_path);
    if slot_from_filename(within_tree) == Some("footer-left") {
        return Some(SpecialFileKind::Footer);
    }
    if within_tree == WELCOME_EMAIL_FILE {
        return Some(SpecialFileKind::WelcomeEmail);
    }
    if dir.is_empty() && is_page_path(Path::new(name)) {
        let stem = Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        if crate::frontmatter_typed::compute_url_path(name, false, None, stem)
            == "404/index.html"
        {
            return Some(SpecialFileKind::NotFound);
        }
    }
    let is_index_page = is_page_path(Path::new(name))
        && Path::new(name).file_stem().and_then(|s| s.to_str()).is_some_and(crate::home::is_index_stem);
    (is_index_page && page_settings_dirs.contains(dir)).then_some(SpecialFileKind::PageSettings)
}


/// Extensions the build turns into pages.
///
/// The ONE definition in the repo: the scan's markdown arm calls it, and
/// `describe_source_role` reports its verdict to the editor so the frontend
/// never restates this list. A copy of it in TypeScript is the Rust↔TS parity
/// trap the preview follower's slot check already avoids.
pub fn is_page_source(extension: &str) -> bool {
    crate::page_kind::PAGE_EXTENSIONS.iter().any(|known| known.eq_ignore_ascii_case(extension))
}

/// [`is_page_source`] for a path, whatever the letter case of its extension:
/// `Post.MD` is a page like `post.md`.
pub fn is_page_path(path: &Path) -> bool {
    path.extension().is_some_and(|e| is_page_source(&e.to_string_lossy()))
}

/// Split a source-relative path into its language-tree bucket and the path
/// within that tree. A leading recognized language folder (e.g. `zh-hans/`)
/// becomes the bucket (lowercased); everything else falls in the default
/// bucket (`None`).
///
/// - `"footer.md"` → `(None, "footer.md")`
/// - `"zh-hans/footer.md"` → `(Some("zh-hans"), "footer.md")`
/// - `"zh-hans/docs/footer.md"` → `(Some("zh-hans"), "docs/footer.md")`
pub fn language_tree_split(rel_path: &str) -> (Option<String>, &str) {
    match crate::home::lang_tree_prefix(rel_path) {
        // `lang_tree_prefix` returns the first path segment, so stripping it
        // plus its `/` leaves the rest of the path.
        Some(prefix) => {
            let rest = rel_path
                .strip_prefix(prefix)
                .and_then(|r| r.strip_prefix('/'))
                .unwrap_or(rel_path);
            (Some(prefix.to_lowercase()), rest)
        }
        None => (None, rel_path),
    }
}


/// Reserved-name slot at a language-tree root.
pub fn slot_from_filename(rel_path: &str) -> Option<&'static str> {
    (rel_path == "footer.md").then_some("footer-left")
}

/// Welcome mail source at a language-tree root.
pub const WELCOME_EMAIL_FILE: &str = "welcome-email.md";

/// Index pages that supply settings for an explicitly copied HTML file.
/// Only the project root and language-tree roots keep their markdown home
/// beside a verbatim index. Config entries apply in order, including negation;
/// directory passthrough alone does not declare an exact HTML file.
pub fn page_settings_dirs(config: &[String], existing_files: &[&str]) -> HashSet<String> {
    let mut exact = HashSet::new();
    for entry in config {
        let (negated, path) = match entry.strip_prefix('!') {
            Some(path) => (true, path),
            None => (false, entry.as_str()),
        };
        let path = path.trim_matches('/');
        if negated { exact.remove(path); }
        else if existing_files.contains(&path) { exact.insert(path.to_string()); }
    }
    exact.into_iter().filter_map(|path| {
        if path == "index.html" { return Some(String::new()); }
        path.strip_suffix("/index.html")
            .filter(|dir| crate::home::is_known_language_code(dir))
            .map(str::to_string)
    }).collect()
}

/// The site-relative passthrough subtree roots: every non-root folder holding
/// an `index.html` or `index.htm`, then the `[build].passthrough` entries in
/// order. An entry adds a folder root, or an exact listed HTML file; a `!`
/// entry removes one. Folder roots carry a trailing `/`, exact files do not.
pub fn compute_passthrough_roots(html_files: &[&str], config_entries: &[String]) -> HashSet<String> {
    let mut roots = HashSet::new();
    for file in html_files {
        let Some((parent, name)) = file.rsplit_once('/') else { continue; };
        if !parent.is_empty() && parent != "." && (name.eq_ignore_ascii_case("index.html") || name.eq_ignore_ascii_case("index.htm")) {
            roots.insert(format!("{}/", parent.trim_end_matches('/')));
        }
    }
    for entry in config_entries {
        let (negate, raw) = entry.strip_prefix('!').map_or((false, entry.as_str()), |rest| (true, rest));
        // Paths are root-relative: a leading `/` in config would never match.
        let normalized = raw.trim_matches('/');
        if normalized.is_empty() { continue; }
        let key = if html_files.contains(&normalized) { normalized.to_string() } else { format!("{normalized}/") };
        if negate { roots.remove(&key); } else { roots.insert(key); }
    }
    roots
}

/// Whether `path` is inside a passthrough root: under a folder root (the
/// trailing `/` keeps `app/` from matching `application/`), or the exact file.
pub fn is_in_passthrough(path: &str, roots: &HashSet<String>) -> bool {
    roots.iter().any(|root| if root.ends_with('/') { path.starts_with(root) } else { path == root })
}
