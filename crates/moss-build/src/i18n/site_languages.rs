//! Site-wide language entry points for the nav language switcher.
//!
//! One question, one answer: "what are the OTHER languages of THIS page,
//! and where is each?" — framed from the current page's point of view.
//!
//! Consolidates what used to be two divergent producers (per-article
//! `translations` vs. a verbatim copy of the root homepage's `translations`).
//! The homepage-copy fallback inherited the homepage's POV — on a site whose
//! root is Chinese, the root homepage lists "EN" as its alternate, so an
//! English page that borrowed that list showed "EN / EN". This module always
//! subtracts the current language and points each remaining language at its
//! real entry point. Mirrors the current-lang discipline in
//! `build_hreflang_link_tags` (page/meta.rs).
//!
//! NOTE — this is intentionally NOT the same set as hreflang alternates. The
//! switcher is a UX navigation affordance ("how do I reach this site in
//! another language?") and falls back to a language's homepage root when no
//! authored translation exists. hreflang alternates must be genuine
//! same-content translations, so they do NOT gain that fallback. The
//! divergence is by design (a homepage root is not a translation of an
//! article; emitting it as hreflang would be an invalid-hreflang SEO bug).
//!
//! That fallback is gated on the site actually publishing more than one
//! language edition — see `site_publishes_multiple_languages` in
//! `build::render::lang_roots`. A monolingual site can still hold pages whose
//! `lang` differs (it is content-detected when frontmatter is silent), and
//! there the fallback has nothing honest to offer.

use super::link::TranslationLink;

/// A language the site publishes, plus the URL of its homepage root.
/// Built from the language-homepage docs: `index.html` (site-default lang)
/// and `<code>/index.html` (each other language).
#[derive(Debug, Clone, PartialEq)]
pub struct LangRoot {
    /// The BCP-47 tag that root's own page emits as `<html lang>` — see
    /// [`TranslationLink::lang_tag`]. Labeled from the homepage doc itself,
    /// because language can come from frontmatter and need not match the
    /// directory name.
    pub lang_tag: String,
    /// url_path of that language's homepage (e.g. "index.html", "en/index.html").
    pub url_path: String,
}

/// Compute the links shown in the nav language switcher for one page.
///
/// Inputs:
/// - `current_lang`: the page's own language (rendered as `nav-lang-current`,
///   never returned as a link).
/// - `per_article`: this page's authored translations (`doc.translations`),
///   which point at real translated articles. May be empty.
/// - `lang_roots`: every language the site offers + its homepage root, derived
///   from the language-homepage docs.
/// - `site_publishes_multiple_languages`: whether the site really has more than
///   one language edition (see
///   `build::render::lang_roots::site_publishes_multiple_languages`). False on a
///   monolingual site, where a page's differing `lang` is a content-detection
///   artifact rather than a second edition.
///
/// Returns one `TranslationLink` per OTHER site language, sorted by
/// `lang.code()`. For each other language: prefer the authored per-article
/// translation; else fall back to that language's homepage root — the fallback
/// runs only when the site publishes multiple languages, since a monolingual
/// site has no other edition to point at. Languages with neither are omitted.
/// The current language is always excluded.
///
/// All entries from `per_article` and `lang_roots` are deduped by language.
/// The current language is excluded via the initial `seen.insert(current_lang)`,
/// so it is safe (and defensive) for `per_article` to contain a same-language
/// entry — it simply won't leak through.
pub fn other_language_links(
    current_lang_tag: &str,
    per_article: &[TranslationLink],
    lang_roots: &[LangRoot],
    site_publishes_multiple_languages: bool,
) -> Vec<TranslationLink> {
    use std::collections::HashSet;

    // Keyed by declared TAG, matching `site_lang_roots` and hreflang. On the
    // enum this set had three possible members, so a site's fourth edition was
    // indistinguishable from its first.
    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(current_lang_tag.to_string()); // current language is never a link — exclude once, here.
    let mut out: Vec<TranslationLink> = Vec::new();

    // 1) Authored per-article translations win (most specific destination).
    for t in per_article {
        if seen.insert(t.lang_tag.clone()) {
            out.push(t.clone());
        }
    }

    // 2) Fill any remaining site language with its homepage root — but ONLY on a
    //    site that actually publishes more than one language edition.
    //
    //    On a single-language site the fallback can only ever offer that one
    //    edition's own root, so handing it to a page whose text merely *detects*
    //    as another language promises a translation nobody wrote: the reader
    //    clicks "EN" and lands on the same site's homepage. Per-doc `lang` is
    //    content-detected when frontmatter is silent, so this fires on any
    //    ordinary monolingual site holding one page in another language — and
    //    it is symmetric, so flipping `site.lang` only moves the bogus toggle
    //    onto the other pages. Authored translations (step 1) are real
    //    destinations and are never gated.
    if site_publishes_multiple_languages {
        for r in lang_roots {
            if seen.insert(r.lang_tag.clone()) {
                out.push(TranslationLink {
                    lang_tag: r.lang_tag.clone(),
                    url_path: r.url_path.clone(),
                    display_name: crate::i18n::switcher_label(&r.lang_tag),
                });
            }
        }
    }

    // Deterministic, cross-platform-stable order (mirrors link.rs sort).
    out.sort_by(|a, b| a.lang_tag.cmp(&b.lang_tag));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(tag: &str, url: &str) -> LangRoot {
        LangRoot { lang_tag: tag.to_string(), url_path: url.to_string() }
    }
    fn link(tag: &str, url: &str) -> TranslationLink {
        TranslationLink { lang_tag: tag.to_string(), url_path: url.to_string(), display_name: crate::i18n::switcher_label(tag) }
    }

    // The bug: EN page, no authored ZH translation, site root is ZH.
    // Must show ZH (pointing at the ZH root "index.html"), and must NOT show EN.
    #[test]
    fn en_page_no_translation_shows_other_lang_root_not_self() {
        let roots = vec![
            root("zh-Hans", "index.html"), // site default = ZH at "/"
            root("en", "en/index.html"),  // EN at "/en/"
        ];
        let got = other_language_links("en", &[], &roots, true);
        assert_eq!(got.len(), 1, "exactly one other language");
        assert_eq!(got[0].lang_tag, "zh-Hans");
        assert_eq!(got[0].url_path, "index.html");
        assert!(got.iter().all(|l| l.lang_tag != "en"), "must never list the current language");
    }

    // Authored per-article translation wins over the homepage-root fallback.
    #[test]
    fn authored_translation_beats_homepage_root() {
        let roots = vec![
            root("zh-Hans", "index.html"),
            root("en", "en/index.html"),
        ];
        let per_article = vec![link("zh-Hans", "writing/shen-mo/index.html")];
        let got = other_language_links("en", &per_article, &roots, true);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].lang_tag, "zh-Hans");
        assert_eq!(got[0].url_path, "writing/shen-mo/index.html", "authored article URL, not the root");
    }

    // Current language is excluded even if it appears in lang_roots AND per_article.
    #[test]
    fn current_language_always_excluded() {
        let roots = vec![
            root("en", "en/index.html"),
            root("zh-Hans", "index.html"),
        ];
        // A stray self-link in per_article (defensive) must not leak through.
        let per_article = vec![link("en", "en/whatever/index.html")];
        let got = other_language_links("en", &per_article, &roots, true);
        assert!(got.iter().all(|l| l.lang_tag != "en"));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].lang_tag, "zh-Hans");
    }

    // Three languages: two others returned, sorted by code, dedup by language.
    #[test]
    fn three_languages_sorted_and_deduped() {
        let roots = vec![
            root("en", "en/index.html"),
            root("zh-Hans", "index.html"),
            root("zh-Hant", "zh-hant/index.html"),
        ];
        let got = other_language_links("en", &[], &roots, true);
        let langs: Vec<_> = got.iter().map(|l| l.lang_tag.as_str()).collect();
        // sorted by code(): "zh-hans" < "zh-hant"
        assert_eq!(langs, vec!["zh-Hans", "zh-Hant"]);
    }

    // Single-language site: no other languages → empty → switcher won't render.
    #[test]
    fn single_language_site_returns_empty() {
        let roots = vec![root("en", "index.html")];
        let got = other_language_links("en", &[], &roots, true);
        assert!(got.is_empty());
    }

    // Monolingual site, but the page's OWN language differs from the root's —
    // because `lang` is content-detected when frontmatter is silent. The site
    // publishes ONE edition, so the only thing the root fallback could offer is
    // that edition's own front door: the reader clicks "EN" and lands on the
    // same site's homepage. Regression: 刘兆永的网站 (site.lang="en", all-Chinese
    // content) put "简 / EN → /" on every article, and flipping site.lang to
    // zh-hans only moved the bogus toggle onto the site's one English essay.
    #[test]
    fn monolingual_site_offers_nothing_to_a_page_in_another_language() {
        let roots = vec![root("en", "index.html")];
        let got = other_language_links("zh-Hans", &[], &roots, false);
        assert!(
            got.is_empty(),
            "a monolingual site must not render a switcher, got {got:?}"
        );
    }

    // The gate is on the homepage-root FALLBACK only. An authored translation is
    // a real destination in a real other language, so it shows even when the
    // site-level flag is false — a defensive pairing, since in practice an
    // authored translation is itself what makes the site multilingual.
    //
    // The third root (ZhHant) is what makes this test bind BOTH halves: without
    // it the roots hold only the current language, so step 2 could contribute
    // nothing whether gated or not and deleting the gate would still pass.
    #[test]
    fn authored_translations_are_never_gated() {
        let roots = vec![root("en", "index.html"), root("zh-Hant", "zh-hant/index.html")];
        let per_article = vec![link("zh-Hans", "about.zh-hans/index.html")];
        let got = other_language_links("en", &per_article, &roots, false);
        assert_eq!(got.len(), 1, "authored translations survive the gate, root fallback does not");
        assert_eq!(got[0].lang_tag, "zh-Hans");
        assert_eq!(got[0].url_path, "about.zh-hans/index.html");
    }

    // A genuinely multilingual site can still have ONE root document — the
    // multilingual fixture is exactly this: `index.md` (en) plus a
    // filename-suffix `about.zh-hans.md`, whose `/zh-hans/` home is synthesized
    // and so never appears in `lang_roots`. That ZH home has no authored
    // translation of its own, so it depends on the root fallback to offer EN.
    #[test]
    fn multilingual_site_with_one_root_still_falls_back() {
        let roots = vec![root("en", "index.html")];
        let got = other_language_links("zh-Hans", &[], &roots, true);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].lang_tag, "en");
        assert_eq!(got[0].url_path, "index.html");
    }

    // No language roots at all (defensive): nothing to fall back to.
    #[test]
    fn no_lang_roots_returns_empty() {
        let got = other_language_links("zh-Hans", &[], &[], true);
        assert!(got.is_empty());
    }

    // display_name on a homepage-root fallback comes from the language itself.
    #[test]
    fn homepage_root_fallback_uses_language_display_name() {
        let roots = vec![
            root("zh-Hans", "index.html"),
            root("en", "en/index.html"),
        ];
        let got = other_language_links("en", &[], &roots, true);
        assert_eq!(got[0].display_name, "简");
    }
}
