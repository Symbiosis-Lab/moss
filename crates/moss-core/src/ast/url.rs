//! URL resolution state machine.
//!
//! Two-state machine: a URL is either author input (`Unresolved`) or has been
//! classified by the pipeline's resolver into a `ResolvedUrl` (`Resolved`).
//! The renderer's contract is that it never sees `Unresolved` — at HTML
//! emission time, every URL must be `Resolved`. `RenderHooks::render_image`
//! and `render_link` take `&ResolvedUrl` directly, so the bypass class is
//! statically unreachable: the only way to read a resolved href is to
//! destructure `Url::Resolved(r)`, which forces callers to handle the
//! `Url::Unresolved(_)` arm.
//!
//! # Why two states, not three
//!
//! moss-core's resolve pipeline (in [`crate::resolve`]) rewrites markdown
//! sources, replacing wikilinks `[[foo]]` with standard markdown links
//! `[foo](moss-resolved:foo.md)`. By the time the AST parser sees the
//! markdown, every URL is one of:
//!
//! - `moss-resolved:<path>` — pipeline output for an internal target
//! - external (`https://...`)
//! - anchor (`#section`)
//! - mailto / tel
//! - already-pretty internal URL
//!
//! All five are a `String` from the parser's view. The visitor's job is to
//! classify and rewrite these into a `ResolvedUrl{href, kind}`. There's no
//! useful intermediate state worth lifting into the type system.

use serde::{Deserialize, Serialize};

/// A URL inside the AST.
///
/// State machine: `Unresolved` → `Resolved`. The transition is performed
/// once per URL by [`crate::ast::visit::visit_urls_mut`]. The renderer's
/// signature accepts only `&ResolvedUrl`; the bypass class is statically
/// unreachable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Url {
    /// Author input as it appears in the parsed markdown source. May carry
    /// a `moss-resolved:` prefix from the upstream resolve pipeline.
    Unresolved(String),
    /// Classified, ready for rendering.
    Resolved(ResolvedUrl),
}

/// A classified URL with the final href and its kind.
///
/// `href` is the string the renderer puts in `href="..."`. `kind` informs
/// which extra HTML attributes the renderer adds (e.g., `class="wikilink"`
/// for `Wikilink`, `target="_blank" rel="noopener"` for `AssetNewtab`).
///
/// `kind` covers the cases the pre-AST pipeline encoded as three string
/// sentinels (`wikilink:`, `moss-newtab:`, bare URL).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedUrl {
    pub href: String,
    pub kind: UrlKind,
}

/// Classification of a resolved URL.
///
/// Drives `RenderHooks` decisions about extra HTML attributes:
///
/// | kind          | extra attributes / behavior                      |
/// |---------------|--------------------------------------------------|
/// | `Internal`    | none (already-pretty internal URL)               |
/// | `Wikilink`    | `class="wikilink"` (resolved internal markdown)  |
/// | `External`    | `target="_blank" rel="noopener"` (http/https)    |
/// | `AssetNewtab` | `target="_blank" rel="noopener"` (HTML/PDF asset)|
/// | `Asset`       | none (img/video src; no special attributes)      |
/// | `Anchor`      | none (in-page `#fragment`)                       |
/// | `Mailto`      | none                                             |
/// | `Tel`         | none                                             |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlKind {
    /// Internal URL that was already pretty (no `moss-resolved:` prefix).
    Internal,
    /// Internal URL that came from `[[wikilink]]` syntax. Carries
    /// `class="wikilink"` so hover-preview targeting works.
    Wikilink,
    /// External URL (`https://...`, etc).
    External,
    /// Internal asset that should open in a new tab (HTML, PDF, etc).
    /// Replaces the `moss-newtab:` sentinel of the pre-AST pipeline.
    AssetNewtab,
    /// Internal asset for `<img src>` / `<video src>` (image/video binary).
    Asset,
    /// In-page anchor (`#section-id`).
    Anchor,
    /// `mailto:user@example.com`
    Mailto,
    /// `tel:+1...`
    Tel,
}

impl Url {
    /// Construct an unresolved URL from author input.
    pub fn unresolved(input: impl Into<String>) -> Self {
        Url::Unresolved(input.into())
    }

    /// Construct a resolved URL.
    pub fn resolved(href: impl Into<String>, kind: UrlKind) -> Self {
        Url::Resolved(ResolvedUrl {
            href: href.into(),
            kind,
        })
    }

    /// True if the URL has not yet been classified.
    pub fn is_unresolved(&self) -> bool {
        matches!(self, Url::Unresolved(_))
    }

    /// True if the URL has been classified.
    pub fn is_resolved(&self) -> bool {
        matches!(self, Url::Resolved(_))
    }
}

impl ResolvedUrl {
    pub fn new(href: impl Into<String>, kind: UrlKind) -> Self {
        ResolvedUrl {
            href: href.into(),
            kind,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_construction_carries_input_verbatim() {
        let u = Url::unresolved("docs/");
        match u {
            Url::Unresolved(s) => assert_eq!(s, "docs/"),
            Url::Resolved(_) => panic!("expected Unresolved"),
        }
    }

    #[test]
    fn unresolved_preserves_moss_resolved_prefix() {
        // The resolve pipeline upstream emits this shape; the AST parser
        // sees it as opaque author-input until visit_urls_mut classifies it.
        let u = Url::unresolved("moss-resolved:docs/index.md");
        assert!(u.is_unresolved());
        match u {
            Url::Unresolved(s) => assert_eq!(s, "moss-resolved:docs/index.md"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn resolved_with_wikilink_kind() {
        let u = Url::resolved("../docs/", UrlKind::Wikilink);
        assert!(u.is_resolved());
        let Url::Resolved(r) = &u else {
            panic!("expected Resolved, got {u:?}")
        };
        assert_eq!(r.href, "../docs/");
        assert_eq!(r.kind, UrlKind::Wikilink);
    }

    #[test]
    fn resolved_kinds_are_distinct() {
        // Each kind carries different rendering implications; the enum is
        // a flat closed set.
        let kinds = [
            UrlKind::Internal,
            UrlKind::Wikilink,
            UrlKind::External,
            UrlKind::AssetNewtab,
            UrlKind::Asset,
            UrlKind::Anchor,
            UrlKind::Mailto,
            UrlKind::Tel,
        ];
        // Hash-set count == array length means all distinct.
        let unique: std::collections::HashSet<_> = kinds.iter().collect();
        assert_eq!(unique.len(), kinds.len());
    }

    #[test]
    fn state_distinction_via_is_methods() {
        let u = Url::unresolved("foo");
        assert!(u.is_unresolved());
        assert!(!u.is_resolved());

        let r = Url::resolved("foo", UrlKind::Internal);
        assert!(r.is_resolved());
        assert!(!r.is_unresolved());
    }

    #[test]
    fn serde_round_trip_unresolved() {
        let u = Url::unresolved("docs/");
        let s = serde_json::to_string(&u).expect("serialize");
        let back: Url = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(u, back);
    }

    #[test]
    fn serde_round_trip_resolved() {
        let u = Url::resolved("../docs/", UrlKind::Wikilink);
        let s = serde_json::to_string(&u).expect("serialize");
        let back: Url = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(u, back);
    }

    #[test]
    fn serde_uses_externally_tagged_form() {
        // Lock the wire format. External consumers (specta-bound TS, JSON
        // dumps for debugging) need a stable discriminant. The default
        // externally-tagged form `{"Unresolved":"foo"}` is fine; this test
        // exists so future serde-attribute changes are deliberate.
        let u = Url::unresolved("x");
        let s = serde_json::to_string(&u).expect("serialize");
        assert_eq!(s, r#"{"unresolved":"x"}"#);

        let r = Url::resolved("x", UrlKind::Internal);
        let s = serde_json::to_string(&r).expect("serialize");
        assert_eq!(s, r#"{"resolved":{"href":"x","kind":"internal"}}"#);
    }
}
