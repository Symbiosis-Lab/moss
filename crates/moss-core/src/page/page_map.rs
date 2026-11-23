//! The site's page map, from file paths and their frontmatter alone: which
//! file is each folder's home, and each file's address. The build reads the
//! files and calls this; a host with the texts calls it the same way.

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A note on the home election for the site's log; `warn` names a conflict
/// the author should settle, the rest are diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeNotice {
    pub warn: bool,
    pub text: String,
}

/// One file's home-relevant frontmatter: `home:` and `translationKey:`.
#[derive(Debug, Clone, Copy)]
pub struct HomeClaim<'a> {
    pub path: &'a str,
    pub home: Option<bool>,
    pub translation_key: Option<&'a str>,
}

fn parent_dir(path: &str) -> &str {
    Path::new(path).parent().and_then(|p| p.to_str()).unwrap_or("")
}

/// Pre-scan markdown frontmatter to elect each folder's home file, returning a
/// map from `parent_directory → file_path`. A file in this map wins its
/// folder's home slot regardless of filename — see [`compute_home_file_winners`].
///
/// This is the supported way to make `en/Mountain Home.md` (or any other
/// non-INDEX_STEM, non-self-named filename) the homepage of a folder.
/// Without this, moss falls back to the filename-only detection
/// in [`crate::home::detect_home_file_in_folder`] and the file lands
/// at a slug-based URL (`en/mountain-home/`) while moss synthesizes an empty
/// `en/index.html` titled `"En"`.
///
/// # Election rules
///
/// 1. **Anchor** — a file that is its OWN folder's home: `home == Some(true)`
///    OR [`crate::home::is_home_file`] (self-named, index-stem, or
///    lang-suffixed, case-insensitive). The folder basename for a root-level
///    file is the project root's name, read from the site folder's name — the same
///    string the build feeds to
///    [`compute_home_file_winners`], so the two maps can never disagree.
/// 2. **Direct promotion** — every `home == Some(true)` file becomes its
///    folder's home in the override map (filename irrelevant).
/// 3. **Translation groups** — files are grouped by `translationKey` *value*
///    (any value; files with no key are skipped).
/// 4. **Inherited promotion** — for each group containing ≥1 anchor, every
///    member whose folder has no anchor of its own becomes its folder's home.
///    Rationale: the `en/` translation of a home inherits `/en/` home-ness via
///    the shared key, without needing its own marker. The `translationKey`
///    *value* never means "home" — only the translation *relationship* (sharing
///    a key with an anchor) propagates home-ness.
/// 5. **Precedence** — a folder's own anchor (`home: true` or name-based)
///    always beats an inherited candidate. The override map only carries the
///    overriding (frontmatter-marker or inherited) winners; name-based anchors
///    are resolved by [`compute_home_file_winners`] from filenames alone, so an
///    inherited candidate is suppressed for any folder that has a name-based
///    anchor.
///
/// At most one promotion per folder; if a folder gets multiple direct or
/// multiple inherited candidates, the first one (alphabetical by filename)
/// wins for determinism and a warning [`HomeNotice`] names the demoted file(s).
pub fn home_overrides(claims: &[HomeClaim<'_>], root_basename: &str) -> (HashMap<String, String>, Vec<HomeNotice>) {
    struct FileFacts {
        path: String,
        parent: String,
        filename: String,
        home: Option<bool>,
        translation_key: Option<String>,
        /// The file is its own folder's home by name (self-named, index-stem
        /// or language-suffixed).
        name_anchor: bool,
    }
    let mut notices = Vec::new();
    let facts: Vec<FileFacts> = claims.iter().map(|c| {
        let parent = parent_dir(c.path).to_string();
        let filename = Path::new(c.path).file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        // The folder's name for self-named detection; the site folder's for a root file.
        let folder_basename = if parent.is_empty() {
            root_basename
        } else {
            Path::new(&parent).file_name().and_then(|n| n.to_str()).unwrap_or("")
        };
        let stem_lower = Path::new(c.path).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        let name_anchor = crate::home::is_home_file(&stem_lower, &folder_basename.to_lowercase());
        FileFacts { path: c.path.to_string(), parent, filename, home: c.home, translation_key: c.translation_key.map(str::to_string), name_anchor }
    }).collect();

    // A folder "has its own anchor" when some file in it is an anchor —
    // either `home: true` (direct) or name-based (self-named / index-stem).
    let mut folders_with_anchor: HashSet<String> =
        HashSet::new();
    for f in &facts {
        if f.home == Some(true) || f.name_anchor {
            folders_with_anchor.insert(f.parent.clone());
        }
    }

    // Collect per-folder candidates as (filename, path) so we can apply the
    // shared deterministic election + multi-claim warning. A `reason` label
    // makes the warning accurate for both the direct and inherited paths.
    let mut by_folder: HashMap<String, Vec<(String, String)>> = HashMap::new();

    // Rule 2: direct promotion — every `home: true` file.
    for f in &facts {
        if f.home == Some(true) {
            by_folder
                .entry(f.parent.clone())
                .or_default()
                .push((f.filename.clone(), f.path.clone()));
        }
    }

    // Rules 3+4: translation groups → inherited promotion.
    // Group files by translationKey value (skip files with no key).
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, f) in facts.iter().enumerate() {
        if let Some(key) = &f.translation_key {
            groups.entry(key.clone()).or_default().push(i);
        }
    }
    for members in groups.values() {
        // Does this group contain an anchor (rules 1: home:true OR name-based)?
        let has_anchor = members
            .iter()
            .any(|&i| facts[i].home == Some(true) || facts[i].name_anchor);
        if !has_anchor {
            continue;
        }
        // Name an anchor for the inheritance diagnostic below (any one — the
        // anchor's identity does not affect the result, only group membership).
        let anchor_path = members
            .iter()
            .find(|&&i| facts[i].home == Some(true) || facts[i].name_anchor)
            .map(|&i| facts[i].path.clone())
            .unwrap_or_default();
        // Inherited promotion: every member whose folder has NO anchor of its
        // own becomes its folder's home. (Rule 5 precedence: skip folders that
        // already have a direct/name anchor.) A member that is itself the
        // anchor lives in a folder with an anchor, so it is skipped here — it is
        // already handled by rule 2 (if home:true) or by name-detection.
        for &i in members {
            let f = &facts[i];
            if folders_with_anchor.contains(&f.parent) {
                continue;
            }
            // Diagnostic: this folder's home is elected purely by inheritance (no
            // local marker/index/self-name). A shared arbitrary translationKey is
            // a weaker intent signal than a name/marker, so make the "why is this
            // a home?" answer discoverable.
            // One line per inheriting folder, every build — on a large
            // multilingual site that scales with the tree, so DEBUG. Turn on
            // debug logging when the question is actually "why is this a home?".
            notices.push(HomeNotice { warn: false, text: format!(
                "`{}` inherits home-ness for `{}` via translation link to anchor `{}`",
                f.path,
                if f.parent.is_empty() { "/" } else { f.parent.as_str() },
                anchor_path,
            ) });
            by_folder
                .entry(f.parent.clone())
                .or_default()
                .push((f.filename.clone(), f.path.clone()));
        }
    }

    let mut overrides = HashMap::new();
    let mut warned_folders: HashSet<String> = HashSet::new();
    for (parent, mut candidates) in by_folder {
        // Dedup: a file could be added by both direct + inherited paths.
        candidates.sort_by(|a, b| a.0.cmp(&b.0));
        candidates.dedup_by(|a, b| a.1 == b.1);
        if candidates.len() > 1 {
            warned_folders.insert(parent.clone());
            let display_folder = if parent.is_empty() { "/" } else { parent.as_str() };
            let losers: Vec<&str> = candidates[1..].iter().map(|(_, p)| p.as_str()).collect();
            notices.push(HomeNotice { warn: true, text: format!(
                "{} files claim the home slot in `{}`; using `{}` (alphabetical first), ignoring: {}",
                candidates.len(),
                display_folder,
                candidates[0].1,
                losers.join(", "),
            ) });
        }
        if let Some((_, path)) = candidates.into_iter().next() {
            overrides.insert(parent, path);
        }
    }

    // Broadened multi-claim diagnostic — NAME collisions. The warning above fires
    // only for marker/inherited candidates collected in `by_folder`. A name-only
    // collision (a self-named file + an `index`-stem, or marker + name) is resolved
    // silently by `detect_home_file_in_folder` precedence downstream, so the demoted
    // file becomes a regular page with no diagnostic (the source of stray `/index/`
    // nav items). Surface it. Gate on ≥1 name anchor AND skip folders the loop
    // above already warned, so pure marker/inherited collisions (and the rare
    // double-marker that is also name-anchored) never double-fire here.
    let mut name_candidates: HashMap<String, Vec<String>> = HashMap::new();
    let mut folder_has_name_anchor: HashSet<String> =
        HashSet::new();
    for f in &facts {
        if f.name_anchor || f.home == Some(true) {
            name_candidates
                .entry(f.parent.clone())
                .or_default()
                .push(f.path.clone());
            if f.name_anchor {
                folder_has_name_anchor.insert(f.parent.clone());
            }
        }
    }
    for (folder, mut cands) in name_candidates {
        if !folder_has_name_anchor.contains(&folder) || warned_folders.contains(&folder) {
            continue;
        }
        cands.sort();
        cands.dedup();
        if cands.len() >= 2 {
            let display = if folder.is_empty() { "/" } else { folder.as_str() };
            notices.push(HomeNotice { warn: true, text: format!(
                "{} files claim the home slot in `{}` by name/marker: {} — moss elects by precedence (home:true › index › self-named); the rest become regular pages",
                cands.len(),
                display,
                cands.join(", "),
            ) });
        }
    }

    (overrides, notices)
}


/// Groups markdown files by their parent directory, then uses
/// `detect_home_file_in_folder()` to pick one winner per group.
/// Returns a set of winning file paths (relative, e.g. "index.md" or "recipes/index.md").
///
/// `home_overrides` (from [`home_overrides`]) wins over filename-based
/// detection: any folder listed there gets its overriding file as the
/// home, regardless of what `detect_home_file_in_folder` would pick from
/// filenames alone.
pub fn home_file_winners<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    root_folder_name: &str,
    home_overrides: &std::collections::HashMap<String, String>,
) -> std::collections::HashSet<String> {
    use std::collections::HashMap;

    // Group filenames by parent directory
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for path in paths {
        let parent = Path::new(path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();
        let filename = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        groups.entry(parent).or_default().push(filename);
    }

    let mut winners = std::collections::HashSet::new();
    for (parent_dir, filenames) in &groups {
        // home: true marker wins outright when set.
        if let Some(override_path) = home_overrides.get(parent_dir) {
            winners.insert(override_path.clone());
            continue;
        }

        // Determine the folder name for self-named detection
        let folder_name = if parent_dir.is_empty() {
            root_folder_name.to_string()
        } else {
            Path::new(parent_dir)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string()
        };
        let refs: Vec<&str> = filenames.iter().map(|s| s.as_str()).collect();
        if let Some(winner_filename) = crate::home::detect_home_file_in_folder(&refs, &folder_name) {
            let winner_path = if parent_dir.is_empty() {
                winner_filename.to_string()
            } else {
                format!("{}/{}", parent_dir, winner_filename)
            };
            winners.insert(winner_path);
        }
    }
    winners
}

/// Whether a file that lost its folder's home election is that home's
/// language variant (`index.fr.md`), which shares the winner's address so slug
/// deduplication can place it. A folder without a winner has no address to
/// share.
pub fn is_home_translation(file_path: &str, winners: &HashSet<String>) -> bool {
    let stem = Path::new(file_path).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let dir = parent_dir(file_path);
    crate::home::strip_lang_suffix(&stem).is_some_and(crate::home::is_index_stem)
        && winners.iter().any(|w| parent_dir(w) == dir)
}

/// Whether the file's name makes it its folder's home (index-named or
/// self-named), as the page map decides it.
fn is_home_by_name(file_path: &str, root_folder_name: &str) -> bool {
    let stem = Path::new(file_path).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let parent_name = match file_path.rsplit_once('/') {
        Some((parent, _)) => parent.rsplit('/').next().unwrap_or(""),
        None => root_folder_name,
    };
    crate::home::is_home_file(&stem, parent_name)
}

/// The folder homes the page map serves at `<folder>/index.html`: the
/// election's winners that won by name or by the `home: true` marker. A
/// folder whose winner is only its first page has no home.
pub fn folder_homes<'a>(
    winners: &'a std::collections::HashSet<String>,
    overrides: &std::collections::HashMap<String, String>,
    root_folder_name: &str,
) -> Vec<&'a str> {
    winners.iter().map(String::as_str)
        .filter(|w| is_home_by_name(w, root_folder_name) || overrides.values().any(|o| o == w))
        .collect()
}

/// One file's address: `(url_path, is_index, dir_override)`, given its `url:`
/// frontmatter. `dir_override` is the `(folder, slug)` a winning folder home's
/// `url:` cascades to the pages under it ([`apply_cascading_dir_overrides`]).
pub fn page_map_entry(
    file_path: &str,
    frontmatter_url: Option<&str>,
    root_folder_name: &str,
    home_file_winners: &std::collections::HashSet<String>,
    home_overrides: &std::collections::HashMap<String, String>,
) -> (String, bool, Option<(String, String)>) {
    let filename_match_is_home = is_home_by_name(file_path, root_folder_name);
    let mut is_index = filename_match_is_home;

    // Demote non-winner index files (same logic as in generate_blocking_content)
    let is_home_translation = if filename_match_is_home && !home_file_winners.contains(file_path) {
        // A language variant of the home keeps the winner's URL so slug
        // deduplication can place it, rather than getting a directory URL.
        is_index = false;
        is_home_translation(file_path, home_file_winners)
    } else {
        false
    };

    // Promote home-override files (the `home: true` marker) — files
    // that won the home slot via frontmatter, not filename. These need
    // to go to `<folder>/index.html` so the folder home is the file's
    // URL, not a synthesized empty page.
    let is_home_override = home_overrides.values().any(|p| p == file_path);
    if !filename_match_is_home && is_home_override {
        is_index = true;
    }

    // Derive clean_stem: strip extension, take last segment
    let filename_stem = Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled");
    // Pre-scan only needs the clean stem (strips any language suffix).
    // The doc's resolved language is computed later in the markdown
    // pipeline where the full priority chain matters.
    let clean_stem = crate::i18n::clean_stem_only(filename_stem);

    // For language-suffixed home translations, compute the URL as if still
    // an index file so it collides with the winner and gets deduplicated.
    let url_path = crate::frontmatter_typed::compute_url_path(
        file_path,
        is_index || is_home_translation,
        frontmatter_url,
        &clean_stem,
    );

    // Collect url override for index files (for cascading to children)
    let dir_override = if is_index {
        frontmatter_url.and_then(|url_override| {
            let dir = Path::new(file_path)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            if dir.is_empty() {
                None
            } else {
                Some((dir.to_string(), crate::slug::slugify_url_override(url_override)))
            }
        })
    } else {
        None
    };

    (url_path, is_index, dir_override)
}

/// Rewrite every entry's `url_path` prefix for the cascading `dir_overrides`
/// that [`page_map_entry`] collected.
///
/// For non-index files: resolve source_dir through overrides, rewrite url_path prefix.
/// For index files: compute_url_path already replaced the OWN segment, but
///   ancestor segments still need rewriting (e.g. 文字/travel/index.html → writings/travel/index.html).
pub fn apply_cascading_dir_overrides(
    entries: &mut [(String, String, bool)],
    dir_overrides: &std::collections::HashMap<String, String>,
) {
    if dir_overrides.is_empty() {
        return;
    }
    for (file_path, url_path, is_index) in entries.iter_mut() {
        let source_dir = Path::new(file_path.as_str())
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("");
        if source_dir.is_empty() {
            continue;
        }

        // Compare and rewrite using fully-slugified directory paths.
        //
        // resolve_path_with_overrides preserves the LAST segment verbatim
        // because most callers pass file paths whose last segment is a
        // filename. Pass 2 always passes directory-only paths, so we
        // re-slugify the result to drop any case in the trailing segment
        // (e.g. "my-section/Sub Section" → "my-section/sub-section").
        //
        // Comparing slug-form vs slug-form means we only enter the
        // strip-and-replace branch when an override actually changed the
        // path — not when slugification alone would. Without this, a
        // sibling Title-Case folder with no override would corrupt its
        // children's url_paths whenever ANY override exists in the site.
        if *is_index {
            let grandparent = Path::new(source_dir)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            if grandparent.is_empty() {
                continue;
            }
            let new_grandparent_raw = crate::resolve::output_url::resolve_path_with_overrides(grandparent, dir_overrides);
            let new_grandparent = crate::slug::slugify_path_segments(&new_grandparent_raw);
            let slugified_grandparent = crate::slug::slugify_path_segments(grandparent);
            if new_grandparent != slugified_grandparent {
                if let Some(rest) = url_path.strip_prefix(slugified_grandparent.as_str()) {
                    let rest = rest.trim_start_matches('/');
                    *url_path = format!("{}/{}", new_grandparent, rest);
                }
            }
        } else {
            let new_parent_raw = crate::resolve::output_url::resolve_path_with_overrides(source_dir, dir_overrides);
            let new_parent = crate::slug::slugify_path_segments(&new_parent_raw);
            let slugified_source = crate::slug::slugify_path_segments(source_dir);
            if new_parent != slugified_source {
                if let Some(rest) = url_path.strip_prefix(slugified_source.as_str()) {
                    let rest = rest.trim_start_matches('/');
                    *url_path = if new_parent.is_empty() {
                        rest.to_string()
                    } else {
                        format!("{}/{}", new_parent, rest)
                    };
                }
            }
        }
    }
}

/// One document that lost a URL contest, and what it lost.
///
/// The repair is always an edit to the **loser's** `url:` — the keeper is
/// named only so the author can see which pair is in conflict. Produced by
/// [`resolve_duplicate_slugs`] and reported at the frontmatter
/// field and as a build problem.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UrlCollision {
    /// Source path of the file whose `url:` must change.
    pub loser: String,
    /// Source path of the file keeping the contested address.
    pub keeper: String,
    /// The address both wanted, in served form (`awards/comics/`).
    pub wanted: String,
    /// Where the loser is published instead, in served form.
    pub moved_to: String,
}

/// What slug deduplication reads about one page.
#[derive(Debug, Clone, Copy)]
pub struct SlugEntry<'a> {
    pub url_path: &'a str,
    pub is_folder: bool,
    pub lang: crate::i18n::Language,
    pub slot_only: bool,
    pub source_path: Option<&'a str>,
}

/// Resolves duplicate URL paths with language awareness.
///
/// When multiple documents share the same `url_path`, the document whose
/// language matches `site_lang` keeps the primary slot; folder indexes outrank
/// both. Within a tier, filesystem order (slice order) is preserved.
///
/// Every loser then *claims* a URL: its preferred one — lang-prefixed when its
/// language differs from the keeper's, the original otherwise — or, if that is
/// already spoken for, the next free numbered directory. One claim rule rather
/// than one per branch, because the branches had drifted: the lang-prefix path
/// carried no counter (three `zh-hans` documents against one `en` primary all
/// landed on `zh-hans/about/index.html`) and never checked whether a real page
/// already sat at the prefixed address.
///
/// Returns each moved page's index and new address, and the collisions an
/// author must settle: those where numbering was
/// needed, minus the ones a numbered ancestor already explains. A
/// cross-language pair resolved by its prefix is the designed outcome and is
/// not a collision.
pub fn resolve_duplicate_slugs(
    documents: &[SlugEntry<'_>],
    site_lang: crate::i18n::Language,
) -> (Vec<(usize, String)>, Vec<UrlCollision>) {
    use crate::resolve::output_url::to_pretty_url;
    // Each page's address as claims settle it; read and written below.
    let mut url: Vec<String> = documents.iter().map(|d| d.url_path.to_string()).collect();
    let mut moved: Vec<(usize, String)> = Vec::new();

    // Who holds each URL. Seeded with every document so a preferred
    // lang-prefixed address that belongs to a REAL page (a site with a literal
    // `zh-hans/` folder) is seen as taken instead of being clobbered.
    let mut holder: HashMap<String, usize> = HashMap::new();
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, doc) in documents.iter().enumerate() {
        // Never emitted (a `partial: true` source, `footer.md`), so it holds no address.
        if doc.slot_only {
            continue;
        }
        holder.entry(doc.url_path.to_string()).or_insert(i);
        groups.entry(doc.url_path.to_string()).or_default().push(i);
    }

    // `groups` is a HashMap, so sort by path to keep assignment (and therefore
    // which page gets `-2`) stable across builds.
    let mut collisions: Vec<(&String, &Vec<usize>)> =
        groups.iter().filter(|(_, idx)| idx.len() > 1).collect();
    collisions.sort_by(|a, b| a.0.cmp(b.0));

    // Reported collisions, each paired with the directory it cascades over
    // (`Some` only for a folder index, the only kind whose `url:` cascades).
    let mut found: Vec<(UrlCollision, Option<String>)> = Vec::new();

    for (_, indices) in collisions {
        // Rank so the canonical doc comes first. `sort_by_key` is stable, so
        // filesystem order still decides within a tier: folder indexes (home
        // file winners keep the primary URL slot), then site_lang docs, then
        // the rest.
        let mut sorted_indices: Vec<usize> = indices.clone();
        sorted_indices.sort_by_key(|&i| {
            if documents[i].is_folder {
                0u8
            } else if documents[i].lang == site_lang {
                1
            } else {
                2
            }
        });

        let keeper_idx = sorted_indices[0];
        let original_path = url[keeper_idx].clone();
        let primary_lang = documents[keeper_idx].lang;
        holder.insert(original_path.clone(), keeper_idx);

        for &doc_idx in sorted_indices.iter().skip(1) {
            let preferred = if documents[doc_idx].lang != primary_lang {
                format!("{}/{}", documents[doc_idx].lang.code(), original_path)
            } else {
                original_path.clone()
            };

            // Claim `preferred`, or the first free numbered directory under it.
            // Numbering means someone else already answers at that address, and
            // that someone is the keeper the author needs named.
            let blocked_by = holder.get(&preferred).copied();
            let assigned = match blocked_by {
                None => preferred.clone(),
                Some(_) => (2u32..)
                    .map(|n| numbered(&preferred, n))
                    .find(|cand| !holder.contains_key(cand))
                    .unwrap_or_default(),
            };
            holder.insert(assigned.clone(), doc_idx);
            url[doc_idx] = assigned.clone();
            moved.push((doc_idx, assigned.clone()));

            // A prefixed cross-language page is the designed outcome; only a
            // page that had to be NUMBERED is one the author must settle.
            // Renaming keeps it reachable but cannot save its images: asset
            // output paths resolve from the source directory, not from
            // `url_path`, so the two trees still overwrite each other.
            let Some(blocker) = blocked_by else { continue };
            let source_of = |i: usize| documents[i].source_path.map_or_else(|| url[i].clone(), crate::slug::normalize_separators);
            let loser = source_of(doc_idx);
            let cascades_over = documents[doc_idx].is_folder
                .then(|| source_dir(&loser).to_string());
            found.push((
                UrlCollision {
                    loser,
                    keeper: source_of(blocker),
                    wanted: to_pretty_url(&preferred),
                    moved_to: to_pretty_url(&assigned),
                },
                cascades_over,
            ));
        }
    }

    (moved, root_causes(found))
}

fn numbered(url_path: &str, n: u32) -> String {
    match url_path.strip_suffix("/index.html") {
        Some(dir) => format!("{}-{}/index.html", dir, n),
        None => format!("{}-{}/index.html", url_path.trim_end_matches(".html"), n),
    }
}


/// The directory part of a source path (`a/b/c.md` → `a/b`, `c.md` → ``).
fn source_dir(source_path: &str) -> &str {
    source_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

/// Drop every collision a numbered folder above it already explains.
///
/// A folder's `url:` cascades to its descendants, so one duplicated folder
/// produces a collision for the folder AND for every page beneath it. Every deeper
/// one is a consequence, and reporting them all buries the single edit that
/// fixes the lot: the children's own `url:` segments are relative and correct,
/// so changing the parent's resolves them untouched.
fn root_causes(found: Vec<(UrlCollision, Option<String>)>) -> Vec<UrlCollision> {
    // Paired with its own index: a folder note lives INSIDE the directory it
    // cascades over (`awards/essays/essays.md`), so a collision compared against
    // its own directory would suppress itself and report nothing at all.
    let cascading: Vec<(usize, String)> = found
        .iter()
        .enumerate()
        .filter_map(|(i, (_, dir))| dir.as_ref().map(|d| (i, format!("{}/", d))))
        .collect();
    found
        .iter()
        .enumerate()
        .filter(|(i, (c, _))| {
            !cascading
                .iter()
                .any(|(j, dir)| j != i && c.loser.starts_with(dir.as_str()))
        })
        .map(|(_, (c, _))| c.clone())
        .collect()
}
