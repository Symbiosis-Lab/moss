//! Canonical slot vocabulary for the moss build pipeline.
//!
//! moss's HTML templates contain named insertion points (slots) where the
//! build pipeline injects per-feature content (the email subscribe form,
//! analytics scripts, footer attribution, etc.). Slot names are referenced
//! both internally (via `slots.merge(..., name)` keys) and externally (via
//! the public `slot:` frontmatter field on author markdown files).
//!
//! ## Two slot surfaces — what this enum models
//!
//! moss has TWO related-but-distinct slot surfaces:
//!
//! 1. **Frontmatter-targetable slots** (this enum). The set of slot names
//!    a markdown author may write in their `slot:` frontmatter field. Modeled
//!    here, gated by `Slot::is_authorable()`.
//! 2. **Plugin-emit / template-marker slots** (the broader set). Every
//!    `<!-- slot:NAME -->` marker the template emits, including the four with
//!    no frontmatter contract (`after-title`, `before-article-end`,
//!    `after-article`, `body-end`).
//!
//! Both sets are this enum. Until 2026-08-17 the second lived in a separate
//! `SLOT_NAMES` const in the enhance module, and a third copy — with its own
//! prose for each position — sat in `cli/describe.rs`, each carrying a comment
//! saying it mirrored the others. Three lists of the same seven strings, kept
//! aligned by hand, is how a slot ends up recognized in one place and unknown
//! in another. [`Slot::ALL`] is now the only list; `is_authorable()` is what
//! separates the two surfaces, and [`Slot::position`] carries the prose
//! `moss describe` prints.
//!
//! ## Public contract surface (#508)
//!
//! Per `docs/reference/html-css-contract.md`, the `slot:` frontmatter
//! value space (the strings authors are allowed to write) is exactly the
//! set of `Slot::as_str()` outputs FOR variants where `is_authorable()`
//! returns true. Adding new variants is a minor-version contract addition;
//! renaming or removing one (or flipping a slot's authorability) is a
//! breaking change.
//!
//! `Slot::FooterRight` was removed 2026-05-06 — it had been a dead path
//! since the verbatim-footer refactor (commit 6e47a8024, 2026-04-30) which
//! dropped `<!-- slot:footer-right -->` from the template. Any author
//! writing `slot: footer-right` now gets an "unknown slot" warning instead
//! of a silent no-op.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    /// Last position before `</head>`. Used for stylesheets, scripts, meta.
    /// Producers: native code (analytics, beacon) and plugins. NOT
    /// author-targetable via frontmatter — head injection is a security
    /// boundary.
    HeadEnd,
    /// The single author-targetable footer slot. Filled by `footer.md` (or
    /// any file with `slot: footer-left` frontmatter); leads the footer
    /// chrome above the auto-generated link list.
    ///
    /// Naming note: the variant is `Footer` (one slot, no left/right pair),
    /// but `as_str()` returns `"footer-left"` to preserve the existing
    /// `slot: footer-left` frontmatter contract. The pre-verbatim era
    /// (commit 6e47a8024) had a `.footer-left` / `.footer-right` flex pair;
    /// the right half was retired (replaced by inline `:::subscribe`), and
    /// the left half survived as the only author-targetable footer slot.
    /// A future minor may deprecate `"footer-left"` in favor of `"footer"`
    /// with a migration cycle; that's not in this commit.
    Footer,
    /// Trailing footer position, after the auto-generated link list.
    /// Producers: native moss code (auto-injected subscribe form on
    /// moss-hosted sites without `footer.md`) and plugins via the enhance
    /// hook. Multiple producers compose by `slots.merge` priority.
    ///
    /// NOT author-targetable — markdown authors compose footer content
    /// via `footer.md` (which fills `Slot::Footer`) instead.
    FooterEnd,
    /// Inside `<article>`, after the title/date row. Article metadata — the
    /// review colophon, the book block. Article-only.
    AfterTitle,
    /// Inside `<article>`, before `</article>`. Article addenda. Article-only.
    BeforeArticleEnd,
    /// Between `</article>` and `</main>` — comments and reactions, which are
    /// *not* part of the article. Both content templates carry this marker, so
    /// it reaches folder-index and `layout: page` pages too (#1013); the other
    /// three article-area markers are article-only.
    AfterArticle,
    /// Before `</body>`. Scripts.
    BodyEnd,
}

impl Slot {
    /// Every slot moss emits a `<!-- slot:NAME -->` marker for, **in template
    /// order** — head, article, footer, body.
    ///
    /// The order is load-bearing for `inject_slots`, which walks this list to
    /// find and replace markers; it is the order the old `SLOT_NAMES` const
    /// used, preserved exactly.
    pub const ALL: [Slot; 7] = [
        Slot::HeadEnd,
        Slot::AfterTitle,
        Slot::BeforeArticleEnd,
        Slot::AfterArticle,
        Slot::Footer,
        Slot::FooterEnd,
        Slot::BodyEnd,
    ];

    /// Where this slot sits in the page, in the words `moss describe` prints.
    ///
    /// Part of the published contract surface, not a code comment: the strings
    /// land in `moss describe --json` and in `docs/reference/contract.md`.
    pub fn position(&self) -> &'static str {
        match self {
            Slot::HeadEnd => "Before </head> — for stylesheets, scripts, and meta tags.",
            Slot::AfterTitle => "Inside <article>, after the title/date row — for article metadata (e.g. book block, review colophon).",
            Slot::BeforeArticleEnd => "Inside <article>, before </article> — for article addenda.",
            Slot::AfterArticle => "Between </article> and </main> — for comments, reactions (NOT part of the article).",
            Slot::Footer => "Inside footer, leading position — filled by footer.md or any file with slot: footer-left frontmatter.",
            Slot::FooterEnd => "Inside footer, trailing position — for the auto-injected subscribe form and plugin widgets.",
            Slot::BodyEnd => "Before </body> — for scripts that must run after DOM is ready.",
        }
    }

    /// Stable string identifier. Used as the `slots.merge` key AND as the
    /// recognized frontmatter `slot:` value where applicable.
    ///
    /// Keep stable: changing one of these strings is a breaking change to
    /// the frontmatter / plugin-manifest contract.
    pub fn as_str(&self) -> &'static str {
        match self {
            Slot::HeadEnd => "head-end",
            // Variant is `Footer` but the string remains `footer-left` for
            // backward compatibility. See variant doc-comment for context.
            Slot::Footer => "footer-left",
            Slot::FooterEnd => "footer-end",
            Slot::AfterTitle => "after-title",
            Slot::BeforeArticleEnd => "before-article-end",
            Slot::AfterArticle => "after-article",
            Slot::BodyEnd => "body-end",
        }
    }

    /// Whether markdown authors may target this slot via the `slot:`
    /// frontmatter field. The single source of truth for the authorability
    /// classification — previously a const list in `build/footer.rs` that
    /// could drift from this enum.
    ///
    /// `true` only for slots whose injection is content-safe and whose
    /// position in the page makes sense as an author-composed unit. Head
    /// and trailing-widget slots are reserved for plugin and native
    /// producers (sanitization boundaries / native-injection contracts).
    ///
    /// Scope: this is the **frontmatter** gate only. Plugin emits go
    /// through `crate::build::enhance` with a separate trust model and
    /// legitimately target `head-end` / `footer-end`; do NOT use
    /// `is_authorable` to gate plugin enhance-hook output.
    pub fn is_authorable(&self) -> bool {
        match self {
            Slot::Footer => true,
            Slot::HeadEnd
            | Slot::FooterEnd
            | Slot::AfterTitle
            | Slot::BeforeArticleEnd
            | Slot::AfterArticle
            | Slot::BodyEnd => false,
        }
    }

    /// Parse a string into a Slot, or `None` if unrecognized.
    ///
    /// `from_str` recognizes every variant for round-trip purposes. The
    /// frontmatter validator additionally gates on `is_authorable()` (see
    /// `crate::build::footer::collect_footer_slots_by_language`); the head and
    /// trailing-widget slots parse here but are rejected at the
    /// frontmatter boundary.
    pub fn from_str(s: &str) -> Option<Slot> {
        Slot::ALL.into_iter().find(|slot| slot.as_str() == s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_round_trip() {
        for slot in Slot::ALL {
            assert_eq!(Slot::from_str(slot.as_str()), Some(slot));
        }
    }

    #[test]
    fn test_slot_unknown_returns_none() {
        assert_eq!(Slot::from_str("footer-center"), None);
        assert_eq!(Slot::from_str("sidebar-left"), None);
        assert_eq!(Slot::from_str(""), None);
    }

    #[test]
    fn test_footer_right_no_longer_recognized() {
        // Removed 2026-05-06 — had been dead since 2026-04-30 verbatim-footer
        // refactor. Author writing `slot: footer-right` now triggers the
        // unknown-slot warning in collect_footer_slots_by_language.
        assert_eq!(Slot::from_str("footer-right"), None);
    }

    #[test]
    fn test_authorability_per_variant() {
        // Only `Slot::Footer` is author-targetable; `HeadEnd` and
        // `FooterEnd` are reserved for plugin / native producers. If this
        // test starts failing, someone changed `is_authorable()` — make
        // sure that's deliberate (it's a public-API contract change).
        assert!(Slot::Footer.is_authorable());
        for slot in Slot::ALL.into_iter().filter(|s| *s != Slot::Footer) {
            assert!(!slot.is_authorable(), "{} must not be authorable", slot.as_str());
        }
    }

    #[test]
    fn all_lists_every_marker_in_template_order() {
        // `inject_slots` walks ALL, and `moss describe` prints it. Both used to
        // read their own copy of this list.
        assert_eq!(
            Slot::ALL.map(|s| s.as_str()),
            [
                "head-end",
                "after-title",
                "before-article-end",
                "after-article",
                "footer-left",
                "footer-end",
                "body-end",
            ]
        );
    }

    #[test]
    fn test_footer_slot_string_preserves_legacy_name() {
        // Variant rename (2026-05-06) was Rust-internal only — the public
        // string remains `footer-left` so existing frontmatter and plugin
        // manifests continue to work. If this test starts failing, the
        // string was changed — that's a breaking-change contract bump and
        // needs a migration plan, not just a test edit.
        assert_eq!(Slot::Footer.as_str(), "footer-left");
    }
}
