//! Pure asset-reference resolver shared by editor + build (single source of truth).
//! Mirrors `link_class.rs`. Zero I/O — data injected via `AssetIndex` (ADR-018).

use crate::resolve::parent_dir;

pub trait AssetIndex {
    fn contains(&self, root_rel: &str) -> bool;
    fn contains_ci(&self, root_rel: &str) -> Option<String>;
    fn find_by_suffix(&self, suffix: &str) -> Vec<String>;
}

#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetProvenance {
    Literal,
    BareFuzzy,
    SeparatorFallback,
    CaseMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetResolution {
    Resolved { root_rel: String, provenance: AssetProvenance },
    Ambiguous { chosen: String, candidates: Vec<String> },
    NotFound,
}

/// Lexically collapse `.`/`..` against a base dir; returns None if it escapes root.
fn lexical_join(base_dir: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = if base_dir.is_empty() { vec![] } else { base_dir.split('/').collect() };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => { parts.pop()?; } // pop; underflow → escape → None
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

fn has_separator(t: &str) -> bool {
    t.contains('/')
}

pub fn resolve_asset_ref(target: &str, from_source: &str, index: &dyn AssetIndex) -> AssetResolution {
    let from_dir = parent_dir(from_source);

    // Step 1: `/`-absolute → root.
    if let Some(stripped) = target.strip_prefix('/') {
        return finish(stripped.to_string(), AssetProvenance::Literal, index);
    }

    // Step 2: literal source-relative (all targets).
    if let Some(cand) = lexical_join(from_dir, target) {
        if let Some(res) = finish_opt(&cand, AssetProvenance::Literal, index) { return res; }
    }

    // Step 3: project-root-relative — SEPARATOR targets only (bare handled in step 4).
    //
    // Also detect containment escape: a separator target whose lexical resolution
    // underflows the project root in BOTH step 2 and step 3 (i.e. both lexical_join
    // calls return None) must never fall through to basename fuzzy matching. An explicit
    // relative path like "../../etc/x.jpg" that escapes the project root is invalid;
    // resolving it by basename would silently return an unrelated file. Containment rule:
    // escaped paths → NotFound, always.
    let escaped = if has_separator(target) {
        let step2_escaped = lexical_join(from_dir, target).is_none();
        let step3_cand = lexical_join("", target);
        if let Some(cand) = step3_cand {
            if let Some(res) = finish_opt(&cand, AssetProvenance::SeparatorFallback, index) { return res; }
            false // step 3 resolved a valid path (even if index miss); not an escape
        } else {
            // Both step 2 and step 3 underflowed — target escapes the root.
            step2_escaped
        }
    } else {
        false
    };

    // Containment guard: an escaping explicit path must not fuzzy-resolve by basename.
    if escaped {
        return AssetResolution::NotFound;
    }

    // Step 4: fuzzy. Separator targets: try path-suffix then basename. Bare: basename.
    let basename = target.rsplit('/').next().unwrap_or(target);
    let mut matches = if has_separator(target) {
        let mut m = index.find_by_suffix(target);
        if m.is_empty() { m = index.find_by_suffix(basename); }
        m
    } else {
        index.find_by_suffix(basename)
    };
    matches.sort_by(|a, b| a.matches('/').count().cmp(&b.matches('/').count()).then(a.cmp(b)));
    match matches.len() {
        0 => AssetResolution::NotFound,
        1 => {
            let prov = if has_separator(target) {
                AssetProvenance::SeparatorFallback
            } else {
                AssetProvenance::BareFuzzy
            };
            AssetResolution::Resolved { root_rel: matches.remove(0), provenance: prov }
        }
        _ => AssetResolution::Ambiguous { chosen: matches[0].clone(), candidates: matches },
    }
}

/// Exact hit → Resolved(provenance); case-only hit → Resolved(CaseMismatch, canonical); else None.
fn finish_opt(cand: &str, prov: AssetProvenance, index: &dyn AssetIndex) -> Option<AssetResolution> {
    if index.contains(cand) {
        return Some(AssetResolution::Resolved { root_rel: cand.to_string(), provenance: prov });
    }
    if let Some(canon) = index.contains_ci(cand) {
        return Some(AssetResolution::Resolved { root_rel: canon, provenance: AssetProvenance::CaseMismatch });
    }
    None
}
fn finish(cand: String, prov: AssetProvenance, index: &dyn AssetIndex) -> AssetResolution {
    finish_opt(&cand, prov, index).unwrap_or(AssetResolution::NotFound)
}

/// Cross-module test fake for `AssetIndex`. Module-level so `reference.rs` tests can import it.
/// Logic mirrors the private `FakeIndex` inside `mod tests` below (duplication accepted —
/// see task comment; dedup can happen later without affecting behaviour).
#[cfg(test)]
pub(crate) struct FakeAssetIndex(std::collections::HashSet<String>);

#[cfg(test)]
impl FakeAssetIndex {
    pub fn new(paths: &[&str]) -> Self {
        FakeAssetIndex(paths.iter().map(|s| s.to_string()).collect())
    }
}

#[cfg(test)]
impl AssetIndex for FakeAssetIndex {
    fn contains(&self, p: &str) -> bool {
        self.0.contains(p)
    }
    fn contains_ci(&self, p: &str) -> Option<String> {
        let lp = p.to_lowercase();
        self.0.iter().find(|x| x.to_lowercase() == lp).cloned()
    }
    fn find_by_suffix(&self, s: &str) -> Vec<String> {
        let ls = s.to_lowercase();
        let mut v: Vec<String> = self.0.iter()
            .filter(|x| x.to_lowercase().ends_with(&ls)
                && (x.len() == s.len() || x.as_bytes()[x.len() - s.len() - 1] == b'/'))
            .cloned().collect();
        v.sort();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Fake index over a fixed real-case path set (mirrors link_class.rs::FakeIndex).
    struct FakeIndex(HashSet<String>);
    impl FakeIndex {
        fn new(paths: &[&str]) -> Self {
            FakeIndex(paths.iter().map(|s| s.to_string()).collect())
        }
    }
    impl AssetIndex for FakeIndex {
        fn contains(&self, p: &str) -> bool {
            self.0.contains(p)
        }
        fn contains_ci(&self, p: &str) -> Option<String> {
            let lp = p.to_lowercase();
            self.0.iter().find(|x| x.to_lowercase() == lp).cloned()
        }
        fn find_by_suffix(&self, s: &str) -> Vec<String> {
            let ls = s.to_lowercase();
            let mut v: Vec<String> = self.0.iter()
                .filter(|x| x.to_lowercase().ends_with(&ls)
                    && (x.len() == s.len() || x.as_bytes()[x.len() - s.len() - 1] == b'/'))
                .cloned().collect();
            v.sort();
            v
        }
    }

    #[test]
    fn literal_exact_relative_hit() {
        let idx = FakeIndex::new(&["assets/Hoon.JPG", "team/photo.jpg"]);
        // ./photo.jpg authored next to team/Team.md → exists at team/photo.jpg
        let r = resolve_asset_ref("./photo.jpg", "team/Team.md", &idx);
        assert_eq!(r, AssetResolution::Resolved {
            root_rel: "team/photo.jpg".into(), provenance: AssetProvenance::Literal });
    }
    #[test]
    fn separator_fallback_to_root() {
        // ./assets/AGU2025.jpg from a subfolder; real file at root assets/
        let idx = FakeIndex::new(&["assets/AGU2025.jpg"]);
        let r = resolve_asset_ref("./assets/AGU2025.jpg", "News/2025-12-agu.md", &idx);
        assert_eq!(r, AssetResolution::Resolved {
            root_rel: "assets/AGU2025.jpg".into(),
            provenance: AssetProvenance::SeparatorFallback });
    }
    #[test]
    fn case_mismatch_on_literal() {
        // ./assets/Hoon.jpg authored at root; disk is Hoon.JPG
        let idx = FakeIndex::new(&["assets/Hoon.JPG"]);
        let r = resolve_asset_ref("./assets/Hoon.jpg", "Team.md", &idx);
        assert_eq!(r, AssetResolution::Resolved {
            root_rel: "assets/Hoon.JPG".into(),  // canonical real case
            provenance: AssetProvenance::CaseMismatch });
    }
    #[test]
    fn bare_basename_fuzzy_silent() {
        // bare from subfolder; not adjacent; unique basename at root → BareFuzzy (no warn)
        let idx = FakeIndex::new(&["assets/AGU2025.jpg"]);
        let r = resolve_asset_ref("AGU2025.jpg", "News/post.md", &idx);
        assert_eq!(r, AssetResolution::Resolved {
            root_rel: "assets/AGU2025.jpg".into(), provenance: AssetProvenance::BareFuzzy });
    }
    #[test]
    fn bare_prefers_source_adjacent_sibling() {
        // R2: documented, TESTED behaviour — adjacent sibling wins over a root copy.
        let idx = FakeIndex::new(&["News/photo.jpg", "assets/photo.jpg"]);
        let r = resolve_asset_ref("photo.jpg", "News/post.md", &idx);
        assert_eq!(r, AssetResolution::Resolved {
            root_rel: "News/photo.jpg".into(), provenance: AssetProvenance::Literal });
    }
    #[test]
    fn ambiguous_picks_shortest_then_lexical() {
        let idx = FakeIndex::new(&["a/photo.jpg", "deep/dir/photo.jpg"]);
        let r = resolve_asset_ref("photo.jpg", "post.md", &idx);
        assert_eq!(r, AssetResolution::Ambiguous {
            chosen: "a/photo.jpg".into(),
            candidates: vec!["a/photo.jpg".into(), "deep/dir/photo.jpg".into()] });
    }
    #[test]
    fn absolute_path_resolves_from_root() {
        let idx = FakeIndex::new(&["assets/x.jpg"]);
        let r = resolve_asset_ref("/assets/x.jpg", "News/post.md", &idx);
        assert_eq!(r, AssetResolution::Resolved {
            root_rel: "assets/x.jpg".into(), provenance: AssetProvenance::Literal });
    }
    #[test]
    fn escapes_root_is_not_found() {
        let idx = FakeIndex::new(&["assets/x.jpg"]);
        // ../../etc from a depth-1 file escapes the project → NotFound (never resolve outside)
        assert_eq!(resolve_asset_ref("../../etc/x.jpg", "News/post.md", &idx), AssetResolution::NotFound);
    }
}
