//! Site language roots for the nav language switcher (render layer).
//!
//! Maps the language-homepage docs to `LangRoot`s: the site-default language's
//! homepage at `index.html`, and every other language `L` at
//! `<L.code()>/index.html`. Same structure `find_homepage_doc` (html.rs) and the
//! per-language cascade already rely on, so the switcher can't drift from the
//! rest of the build. Shared by `html.rs` and `blocking.rs`.

use crate::build::types::ParsedDocument;
use crate::i18n::Language;
use crate::i18n::site_languages::LangRoot;

/// True for a root-level homepage url_path: `index.html` or `<seg>/index.html`
/// where `<seg>` is a language tree (exactly one segment before index.html).
/// `<lang>/sub/index.html` is a sub-page, not a language root.
///
/// `pub(crate)` beyond this module because `render::incremental::verdict`
/// needs the same structural test: a homepage root's own `lang` feeds
/// [`site_lang_roots`]'s nav-switcher entry for every page site-wide, so a
/// global-invalidator digest over exactly this set of docs must not drift
/// from the set this function itself walks — see `lang_switcher_globals`.
pub(crate) fn is_language_root(url_path: &str) -> bool {
    if url_path == "index.html" {
        return true;
    }
    match url_path.strip_suffix("/index.html") {
        // Single segment (no further '/') AND a recognized language tree.
        Some(prefix) => {
            !prefix.contains('/') && moss_core::home::lang_tree_prefix(url_path).is_some()
        }
        None => false,
    }
}

/// Give every document that declared no language of its own the SITE's tag,
/// before anything reads `lang_tag`.
///
/// Three consumers each carried their own fallback and two of them disagreed:
/// a page fell back to the site tag for its own `<html lang>`, while every
/// sibling's link to that page fell back to the three-variant UI enum. On a
/// `fr` site the page said `<html lang="fr">` and was advertised as
/// `hreflang="en"` — the same contradiction, one layer down. Owning the
/// fallback in exactly one place is what closes it; downstream code reads the
/// field and never substitutes.
pub(crate) fn fill_missing_lang_tags(docs: &mut [ParsedDocument], site_lang: &str) {
    let site_tag = crate::i18n::lang_tag(site_lang);
    for doc in docs.iter_mut().filter(|d| d.lang_tag.is_none()) {
        doc.lang_tag = Some(site_tag.clone());
    }
}

/// The languages this site publishes, each paired with its homepage root URL.
///
/// A language-root homepage is identified STRUCTURALLY — its `url_path` is the
/// site root `index.html`, or a single-segment `<tree>/index.html` where
/// `<tree>` is a recognized language directory (`moss_core::home::lang_tree_prefix`).
/// Each root is labeled with the homepage doc's OWN resolved `lang`, never a
/// recomputed value: language can be set by frontmatter/filename and need not
/// match the directory name, so trusting the directory (or the `site_lang`
/// parameter) would mislabel the root. Dedup by language — first occurrence
/// wins. `site_lang` only orders the result (the site-default homepage first).
pub(crate) fn site_lang_roots(
    all_docs: &[ParsedDocument],
    site_lang: Language,
) -> Vec<LangRoot> {
    let mut roots: Vec<LangRoot> = Vec::new();
    for d in all_docs {
        // Dedup on the DECLARED TAG, not the interface enum. Keying on the enum
        // collapsed every language moss has no interface for onto `En`, so an
        // `en`+`fr`+`de` site minted ONE root, `site_publishes_multiple_languages`
        // then said no, and no switcher rendered at all — while hreflang, already
        // tag-keyed, advertised all three editions (the other half of that bug).
        if is_language_root(&d.url_path)
            && !roots.iter().any(|r| Some(&r.lang_tag) == d.lang_tag.as_ref())
        {
            roots.push(LangRoot {
                // `fill_missing_lang_tags` runs before rendering, so this is
                // the string the root's own page emits. The fallback is for an
                // entry point that forgets to fill: the site tag is wrong-ish,
                // `""` is an invalid `hreflang` that also makes every tag
                // compare equal in `same_language_edition`.
                lang_tag: d.lang_tag.clone()
                    .unwrap_or_else(|| site_lang.as_bcp47_attr().to_string()),
                url_path: d.url_path.clone(),
            });
        }
    }

    // Order: the site-default-language root first (it lives at "/"), then the
    // rest by lang code for determinism. Labeling already used each doc's own
    // lang, so this is purely presentation order.
    let site_tag = site_lang.as_bcp47_attr();
    roots.sort_by(|a, b| {
        let rank = |t: &str| if t == site_tag { 0 } else { 1 };
        rank(&a.lang_tag).cmp(&rank(&b.lang_tag)).then_with(|| a.lang_tag.cmp(&b.lang_tag))
    });
    roots
}

/// Whether this site actually **publishes** more than one language edition.
///
/// Gates the nav switcher's homepage-root fallback in
/// `i18n::site_languages::other_language_links`. Three independent pieces of
/// evidence, any one of which is enough:
///
/// 1. **Root documents in more than one language** — each edition authored its
///    own front door.
/// 2. **A public document has a translation counterpart in ANOTHER language** —
///    a `foo.zh-hans.md` beside `foo.md`, or a shared `translationKey:`. The
///    multilingual fixture is this shape: one root doc (`index.md`) but a
///    genuine second edition reachable at `/zh-hans/`, whose home is
///    synthesized and so never appears in `lang_roots`. Root count alone would
///    call that site monolingual and strip its switcher.
/// 3. **A public document lives in a `<code>/` tree, is written in the language
///    `<code>` names, and that language is not the site default** — the edition
///    exists as a whole subtree. `zh-hans/index.md` is optional: the build never
///    synthesizes a folder doc for a language tree (`blocking.rs`,
///    synthetic-index pass), so an authored-index-less tree contributes no root
///    and its pages group with nothing, satisfying neither 1 nor 2 — yet the
///    build still emits `/zh-hans/` and its readers still need a way back out.
///
/// None of the three holds on a single-language site, and that is the point.
/// Every comparison here reads the page's DECLARED tag (`lang_tag`), which
/// comes from frontmatter, filename suffix or language folder and from nothing
/// else. That is what keeps the gate honest: a page's `lang` — the interface
/// enum — is **content-detected** when frontmatter is silent, so reading it
/// here made an ordinary monolingual site sprout a second "language" the moment
/// one page's prose detected differently from `site.lang`, and the switcher
/// offered that page the site root as a translation nobody wrote.
///
/// Two deliberate narrowings remain:
///
/// - Evidence 2 compares the two tags. `build_translation_links` groups by
///   stem/`translationKey` with no language comparison, so a same-language pair
///   is one page's variants, not an edition.
/// - Evidence 2 and 3 both require `is_public_page()`: a draft or a `slot_only`
///   layout fragment is not something a reader can switch to.
///
/// A third narrowing is gone with the enum. Evidence 3 used to require the
/// folder's code to resolve through [`Language::from_code`], which knows six
/// codes against `lang_tree_prefix`'s ~51 — a guard against the folder name
/// coinciding with a detected language, and the reason a `de/`, `ja/`,
/// `en-us/` or `en-gb/` tree got no switcher at all. Comparing
/// declared tags needs no such guard: an `it/` folder on an all-Chinese site
/// declares `it` only if its pages actually say so.
///
/// One asymmetry falls out of the last narrowing and is deliberate. An
/// *entirely draft* language tree publishes no edition and gets no switcher,
/// while a draft translation *pair* keeps the drafts' links to each other —
/// because those are authored translations, which
/// `other_language_links` never gates. There is no authored link in the tree
/// case to be left ungated.
///
/// One known limit remains:
///
/// - Evidence 1 is deliberately not narrowed the same way. [`site_lang_roots`]
///   recognizes a root structurally and labels it with the document's OWN lang,
///   precisely so a `lang:` override inside a language folder survives (see this
///   module's other docstring and
///   `lang_folder_root_labeled_by_resolved_lang_not_directory`). Requiring
///   folder/language agreement there would discard that authoring. The cost is
///   that a look-alike folder WITH an index — `it/index.md` holding Chinese —
///   still mints a second root, and on a site whose homepage has no prose to
///   detect from the spurious switcher returns in full. Fixing that needs a
///   root's label to carry whether it was authored or guessed, rather than
///   a tighter folder test.
///
/// Takes `lang_roots` rather than recomputing it: every caller has already
/// called [`site_lang_roots`] to build the switcher's destinations.
pub(crate) fn site_publishes_multiple_languages(
    all_docs: &[ParsedDocument],
    lang_roots: &[LangRoot],
    site_lang: Language,
) -> bool {
    let site_tag = site_lang.as_bcp47_attr();
    // A page that declared nothing is a page in the site's own language.
    // `fill_missing_lang_tags` has normally already written exactly this.
    fn tag<'a>(d: &'a ParsedDocument, site_tag: &'a str) -> &'a str {
        d.lang_tag.as_deref().unwrap_or(site_tag)
    }
    lang_roots.len() > 1
        || all_docs.iter().any(|d| {
            d.is_public_page() && d.translations.iter().any(|t| t.lang_tag != tag(d, site_tag))
        })
        || all_docs.iter().any(|d| {
            d.is_public_page()
                && tag(d, site_tag) != site_tag
                && moss_core::home::lang_tree_prefix(&d.url_path)
                    .is_some_and(|p| crate::i18n::lang_tag(p) == tag(d, site_tag))
        })
}

/// Build-global inputs to the nav language switcher, the auto-generated
/// nav/footer link lists, and the subscribe-form language sections — the
/// site-wide, non-graph consumers of a page's OWN `lang` (an earlier audit
/// found `lang` "genuinely cross-page-visible" and stopped there; this closes the gap the
/// same way the listing-group model closed listing hosts' — by hashing the actual downstream
/// VALUE rather than the field that feeds it, so a page's `lang` need not
/// force a site-wide render just because it MIGHT move one of these).
///
/// Four keys, one per computation that reads across the whole corpus:
///
/// - `lang_roots`: [`site_lang_roots`]'s output. Moves when a homepage-root
///   doc's own `lang` changes, or a root appears/disappears (already covered
///   by `PathSetMoved`, but hashed here too for a cheap witness).
/// - `lang_multi`: [`site_publishes_multiple_languages`]'s boolean. Gates
///   whether the switcher's homepage-root fallback exists at all; its three
///   evidences (root count, an authored translation whose target disagrees
///   with the source's own lang, a public page inside a same-coded language
///   tree) each range over EVERY public document, not just homepage roots.
/// - `lang_email_sections`: `features::email::derive_language_sections`'s
///   output. A different site-wide consumer of a homepage's own `lang` (the
///   subscribe-form audience label for pages sharing the root's
///   `translationKey`), computed from an overlapping but not identical corpus.
/// - `nav_footer_lang_membership`: `(url_path, lang)` for every doc
///   `components::nav::generate_navigation`/`generate_footer` can select —
///   nav-bar-eligible docs (`is_nav_bar_item_doc`) and footer-eligible docs
///   (`footer == Some(true)`). Both filter the WHOLE corpus by
///   `doc.lang == effective_lang`/`doc.current_lang` on every render
///   (`components/nav.rs:183-190`, `:357-361`), so any OTHER document's lang
///   changing can add or remove itself from a nav bar or footer this page's
///   own facade never touches. Found in review after the first cut of this
///   fix only covered the switcher and the subscribe form.
///
/// Each is the FULL computed value, not a per-page approximation: an earlier
/// draft of this fix tried to name which pages "contribute" to `lang_multi`
/// and missed two of its three evidences (any public page in a matching
/// language tree, and any public page whose translations disagree — neither
/// scoped to homepage roots). Hashing what the consumer actually reads is the
/// same discipline `FacadeCache::asset_versions`'s doc comment states: "Hash
/// the fact," not one hop upstream of it.
///
/// A mismatch is a full-render bypass, same shape as `listing_globals` and for
/// the same reason: the switcher, the nav bar, the footer, and the subscribe
/// form are rendered into EVERY page's chrome, so there is no group narrower
/// than the whole site for any of them to invalidate at.
pub(crate) fn lang_switcher_globals(
    all_docs: &[ParsedDocument],
    site_lang: Language,
    has_content_folders: bool,
) -> std::collections::BTreeMap<String, String> {
    let roots = site_lang_roots(all_docs, site_lang);
    let multi = site_publishes_multiple_languages(all_docs, &roots, site_lang);
    let email_sections = crate::build::features::email::derive_language_sections(all_docs);
    let nav_footer_membership: Vec<(&str, Language)> = all_docs
        .iter()
        .filter(|doc| {
            crate::build::components::nav::is_nav_bar_item_doc(doc, has_content_folders)
                || doc.footer == Some(true)
        })
        .map(|doc| (doc.url_path.as_str(), doc.lang))
        .collect();
    [
        ("lang_roots", crate::build::facade::debug_hash(&roots)),
        ("lang_multi", crate::build::facade::debug_hash(&multi)),
        ("lang_email_sections", crate::build::facade::debug_hash(&email_sections)),
        ("nav_footer_lang_membership", crate::build::facade::debug_hash(&nav_footer_membership)),
    ]
    .into_iter()
    .map(|(name, digest)| (name.to_string(), digest))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A doc whose language was only DETECTED from its prose — the state the
    /// gate exists to refuse, so `lang_tag` stays `None`.
    fn doc_with_lang_url(lang: Language, url: &str) -> ParsedDocument {
        ParsedDocument { lang, url_path: url.to_string(), ..Default::default() }
    }

    /// A doc that DECLARED its language — frontmatter, filename suffix or
    /// language folder, the three rungs of `i18n::declared_lang_tag`. `lang` is
    /// what the interface resolves to alongside it, which for a language moss
    /// does not ship is the site default and never the declared tag.
    fn declaring(lang: Language, tag: &str, url: &str) -> ParsedDocument {
        ParsedDocument {
            lang,
            lang_tag: Some(tag.to_string()),
            url_path: url.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn site_lang_roots_from_homepages() {
        // index.html = site default (ZhHans); en/index.html = En.
        let docs = vec![
            declaring(Language::ZhHans, "zh-Hans", "index.html"),
            declaring(Language::En, "en", "en/index.html"),
            declaring(Language::En, "en", "en/writing/x/index.html"), // not a homepage
        ];
        let roots = site_lang_roots(&docs, Language::ZhHans);
        assert_eq!(roots.len(), 2);
        assert!(roots.iter().any(|r| r.lang_tag == "zh-Hans" && r.url_path == "index.html"));
        assert!(roots.iter().any(|r| r.lang_tag == "en" && r.url_path == "en/index.html"));
    }

    #[test]
    fn single_language_site_one_root() {
        let docs = vec![declaring(Language::En, "en", "index.html")];
        let roots = site_lang_roots(&docs, Language::En);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].lang_tag, "en");
    }

    #[test]
    fn root_index_uses_actual_doc_lang_not_site_lang_param() {
        // The root index.html is authored in ZhHans (e.g. via frontmatter),
        // even though the site_lang parameter passed in is En. The root must be
        // labeled with the DOC's language (ZhHans), never the site_lang param.
        let docs = vec![declaring(Language::ZhHans, "zh-Hans", "index.html")];
        let roots = site_lang_roots(&docs, Language::En);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].lang_tag, "zh-Hans", "root must use the doc's own lang");
        assert_eq!(roots[0].url_path, "index.html");
    }

    #[test]
    fn lang_folder_root_labeled_by_resolved_lang_not_directory() {
        // A doc lives in the zh-hant/ tree but resolved to ZhHans (e.g. a
        // frontmatter `lang: zh-hans` override). It is still recognized as a
        // language-root homepage (structural: one segment + known lang tree),
        // and labeled with its RESOLVED lang. It is NOT silently dropped.
        let docs = vec![
            declaring(Language::En, "en", "index.html"),          // site default
            declaring(Language::ZhHans, "zh-Hans", "zh-hant/index.html"), // mislabeled folder
        ];
        let roots = site_lang_roots(&docs, Language::En);
        // Both languages present; the zh-hant/ root is labeled ZhHans (resolved).
        assert_eq!(roots.len(), 2);
        let zh = roots.iter().find(|r| r.url_path == "zh-hant/index.html").unwrap();
        assert_eq!(zh.lang_tag, "zh-Hans", "labeled by resolved lang, not the directory");
    }

    #[test]
    fn non_language_prefix_folder_index_is_not_a_root() {
        // "writing/index.html" is a normal folder index, not a language root —
        // "writing" is not a recognized language tree, so it must be ignored.
        let docs = vec![
            declaring(Language::En, "en", "index.html"),
            declaring(Language::En, "en", "writing/index.html"),
        ];
        let roots = site_lang_roots(&docs, Language::En);
        assert_eq!(roots.len(), 1, "only the site root counts; 'writing/' is not a lang root");
        assert_eq!(roots[0].url_path, "index.html");
    }

    #[test]
    fn site_default_root_sorts_first() {
        // Ordering: the site-default-language root (at "/") comes first.
        let docs = vec![
            declaring(Language::En, "en", "en/index.html"),
            declaring(Language::ZhHans, "zh-Hans", "index.html"), // site default
        ];
        let roots = site_lang_roots(&docs, Language::ZhHans);
        assert_eq!(roots[0].lang_tag, "zh-Hans", "site-default root first");
        assert_eq!(roots[0].url_path, "index.html");
    }

    // ---- site_publishes_multiple_languages ---------------------------------

    fn translated(mut doc: ParsedDocument, to: Language, url: &str) -> ParsedDocument {
        doc.translations = vec![crate::i18n::link::TranslationLink {
            lang_tag: to.as_bcp47_attr().to_string(),
            url_path: url.to_string(),
            display_name: to.display_name(),
        }];
        doc
    }

    /// `site_publishes_multiple_languages` against the roots the same docs
    /// produce — the shape every call site uses.
    fn publishes_multiple(docs: &[ParsedDocument], site_lang: Language) -> bool {
        site_publishes_multiple_languages(docs, &site_lang_roots(docs, site_lang), site_lang)
    }

    // The 刘兆永的网站 shape: one root, no translations anywhere, but an article
    // whose prose detects as a different language than site.lang. Monolingual —
    // the switcher's root fallback must stay off.
    #[test]
    fn detected_lang_alone_does_not_make_a_site_multilingual() {
        let docs = vec![
            doc_with_lang_url(Language::En, "index.html"),
            doc_with_lang_url(Language::ZhHans, "writing/x/index.html"),
            doc_with_lang_url(Language::ZhHans, "writing/y/index.html"),
        ];
        assert!(!publishes_multiple(&docs, Language::En));
    }

    // Evidence 1: two language roots (`/` + `/en/`).
    #[test]
    fn two_language_roots_make_a_site_multilingual() {
        let docs = vec![
            declaring(Language::ZhHans, "zh-Hans", "index.html"),
            declaring(Language::En, "en", "en/index.html"),
        ];
        assert!(publishes_multiple(&docs, Language::ZhHans));
    }

    // Evidence 2: the multilingual-fixture shape — ONE root doc, but `about.md`
    // and `about.zh-hans.md` are linked as translations, so a second edition
    // really is published (and reachable at the synthesized `/zh-hans/`).
    #[test]
    fn authored_translation_makes_a_one_root_site_multilingual() {
        let docs = vec![
            doc_with_lang_url(Language::En, "index.html"),
            translated(
                doc_with_lang_url(Language::En, "about/index.html"),
                Language::ZhHans,
                "zh-hans/about/index.html",
            ),
        ];
        assert_eq!(site_lang_roots(&docs, Language::En).len(), 1, "only one root doc");
        assert!(publishes_multiple(&docs, Language::En));
    }

    // Evidence 2 must compare languages. `build_translation_links` groups by
    // stem / `translationKey` with NO language comparison, so two pages both
    // declaring Chinese cross-link each other. That is a variant of one page,
    // not a second edition — and on the monolingual site above it re-enabled
    // the root fallback for every unrelated page.
    #[test]
    fn same_language_translation_pair_is_not_a_second_edition() {
        let docs = vec![
            doc_with_lang_url(Language::En, "index.html"),
            doc_with_lang_url(Language::ZhHans, "writing/x/index.html"),
            translated(
                declaring(Language::ZhHans, "zh-Hans", "intro/index.html"),
                Language::ZhHans,
                "intro-2/index.html",
            ),
        ];
        assert!(!publishes_multiple(&docs, Language::En));
    }

    // Evidence 2 must ignore unpublished pages: a draft pair is work in
    // progress, and `slot_only` docs are layout chrome. Neither is a
    // destination a reader can switch to.
    #[test]
    fn draft_translation_pair_is_not_a_published_edition() {
        let mut draft = translated(
            doc_with_lang_url(Language::En, "about/index.html"),
            Language::ZhHans,
            "about/index.zh-hans.html",
        );
        draft.draft = Some(true);
        let docs = vec![doc_with_lang_url(Language::En, "index.html"), draft];
        assert!(!publishes_multiple(&docs, Language::En));
    }

    // Evidence 3: a `<code>/` tree with NO authored `<code>/index.md`. The
    // build declines to synthesize a folder doc for a language tree, so the
    // tree contributes no root (evidence 1) and its pages group with nothing
    // (evidence 2) — but `/zh-hans/` is still emitted and still needs a way
    // back to the root edition.
    #[test]
    fn lang_tree_without_an_authored_index_still_publishes_an_edition() {
        let docs = vec![
            doc_with_lang_url(Language::En, "index.html"),
            doc_with_lang_url(Language::En, "about/index.html"),
            declaring(Language::ZhHans, "zh-Hans", "zh-hans/about/index.html"),
            declaring(Language::ZhHans, "zh-Hans", "zh-hans/news/index.html"),
        ];
        assert_eq!(site_lang_roots(&docs, Language::En).len(), 1, "tree has no root doc");
        assert!(publishes_multiple(&docs, Language::En));
    }

    // Evidence 3 is about OTHER languages. A site whose only `<code>/` tree
    // holds the site's own language (`en/` on an English site) publishes one
    // edition filed under a redundant folder, not two.
    #[test]
    fn lang_tree_in_the_site_language_is_not_a_second_edition() {
        let docs = vec![
            doc_with_lang_url(Language::En, "index.html"),
            doc_with_lang_url(Language::En, "en/notes/index.html"),
        ];
        assert!(!publishes_multiple(&docs, Language::En));
    }

    // `zh-hant/` holding Traditional Chinese IS an edition. Guards against
    // "fix the false positive by deleting evidence 3". The false positive it
    // used to be paired with — an `it/` folder of Chinese prose — is now a real
    // edition, because the per-folder-language rung declares it and `<html lang>` has
    // said so ever since; `nav_lang_switcher_gate` pins that through the real
    // pipeline, which is the only layer that can see the declaration happen.
    #[test]
    fn lang_tree_matching_its_documents_language_is_an_edition() {
        let docs = vec![
            doc_with_lang_url(Language::En, "index.html"),
            declaring(Language::ZhHant, "zh-Hant", "zh-hant/關於/index.html"),
        ];
        assert!(publishes_multiple(&docs, Language::En));
    }

    // An index-less tree in a language moss ships no interface for is an
    // edition like any other. Routing evidence 3 through
    // `Language::from_code`, which knows six codes, silently excluded every
    // tree `lang_tree_prefix` accepts and `from_code` does not — `en-gb/`,
    // `de/`, `ja/` — so those readers got no switcher at all. Comparing
    // declared tags has no such ceiling.
    #[test]
    fn a_tree_in_a_language_moss_has_no_interface_for_is_still_an_edition() {
        for (folder, tag) in [("en-gb", "en-GB"), ("de", "de"), ("ja", "ja")] {
            let url = format!("{folder}/about/index.html");
            assert_eq!(Language::from_code(folder), None, "not one of the UI three");
            let docs = vec![
                doc_with_lang_url(Language::ZhHans, "index.html"),
                declaring(Language::ZhHans, tag, &url),
            ];
            assert!(publishes_multiple(&docs, Language::ZhHans), "`{folder}/` is an edition");
        }
    }

    // The same tree WITH an authored index is recognized — by evidence 1, which
    // is structural and does not consult `from_code`. This is the documented
    // workaround for the gap above, and it must keep working.
    #[test]
    fn region_tagged_english_tree_with_an_index_is_an_edition() {
        let docs = vec![
            declaring(Language::ZhHans, "zh-Hans", "index.html"),
            declaring(Language::En, "en-US", "en-us/index.html"),
        ];
        assert!(publishes_multiple(&docs, Language::ZhHans));
    }

    // Evidence 1 is NOT narrowed by folder/language agreement, so a look-alike
    // folder WITH an index still mints a second root. Characterizes the half of
    // the look-alike trap this gate does not close: on an all-Chinese site whose
    // homepage has no prose (root label falls through to `site.lang`), adding
    // `it/index.md` returns the spurious switcher. Closing it requires knowing a
    // root's label was authored rather than guessed. When that lands,
    // this assertion flips.
    #[test]
    fn look_alike_folder_with_an_index_still_mints_a_root() {
        let docs = vec![
            doc_with_lang_url(Language::En, "index.html"), // label came from site.lang
            doc_with_lang_url(Language::ZhHans, "it/index.html"), // Chinese prose in `it/`
            doc_with_lang_url(Language::ZhHans, "文学/index.html"),
        ];
        assert_eq!(site_lang_roots(&docs, Language::En).len(), 2, "structural root test");
        assert!(
            publishes_multiple(&docs, Language::En),
            "known gap: evidence 1 fires"
        );
    }

    // Nested lang folders are not editions: `lang_tree_prefix` inspects only the
    // first path component, so `notes/zh-hans/` is an ordinary subfolder.
    #[test]
    fn nested_lang_folder_is_not_a_top_level_edition() {
        let docs = vec![
            doc_with_lang_url(Language::En, "index.html"),
            doc_with_lang_url(Language::ZhHans, "notes/zh-hans/x/index.html"),
        ];
        assert!(!publishes_multiple(&docs, Language::En));
    }

    #[test]
    fn empty_site_is_not_multilingual() {
        assert!(!site_publishes_multiple_languages(&[], &[], Language::En));
    }
}
