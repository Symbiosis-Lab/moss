use super::*;
use crate::build::page::shell::tests::{blanked, split_selector_list};
use std::collections::BTreeSet;

/// The `.class` and `[attr…]` tokens a selector is built from.
///
/// These are the parts that *narrow* a selector to specific elements. Bare
/// type selectors (`body`, `h2`) and pseudo-classes are deliberately excluded:
/// they match elements core also styles, which is the thing an owned token
/// must rule out.
fn selector_tokens(selector: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let chars: Vec<char> = selector.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '[' => {
                let start = i;
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
                out.insert(chars[start..=i.min(chars.len() - 1)].iter().collect());
                i += 1;
            }
            '.' => {
                let start = i;
                i += 1;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || chars[i] == '-' || chars[i] == '_')
                {
                    i += 1;
                }
                out.insert(chars[start..i].iter().collect());
            }
            _ => i += 1,
        }
    }
    out
}

/// True when two property names could shadow each other in the cascade.
///
/// Not string equality: `margin` and `margin-top` are different names that
/// set overlapping state, so `.x { margin: 0 }` in core and
/// `.x { margin-top: 1px }` in a partial are NOT order-independent even
/// though the names differ. The same holds for `border`/`border-left-color`,
/// `background`/`background-color`, `font`/`font-size`, `inset`/`top`, and
/// `all`/anything. Treating a `-`-separated prefix relationship as a clash
/// covers the shorthand families without a hand-maintained table; `all` is
/// the one shorthand whose longhands don't share its name, so it is special-
/// cased.
fn properties_can_shadow(a: &str, b: &str) -> bool {
    if a == b || a == "all" || b == "all" {
        return true;
    }
    let prefix_of = |short: &str, long: &str| {
        long.len() > short.len()
            && long.starts_with(short)
            && long.as_bytes()[short.len()] == b'-'
            // Custom properties are a flat namespace: `--callout-note` does
            // not shadow `--callout-note-bg`, they are simply two variables.
            && !short.starts_with("--")
    };
    prefix_of(a, b) || prefix_of(b, a)
}

/// Every `(selector, property)` pair in `mine` that could shadow, or be
/// shadowed by, a pair in `theirs`.
fn shadowing_pairs<'a>(
    mine: &'a BTreeSet<(String, String)>,
    theirs: &BTreeSet<(String, String)>,
) -> Vec<&'a (String, String)> {
    mine.iter()
        .filter(|(sel, prop)| {
            theirs
                .iter()
                .any(|(osel, oprop)| sel == osel && properties_can_shadow(prop, oprop))
        })
        .collect()
}

/// Extract `(selector, property)` pairs from a stylesheet.
///
/// **Pairs, not bare selectors.** Reordering two rules changes the rendered
/// result only if they match the same element *and* set the same property.
/// `callouts.css` is the case that proves it: it opens a `:root` block, as
/// core does, but declares only `--callout-*` custom properties, so no
/// declaration in either block can shadow one in the other and the two are
/// order-independent despite sharing a selector.
///
/// Deliberately crude and deliberately *over*-collecting — an at-rule prelude
/// or keyframe stop wrongly read as a selector can only make
/// `partials_are_order_independent` stricter. Under-collecting *properties*
/// would be the unsafe direction, so every `name:` before a `;` or `}` counts.
fn declarations(css: &str) -> BTreeSet<(String, String)> {
    let cleaned = blanked(css);
    let mut out = BTreeSet::new();
    // Preludes of the currently-open blocks, innermost last.
    let mut stack: Vec<String> = Vec::new();
    let mut buf = String::new();

    let normalize = |s: &str| -> String { s.split_whitespace().collect::<Vec<_>>().join(" ") };

    let flush_declaration = |buf: &str, stack: &[String], out: &mut BTreeSet<_>| {
        let text = buf.trim();
        let Some((prop, _)) = text.split_once(':') else {
            return;
        };
        let prop = prop.trim();
        if prop.is_empty() || prop.contains('{') {
            return;
        }
        // Attribute the declaration to the innermost prelude that is a
        // selector list — `@media`/`@supports` wrap selectors, never the
        // reverse.
        let Some(prelude) = stack.iter().rev().find(|p| !p.starts_with('@')) else {
            return;
        };
        for sel in split_selector_list(prelude) {
            let sel = normalize(&sel);
            let is_keyframe_stop = sel.ends_with('%') || sel == "from" || sel == "to";
            if !sel.is_empty() && !is_keyframe_stop {
                out.insert((sel, prop.to_string()));
            }
        }
    };

    // `@keyframes` names, recorded separately. A keyframes block has no
    // selectors — its `0%`/`to` stops are timeline positions, dropped above —
    // so without this it would contribute nothing and a partial could silently
    // redefine a core animation (`@keyframes spin`) and change how core's own
    // elements move. Recorded as a synthetic pair so the ownership rule can
    // ask the only question that matters for a keyframes block: is the NAME
    // one core uses? Elements are reached only via `animation-name`, and the
    // rule that sets it is itself ownership-checked.
    let mut keyframes: Vec<String> = Vec::new();

    for ch in cleaned.chars() {
        match ch {
            '{' => {
                let prelude = normalize(&buf);
                if let Some(rest) = prelude.strip_prefix("@keyframes ") {
                    keyframes.push(rest.trim().to_string());
                }
                stack.push(prelude);
                buf.clear();
            }
            '}' => {
                flush_declaration(&buf, &stack, &mut out);
                buf.clear();
                stack.pop();
            }
            ';' => {
                flush_declaration(&buf, &stack, &mut out);
                buf.clear();
            }
            _ => buf.push(ch),
        }
    }
    for name in keyframes {
        out.insert((format!("@keyframes {name}"), KEYFRAMES_PROP.to_string()));
    }
    out
}

/// Synthetic property marking a `@keyframes` row from [`declarations`].
const KEYFRAMES_PROP: &str = "@keyframes";

fn partials_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/assets/css/site")
}

/// Totality, both directions: a partial cannot be dropped in `assets/css/site/`
/// and silently never shipped, nor listed in the table with no file behind it
/// (the second half is enforced by `include_str!` at compile time, so this
/// asserts only that the row's `name` matches the file it embeds).
#[test]
fn every_partial_file_has_a_row() {
    let on_disk: BTreeSet<String> = std::fs::read_dir(partials_dir())
        .expect("assets/css/site/ must exist")
        .map(|e| e.expect("readable dir entry").file_name())
        .filter_map(|n| n.to_str().and_then(|s| s.strip_suffix(".css")).map(String::from))
        .collect();
    let in_table: BTreeSet<String> = CSS_PARTIALS.iter().map(|p| p.name.to_string()).collect();
    assert_eq!(
        on_disk, in_table,
        "assets/css/site/*.css and CSS_PARTIALS disagree; a partial with no row is never shipped"
    );
}

/// The gate for adding a partial: **a partial may only style elements it
/// owns.**
///
/// Concretely, every rule a partial declares must satisfy one of:
///
/// 1. its selector carries a `.class` or `[attr]` token that appears in NO
///    core selector — so the rule can only match elements that are marked as
///    belonging to this feature; or
/// 2. it declares only custom properties whose names core never declares — a
///    variable definition shadows nothing.
///
/// Under (1) or (2) the partial cannot change the winner of any cascade
/// contest core participates in, so appending it after core is equivalent to
/// leaving its rules where they sat inside `@layer shortcodes`.
///
/// **Why ownership rather than selector-string disjointness.** An earlier
/// version compared `(selector, property)` pairs for equality. That is unsound
/// in the dangerous direction: selector *strings* are not element *sets*, so
/// core's `:is(h1, h2, h3) { margin-top }` and a partial's
/// `h2 { margin-top }` compare as disjoint while fighting over the same
/// elements. `.a .b` vs `.b`, `*`, and `:where()` are the same hole. Only a
/// real selector engine could decide overlap in general — ownership sidesteps
/// the question by requiring a marker core has never heard of.
///
/// **`@keyframes` is checked by name.** Its stops (`0%`, `to`) are timeline
/// positions, not selectors, so the element test cannot apply. A partial that
/// redefined core's `@keyframes spin` would change how core's elements move
/// while sharing a selector with none of them — so a keyframes row is owned
/// iff core defines no animation of that name.
///
/// This rule is what made the `Site search` extraction safe rather than
/// assumed. The section's comment header spanned 1469–1975, but search's own
/// rules end at 1706; everything after — `.theme-toggle-icon`,
/// `.github-link`, `.mobile-menu-button`, the reading-font control — is
/// unrelated nav chrome that had merely been filed under the wrong heading.
/// Splitting at the real boundary leaves a partial that owns every element it
/// touches; splitting at the comment would have moved core's nav rules.
#[test]
fn a_partial_only_styles_elements_it_owns() {
    let core = declarations(CORE_CSS);
    let core_tokens: BTreeSet<String> = core
        .iter()
        .flat_map(|(sel, _)| selector_tokens(sel))
        .collect();
    let core_custom_props: BTreeSet<&String> = core
        .iter()
        .filter(|(_, prop)| prop.starts_with("--"))
        .map(|(_, prop)| prop)
        .collect();

    // MARK_CSS is not a `CssPartial` — it has no gate and is appended
    // unconditionally — but `assemble` appends it exactly the way a partial is
    // appended, after core and into the same layer, so it owes the same
    // guarantee. Iterated alongside rather than made a partial: a partial
    // carries its own `@layer` block, and the launcher `@import`s this same
    // file into a bundle that has no layers at all.
    let appended: Vec<(&str, &str)> = CSS_PARTIALS
        .iter()
        .map(|p| (p.name, p.source))
        .chain(std::iter::once(("mark", MARK_CSS)))
        .collect();

    for (name, source) in appended {
        let mine = declarations(source);
        assert!(
            !mine.is_empty(),
            "partial '{name}' contributes no declarations — is it wired to the wrong file?"
        );
        let unowned: Vec<&(String, String)> = mine
            .iter()
            .filter(|(sel, prop)| {
                if prop == KEYFRAMES_PROP {
                    // A keyframes block is owned iff core does not define an
                    // animation of that name. Redefining core's would change
                    // how core's own elements move without sharing a selector
                    // with any of them.
                    return core.contains(&((*sel).clone(), (*prop).clone()));
                }
                let owns_element = selector_tokens(sel).iter().any(|t| !core_tokens.contains(t));
                let owns_variable = prop.starts_with("--") && !core_custom_props.contains(prop);
                !owns_element && !owns_variable
            })
            .collect();
        assert!(
            unowned.is_empty(),
            "partial '{name}' styles elements core also styles, so appending it can change \
             which rule wins. Each entry is a (selector, property) whose selector carries \
             no token unique to this partial: {:?}",
            &unowned[..unowned.len().min(10)]
        );
    }
}

/// Two partials must not fight each other either. Weaker than the core rule
/// above — partials are appended in table order, and only a shared selector
/// with shadowing properties can make that order matter.
#[test]
fn partials_do_not_shadow_each_other() {
    let mut seen: Vec<(&str, BTreeSet<(String, String)>)> = Vec::new();
    for partial in CSS_PARTIALS {
        let mine = declarations(partial.source);
        for (other_name, other) in &seen {
            let clash = shadowing_pairs(&mine, other);
            assert!(
                clash.is_empty(),
                "partials '{}' and '{}' declare shadowing properties: {:?}",
                partial.name,
                other_name,
                &clash[..clash.len().min(10)]
            );
        }
        seen.push((partial.name, mine));
    }
}

/// Every partial stays inside `@layer shortcodes`, the layer its rules held
/// while they lived in `site.css`. A partial that forgot the wrapper would
/// land unlayered and outrank every layered rule in the sheet.
///
/// The check is a brace-depth scan, not `source.contains("@layer shortcodes")`.
/// A partial shaped
///
/// ```css
/// @layer shortcodes { .moss-x { color: red } }
/// .moss-x:hover { color: blue }   /* after the close — unlayered */
/// ```
///
/// satisfies the substring and `assemble` concatenates it verbatim, so the
/// trailing rule outranks the entire layered sheet: exactly the failure this
/// test names. Depth also catches the string appearing only in a comment.
#[test]
fn partials_declare_their_cascade_layer() {
    for partial in CSS_PARTIALS {
        assert!(
            blanked(partial.source).contains("@layer shortcodes"),
            "partial '{}' must re-open @layer shortcodes",
            partial.name
        );
        assert!(
            content_is_all_layered(partial.source),
            "partial '{}' has rules outside @layer shortcodes",
            partial.name
        );
    }
}

/// The depth scan above must actually reject the shape it describes.
#[test]
fn cascade_layer_check_rejects_a_trailing_unlayered_rule() {
    let good = "@layer shortcodes {\n  .moss-x { color: red }\n}\n";
    let bad = "@layer shortcodes {\n  .moss-x { color: red }\n}\n.moss-x:hover { color: blue }\n";
    assert!(content_is_all_layered(good), "the good shape must pass");
    assert!(!content_is_all_layered(bad), "a trailing rule must be rejected");
}

/// Extracted predicate, so the rejection test can exercise it on a literal.
fn content_is_all_layered(source: &str) -> bool {
    let stripped = blanked(source);
    let mut depth = 0usize;
    let mut pending = String::new();
    for ch in stripped.chars() {
        match ch {
            '{' => {
                depth += 1;
                pending.clear();
            }
            '}' => {
                depth = depth.saturating_sub(1);
                pending.clear();
            }
            _ if depth == 0 => pending.push(ch),
            _ => {}
        }
        if depth == 0 {
            let t = pending.trim();
            if !(t.is_empty() || "@layer shortcodes".starts_with(t)) {
                return false;
            }
        }
    }
    depth == 0
}

/// The whole point: a build that uses nothing optional ships less than one
/// that uses everything, and the difference is exactly the gated partials.
#[test]
fn gates_change_what_ships() {
    let none = assemble(&SiteAssets::default());
    let all = assemble(&SiteAssets { callouts: true, vertical: true, ..SiteAssets::default() });
    assert!(
        none.len() < all.len(),
        "an all-false asset set must produce a smaller stylesheet"
    );
    assert!(
        !none.contains(".callout"),
        "callout rules must not ship when no page has a callout"
    );
    assert!(all.contains(".callout"), "callout rules must ship when a page has one");
    assert!(
        !none.contains("data-typesetting"),
        "vertical-typesetting rules must not ship for a horizontal site"
    );
    assert!(all.contains("data-typesetting"));
}

/// Each gate is independent — turning one on must not drag the other in.
#[test]
fn gates_are_independent() {
    let callouts_only = assemble(&SiteAssets { callouts: true, vertical: false, ..SiteAssets::default() });
    assert!(callouts_only.contains(".callout"));
    assert!(!callouts_only.contains("data-typesetting"));

    let vertical_only = assemble(&SiteAssets { callouts: false, vertical: true, ..SiteAssets::default() });
    assert!(vertical_only.contains("data-typesetting"));
    assert!(!vertical_only.contains(".callout"));
}

/// Distinct asset sets must produce distinct stylesheet BYTES, because the
/// stylesheet's content hash is what the Stage 5b global invalidator keys on
/// (`FacadeCache::asset_versions`). If two different sets could assemble to
/// identical bytes, a partial-set change would slip past the invalidator
/// while still changing what the site needs.
#[test]
fn distinct_asset_sets_produce_distinct_bytes() {
    let sets = [
        SiteAssets { callouts: false, vertical: false, ..SiteAssets::default() },
        SiteAssets { callouts: true, vertical: false, ..SiteAssets::default() },
        SiteAssets { callouts: false, vertical: true, ..SiteAssets::default() },
        SiteAssets { callouts: true, vertical: true, ..SiteAssets::default() },
    ];
    let assembled: Vec<String> = sets.iter().map(assemble).collect();
    for (i, a) in assembled.iter().enumerate() {
        assert_eq!(a, &assemble(&sets[i]), "assembly must be deterministic");
        for (j, b) in assembled.iter().enumerate().skip(i + 1) {
            assert_ne!(a, b, "sets {:?} and {:?} assembled identically", sets[i], sets[j]);
        }
    }
}

/// The layer-order declaration must come first: `@layer a, b, c;` only sets
/// precedence when it precedes the blocks it names.
#[test]
fn layer_order_leads_the_sheet() {
    let css = assemble(&SiteAssets::default());
    let order = css.find("@layer reset").expect("layer order must be present");
    let tokens = css.find("@layer tokens").expect("tokens layer must be present");
    assert!(order < tokens, "layer order must precede the first @layer block");
}

/// The scanner is what `partials_are_order_independent` trusts, so pin its
/// edges: comments and quoted values must not read as structure, at-rules
/// must resolve to the selector they wrap, and a selector list must expand.
#[test]
fn declaration_scanner_handles_at_rules_comments_and_quoted_braces() {
    let css = r#"
        /* .commented-out { color: red } */
        @media (min-width: 10rem) { .real { color: blue } }
        @keyframes spin { 0% { opacity: 0 } to { opacity: 1 } }
        .a, .b > .c { content: "{ not-a-selector: x;"; margin: 0 }
    "#;
    let got = declarations(css);
    let has = |s: &str, p: &str| got.contains(&(s.to_string(), p.to_string()));

    // An @media-wrapped rule is attributed to the selector, not the at-rule.
    assert!(has(".real", "color"));
    // Selector lists expand; the last declaration needs no trailing `;`.
    assert!(has(".a", "content") && has(".b > .c", "content"));
    assert!(has(".a", "margin") && has(".b > .c", "margin"));
    // A brace and a colon inside a quoted value are not structure.
    assert!(!got.iter().any(|(s, _)| s.contains("not-a-selector")));
    assert!(!got.iter().any(|(_, p)| p.contains("not-a-selector")));
    // Commented-out rules, at-rule preludes and keyframe stops are excluded.
    assert!(!got.iter().any(|(s, _)| s.contains("commented-out")));
    assert!(!got.iter().any(|(s, _)| s == "0%" || s == "to"));
    assert!(!got.iter().any(|(s, _)| s.starts_with("@media") || s.starts_with("@supports")));
    // …but a `@keyframes` block IS recorded, by name, under the synthetic
    // property — it has no selectors of its own, and without a row a partial
    // could silently redefine a core animation.
    assert!(has("@keyframes spin", KEYFRAMES_PROP), "got: {got:?}");
}

/// The keyframes row must be able to REJECT, not just to exist.
#[test]
fn redefining_a_core_animation_is_not_owned() {
    let core = declarations("@keyframes spin { 0% { opacity: 0 } to { opacity: 1 } }");
    let partial = declarations("@keyframes spin { 0% { opacity: 1 } to { opacity: 0 } }");
    let shared: Vec<_> = partial.intersection(&core).collect();
    assert!(
        !shared.is_empty(),
        "a partial redefining core's `spin` must collide with core's row"
    );
    // A differently-named animation does not.
    let fresh = declarations("@keyframes moss-search-progress { 0% { left: 0 } }");
    assert!(fresh.intersection(&core).count() == 0);
}

/// The collision check must actually fire. Without this, a scanner that
/// silently returned nothing would make `partials_are_order_independent`
/// pass for every possible partition.
#[test]
fn order_independence_check_rejects_real_collisions() {
    let clashes = |core: &str, partial: &str| {
        !shadowing_pairs(&declarations(partial), &declarations(core)).is_empty()
    };

    // Same selector, same property.
    assert!(clashes(".shared { color: red }", ".shared { color: blue }"));

    // Same selector, shorthand vs longhand — different property NAMES, but
    // `margin` sets `margin-top`, so order decides the outcome.
    assert!(clashes(".shared { margin: 0 }", ".shared { margin-top: 1px }"));
    assert!(clashes(".shared { border-left-color: red }", ".shared { border: 0 }"));
    assert!(clashes(".shared { color: red }", ".shared { all: unset }"));

    // Genuinely order-independent: same selector, unrelated properties.
    // This is the `:root` custom-property case that made callouts extractable
    // — core's `:root` sets `--moss-*`, the partial's sets `--callout-*`.
    assert!(!clashes(
        ":root { --moss-color-bg: white }",
        ":root { --callout-note: blue }"
    ));
    assert!(!clashes(".shared { color: red }", ".shared { margin-top: 1px }"));
    // Custom properties are a flat namespace — the `-` prefix rule must not
    // treat two sibling variables as a shorthand pair.
    assert!(!clashes(
        ":root { --callout-note: blue }",
        ":root { --callout-note-bg: white }"
    ));
    // Different selectors never clash, whatever they declare.
    assert!(!clashes(".a { color: red }", ".b { color: blue }"));
}

/// The ownership rule's own falsifier: it must reject the cases that motivated
/// it, including the one that defeated the previous string-comparison rule.
#[test]
fn ownership_rule_rejects_unowned_selectors() {
    // Mirrors `a_partial_only_styles_elements_it_owns` over hand-written input.
    let owns = |core_css: &str, partial_css: &str| {
        let core = declarations(core_css);
        let core_tokens: BTreeSet<String> =
            core.iter().flat_map(|(s, _)| selector_tokens(s)).collect();
        let core_props: BTreeSet<&String> = core
            .iter()
            .filter(|(_, p)| p.starts_with("--"))
            .map(|(_, p)| p)
            .collect();
        declarations(partial_css).iter().all(|(sel, prop)| {
            selector_tokens(sel).iter().any(|t| !core_tokens.contains(t))
                || (prop.starts_with("--") && !core_props.contains(prop))
        })
    };

    // THE CASE THAT BROKE THE OLD RULE: selector strings differ, element sets
    // overlap. `h2` carries no owned token, so ownership rejects it.
    assert!(!owns(":is(h1, h2, h3) { margin-top: 1rem }", "h2 { margin-top: 2rem }"));
    // Same shape via descendant combinator and universal selector.
    assert!(!owns(".card .title { color: red }", ".title { color: blue }"));
    assert!(!owns("p { margin: 0 }", "* { margin: 1px }"));
    // Restyling a class core already uses is rejected even under a marker-ish
    // ancestor, because `.nav-divider` itself is core's.
    assert!(!owns(".nav-divider { display: block }", ".nav-divider { display: none }"));

    // ACCEPTED: the two shapes the shipped partials actually use.
    assert!(owns("p { color: red }", ".callout .callout-title { color: blue }"));
    assert!(owns(
        "body { writing-mode: horizontal-tb }",
        r#"body[data-typesetting="vertical"] { writing-mode: vertical-rl }"#
    ));
    // ACCEPTED: a `:root` block that only defines variables core never defines.
    assert!(owns(":root { --moss-bg: white }", ":root { --callout-note: blue }"));
    // REJECTED: the same block redefining one of core's variables.
    assert!(!owns(":root { --moss-bg: white }", ":root { --moss-bg: black }"));
}

/// [`COLOPHON_FONT_RANGE`] must cover every wordmark the colophon can hold.
///
/// The range is what makes a two-glyph face safe to name in a stack, but it
/// only helps if it matches the strings. A character inside the range that the
/// file lacks draws a blank box; a character outside it silently falls to the
/// body font, which would set one wordmark in two faces.
///
/// The face has a second caller since 2026-08-30 — the launcher's title bar
/// names it too — but `colophon_name` is still the whole input set, because
/// `mark_sync_test` holds the launcher's wordmark equal to it and its
/// `unicode-range` equal to this const. A third caller that did neither would
/// need adding here.
#[test]
fn colophon_font_covers_the_wordmark() {
    use crate::i18n::Language;
    let langs = [Language::En, Language::ZhHans, Language::ZhHant];
    // Adding a language without extending `langs` fails to compile here, so the
    // new wordmark cannot reach the range unchecked.
    for lang in langs {
        match lang {
            Language::En | Language::ZhHans | Language::ZhHant => {}
        }
    }

    for lang in langs {
        let name = crate::i18n::t(lang, "colophon_name");
        for ch in name.chars() {
            // ASCII is the deliberate exception: the Latin wordmark stays in
            // the body stack (see the `:lang(zh)` rule in site.css).
            assert!(
                ch.is_ascii() || COLOPHON_FONT_RANGE.contains(&ch),
                "colophon_name for {lang:?} contains {ch:?}, which is neither ASCII nor in \
                 COLOPHON_FONT_RANGE — resubset the face and widen the range together"
            );
        }
    }
}

/// The sidenote gutter's eligibility selector must read identically in all
/// three rules it governs.
///
/// `sidenotes.css` decides which pages get a margin gutter once per rule: it
/// declares `--moss-sidenote-reserve` on an eligible `body`, gives
/// `.moss-sidenote` its box under the same condition, and retires the endnote
/// list under it again. There is no way to state the condition once — hoisting
/// it into a custom property needs two properties of opposite sense, and
/// `CUSTOM_PROPS` is a catalogue of theme hooks, not of internal plumbing — so
/// the three copies are held equal here instead.
///
/// Drift is not cosmetic. A page scoped out of the reserve but not out of the
/// other two loses `--moss-sidenote-reserve`, which makes
/// `--moss-sidenote-width` invalid at computed-value time: the asides fall back
/// to `width: auto` and float full-measure over the prose, while the endnote
/// list still retires to `display: none`. The notes end up broken and missing
/// at the same time, on exactly the pages someone was trying to protect.
///
/// Added 2026-08-30 with the front-page scope-out
/// (docs/archive/2026-08-30-sidenote-gutter-page-kind.md), which was the first
/// change that had to edit all three.
#[test]
fn sidenote_eligibility_selector_is_written_identically() {
    let css = CSS_PARTIALS
        .iter()
        .find(|p| p.name == "sidenotes")
        .expect("the sidenotes partial")
        .source;

    let selectors: Vec<&str> = css
        .match_indices("body:where(")
        .map(|(start, _)| {
            // Balanced-paren scan: the selector nests `:not(:has(…))`, so the
            // first `)` is not the end of it.
            let bytes = css.as_bytes();
            let mut depth = 0usize;
            let mut i = start;
            while i < bytes.len() {
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            return &css[start..=i];
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            panic!("unbalanced parentheses in a body:where(…) selector");
        })
        .collect();

    assert_eq!(
        selectors.len(),
        3,
        "expected exactly three `body:where(…)` eligibility selectors in \
         sidenotes.css, found {}. If a rule was legitimately added or removed, \
         update this count deliberately — the point of the test is that nobody \
         edits a subset of them.\nfound: {selectors:#?}",
        selectors.len()
    );

    for (i, sel) in selectors.iter().enumerate().skip(1) {
        assert_eq!(
            *sel, selectors[0],
            "sidenote eligibility selector #{} differs from the first. All three \
             must scope out the same pages, or a page gets the reserve without \
             the notes (or the notes without the reserve).",
            i + 1
        );
    }

    // The scope-outs the design commits to, spelled out so deleting one is a
    // deliberate act rather than a selector edit nobody reads.
    for condition in [
        r#":not([data-page="home"])"#,
        r#":not([data-content-width="full"])"#,
        ":not(:has(main.has-sidebar))",
    ] {
        assert!(
            selectors[0].contains(condition),
            "the eligibility selector no longer scopes out {condition}; \
             see docs/archive/2026-08-30-sidenote-gutter-page-kind.md"
        );
    }
}
