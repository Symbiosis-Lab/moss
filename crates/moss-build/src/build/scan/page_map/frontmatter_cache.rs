//! Persisted frontmatter pre-scan cache for [`super::build_page_map`] /
//! [`super::build_external_url_map`] — see [`FrontmatterScanCache`] for the
//! corpus-scaled cost this closes.

use crate::build::types::identity_disagrees;
use std::path::Path;

/// One file's cached frontmatter pre-scan result, plus the stat identity it
/// was captured against. See [`FrontmatterScanCache`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct FrontmatterScanEntry {
    size: u64,
    mtime: u64,
    mtime_nanos: Option<u32>,
    ctime: Option<i64>,
    inode: Option<u64>,
    pub(super) url_override: Option<String>,
    external_url: Option<String>,
    /// The `lang:` this file declares, trimmed and allowlist-validated — the
    /// same string [`crate::i18n::declared_lang_in_file`] would return. Carried
    /// here so `folder_lang`'s declaration rung reads the folder's index out of
    /// the pass that already read every file, instead of re-reading one small
    /// file per folder on every build (118 of them on a 251-file vault).
    pub(super) lang: Option<String>,
}

/// Bumped whenever [`FrontmatterScanEntry`] gains or loses a field. An entry
/// written before a new field existed is not merely incomplete, it is WRONG:
/// a missing `lang` is indistinguishable from "this file declares none", so
/// the folder rung would read every declaration as absent for a whole build.
/// A mismatch discards the file, which costs one full re-scan exactly once.
const SCHEMA: u32 = 2;

/// Persisted `.moss/build.nosync/cache/frontmatter-scan.json`: the `url:` and
/// `external_url:` frontmatter fields `build_page_map`/`build_external_url_map`
/// pre-scan out of every markdown file, keyed by source path, so a rebuild
/// that touches one file does not re-read and re-parse all of them.
///
/// The corpus-scaled cost this closes: on a 226-page vault, editing ONE page
/// still paid ~150ms in `page_map` and ~180ms in `external_url_map` on
/// EVERY build — full time regardless of how many pages changed, and
/// `external_url_map` costs the same whether it finds 0 entries or 20,
/// because the cost is the read+parse of every file, not the size of the
/// result.
///
/// Correctness: an entry is trusted only when the file's `(size, mtime,
/// mtime_nanos)` match exactly AND any ctime/inode recorded on both sides
/// agree — the same "both sides present and disagree ⇒ don't trust" rule
/// the watcher's admission gate uses ([`identity_disagrees`]; this cache has
/// no hash tier to demote to on disagreement, so unlike the watcher it always
/// fails open to a full re-parse rather than trusting a forged mtime). A missed cache
/// hit costs one extra file read; a false hit would silently ship a stale
/// URL, so the bar here is "never wrong", not "never re-parse".
///
/// Known gap: unlike the watcher (`build::watch`'s `mtime_is_racy`), this
/// cache has no same-second racy-write epsilon and no hash tier to fall
/// back to. A same-second rewrite that preserves size and lands with
/// identical `mtime_nanos` (coarse-resolution filesystems/mounts) can read
/// as a hit. Bounded and self-healing: it costs one build serving the
/// prior URL, corrected by the next edit or build once the clock ticks
/// past the collision — never a wrong *final* state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct FrontmatterScanCache {
    /// `#[serde(default)]` uses the FIELD type's default (0), not this
    /// struct's — which is what makes a legacy file readable-then-rejected
    /// rather than a parse error, while a cache built in memory carries the
    /// current schema and round-trips.
    #[serde(default)]
    schema: u32,
    pub(super) entries: std::collections::HashMap<String, FrontmatterScanEntry>,
}

impl Default for FrontmatterScanCache {
    fn default() -> Self {
        Self { schema: SCHEMA, entries: std::collections::HashMap::new() }
    }
}

impl FrontmatterScanCache {
    /// Load from disk. A missing or unparsable file is a cold cache, not an
    /// error — same fail-open convention as `cache::HashIndex::load`.
    pub(crate) fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Self>(&s).ok())
            .filter(|c| c.schema == SCHEMA)
            .unwrap_or_default()
    }

    /// A cache populated by the real scan over `files`, for tests that need
    /// the declarations the page-map pass would have left behind.
    #[cfg(test)]
    pub(super) fn scanned_for_test(
        files: &[crate::types::content::FileInfo],
        source_path: &Path,
    ) -> Self {
        let mut cache = Self::default();
        scan_frontmatter_urls_with_evicted(files, source_path, &mut cache, &|_| false);
        cache
    }

    /// The validated `lang:` recorded for one source path, if that file
    /// declares one.
    ///
    /// Deliberately NOT equivalent to re-reading the file, and the difference
    /// cuts both ways. A file that was evicted or unreadable THIS build keeps
    /// the entry it had, so a declaration survives a cloud eviction instead of
    /// collapsing to inference — but the same mechanism serves a STALE
    /// declaration for an index edited on another machine and then evicted
    /// locally, where the read this replaced would have fallen to inference.
    /// Bounded: the next download corrects it, and the direction is deliberate
    /// — a language the author declared beats one guessed from bodies.
    pub(super) fn declared_lang(&self, source_path: &str) -> Option<&str> {
        self.entries.get(source_path)?.lang.as_deref()
    }

    /// Persist to disk. Goes through `io_utils` because this file lives
    /// under `.moss/build.nosync/` (dataless is treated as absent there).
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        crate::build::io_utils::write_output(path, &json)
    }
}

/// Strip one layer of matching surrounding quotes, as a YAML load would — for
/// the simplified dialect only, whose parser hands back raw text.
fn unquote(v: &str) -> &str {
    for q in ['"', '\''] {
        if let Some(inner) = v.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner;
        }
    }
    v
}

/// Scans every markdown file's frontmatter for `url:` and `external_url:` in
/// ONE read+parse pass per file — both fields live on the same parsed
/// frontmatter struct, so `build_page_map`/`build_external_url_map` running
/// separately used to each pay for their own pass over the same bytes.
/// Consults `cache` first and skips the read entirely for a file whose stat
/// identity still matches what was recorded when its entry was cached.
///
/// Returns `path -> (url_override, external_url)` for every markdown file
/// that is not cloud-evicted or unreadable, and updates `cache` in place
/// with a fresh entry for every file it actually read (cache hits are left
/// untouched). Stale entries for files no longer in `markdown_files` are
/// left in the map too — harmless, and pruning them is the caller's call
/// since eviction/deletion already has its own stale-cleanup pass.
fn scan_frontmatter_urls_with_evicted(
    markdown_files: &[crate::types::content::FileInfo],
    source_path: &Path,
    cache: &mut FrontmatterScanCache,
    is_evicted: &dyn Fn(&Path) -> bool,
) -> std::collections::HashMap<String, (Option<String>, Option<String>)> {
    use std::collections::HashMap;
    use std::time::UNIX_EPOCH;

    let mut out: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();

    for file_info in markdown_files {
        let file_path = &file_info.path;
        let source_file_path = source_path.join(file_path);

        if is_evicted(&source_file_path) {
            crate::build::cloud_readiness::request_download(&source_file_path);
            continue;
        }

        let stat = std::fs::metadata(&source_file_path).ok();
        let cached = cache.entries.get(file_path);

        let hit = match (&stat, cached) {
            (Some(md), Some(entry)) => {
                let (ctime, inode) = crate::build::types::stat_identity(md);
                let mtime = md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok());
                md.len() == entry.size
                    && !identity_disagrees(entry.ctime, ctime)
                    && !identity_disagrees(entry.inode, inode)
                    && matches!(
                        (mtime, entry.mtime_nanos),
                        (Some(d), Some(nanos)) if d.as_secs() == entry.mtime && d.subsec_nanos() == nanos
                    )
            }
            _ => false,
        };

        if hit {
            let entry = cached.expect("hit implies cached.is_some()");
            out.insert(file_path.clone(), (entry.url_override.clone(), entry.external_url.clone()));
            continue;
        }

        let content = match std::fs::read_to_string(&source_file_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("frontmatter scan skipping unreadable '{}': {}", file_path, e);
                continue;
            }
        };

        // ONE parser, one pass, both fields — same parse
        // `build_page_map`/`build_external_url_map` used to run separately.
        let (frontmatter_url, external_url_raw, lang_raw) =
            if crate::build::markdown::is_simplified_frontmatter(&content) {
                let (fm, _) = crate::build::markdown::parse_simplified_frontmatter(&content);
                // Unquoted HERE, not below, because only this dialect needs it:
                // its parser splits on the first colon and hands back the raw
                // text, so `lang: "zh-hant"` still wears its quotes. The typed
                // parser went through serde, which already unquoted — stripping
                // again there would newly ACCEPT `lang: '"en"'`.
                (fm.url, fm.external_url, fm.lang.map(|c| unquote(c.trim()).to_string()))
            } else {
                let fm = crate::build::markdown::parse_typed_frontmatter(&content);
                (fm.url, fm.external_url, fm.lang)
            };

        // Validated HERE, not at the read site, so a stored code is always one
        // the language rungs may act on — the same allowlist gate
        // `crate::i18n::declared_lang_in_file` applies. That allowlist also
        // contains the one dialect difference this parser has from the
        // serde_yaml reader in `declared_lang_in_file`: the simplified dialect
        // has no comment concept, so `lang: en # note` is the value `en # note`
        // here and `en` there. Rejecting it costs that folder its declaration
        // (it falls to inference) rather than acting on a language nobody
        // declared, and it keeps `lang` reading like every other field of that
        // dialect, which stores `foo # bar` verbatim too.
        let lang = lang_raw
            .map(|c| c.trim().to_string())
            .filter(|c| moss_core::home::is_known_language_code(c));

        if let Some(raw) = external_url_raw.as_deref() {
            // http(s) only — same safety guard as the card-href substitution
            // in page.rs and the wikilink resolver in pipeline.rs. Warn
            // before silently dropping so the user knows why.
            super::warn_invalid_external_url(raw, file_path);
        }
        let external_url = external_url_raw.filter(|u| super::is_valid_external_url(u));

        if let Some(md) = stat {
            let (ctime, inode) = crate::build::types::stat_identity(&md);
            let mtime = md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok());
            cache.entries.insert(
                file_path.clone(),
                FrontmatterScanEntry {
                    size: md.len(),
                    mtime: mtime.map(|d| d.as_secs()).unwrap_or(0),
                    mtime_nanos: mtime.map(|d| d.subsec_nanos()),
                    ctime,
                    inode,
                    url_override: frontmatter_url.clone(),
                    external_url: external_url.clone(),
                    lang,
                },
            );
        } else {
            // Stat unavailable (raced delete between the eviction check and
            // here): drop any stale entry rather than caching against an
            // identity we couldn't observe.
            cache.entries.remove(file_path);
        }

        out.insert(file_path.clone(), (frontmatter_url, external_url));
    }

    out
}

/// Cached, merged variant of `build_page_map` + `build_external_url_map`:
/// one frontmatter pass per file (via [`scan_frontmatter_urls_with_evicted`])
/// instead of two, with `cache` absorbing the corpus-scaled cost for files
/// unchanged since it was last written. Returns the same three maps
/// production code already computed with two separate calls:
/// `(page_map, dir_overrides, external_url_map)`.
pub(crate) fn build_page_map_and_external_urls_cached(
    markdown_files: &[crate::types::content::FileInfo],
    source_path: &Path,
    root_folder_name: &str,
    home_file_winners: &std::collections::HashSet<String>,
    home_overrides: &std::collections::HashMap<String, String>,
    cache: &mut FrontmatterScanCache,
) -> (
    std::collections::HashMap<String, String>,
    std::collections::HashMap<String, String>,
    std::collections::HashMap<String, String>,
) {
    build_page_map_and_external_urls_cached_with_evicted(
        markdown_files,
        source_path,
        root_folder_name,
        home_file_winners,
        home_overrides,
        cache,
        &crate::build::icloud::is_evicted,
    )
}

/// Same as [`build_page_map_and_external_urls_cached`] with an injectable
/// eviction predicate — see `build_page_map_with_evicted` for the same pattern.
pub(crate) fn build_page_map_and_external_urls_cached_with_evicted(
    markdown_files: &[crate::types::content::FileInfo],
    source_path: &Path,
    root_folder_name: &str,
    home_file_winners: &std::collections::HashSet<String>,
    home_overrides: &std::collections::HashMap<String, String>,
    cache: &mut FrontmatterScanCache,
    is_evicted: &dyn Fn(&Path) -> bool,
) -> (
    std::collections::HashMap<String, String>,
    std::collections::HashMap<String, String>,
    std::collections::HashMap<String, String>,
) {
    use std::collections::HashMap;

    let scanned = scan_frontmatter_urls_with_evicted(markdown_files, source_path, cache, is_evicted);

    let mut entries: Vec<(String, String, bool)> = Vec::new();
    let mut dir_overrides: HashMap<String, String> = HashMap::new();
    let mut external_url_map: HashMap<String, String> = HashMap::new();

    for file_info in markdown_files {
        let file_path = &file_info.path;
        let Some((frontmatter_url, external_url)) = scanned.get(file_path) else {
            // Evicted or unreadable — same skip as build_page_map_with_evicted.
            continue;
        };
        if let Some(url) = external_url {
            external_url_map.insert(file_path.clone(), url.clone());
        }

        let (url_path, is_index, dir_override) = super::page_map_entry(
            file_path,
            frontmatter_url.as_deref(),
            root_folder_name,
            home_file_winners,
            home_overrides,
        );
        if let Some((dir, slug)) = dir_override {
            dir_overrides.insert(dir, slug);
        }

        entries.push((file_path.to_string(), url_path, is_index));
    }

    super::apply_cascading_dir_overrides(&mut entries, &dir_overrides);

    let page_map = entries.into_iter().map(|(k, v, _)| (k, v)).collect();
    (page_map, dir_overrides, external_url_map)
}
