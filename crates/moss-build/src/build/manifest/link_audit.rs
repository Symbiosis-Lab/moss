//! Root-relative links this build's own output cannot satisfy.
//!
//! A `<a href="/authors/ling/">` is unambiguous: no base to guess, nothing to
//! fetch. Either the build wrote something at that path or it did not, and moss
//! holds the complete list of what it wrote by the time the manifest seals. So
//! the check is a set difference, and until moss#1187 it was simply never made
//! — a term-page namespace rename (`/author/` → `/authors/`, same day, one
//! release apart) turned 36 already-authored links on a live site into 404s and
//! the build said `Build complete` and nothing else.
//!
//! # What counts as satisfied
//!
//! Three answers, cheapest first, and a reference needs only one of them:
//!
//! 1. **A manifest key**, exact or as `<path>/index.html` for a directory URL.
//!    Redirect stubs are in here too — `feeds::redirects::emit_redirect_stubs`
//!    writes a real meta-refresh file through the same `emit`, so a link to a
//!    renamed page's OLD url resolves against the stub and this module needs no
//!    knowledge of `.moss/data/redirects.json` at all.
//! 2. **A file in the stage**, for the same candidates. The manifest is meant to
//!    cover everything moss ships, but a passthrough root is a directory moss
//!    copies rather than renders, and an advisory that cries wolf gets muted —
//!    so presence on disk settles it regardless of registration.
//! 3. Otherwise it is reported.
//!
//! Percent-decoding runs on the href. moss emits a CJK slug percent-encoded
//! from one code path and literal from another, and both name the same file —
//! whose manifest key is always the literal one, because `ServedPath` is built
//! from a source path and only lowercases directory segments. So decoding the
//! reference is what makes the two encodings meet.
//!
//! # Advisory, not a `--strict` failure
//!
//! Reported with a plain [`log::warn!`], which the headless logger prints but
//! `cli_output`'s problem counter never sees. An unresolved `[[wikilink]]` is
//! unambiguously the author's mistake — moss owns the whole resolution — and so
//! it earns `log_warn_problem!`. A root-relative href does not have that
//! property: a site can legitimately name a path served by a proxy, a redirect
//! rule, or a host in front of moss, and none of those are visible from here.
//! Counting it would fail an existing `--strict` CI on upgrade for a site that
//! is not broken, which is a worse outcome than the miss this closes. Promoting
//! it later is a one-word change once the field tells us the false-positive rate.
//!
//! One exception, added 2026-09-16: [`dead_links_among_promises`] scopes this
//! audit's output down to the references THIS build itself made and has not
//! yet kept — a video (or its poster) dispatched to a background encode
//! before the page that embeds it was sealed. That subset is not a maybe;
//! moss knows for certain it will produce the file, on this same folder, from
//! this same build. `deploy::refuse_publish` refuses on it, the same way it
//! already refuses on missing media — closing the window where a publish
//! could land between a generation's seal and the follow-up rebuild that
//! completes it, shipping a live page with a dead `<video>`. Every other dead
//! link this module finds stays exactly as advisory as before.

use std::collections::HashSet;
use std::path::Path;

use super::SealedManifest;
use crate::build::served_path::ServedPath;

/// Attribute names whose value can be a root-relative URL. Held as data rather
/// than written into the scan loop so ratchet row (p) — which reads a literal
/// `href=\"` inside a `find`/`split` argument as an HTML string-mutation pass —
/// does not count a read-only audit as one.
///
/// `poster=` joined the other two 2026-09-16: `moss_core::render::video`
/// emits it unconditionally (the still frame shown before a `<video>`'s real
/// source lands), derived from the same source path as `src=` via the same
/// `to_thumb`/`to_mp4` pair — so a video still mid-encode leaves exactly the
/// same kind of unsatisfied reference in `poster=` that it does in `src=`,
/// and until now this module could not see it.
const URL_ATTRS: [&str; 3] = ["href=", "src=", "poster="];

/// One root-relative reference whose target the build did not write.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeadLink {
    /// Manifest key of the page carrying it, e.g. `about/fellows/index.html`.
    pub page: String,
    /// The attribute value verbatim, so the author can grep their source for it.
    pub href: String,
}

/// Manifest keys a root-relative reference could name, or `None` when the
/// reference is not this module's business.
///
/// `None` covers everything that is not a root-relative path: a scheme
/// (`https:`, `mailto:`, `data:`, `moss-source:`), a protocol-relative `//host`,
/// an anchor, a relative path — plus a decoded path escaping the site root,
/// which is a reference no output tree can satisfy and no author wrote on
/// purpose.
///
/// Both forms are always returned for a directory-shaped URL, because moss
/// writes `slug/index.html` and authors write `/slug/`, `/slug`, and
/// `/slug/index.html` interchangeably.
pub fn candidate_keys(href: &str) -> Option<Vec<String>> {
    let path = href.split(['?', '#']).next().unwrap_or("");
    if !path.starts_with('/') || path.starts_with("//") {
        return None;
    }
    let decoded = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .ok()?;
    let trimmed = decoded.trim_start_matches('/');
    if trimmed.split('/').any(|seg| seg == ".." || seg == ".") {
        return None;
    }
    let mut keys = Vec::with_capacity(2);
    if !trimmed.is_empty() {
        keys.push(trimmed.to_string());
    }
    let dir = trimmed.trim_end_matches('/');
    keys.push(if dir.is_empty() {
        "index.html".to_string()
    } else {
        format!("{dir}/index.html")
    });
    keys.dedup();
    Some(keys)
}

/// Every `href`/`src` attribute value in `html`, in document order.
///
/// A substring scan, not a parse. The alternative is a second HTML parser in a
/// tree that has fought to keep one markdown parser (ADR-036), for a check whose
/// worst failure mode is a missing advisory. The `=` must follow the attribute
/// name immediately and the name must be preceded by whitespace, so `data-src`,
/// `xlink:href` and `srcset` are all left alone.
pub fn url_attr_values(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for attr in URL_ATTRS {
        let mut rest = html;
        while let Some(pos) = rest.find(attr) {
            let (before, after) = rest.split_at(pos);
            rest = &after[attr.len()..];
            let preceded_by_space = before
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_whitespace());
            if !preceded_by_space {
                continue;
            }
            let quote = match rest.chars().next() {
                Some(q @ ('"' | '\'')) => q,
                _ => continue,
            };
            rest = &rest[1..];
            let Some(end) = rest.find(quote) else { break };
            out.push(rest[..end].to_string());
            rest = &rest[end..];
        }
    }
    out
}

/// Answer 2 of the module docs: a passthrough root's files reach the stage
/// without necessarily reaching the manifest, so disk gets the last word.
/// Routed through [`ServedPath`] rather than a bare `join` so a candidate that
/// is not a legal served path cannot address anything outside `stage_dir`.
fn present_in_stage(stage_dir: &Path, key: &str) -> bool {
    ServedPath::from_source(key)
        .is_ok_and(|sp| crate::build::io_utils::output_present(&sp.to_disk(stage_dir)))
}

/// Root-relative references in `page`'s HTML that nothing in `sealed` or
/// `stage_dir` satisfies. Deduplicated: 36 copies of one broken href on one page
/// is one thing to fix, and printing it 36 times buries the other 35 problems.
pub fn dead_links_in_page(
    page: &str,
    html: &str,
    satisfied: &HashSet<&str>,
    stage_dir: &Path,
) -> Vec<DeadLink> {
    let mut seen = HashSet::new();
    let mut dead = Vec::new();
    for href in url_attr_values(html) {
        let Some(keys) = candidate_keys(&href) else { continue };
        if keys.iter().any(|k| satisfied.contains(k.as_str())) {
            continue;
        }
        if keys.iter().any(|k| present_in_stage(stage_dir, k)) {
            continue;
        }
        if seen.insert(href.clone()) {
            dead.push(DeadLink { page: page.to_string(), href });
        }
    }
    dead
}

/// Audit every HTML page the manifest names, reading it back from `stage_dir`.
///
/// The page list comes from the manifest, not a directory walk: those keys are
/// exactly what this build shipped, so nothing left over from an earlier
/// generation is scanned and no tree traversal is added to the seal tail.
/// A page that cannot be read (an eviction between write and audit) is skipped
/// — the alternative is inventing broken links out of an I/O error.
pub fn audit(stage_dir: &Path, sealed: &SealedManifest) -> Vec<DeadLink> {
    let satisfied: HashSet<&str> = sealed.files().keys().map(String::as_str).collect();
    let mut pages: Vec<&&str> = satisfied.iter().filter(|k| k.ends_with(".html")).collect();
    pages.sort();
    let mut dead = Vec::new();
    for page in pages {
        let Ok(html) = std::fs::read_to_string(stage_dir.join(page)) else { continue };
        dead.extend(dead_links_in_page(page, &html, &satisfied, stage_dir));
    }
    dead
}

/// Run the audit and say what it found. The entry point the seal tail calls.
/// Returns the full list (still advisory) so the caller can additionally
/// scope it down to this build's own unfulfilled promises — see
/// [`dead_links_among_promises`].
pub fn audit_and_report(stage_dir: &Path, sealed: &SealedManifest) -> Vec<DeadLink> {
    let dead = audit(stage_dir, sealed);
    for link in &dead {
        // `log::warn!`, not `log_warn_problem!` — see the module docs on why
        // this stays out of the `--strict` count.
        log::warn!(
            "Dead link: '{}' in '{}' — this build wrote no page or asset there",
            link.href,
            link.page
        );
    }
    dead
}

/// Of `dead`, the ones a key in `promised` would resolve — this build's own
/// still-pending promise (a video, or its poster, the render phase already
/// referenced but whose encode has not landed) rather than a pre-existing
/// broken link. `promised` is `AssetRegistry::pending_keys()` plus, for each
/// pending video, its derived poster key (posters are never registry-tracked
/// — see `DeliveryKind::registry_tracked` in `build/media/video.rs`).
///
/// This is the one subset the publish gate refuses on (`deploy::
/// refuse_publish`); everything else `audit` finds stays advisory-only,
/// exactly as the module docs describe — a stale link to a deleted page, an
/// external host, a deliberately unbuilt draft, or an optional variant the
/// site never emits all resolve to no promise and pass through untouched.
pub fn dead_links_among_promises(dead: &[DeadLink], promised: &HashSet<String>) -> Vec<DeadLink> {
    dead.iter()
        .filter(|link| {
            candidate_keys(&link.href).is_some_and(|keys| keys.iter().any(|k| promised.contains(k)))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "link_audit_tests.rs"]
mod tests;
