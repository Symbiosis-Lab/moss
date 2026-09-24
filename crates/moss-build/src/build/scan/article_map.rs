//! Article URL mapping for syndication support.
//!
//! Provides a mapping from generated HTML URL paths to source file information,
//! enabling the preview system to determine if the current page is an article
//! that can be syndicated.
//!
//! ## Usage
//!
//! After site generation, call `build_article_map` to create a mapping from
//! URL paths (e.g., "posts/my-article.html") to article metadata.
//!
//! The map is persisted to `.moss/build.nosync/article-map.json` and can be queried by:
//! - Preview system to determine if current page is syndicatable
//! - Syndication system to get article info for current preview URL

use crate::build::terms::{TermIndex, TermKind, TermSite};
use crate::moss_paths::MossPaths;
use crate::build::scan::slug::UrlCollision;
use crate::build::types::ParsedDocument;
use moss_core::PageKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use std::collections::HashMap;
use std::path::Path;

/// Convert file path to pretty URL format.
///
/// Must match frontend `extractDisplayPath()` behavior exactly:
/// - `foo/bar/index.html` → `foo/bar/` (WITH trailing slash for directory-style)
/// - `foo/bar.html` → `foo/bar` (NO trailing slash for file-style)
/// - Leading slashes are stripped
///
/// # Examples
/// ```ignore
/// assert_eq!(to_pretty_url("news/article/index.html"), "news/article/");
/// assert_eq!(to_pretty_url("posts/hello.html"), "posts/hello");
/// ```
pub fn to_pretty_url(file_path: &str) -> String {
    let path = file_path.trim_start_matches('/');
    if path == "index.html" {
        // Root homepage: "index.html" -> "" (becomes "/" when nav prepends slash)
        String::new()
    } else if path.ends_with("/index.html") {
        // "foo/bar/index.html" -> "foo/bar/"
        path.strip_suffix("index.html").unwrap().to_string()
    } else if path.ends_with(".html") {
        // "foo/bar.html" -> "foo/bar" (NO trailing slash!)
        path.strip_suffix(".html").unwrap().to_string()
    } else {
        path.to_string()
    }
}

// Lived in `plugins/types.rs` until 2026-08-17 because its *readers* are the
// syndication plugins. But the build is what fills it and what persists it —
// `ArticleMap` right below is nothing but a map of these — so the plugin module
// was owning a type the build produced, which points the dependency edge the
// wrong way. Plugins import it from here now.
//
// The rationale is a plain comment, not a doc comment: this type carries
// `Type`, so every `///` line here is copied verbatim into `bindings.ts` and
// read by people who do not care where the struct used to live. Specta keys on
// the type name rather than the module path, so the emitted shape is unchanged
// by the move.
/// Article information for syndication plugins
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ArticleInfo {
    /// Relative path to source file
    pub source_path: String,

    /// Title extracted from frontmatter or content
    pub title: String,

    /// Article content (markdown)
    pub content: String,

    /// Rendered HTML content (article body, no page template)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html_content: Option<String>,

    /// Frontmatter metadata
    #[serde(default)]
    pub frontmatter: HashMap<String, serde_json::Value>,

    /// URL path in generated site
    pub url_path: String,

    /// Publication date (from frontmatter or file metadata)
    pub date: Option<String>,

    /// Tags/categories
    #[serde(default)]
    pub tags: Vec<String>,

    /// Stable note identity: 8 random hex chars minted once and stored in frontmatter — NOT derived from the path, and unrecoverable once lost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
}

/// Mapping from URL path to article source information
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArticleMap {
    /// Map from url_path (e.g., "posts/article.html") to ArticleInfo
    pub articles: HashMap<String, ArticleInfo>,

    /// Map from url_path to source_path for non-article pages (homepage, section indexes).
    /// These pages aren't articles but still need url→source resolution for editor sync.
    /// Populated by build_article_map alongside the articles map.
    #[serde(default)]
    pub pages: HashMap<String, String>,
    /// Title of every `pages` entry, same key. Recorded at scan time, where the
    /// frontmatter is already parsed, so no editor surface has to open the
    /// index file again to name a folder (the folder-embed card and the
    /// link-target completion both read it).
    #[serde(default)]
    pub page_titles: HashMap<String, String>,

    /// File-tree → page-tree directory rename map (e.g., `{"图片" → "image"}`).
    ///
    /// Captured here so the post-build native-slots phase — which loads this map
    /// from disk in a separate `spawn_blocking` task, well after the main render
    /// pipeline that originally derived `dir_overrides` has dropped its state —
    /// can reconstruct a faithful `MediaDimensionLookup` for asset references
    /// stored in `ArticleInfo.frontmatter.cover` (the cover string itself is
    /// pre-resolved through these same overrides — see line 290 of this file —
    /// so consumers comparing the cover against the lookup must use a matching
    /// override map). Used by the review colophon's synthesizer-routed cover
    /// image for `<source srcset>` gating.
    ///
    /// Empty on most sites; CJK-named directories are the primary case.
    #[serde(default)]
    pub dir_overrides: HashMap<String, String>,

    /// Files whose `url:` lost a contest with another file's, keyed by source
    /// path. Recorded here for the same reason as `dir_overrides`: the fact is
    /// derived during scan, and its consumer — `validate_content`, which puts
    /// it on the `url` chip in the editor — runs later with none of that state.
    ///
    /// Root causes only, never the pages a duplicated folder dragged along
    /// with it. Empty on a site with no duplicated `url:`, which is nearly all
    /// of them.
    #[serde(default)]
    pub url_collisions: HashMap<String, UrlCollision>,

    /// URL keys (`posts/`, `authors/`, `authors/林小滿/`) of every index page the
    /// build SYNTHESIZED: index-less folders, unclaimed term pages, the term
    /// namespace roots. Collected where those pages are emitted (the auto-index
    /// loop in `render/blocking.rs`), the only place the set is true — the
    /// namespace roots are not documents at all. Kept apart from `pages`
    /// because every `pages` consumer joins its value to a source file, and a
    /// synthesized page has none. The editor's URL index reads this so a link
    /// to a generated page classifies the way the build deploys it.
    #[serde(default)]
    pub generated: Vec<String>,

    /// Every term the build derived, keyed by pseudo-folder key
    /// (`authors/林小滿`), exactly as `derive_terms` built it. The claim is
    /// recorded here so the editor never re-derives it: a claimed term's
    /// generated URL is `Moved` to the claiming page, and the chip gesture
    /// resolves against this record.
    #[serde(default)]
    pub terms: std::collections::BTreeMap<String, TermSite>,

    /// The kinds table this build derived its terms from, in declaration
    /// order. Persisted so the editor's takeover resolver answers "what
    /// namespace is this, and what is it called" from the build's own
    /// answer instead of re-reading `.moss/config.toml` and risking a
    /// different one. Empty in a map written before term kinds existed.
    #[serde(default)]
    pub kinds: Vec<TermKind>,

    /// Term key (`people/ada-lin`) → the subset of its kind's `fields`, in
    /// `kind.fields` order, that claim at least one member of THAT term. A
    /// derived summary of `derive_terms`'s per-field membership, not the
    /// member URLs themselves: the editor needs to know which field to
    /// write when offering to claim a term, and that is the only question
    /// it asks. A term with no members has no entry at all.
    #[serde(default)]
    pub fields_with_members: std::collections::BTreeMap<String, Vec<String>>,

    /// Every directory that gets an index page — real (an authored
    /// `index.md`) or synthesized — keyed by its SOURCE directory, not its
    /// URL slug (see `folder_index_keys`, the single place this mapping is
    /// written). A directory with a real index is included too: the value
    /// still names the same URL the build serves that page at, which is
    /// what lets `ArticleMapIndex::resolve_reference_to_url` fall through to
    /// it for a folder whose index file isn't stem-matchable (e.g. a bare
    /// `news/index.md`, whose filename stem is `index`, not `news`).
    /// `#[serde(default)]` so an older persisted map still loads.
    ///
    /// Kept apart from `generated`: that set mixes an index-less content
    /// folder's auto-generated page with term/namespace-root pages, and
    /// neither its URL-keyed shape nor `ArticleMapIndex::by_stem`'s
    /// ambiguity-collapsing construction can tell them apart safely. This
    /// field exists so `ArticleMapIndex::resolve_reference_to_url` can
    /// resolve a bare folder wikilink to a folder's index page without ever
    /// reading `generated`/`terms`/`kinds` for that purpose.
    #[serde(default)]
    pub folder_indexes: std::collections::BTreeMap<String, String>,
}

impl ArticleMap {
    /// Create a new empty article map
    pub fn new() -> Self {
        Self {
            articles: HashMap::new(),
            pages: HashMap::new(),
            page_titles: HashMap::new(),
            dir_overrides: HashMap::new(),
            url_collisions: HashMap::new(),
            generated: Vec::new(),
            terms: std::collections::BTreeMap::new(),
            kinds: Vec::new(),
            fields_with_members: std::collections::BTreeMap::new(),
            folder_indexes: std::collections::BTreeMap::new(),
        }
    }

    /// Check if a URL path corresponds to an article
    #[cfg(test)]
    pub fn is_article(&self, url_path: &str) -> bool {
        // Normalize path: strip leading slash if present
        let normalized = url_path.trim_start_matches('/');
        self.articles.contains_key(normalized)
    }

    /// Save article map to .moss/build.nosync/article-map.json
    pub fn save(&self, moss_dir: &Path) -> Result<(), String> {
        let paths = MossPaths::from_moss_dir(moss_dir.to_path_buf());
        let map_path = paths.article_map();
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize article map: {}", e))?;
        // Atomic write: the map is rewritten on every build, and concurrent
        // readers (editor `resolve_page_source`, syndication) must never catch a
        // half-truncated file. A plain `fs::write` truncates-then-writes, leaving a
        // window where a reader gets partial/empty JSON → a parse error. Write to a
        // sibling temp on the SAME filesystem, then `rename(2)` into place — POSIX
        // rename is atomic for readers (old file or complete new file, never a
        // partial). Durability (fsync) is intentionally skipped: the map is a
        // derived cache regenerated by the next build, so reader-atomicity is the
        // only property we need here.
        let tmp_path = map_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, json)  // allow:raw_write the temp for this file's own atomic save; the rename below places it
            .map_err(|e| format!("Failed to write article map: {}", e))?;
        // allow:unlink rename into place for the article map, not staging
        std::fs::rename(&tmp_path, &map_path)
            .map_err(|e| format!("Failed to commit article map: {}", e))?;
        Ok(())
    }

    /// Every page with a source file, as `(pretty URL key, source path,
    /// title)`. The URL key is the map's own form (`awards/a`, `awards/`, ``
    /// for home); [`Self::canonical_url`] turns it into the deployed path.
    /// The one enumeration the editor's URL index and the link-target
    /// completion both read, so the two can never disagree on what a
    /// deployed page is.
    pub fn sourced_pages(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.articles
            .iter()
            .map(|(u, a)| (u.as_str(), a.source_path.as_str(), a.title.as_str()))
            .chain(self.pages.iter().map(|(u, s)| {
                let title = self.page_titles.get(u).map(String::as_str).unwrap_or("");
                (u.as_str(), s.as_str(), title)
            }))
    }

    /// The deployed path for a map key: `/awards/a/`, `/` for home.
    pub fn canonical_url(key: &str) -> String {
        let key = key.trim_matches('/');
        if key.is_empty() { "/".to_string() } else { format!("/{key}/") }
    }

    /// Load article map from .moss/build.nosync/article-map.json
    pub fn load(moss_dir: &Path) -> Result<Self, String> {
        let paths = MossPaths::from_moss_dir(moss_dir.to_path_buf());
        let map_path = paths.article_map();
        if !map_path.exists() {
            return Ok(Self::new());
        }
        let json = std::fs::read_to_string(&map_path)
            .map_err(|e| format!("Failed to read article map: {}", e))?;
        serde_json::from_str(&json)
            .map_err(|e| format!("Failed to parse article map: {}", e))
    }
}

/// Parse YAML frontmatter from raw markdown content into a generic HashMap.
///
/// Extracts the `---` delimited YAML block at the start of the content and
/// parses it with `serde_yaml` into `HashMap<String, serde_json::Value>`.
/// This preserves all frontmatter fields including plugin-specific ones
/// (e.g., `syndicated`) that aren't part of the typed `FrontMatter` struct.
///
/// Returns an empty map if:
/// - The content has no frontmatter delimiters
/// - The YAML is malformed
/// - The frontmatter doesn't parse as a mapping
///
/// Returns a `BTreeMap`, not a `HashMap`: this map ends up (unmodified,
/// via `ParsedDocument::raw_frontmatter`) inside the `Debug` string
/// `PageFacade` hashes. `HashMap`'s per-process-random hasher
/// makes its `Debug` iteration order — and thus the facade hash — differ
/// between build invocations for byte-identical frontmatter; verified
/// empirically against a real vault (208/216 pages "changed" with zero
/// edits). `BTreeMap` iterates in sorted-key order regardless of process,
/// so its `Debug` output is deterministic.
///
/// # Examples
/// ```ignore
/// let fm = parse_frontmatter("---\ntitle: Hello\ntags:\n  - rust\n---\n\n# Content");
/// assert_eq!(fm.get("title"), Some(&Value::String("Hello".into())));
/// ```
pub fn parse_frontmatter(content: &str) -> std::collections::BTreeMap<String, Value> {
    // Normalize CRLF → LF so the offsets index what we slice below.
    let owned;
    let content = if content.contains("\r\n") {
        owned = content.replace("\r\n", "\n");
        owned.as_str()
    } else {
        content
    };

    // Where the block ends is `frontmatter_span`'s call, for both dialects.
    let Some(span) = moss_core::frontmatter::frontmatter_span(content) else {
        return std::collections::BTreeMap::new();
    };
    if span.kind == moss_core::frontmatter::FrontmatterKind::Simplified {
        // Not YAML. The simplified dialect's untyped view is `frontmatter_map`.
        return moss_core::frontmatter::frontmatter_map(content)
            .into_iter()
            .map(|(k, v)| (k, yaml_to_json(v)))
            .collect();
    }
    // Char-aligned: `fields` is a line-boundary range from the splitter.
    #[allow(clippy::string_slice)]
    let yaml_str = &content[span.fields];

    if yaml_str.trim().is_empty() {
        return std::collections::BTreeMap::new();
    }

    // Parse YAML into serde_yaml::Value first, then convert to serde_json::Value
    // This avoids serde_yaml's quirks with direct HashMap deserialization
    let yaml_value: serde_yaml::Value = match serde_yaml::from_str(yaml_str) {
        Ok(v) => v,
        Err(_) => return std::collections::BTreeMap::new(),
    };

    // Convert serde_yaml::Value -> serde_json::Value via the mapping
    match yaml_value {
        serde_yaml::Value::Mapping(mapping) => {
            let mut result = std::collections::BTreeMap::new();
            for (k, v) in mapping {
                if let serde_yaml::Value::String(key) = k {
                    result.insert(key, yaml_to_json(v));
                }
            }
            result
        }
        _ => std::collections::BTreeMap::new(),
    }
}

/// Convert a serde_yaml::Value to serde_json::Value.
///
/// Handles the type mapping between YAML and JSON value representations.
fn yaml_to_json(v: serde_yaml::Value) -> Value {
    match v {
        serde_yaml::Value::Null => Value::Null,
        serde_yaml::Value::Bool(b) => Value::Bool(b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        serde_yaml::Value::String(s) => Value::String(s),
        serde_yaml::Value::Sequence(seq) => {
            Value::Array(seq.into_iter().map(yaml_to_json).collect())
        }
        serde_yaml::Value::Mapping(mapping) => {
            let map: serde_json::Map<String, Value> = mapping
                .into_iter()
                .filter_map(|(k, v)| {
                    if let serde_yaml::Value::String(key) = k {
                        Some((key, yaml_to_json(v)))
                    } else {
                        None
                    }
                })
                .collect();
            Value::Object(map)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_to_json(tagged.value),
    }
}

/// Extract tags from a parsed frontmatter HashMap.
///
/// Looks for the `"tags"` key and converts its value to `Vec<String>`.
/// Returns an empty Vec if the key is missing or isn't an array of strings.
pub fn extract_tags(frontmatter: &std::collections::BTreeMap<String, Value>) -> Vec<String> {
    match frontmatter.get("tags") {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => vec![],
    }
}

/// Build article map from parsed documents.
///
/// This creates a mapping from URL paths to article metadata for all
/// documents that are classified as articles (in subfolders, not index pages).
///
/// Article metadata is built directly from `ParsedDocument` fields rather than
/// re-reading source files, which avoids path mismatches when slug generation
/// converts special characters (e.g., `：` → `-`, ` ` → `-`).
///
/// # Arguments
/// * `documents` - Parsed documents from site generation
/// * `dirs` - Every directory the scan narrowed to "gets an index page"
///   (`ProjectStructure.dirs`), the input `folder_index_keys` turns into
///   `folder_indexes` below
/// * `url_collisions` - Duplicated `url:` values this build had to move
///
/// # Returns
/// An ArticleMap containing metadata for all articles
pub fn build_article_map(
    documents: &[ParsedDocument],
    dirs: &[String],
    dir_overrides: &std::collections::HashMap<String, String>,
    url_collisions: &[UrlCollision],
    generated: &[String],
    terms: &TermIndex,
) -> ArticleMap {
    let mut map = ArticleMap::new();
    map.generated = generated.to_vec();
    map.terms = terms.sites().clone();
    map.kinds = terms.kinds().to_vec();
    // The single producer of the source-directory→URL mapping (see the field
    // doc on `ArticleMap::folder_indexes`) — reused here, not re-derived, so
    // this can never disagree with the render's own folder-index synthesis.
    map.folder_indexes = crate::build::scan::classify::folder_index_keys(dirs, dir_overrides)
        .map(|(dir, url)| (dir.clone(), url))
        .collect();
    // Term key → the subset of that term's owning kind's fields with at
    // least one member of THIS term specifically — a derived summary of
    // `members_by_field`, not the full member-URL lists. Empty when no
    // field has a member, rather than an empty Vec, so a reader's `.get()`
    // and a fallback-to-`fields[0]` branch agree on "no members" either way.
    for term_key in terms.sites().keys() {
        let Some((ns, _)) = term_key.split_once('/') else { continue };
        let Some(kind) = terms.kinds().iter().find(|k| k.key == ns) else { continue };
        let fields_with_a_member: Vec<String> = kind
            .fields
            .iter()
            .filter(|field| {
                terms
                    .members_by_field()
                    .get(&(term_key.clone(), (*field).clone()))
                    .is_some_and(|members| !members.is_empty())
            })
            .cloned()
            .collect();
        if !fields_with_a_member.is_empty() {
            map.fields_with_members.insert(term_key.clone(), fields_with_a_member);
        }
    }

    for doc in documents {
        // Slot files (`footer.md`) fill layout chrome on every page and emit
        // no page of their own — the render partition skips them
        // (`render/blocking.rs`, the `doc.slot_only` filter). They deliberately
        // stay in `documents` so the slot collector can reach their parsed HTML
        // through the normal data path, which is exactly why this
        // loop has to exclude them explicitly: without this gate `footer.md`
        // lands under the key `footer/`, and the map is the ONLY reason
        // anything believes that URL exists. Two consequences followed —
        // `editor::resolve::links::resolve_url_for_file_inner` handed the preview a
        // `/footer/` that was never emitted (404), and
        // `editor::commands::resolve_page_source` reported `is_article: true`,
        // which arms the syndicate path in `plugins::syndicate`.
        if doc.slot_only {
            continue;
        }

        // Index pages (homepage, section listings) go in the pages map,
        // not the articles map. This preserves the url→source mapping for
        // editor sync without polluting the article list used by plugins.
        if doc.kind == PageKind::Folder || doc.url_path == "index.html" {
            let pretty_url = to_pretty_url(&doc.url_path);
            if let Some(ref src) = doc.source_path {
                map.page_titles.insert(pretty_url.clone(), doc.title.clone());
                map.pages.insert(pretty_url, src.clone());
            }
            continue;
        }

        // Build ArticleInfo directly from ParsedDocument fields instead of
        // re-reading the source file. This avoids the bug where slug generation
        // converts special characters (e.g. `：` → `-`, ` ` → `-`) making the
        // reconstructed path not match the actual filename on disk.
        let pretty_url = to_pretty_url(&doc.url_path);
        let mut frontmatter = doc.raw_frontmatter.clone();

        // Resolve cover path through dir_overrides so plugins get URL-ready paths
        if let Some(Value::String(cover_val)) = frontmatter.get("cover") {
            if !cover_val.starts_with("http://") && !cover_val.starts_with("https://") {
                let resolved = crate::build::render::resolve_path_with_overrides(cover_val, dir_overrides);
                frontmatter.insert("cover".to_string(), Value::String(resolved));
            }
        }

        let tags = extract_tags(&frontmatter);
        let article_info = ArticleInfo {
            source_path: doc.source_path.clone().unwrap_or_default(),
            title: doc.title.clone(),
            content: doc.content.clone(),
            html_content: if !doc.html_content.is_empty() {
                Some(doc.html_content.clone())
            } else {
                None
            },
            frontmatter: frontmatter.into_iter().collect(),
            url_path: pretty_url.clone(),
            date: doc.date.clone(),
            tags,
            uid: doc.uid.clone(),
        };
        map.articles.insert(pretty_url, article_info);
    }

    // Persist dir_overrides so the post-build native-slots phase can
    // reconstruct an accurate MediaDimensionLookup. See doc on
    // `ArticleMap::dir_overrides`.
    map.dir_overrides = dir_overrides.clone();
    map.url_collisions = url_collisions
        .iter()
        .map(|c| (c.loser.clone(), c.clone()))
        .collect();

    map
}

#[cfg(test)]
#[path = "article_map_tests.rs"]
mod tests;
