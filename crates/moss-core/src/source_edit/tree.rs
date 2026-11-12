//! Build the editor tree from host-supplied directory facts.

use super::policy::{is_hidden_reason, special_file_kind, SpecialFileKind};
pub use crate::date::DateSource;

/// Maximum source prefix used for tree frontmatter, in bytes.
pub const FRONTMATTER_PREFIX_BYTES: usize = 4096;

/// Parse tree frontmatter within the byte window, replacing incomplete UTF-8.
/// A block whose closing delimiter falls outside the window yields no fields.
pub fn frontmatter_from_prefix(bytes: &[u8]) -> std::collections::HashMap<String, serde_yaml::Value> {
    let prefix = &bytes[..bytes.len().min(FRONTMATTER_PREFIX_BYTES)];
    crate::frontmatter::frontmatter_map(&String::from_utf8_lossy(prefix))
}

/// Metadata about a file for the editor's file viewer.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct EditorFileInfo {
    /// File size in bytes.
    pub size: u64,
    /// Last modified time as ISO 8601 string, if available.
    pub modified: Option<String>,
    /// Image width in pixels (images only).
    pub width: Option<u32>,
    /// Image height in pixels (images only).
    pub height: Option<u32>,
}

/// A directory entry returned by `list_directory`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DirEntry {
    /// File or folder name (not the full path).
    pub name: String,
    /// Absolute path to the entry.
    pub path: String,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// Last modified time as seconds since UNIX epoch. None if unavailable.
    pub modified: Option<f64>,
    /// Opaque cross-platform identity for this filesystem entry, serialized as
    /// decimal u64 (`Some("42")`) or `None` when not extractable. Used by
    /// EntryRegistry.reconcileSnapshot to reuse EntryIds when a rename moves
    /// an existing file_id to a new path.
    ///
    /// Unix: `Some(metadata.ino().to_string())` via MetadataExt.
    /// Windows (v1): always `None` — stable Rust doesn't expose file_index
    /// without nightly. Future PR can wire the `file-id` crate (already
    /// transitive via notify-debouncer-full) for proper Windows support.
    pub file_id: Option<String>,
}

/// A recursive tree node for the full project file tree.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    /// Last modified time as seconds since UNIX epoch. None if unavailable.
    pub modified: Option<f64>,
    /// Resolved publish date as `YYYY-MM-DD`, or `None`.
    ///
    /// For markdown files: from frontmatter `date` field, filename `YYYY-MM-DD-…`
    /// prefix, or ctime fallback. For folders: max recency over descendants per
    /// the spec (see `compute_folder_recency`). Always `None` for non-md files.
    pub publish_date: Option<String>,
    /// Provenance of `publish_date`. Drives sort-zone classification:
    /// `Frontmatter` and `FilenamePrefix` are explicit (dated-md zone);
    /// `Ctime` is implicit (undated-md zone). For folders, always `None`.
    pub date_source: DateSource,
    /// Whether this node is its parent folder's ELECTED home file — the single
    /// winner of `crate::home::detect_home_file_in_folder_marked` (order:
    /// `home: true` frontmatter marker › index stems › self-named folder note ›
    /// first document alphabetically), computed per folder in
    /// `build_tree_root` with the folder's REAL basename (so the project
    /// root's self-named home is recognized too). At most one child per folder
    /// is `true`; always `false` for directories. The frontend consumes this
    /// flag (`entries.find(e => e.is_home)`) instead of re-deriving the
    /// election — the core owns the home decision.
    pub is_home: bool,
    /// Children of this directory. `None` for files.
    pub children: Option<Vec<TreeNode>>,
    pub hidden: u32,
    /// See `DirEntry::file_id`.
    pub file_id: Option<String>,
    /// The file's role on the site when it has a special one, decided by
    /// `super::policy::special_file_kind`. Always `None` for folders.
    #[serde(default)]
    pub special: Option<SpecialFileKind>,
}

// ── Sort helpers ──────────────────────────────────────────────────────────

/// Sort comparator: directories first, then by mtime descending (newest
/// first), with case-insensitive alphabetical as tiebreaker.
pub fn cmp_dir_entry(a: &DirEntry, b: &DirEntry) -> std::cmp::Ordering {
    cmp_entry_fields(a.is_dir, a.modified, &a.name, b.is_dir, b.modified, &b.name)
}

/// Shared sort logic: directories first, then mtime descending, then
/// alphabetical.
pub fn cmp_entry_fields(
    a_is_dir: bool,
    a_modified: Option<f64>,
    a_name: &str,
    b_is_dir: bool,
    b_modified: Option<f64>,
    b_name: &str,
) -> std::cmp::Ordering {
    match (a_is_dir, b_is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => {
            let time_ord = match (b_modified, a_modified) {
                (Some(bm), Some(am)) => bm.partial_cmp(&am).unwrap_or(std::cmp::Ordering::Equal),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            };
            time_ord.then_with(|| a_name.to_lowercase().cmp(&b_name.to_lowercase()))
        }
    }
}

// ── Five-zone tree sort (publish-date aware) ──────────────────────────────

/// Sort children of a folder per the spec's five-zone ordering.
///
/// Zone order: elected-home → home-md → dated-md → undated-md → folders → non-md.
/// Within each zone: recency desc → mtime desc → alpha case-insensitive.
///
/// `parent_folder_name` is the containing folder's `file_name`, used by
/// `crate::home::is_home_file` for the self-named index convention.
///
/// ## Sort/flag agreement
///
/// The ELECTED home (`TreeNode::is_home`, set by `list_tree_inner_cached`
/// before sorting) is pinned FIRST, ahead of `Zone::HomeMd`. `zone_of`
/// classifies by filename shape alone, so it can put SEVERAL files in the
/// home zone (index.md + readme.md) — and would let a newer readme.md
/// out-sort the elected index.md — or MISS the election winner entirely
/// (a `home: true` marker file or the alphabetical-fallback home has an
/// ordinary filename). Pinning the single winner keeps the flag and the
/// frontend's home-pair row (folder + home collapsed onto one row, built
/// from the flagged entry) in agreement.
pub fn sort_tree_nodes(nodes: &mut [TreeNode], parent_folder_name: &str) {
    nodes.sort_by(|a, b| {
        match b.is_home.cmp(&a.is_home) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        let za = zone_of(a, parent_folder_name);
        let zb = zone_of(b, parent_folder_name);
        if za != zb {
            return za.cmp(&zb);
        }
        // Recency: empty string sorts last, otherwise lexicographic-as-date.
        let ra = recency_key(a);
        let rb = recency_key(b);
        match (ra.is_empty(), rb.is_empty()) {
            (false, false) => match rb.cmp(&ra) {
                std::cmp::Ordering::Equal => {}
                ord => return ord,
            },
            (false, true) => return std::cmp::Ordering::Less,
            (true, false) => return std::cmp::Ordering::Greater,
            (true, true) => {}
        }
        // mtime tiebreak — desc.
        match b.modified.partial_cmp(&a.modified).unwrap_or(std::cmp::Ordering::Equal) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        a.name.to_lowercase().cmp(&b.name.to_lowercase())
    });
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Zone {
    HomeMd = 0,
    DatedMd = 1,
    UndatedMd = 2,
    Folder = 3,
    NonMd = 4,
}

fn zone_of(node: &TreeNode, parent_folder_name: &str) -> Zone {
    if node.is_dir {
        return Zone::Folder;
    }
    if !super::policy::is_page_path(std::path::Path::new(&node.name)) {
        return Zone::NonMd;
    }
    let stem = node
        .name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(&node.name);
    if crate::home::is_home_file(stem, parent_folder_name) {
        return Zone::HomeMd;
    }
    match node.date_source {
        DateSource::Frontmatter | DateSource::FilenamePrefix => Zone::DatedMd,
        DateSource::Ctime | DateSource::None => Zone::UndatedMd,
    }
}

fn recency_key(node: &TreeNode) -> String {
    node.publish_date.clone().unwrap_or_default()
}

// ── Folder recency ────────────────────────────────────────────────────────

/// Compute a folder's recency string per spec Zone 4 rules.
///
/// Precedence:
///   1. Max explicit publish_date over md descendants (Frontmatter/FilenamePrefix).
///   2. Else max mtime over md descendants (as `YYYY-MM-DD`).
///   3. Else max mtime over non-md descendants.
///   4. Else folder's own mtime.
///
/// Returning `None` only if the folder has no descendants and no own mtime.
pub fn compute_folder_recency(folder: &TreeNode) -> Option<String> {
    let children = folder.children.as_deref().unwrap_or(&[]);

    let md_descendants = collect_descendants(children, &is_md_file);

    let max_explicit = md_descendants
        .iter()
        .filter(|n| matches!(n.date_source, DateSource::Frontmatter | DateSource::FilenamePrefix))
        .filter_map(|n| n.publish_date.clone())
        .max();
    if max_explicit.is_some() {
        return max_explicit;
    }

    let max_md_mtime = md_descendants
        .iter()
        .filter_map(|n| n.modified)
        .fold(None::<f64>, |acc, m| Some(acc.map_or(m, |a| a.max(m))));
    if let Some(m) = max_md_mtime {
        return mtime_to_yyyy_mm_dd(m);
    }

    let max_any_mtime = collect_descendants(children, &|_| true)
        .iter()
        .filter_map(|n| n.modified)
        .fold(None::<f64>, |acc, m| Some(acc.map_or(m, |a| a.max(m))));
    if let Some(m) = max_any_mtime {
        return mtime_to_yyyy_mm_dd(m);
    }

    folder.modified.and_then(mtime_to_yyyy_mm_dd)
}

/// Recursively collect every leaf descendant matching `pred`.
///
/// Walks into directory children but only `pred(node)` decides whether
/// a non-directory node is included. Returns a flat `Vec` of references
/// into the original tree.
fn collect_descendants<'a>(
    nodes: &'a [TreeNode],
    pred: &impl Fn(&TreeNode) -> bool,
) -> Vec<&'a TreeNode> {
    let mut out = Vec::new();
    for n in nodes {
        if !n.is_dir {
            if pred(n) {
                out.push(n);
            }
        } else if let Some(kids) = n.children.as_deref() {
            out.extend(collect_descendants(kids, pred));
        }
    }
    out
}

fn is_md_file(node: &TreeNode) -> bool {
    !node.is_dir && super::policy::is_page_path(std::path::Path::new(&node.name))
}


/// UTC date of an epoch-seconds mtime. Fractional seconds do not affect the date.
pub fn mtime_to_yyyy_mm_dd(secs: f64) -> Option<String> {
    let (year, month, day) = crate::date::civil_from_days((secs as u64 / 86_400) as i64);
    match year {
        ..=9_999 => Some(format!("{year:04}-{month:02}-{day:02}")),
        // ISO 8601's expanded years, up to the last year the date model holds.
        ..=262_142 => Some(format!("+{year}-{month:02}-{day:02}")),
        _ => None,
    }
}

/// A flat directory fact. Paths use `/` separators relative to the project.
/// Hosts include directory entries, including empty directories. Markdown
/// frontmatter is text from the start of the file; only the first
/// `FRONTMATTER_PREFIX_BYTES` bytes participate in tree policy.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ListingEntry {
    pub path: String,
    pub is_dir: bool,
    #[serde(default)]
    pub size: u64,
    pub modified: Option<f64>,
    #[serde(default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub frontmatter: Option<String>,
    /// An optional memo of `resolve_md_date`, for hosts caching parsed dates.
    #[serde(default)]
    pub resolved_date: Option<(Option<String>, DateSource, bool)>,
    /// Curated entries omitted by a host that filters before reading metadata.
    /// Supply zero for an unfiltered listing; core counts its hidden entries.
    #[serde(default)]
    pub hidden: u32,
}

/// Identity and presentation facts that cannot be inferred from a flat listing.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TreeOptions {
    /// The actual project folder basename, resolved by the host.
    pub root_name: String,
    /// Optional host path prefix for returned paths; empty yields relative paths.
    #[serde(default)]
    pub path_prefix: String,
    /// Directory to return, relative to the project; empty means project root.
    #[serde(default)]
    pub directory: String,
    #[serde(default)]
    pub show_internal: bool,
    /// Directories whose index page supplies a verbatim HTML page's settings.
    #[serde(default)]
    pub page_settings_dirs: std::collections::HashSet<String>,
    /// Parsed `[build].passthrough` entries. Core derives settings roles from
    /// these and the listing; the optional set above supports host memoization.
    #[serde(default)]
    pub passthrough: Vec<String>,
}

/// Resolve dates and home markers from one host-supplied frontmatter map.
pub fn resolve_md_date(
    name: &str,
    modified: Option<f64>,
    frontmatter: &std::collections::HashMap<String, serde_yaml::Value>,
) -> (Option<String>, DateSource, bool) {
    let home = frontmatter.get("home").and_then(|v| v.as_bool()).unwrap_or(false);
    let fallback = modified.and_then(mtime_to_yyyy_mm_dd);
    let (date, source) = crate::date::resolve_publish_date(frontmatter, &[name], fallback.as_deref());
    (date, source, home)
}

/// The visible, sorted forest directly under `options.directory`.
pub fn build_tree(entries: &[ListingEntry], options: &TreeOptions) -> Vec<TreeNode> {
    build_tree_root(entries, options).children.unwrap_or_default()
}

/// The engine's `list_tree` shape, including the containing folder and counts.
/// Entries can arrive in any order. Missing parents do not create folders.
pub fn build_tree_root(entries: &[ListingEntry], options: &TreeOptions) -> TreeNode {
    let mut options = options.clone();
    let files: Vec<&str> = entries.iter().filter(|e| !e.is_dir).map(|e| e.path.as_str()).collect();
    options.page_settings_dirs.extend(super::policy::page_settings_dirs(&options.passthrough, &files));
    let mut by_parent: std::collections::HashMap<&str, Vec<&ListingEntry>> = Default::default();
    for entry in entries {
        if !entry.path.is_empty() {
            let (parent, _) = entry.path.rsplit_once('/').unwrap_or(("", &entry.path));
            by_parent.entry(parent).or_default().push(entry);
        }
    }
    let root_entry = entries.iter().find(|entry| entry.path == options.directory && entry.is_dir);
    folder_node(&options.directory, root_entry, &by_parent, &options)
}

fn output_path(relative: &str, options: &TreeOptions) -> String {
    if options.path_prefix.is_empty() { return relative.to_string(); }
    if relative.is_empty() { return options.path_prefix.clone(); }
    std::path::Path::new(&options.path_prefix).join(relative).to_string_lossy().to_string()
}

fn folder_node(
    relative: &str,
    entry: Option<&ListingEntry>,
    by_parent: &std::collections::HashMap<&str, Vec<&ListingEntry>>,
    options: &TreeOptions,
) -> TreeNode {
    let name = if relative.is_empty() { options.root_name.as_str() }
        else { relative.rsplit('/').next().unwrap_or(relative) };
    let mut hidden = entry.map_or(0, |e| e.hidden);
    let mut children = Vec::new();
    let mut marked = Vec::new();
    let mut entries = by_parent.get(relative).cloned().unwrap_or_default();
    entries.sort_by(|a, b| cmp_entry_fields(a.is_dir, a.modified, &a.path, b.is_dir, b.modified, &b.path));
    for child in entries {
        let child_name = child.path.rsplit('/').next().unwrap_or(&child.path);
        if let Some(reason) = is_hidden_reason(child_name, relative, options.show_internal) {
            hidden += u32::from(reason.is_curated());
            continue;
        }
        if child.is_dir {
            children.push(folder_node(&child.path, Some(child), by_parent, options));
        } else {
            let (publish_date, date_source, home) = if super::policy::is_page_path(std::path::Path::new(child_name)) {
                child.resolved_date.clone().unwrap_or_else(|| resolve_md_date(
                    child_name, child.modified,
                    &frontmatter_from_prefix(child.frontmatter.as_deref().unwrap_or("").as_bytes()),
                ))
            } else { (None, DateSource::None, false) };
            if home { marked.push(child_name); }
            children.push(TreeNode {
                name: child_name.to_string(), path: output_path(&child.path, options), is_dir: false,
                modified: child.modified, publish_date, date_source, is_home: false,
                children: None, hidden: 0, file_id: child.file_id.clone(),
                special: special_file_kind(&child.path, &options.page_settings_dirs),
            });
        }
    }
    elect_home(&mut children, name, &marked, relative.is_empty());
    let modified = children.iter().filter_map(|c| c.modified)
        .fold(None::<f64>, |acc, m| Some(acc.map_or(m, |a| a.max(m))));
    sort_tree_nodes(&mut children, name);
    let mut node = TreeNode {
        name: name.to_string(), path: output_path(relative, options), is_dir: true,
        modified, publish_date: None, date_source: DateSource::None, is_home: false,
        children: Some(children), hidden, file_id: entry.and_then(|e| e.file_id.clone()), special: None,
    };
    node.publish_date = compute_folder_recency(&node);
    node
}

fn elect_home(children: &mut [TreeNode], name: &str, marked: &[&str], is_root: bool) {
    let names: Vec<&str> = children.iter().filter(|c| !c.is_dir).map(|c| c.name.as_str()).collect();
    let elected = crate::home::detect_home_file_in_folder_marked(&names, name, marked)
        .filter(|candidate| {
            let stem = std::path::Path::new(candidate).file_stem().and_then(|s| s.to_str()).unwrap_or(candidate);
            !is_root || marked.iter().any(|m| m.eq_ignore_ascii_case(candidate)) || crate::home::is_home_file(stem, name)
        }).map(str::to_string);
    if let Some(elected) = elected {
        for child in children { child.is_home = !child.is_dir && child.name == elected; }
    }
}
