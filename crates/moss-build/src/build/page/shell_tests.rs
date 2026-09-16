use super::*;

/// Assemble the full CSS as blocking.rs does at build time:
/// generated @layer order + @layer tokens { :root ... } prefix + DEFAULT_CSS.
///
/// Tests that check for token definitions (:root values, dark-mode blocks)
/// should call this rather than testing DEFAULT_CSS directly, since Task 1.3
/// moved token definitions out of site.css into a generated prefix.
#[cfg(test)]
fn full_site_css() -> String {
    use moss_core::contract::tokens::{
        format_dark_root_block, format_root_block, load_tokens,
    };
    #[allow(clippy::expect_used)]
    let tokens = load_tokens().expect("tokens.json must parse");
    let layer_order = "@layer reset, tokens, base, layout, shortcodes, plugins, themes;\n";
    let tokens_layer = format!(
        "@layer tokens {{\n{}{}}}\n",
        format_root_block(&tokens),
        format_dark_root_block(&tokens),
    );
    format!("{}{}{}", layer_order, tokens_layer, site_css_with_partials())
}

/// Core rules plus every gated partial — the stylesheet a site that uses
/// *everything* receives.
///
/// `DEFAULT_CSS` stopped being the whole sheet on 2026-08-04: optional
/// sections now live in `assets/css/site/*.css` and ship only when
/// `SiteAssets` says the build needs them (see `build/emit/stylesheet.rs`).
/// A test asserting that some rule exists at all belongs here; one asserting
/// a rule ships *conditionally* belongs in `stylesheet_tests.rs`, which is
/// where the gates are.
fn site_css_with_partials() -> String {
    let mut css = String::from(DEFAULT_CSS);
    css.push('\n');
    css.push_str(crate::build::emit::stylesheet::MARK_CSS);
    for partial in crate::build::emit::stylesheet::CSS_PARTIALS {
        css.push('\n');
        css.push_str(partial.source);
    }
    css
}

#[test]
fn default_favicon_is_moss_logo() {
    // Locks in: when a site has no favicon, moss falls back to its own logo.
    // The DEFAULT_FAVICON constant is included via include_str! from
    // ../../../icons/icon.svg, the moss leaf mark. If this constant ever
    // gets emptied, swapped to a generic placeholder, or the include_str!
    // path breaks silently, this test catches it.
    assert!(
        !DEFAULT_FAVICON.is_empty(),
        "DEFAULT_FAVICON must not be empty"
    );
    assert!(
        DEFAULT_FAVICON.contains("<svg"),
        "DEFAULT_FAVICON must be SVG markup"
    );
}

/// The emitted dark `<meta name="theme-color">` must equal the dark --moss-color-bg
/// from tokens.json (#1c1914), not the old hardcoded value (#1a1816).
#[test]
fn theme_color_dark_matches_token_dark_bg() {
    use moss_core::contract::tokens::{bg_colors, load_tokens};
    let tokens = load_tokens().expect("tokens.json must parse");
    let (_light, dark) = bg_colors(&tokens);
    // The canonical dark bg from tokens.json is #1c1914.
    // If someone edits tokens.json this test catches the drift.
    assert_eq!(dark, "#1c1914",
            "dark --moss-color-bg token must be #1c1914 (got {}); update tokens.json if changed intentionally",
            dark
        );
    // Also verify the processor emits it (not the old wrong literal).
    // Build a minimal shell template and process it.
    let shell = concat!(
        r#"<meta name="theme-color" content="{theme_color_light}" media="(prefers-color-scheme: light)">"#,
        "\n",
        r#"<meta name="theme-color" content="{theme_color_dark}" media="(prefers-color-scheme: dark)">"#
    );
    let replaced_dark = shell
        .replace("{theme_color_light}", _light)
        .replace("{theme_color_dark}", dark);
    assert!(
        replaced_dark.contains("content=\"#1c1914\""),
        "emitted dark theme-color must be #1c1914, got: {}",
        replaced_dark
    );
    assert!(
        !replaced_dark.contains("content=\"#1a1816\""),
        "stale wrong dark value #1a1816 must NOT appear in emitted meta"
    );
}

/// Shorthand: substitute with an empty value map (every lowercase token is
/// "unknown" — exercises the safety-net drop branch).
fn substitute_empty(input: &str) -> String {
    substitute_template_tokens(input, &std::collections::HashMap::new())
}

#[test]
fn test_substitute_drops_unknown_token_basic() {
    assert_eq!(substitute_empty("Hello {world} there"), "Hello  there");
}

#[test]
fn test_substitute_drops_unknown_tokens_multiple() {
    assert_eq!(substitute_empty("{foo} and {bar_baz}"), " and ");
}

#[test]
fn test_substitute_preserves_non_placeholder_braces() {
    // CSS, JS code with braces should be preserved
    let input = "function() { return 1; }";
    assert_eq!(substitute_empty(input), input);
}

#[test]
fn test_substitute_preserves_uppercase() {
    // Uppercase placeholders are not our pattern
    assert_eq!(substitute_empty("{CONSTANT} stays"), "{CONSTANT} stays");
}

#[test]
fn test_substitute_preserves_numbers() {
    assert_eq!(substitute_empty("{item1} stays"), "{item1} stays");
}

#[test]
fn test_substitute_preserves_empty_braces() {
    assert_eq!(substitute_empty("{} stays"), "{} stays");
}

#[test]
fn test_substitute_drops_unknown_token_realistic_html() {
    assert_eq!(
        substitute_empty(r#"<div class="test">{unknown_placeholder}</div>"#),
        r#"<div class="test"></div>"#
    );
}

#[test]
fn test_substitute_known_token_and_drops_unknown_in_same_pass() {
    let values = std::collections::HashMap::from([("known", "VALUE".to_string())]);
    assert_eq!(
        substitute_template_tokens("<a>{known}</a><b>{unknown_token}</b>", &values),
        "<a>VALUE</a><b></b>"
    );
}

#[test]
fn test_substitute_never_rescans_substituted_values() {
    // A value containing a known token literal must be emitted verbatim —
    // single pass over template text only (#847).
    let values = std::collections::HashMap::from([
        (
            "content",
            "literal {title} and {unknown_thing} stay".to_string(),
        ),
        ("title", "T".to_string()),
    ]);
    assert_eq!(
        substitute_template_tokens("<title>{title}</title><main>{content}</main>", &values),
        "<title>T</title><main>literal {title} and {unknown_thing} stay</main>"
    );
}

#[test]
fn test_substitute_keeps_inline_script_braces() {
    // The shell's inline theme script ends in `catch(e){}` — must survive.
    let input = r#"<script>try{var t=1;}catch(e){}</script>"#;
    assert_eq!(substitute_empty(input), input);
}

// CSS Tests - Navigation Redesign
// These tests verify critical CSS properties for mobile menu and nav layout

/// A copy of the sheet with everything that only LOOKS like structure taken
/// out: comments blanked entirely, and `{`, `}` and `;` blanked wherever they
/// sit inside a quoted string or an unquoted `url(...)` body. What is left can
/// be read as blocks and declarations without a `content:` value, a data URI or
/// a prose comment being mistaken for a boundary.
///
/// Blanked rather than removed, and blanked byte for byte (a CJK comment costs
/// three spaces per character), so every offset into the result still points at
/// the same place in the original — which is what lets `get_css_rule` locate a
/// rule in here and then slice the ORIGINAL for its body, comments and quotes
/// intact. Strings keep their characters for the same reason in reverse: an
/// attribute selector is a quoted string too, and `[lang^="zh"]` emptied out no
/// longer names the rule it came from.
///
/// Escapes are honoured inside quotes: without that, `content: "a\"b{"` ends
/// the string early and the `{` is read as a block opener, which shifts every
/// later declaration onto the wrong prelude. For `stylesheet_tests`, whose
/// ownership gate reads this, that is UNDER-collection — a declaration
/// attributed to a bogus prelude trivially satisfies the ownership rule, so the
/// scanner would fail *open*, which is the direction it must never fail in.
/// Neither construct appears in the sheet today; the gate has to hold for the
/// partials that have not been written yet.
pub(crate) fn blanked(css: &str) -> String {
    let chars: Vec<char> = css.chars().collect();
    let mut out = String::with_capacity(css.len());
    // A comment carries no CSS at all, so every byte of it goes.
    let erase = |out: &mut String, span: &[char]| {
        for c in span {
            for _ in 0..c.len_utf8() {
                out.push(' ');
            }
        }
    };
    // A string or a url() body carries a value, so only the three characters
    // that would read as structure go. All three are one byte, so the copy
    // stays aligned with the original.
    let defuse = |out: &mut String, span: &[char]| {
        for &c in span {
            out.push(if matches!(c, '{' | '}' | ';') { ' ' } else { c });
        }
    };
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
            let start = i;
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(chars.len()); // past the `*/`, or the end of an unterminated one
            erase(&mut out, &chars[start..i]);
        } else if chars[i] == '"' || chars[i] == '\'' {
            let quote = chars[i];
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != quote {
                // A backslash escapes the next char, quote included.
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            i = (i + 1).min(chars.len()); // past the closing quote
            defuse(&mut out, &chars[start..i]);
        } else if chars[i] == '(' && out.trim_end().ends_with("url") {
            // An unquoted `url(data:…)` may carry braces and semicolons. Quoted
            // bodies are consumed here too, before the arm above ever sees them.
            let start = i;
            let mut depth = 0usize;
            while i < chars.len() {
                match chars[i] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            defuse(&mut out, &chars[start..i]);
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Split a selector list on TOP-LEVEL commas only.
///
/// A naive `split(',')` shreds the comma inside a functional pseudo-class:
/// `site.css` has 11 preludes of the shape `:is(h1, h2, h3, h4, h5, h6) + .x`
/// and `article :is(p, li, blockquote)`, which would yield the fragments
/// `:is(h1`, `h2`, … `h6) + .x`. In `stylesheet_tests`' ownership gate a
/// partial declaring `h2 { margin-top: … }` would then read as disjoint from
/// core's `:is(h1,…,h6) { margin-top: … }` and be wrongly accepted — the one
/// direction that scanner must never fail in. Here it is what lets a test ask
/// for `.b` and be given the block that `.a, .b` share.
pub(crate) fn split_selector_list(prelude: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in prelude.chars() {
        match ch {
            '(' | '[' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    out.push(current);
    out
}

/// The body of the rule whose prelude names `selector`, at any nesting depth —
/// a rule inside `@media` counts, and so does one selector of a comma-separated
/// list, both of which tests below rely on.
///
/// Named, rather than found by substring: a substring search cannot tell a rule
/// from a descendant of one, so it hands back whichever the sheet happens to
/// declare first, and the failure then names a selector the test never asked
/// for. `.moss-colophon-label` inside `.moss-colophon a:lang(zh) …` was the
/// pair that earned this (2026-08-28); the sheet still holds others.
fn get_css_rule(css: &str, selector: &str) -> Option<String> {
    let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let want = normalize(selector);
    // Structure comes from the blanked copy, the returned body from the
    // original — so a `content:` string or a comment inside the rule survives.
    let scan = blanked(css);
    let bytes = scan.as_bytes();
    let mut prelude_start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                // Either the whole prelude, or one selector of its list: some
                // tests name a four-selector group to assert on the block they
                // share, others name a single member of one.
                let prelude = &scan[prelude_start..i];
                let is_it = normalize(prelude) == want
                    || split_selector_list(prelude).iter().any(|part| normalize(part) == want);
                let body_start = i + 1;
                let mut depth = 1usize;
                let mut end = body_start;
                while end < bytes.len() && depth > 0 {
                    match bytes[end] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    end += 1;
                }
                if is_it {
                    // `end` is one past the closing brace, unless the sheet ran
                    // out first — in which case there is nothing to trim.
                    let close = if depth == 0 { end - 1 } else { end };
                    return Some(css[body_start..close].to_string());
                }
                // Descend rather than skip: `@media` preludes hold rules too.
                prelude_start = body_start;
                i = body_start;
            }
            b'}' | b';' => {
                prelude_start = i + 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

#[test]
fn test_css_nav_content_has_border() {
    // The header rule is on the strip's block-end edge — below it across the
    // page, to its LEFT under vertical-rl. Logical, so one spelling lays out
    // both; a physical `border-bottom` underlined the vertical strip.
    let rule =
        get_css_rule(DEFAULT_CSS, ".nav-content").expect(".nav-content CSS rule should exist");
    assert!(
        rule.contains("border-block-end: 1px solid"),
        ".nav-content should carry its rule on the block-end edge, got: {}",
        rule
    );
}

#[test]
fn test_css_main_nav_has_bottom_margin() {
    // Main nav has bottom margin for spacing from content
    let rule = get_css_rule(DEFAULT_CSS, ".main-nav").expect(".main-nav CSS rule should exist");
    // Uses sm spacing for compact separation from content
    assert!(
        rule.contains("margin-inline: auto") && rule.contains("margin-block: 0 var(--moss-space-sm)"),
        ".main-nav should have a block-end margin, got: {}",
        rule
    );
}

/// A CSS length in px, at the 16px root font size the nav is authored against.
///
/// Accepts a literal (`0.25rem`, `18px`, `1.125em`) or a single
/// `var(--moss-space-*)` reference, which is resolved through tokens.json so
/// this test carries no second copy of the spacing scale.
fn css_px(value: &str) -> f32 {
    use moss_core::contract::tokens::{find_token, load_tokens};
    let value = value.trim();
    if let Some(rest) = value.strip_prefix("var(") {
        let name = rest.trim_end_matches(')').trim().trim_start_matches("--");
        let tokens = load_tokens().expect("tokens.json must parse");
        let (light, _) = find_token(&tokens, name)
            .unwrap_or_else(|| panic!("unknown token in a nav length: {}", value));
        return css_px(light);
    }
    let (num, unit) = value.split_at(
        value
            .find(|c: char| c.is_alphabetic() || c == '%')
            .unwrap_or_else(|| panic!("not a CSS length: {}", value)),
    );
    let n: f32 = num
        .parse()
        .unwrap_or_else(|_| panic!("not a CSS length: {}", value));
    match unit {
        // The nav toggles all sit at font-size: 1rem, so em == rem here.
        "rem" | "em" => n * 16.0,
        "px" => n,
        _ => panic!("unsupported unit in a nav length: {}", value),
    }
}

/// The value of a single declaration in a rule body, e.g. `padding-inline`.
fn decl(rule: &str, prop: &str) -> String {
    let needle = format!("{}:", prop);
    rule.lines()
        .map(str::trim)
        .find(|l| l.starts_with(&needle))
        .unwrap_or_else(|| panic!("rule should declare {}:\n{}", prop, rule))
        .trim_start_matches(&needle)
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_string()
}

/// The narrowest a rule's column gap ever gets, in px — the horizontal gap,
/// which is the one that does the grouping.
///
/// `gap` is `<row> <column>` when it has two top-level values, so the leading
/// one is dropped in that case. The column value may be a `clamp(a, b, c)`,
/// whose narrow-viewport floor is what has to hold the hierarchy — taking the
/// smallest term tests the worst case rather than the roomiest one.
fn column_gap_floor(rule: &str) -> f32 {
    let value = rule
        .lines()
        .find_map(|l| l.trim().strip_prefix("gap:"))
        .expect("rule should declare a gap")
        .trim_end_matches(';')
        .trim();

    // Split on whitespace that is not inside parentheses.
    let mut parts: Vec<String> = Vec::new();
    let (mut depth, mut cur) = (0usize, String::new());
    for ch in value.chars() {
        match ch {
            '(' => { depth += 1; cur.push(ch); }
            ')' => { depth -= 1; cur.push(ch); }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() { parts.push(std::mem::take(&mut cur)); }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() { parts.push(cur); }

    let column = parts.last().expect("gap should have a value");
    // A `clamp(a, b, c)` contributes its terms; a bare length contributes
    // itself. Viewport-relative terms (`4vw`) are skipped — they are the ramp
    // between the floor and the ceiling, not either of them.
    // Strip exactly the one closing paren that matches `clamp(`, not every
    // trailing paren: the last term is normally `var(--moss-space-lg)`, and
    // being greedy here leaves it unbalanced and relies on css_px being
    // equally lenient. Two lenient parsers cancelling out is not a property
    // to build on.
    let inner = column
        .strip_prefix("clamp(")
        .map(|s| s.strip_suffix(')').unwrap_or(s))
        .unwrap_or(column);
    inner
        .split(',')
        .map(str::trim)
        .filter(|t| !t.ends_with("vw") && !t.ends_with("vh"))
        .map(css_px)
        .min_by(|a, b| a.partial_cmp(b).expect("gap terms are finite"))
        .unwrap_or_else(|| panic!("column gap has no usable term: {}", column))
}

/// The `search` CSS partial's source. `.nav-search-btn` and `.search-icon` are
/// gated behind `[site].search`, so they live here rather than in DEFAULT_CSS.
fn search_partial_css() -> &'static str {
    crate::build::emit::stylesheet::CSS_PARTIALS
        .iter()
        .find(|p| p.name == "search")
        .expect("the `search` CSS partial should be registered")
        .source
}

#[test]
fn test_css_search_button_keeps_its_pill() {
    // The resting FILL was removed (it read as a bright disc over a hero
    // image); the pill it filled was not. Dropping the radius along with the
    // fill would leave a square hover patch, which is the over-correction this
    // pins against. The 18px is absolute on purpose — the same radius the app
    // shell's pills use — so it is asserted as a literal, unlike the spacing
    // hierarchy above, which is a ratio because the values are a design call.
    let search = get_css_rule(search_partial_css(), ".nav-search-btn")
        .expect(".nav-search-btn CSS rule should exist");
    assert_eq!(
        decl(&search, "border-radius"),
        "18px",
        ".nav-search-btn must keep its pill radius:\n{}",
        search
    );
    assert_eq!(
        decl(&search, "background"),
        "none",
        ".nav-search-btn must have no resting fill:\n{}",
        search
    );
}

#[test]
fn test_css_nav_toggle_cluster_is_tighter_than_the_group_gap() {
    // The gap SEPARATING the link list from the toggle cluster must exceed the
    // gap INSIDE the toggle cluster, or the toggles read as three unrelated
    // items rather than as one group set apart from the links. This was
    // inverted (nav-right sm=16px vs nav-icons md=24px) until 2026-08-03.
    //
    // Measured in INK, not in `gap`. Comparing the two `gap` values compares
    // box edges, and every toggle is a padded box — so the boxes can be in the
    // right order while the glyphs a reader actually sees are evenly spaced.
    // They were: with gap 32/16 the ink ran 32 / 25 / 29 px, no group break at
    // all. That is the bug this test failed to catch for a whole redesign, and
    // the reason it now adds each side's inset before comparing.
    //
    // Asserted as a ratio, not as literal values: the exact spacing is a design
    // call that may be re-tuned, but 1.5x on ink at the narrow end is the floor
    // the .nav-right comment commits to and must not silently erode.
    let right = get_css_rule(DEFAULT_CSS, ".nav-right").expect(".nav-right CSS rule should exist");
    let icons = get_css_rule(DEFAULT_CSS, ".nav-icons").expect(".nav-icons CSS rule should exist");
    let theme =
        get_css_rule(DEFAULT_CSS, ".nav-theme-btn").expect(".nav-theme-btn CSS rule should exist");
    let lang = get_css_rule(DEFAULT_CSS, ".nav-lang-toggle")
        .expect(".nav-lang-toggle CSS rule should exist");
    let theme_icon = get_css_rule(DEFAULT_CSS, ".theme-toggle-icon")
        .expect(".theme-toggle-icon CSS rule should exist");

    // .nav-search-btn and .search-icon live in the `search` PARTIAL, not in
    // DEFAULT_CSS. Reaching for them there is not incidental: search is the
    // FIRST child of .nav-icons, so it forms the group-break boundary, and its
    // 9px glyph inset is half of the 32/25/29 flat run this test exists to
    // catch. A version of this test that modelled only the theme and language
    // insets would be blind to the very element that caused the bug.
    let search_css = search_partial_css();
    let search = get_css_rule(search_css, ".nav-search-btn")
        .expect(".nav-search-btn CSS rule should exist");
    let search_icon =
        get_css_rule(search_css, ".search-icon").expect(".search-icon CSS rule should exist");

    // css_px treats `em` as 16px. That is only true while the toggles sit at
    // font-size: 1rem, and the glyph widths below are authored in `em`, so a
    // font-size change here would silently return wrong numbers rather than
    // panic. Pin it.
    for (name, rule) in [("nav-theme-btn", &theme), ("nav-search-btn", &search)] {
        assert_eq!(
            css_px(&decl(rule, "font-size")),
            16.0,
            ".{} must stay at 1rem, or the `em` glyph widths below stop being \
             16px-relative and css_px quietly mis-measures the insets",
            name
        );
    }

    // How far each toggle's ink sits inside its own box: a centred glyph in a
    // fixed square, and the language toggle's text behind its inline padding.
    // The two glyphs are different sizes (1.125em vs 1.25em), so they do NOT
    // share an inset — take each separately.
    let inset = |btn: &str, icon: &str| (css_px(btn) - css_px(icon)) / 2.0;
    let search_inset = inset(&decl(&search, "width"), &decl(&search_icon, "width"));
    let theme_inset = inset(&decl(&theme, "width"), &decl(&theme_icon, "width"));
    let lang_inset = css_px(&decl(&lang, "padding-inline"));

    // Worst case at both ends: the narrowest the group gap ever gets against
    // the SMALLEST inset on the cluster's outer edge, versus the widest ink gap
    // found anywhere INSIDE it. The pairs below are enumerated by hand and
    // cover today's three members in their two shipped configurations — a
    // FOURTH toggle would not be modelled here and this list must grow with it.
    let edge_inset = search_inset.min(theme_inset).min(lang_inset);
    let cluster_gap = column_gap_floor(&icons);
    let within_toggles = [
        search_inset + cluster_gap + lang_inset, // search → language
        lang_inset + cluster_gap + theme_inset,  // language → theme
        search_inset + cluster_gap + theme_inset, // monolingual: search → theme
    ]
    .into_iter()
    .fold(f32::MIN, f32::max);
    let between_groups = column_gap_floor(&right) + edge_inset;

    // The bound computed above is an ENVELOPE, not a rendering: pairing the
    // smallest edge inset with the widest internal pair describes no real
    // configuration. It comes out at 32/21 = 1.52x, while the configurations
    // that actually ship are looser — 33/21 = 1.57x with search on, 32/20 =
    // 1.60x with it gated off. That is deliberate: the envelope is chosen so
    // the test cannot pass a layout that some configuration would fail, at the
    // cost of ~4% of false strictness. Do not "correct" it to a real pair, and
    // do not quote 1.52x as a measurement of the nav.
    //
    // Consequence: only ~1.6% separates the envelope from the 1.5x floor. If
    // you have just tripped this, you crossed the line rather than ate slack
    // that was there for you. Re-tune the cluster's own gap or the toggles'
    // insets, or move the floor deliberately here and in the .nav-right
    // comment that commits to it.
    assert!(
        between_groups >= 1.5 * within_toggles,
        "the ink gap between the links and the toggles ({}px) must stay at least \
         1.5x the ink gap inside the cluster ({}px, the widest pair) — currently \
         {:.3}x — or the header stops reading as two groups.\n\
         insets: search {}, theme {}, language {}\n.nav-right: {}\n.nav-icons: {}",
        between_groups,
        within_toggles,
        between_groups / within_toggles,
        search_inset,
        theme_inset,
        lang_inset,
        right,
        icons
    );
}

#[test]
fn test_css_nav_right_edge_aligns_toggles_when_it_wraps() {
    // When a long breadcrumb pushes .nav-right to its own row it grows to fill
    // that row, and these two declarations are what then put the links at the
    // start edge and the toggles at the end edge. Without them the pair stays
    // clumped against the right gutter with a ragged hole under the links.
    // `margin-inline-start: auto` also covers the no-nav-links case, where
    // space-between alone would leave a lone icon cluster at the start edge.
    let right = get_css_rule(DEFAULT_CSS, ".nav-right").expect(".nav-right CSS rule should exist");
    let icons = get_css_rule(DEFAULT_CSS, ".nav-icons").expect(".nav-icons CSS rule should exist");

    assert!(
        right.contains("justify-content: space-between"),
        ".nav-right should space-between its two groups, got: {}",
        right
    );
    assert!(
        right.contains("flex: 1 1 auto"),
        ".nav-right must be able to grow to fill row 2 (and must NOT use a 100% \
         basis, which forces a wrap even when the content fits), got: {}",
        right
    );
    assert!(
        icons.contains("margin-inline-start: auto"),
        ".nav-icons should end-anchor inside nav-right, got: {}",
        icons
    );
}

#[test]
fn test_css_nav_split_keeps_toggles_on_row_one() {
    // `data-nav-split` (set by nav-split.ts on a plain-site-name masthead that
    // has to wrap) re-stacks the nav: toggles on row 1 beside the name, links
    // alone on row 2 spread edge to edge. `display: contents` is what lifts
    // the two groups out of .nav-right so they can land on different rows —
    // flex wrapping alone can never float the later sibling back up.
    // Design: docs/archive/2026-08-09-nav-two-line-split-and-touch-hints.md
    let right = get_css_rule(DEFAULT_CSS, ".nav-content[data-nav-split] .nav-right")
        .expect("split .nav-right rule should exist");
    let links = get_css_rule(DEFAULT_CSS, ".nav-content[data-nav-split] .nav-links")
        .expect("split .nav-links rule should exist");
    let icons = get_css_rule(DEFAULT_CSS, ".nav-content[data-nav-split] .nav-icons")
        .expect("split .nav-icons rule should exist");

    assert!(
        right.contains("display: contents"),
        "split mode must lift links + icons out of .nav-right, got: {}",
        right
    );
    assert!(
        links.contains("flex: 1 1 100%") && links.contains("justify-content: space-between"),
        "split links take their own full row, spread across both edges, got: {}",
        links
    );
    assert!(
        icons.contains("order: 1") && links.contains("order: 2"),
        "icons must precede links in wrap order so they stay on row 1, got icons: {} links: {}",
        icons,
        links
    );
}

#[test]
fn test_css_hover_hints_are_gated_on_a_hover_capable_device() {
    // Touch has no un-hover: a tapped element keeps `:hover` until the next
    // tap, so an ungated hint pill just hangs there (the stuck theme-toggle
    // hint, okagaki 2026-08). Both hover-triggered rules — the pill itself
    // and the .nav-left overflow lift that lets it escape the clip — sit
    // behind `@media (any-hover: hover)`; `:focus-visible` stays ungated
    // because keyboards exist on touch devices too. This is a text property
    // of the emitted stylesheet; whether an engine honours the gate is the
    // render gates' concern.
    //
    // `any-hover`, not `hover`: the narrower query describes only the primary
    // pointer, so an iPad on a trackpad reports `none` and would lose every
    // hint. Pinned here because the two read almost identically in a diff and
    // the difference is a whole class of device.
    // Matched as a PREFIX, stopping after the first declaration: what this test
    // is about is the selector sitting inside the gate, not the rule's whole
    // body. Pinning the closing brace made it fail when the pill gained
    // `pointer-events: auto` for WCAG 1.4.13 — a change with nothing to say
    // about touch, reported as a broken touch gate.
    let flat: String = DEFAULT_CSS.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains("@media (any-hover: hover) { [data-tooltip]:hover::after { opacity: 1;"),
        "the hint pill's hover trigger must sit inside @media (any-hover: hover)"
    );
    assert!(
        flat.contains("@media (any-hover: hover) { .nav-left:has([data-tooltip]:hover) { overflow: visible; } }"),
        "the overflow lift's hover half must sit inside @media (any-hover: hover)"
    );
    assert!(
        flat.contains("[data-tooltip]:focus-visible::after { opacity: 1;"),
        "the focus-visible trigger must stay unconditional"
    );
    // The runtime half: a finger press suppresses the pill even where the
    // query passed (a touchscreen laptop). Must be declared AFTER the hover
    // rule — same specificity, so source order is the whole mechanism.
    let hover_at = flat
        .find("[data-tooltip]:hover::after")
        .expect("hover trigger should exist");
    let suppressed_at = flat
        .find("[data-tooltip][data-hint-suppressed]::after { opacity: 0; }")
        .expect("touch suppression rule should exist");
    assert!(
        suppressed_at > hover_at,
        "the touch-suppression rule must come after the hover rule or it \
         cannot override it (equal specificity)"
    );
    // The toggles no longer emit hints, so their static end-anchor fallback
    // must not linger — hint-place.ts is the only placement authority.
    assert!(
        !DEFAULT_CSS.contains(".nav-icons [data-tooltip]"),
        "no per-element hint anchor rules for the toggle cluster"
    );
}

#[test]
fn test_css_apply_button_check_static_only_on_success() {
    // Regression guard: the apply button's "Apply"/"申请" label must sit
    // centred in idle state. The success pill shows the label + checkmark
    // together (check `position: static`, in flow), but in idle/loading/
    // error the check must stay out of flow (shared `position: absolute`).
    // If the `position: static` override is NOT scoped to
    // [data-state="success"], the invisible in-flow check reserves width
    // beside the label and shifts it left of centre.
    assert!(
        get_css_rule(
            DEFAULT_CSS,
            r#".moss-subscribe-form[data-position="apply"][data-state="success"] .moss-btn__check"#,
        )
        .map_or(false, |r| r.contains("position: static")),
        "apply success check must be position: static (in flow, beside the label)",
    );
    assert!(
        get_css_rule(
            DEFAULT_CSS,
            r#".moss-subscribe-form[data-position="apply"] .moss-btn__check"#,
        )
        .is_none(),
        "apply check must NOT force position: static across all states — that \
             reserves width for the invisible check and de-centres the idle label",
    );
}

/// Helper to extract CSS rule from media query.
/// Searches all matching media query blocks, not just the first one.
fn get_css_rule_in_media(css: &str, media_query: &str, selector: &str) -> Option<String> {
    let mut search_start = 0;

    // Search through all occurrences of the media query
    while let Some(relative_pos) = css[search_start..].find(media_query) {
        let media_start = search_start + relative_pos;
        let css_from_media = &css[media_start..];

        // Find the matching closing brace by counting braces
        let mut brace_count = 0;
        let mut media_end = 0;
        for (i, c) in css_from_media.char_indices() {
            if c == '{' {
                brace_count += 1;
            } else if c == '}' {
                brace_count -= 1;
                if brace_count == 0 {
                    media_end = i;
                    break;
                }
            }
        }

        if media_end > 0 {
            let media_content = &css_from_media[..media_end];
            // Try to find the selector in this media query block
            if let Some(rule) = get_css_rule(media_content, selector) {
                return Some(rule);
            }
        }

        // Move search position past this media query block
        search_start = media_start + media_end.max(1);
    }

    None
}

#[test]
fn test_css_mobile_nav_links_are_a_single_column() {
    // Narrow breakpoint (<20rem) uses hamburger with absolute positioning.
    let rule = get_css_rule_in_media(DEFAULT_CSS, "@media (max-width: 20rem)", ".nav-links")
        .expect(".nav-links mobile CSS rule should exist");
    assert!(
        rule.contains("position: absolute"),
        ".nav-links on mobile should overlay the page, got: {}",
        rule
    );
    // The closed panel collapses so a hidden menu contributes no page scroll.
    assert!(
        rule.contains("max-height: 0"),
        "closed .nav-links should collapse to zero height, got: {}",
        rule
    );
    // `flex-wrap` is `wrap` on the desktop rule. Left inherited here, a COLUMN
    // flex container with a bounded height wraps into extra COLUMNS instead of
    // growing — which is how a five-link nav used to render as a cramped grid
    // and a twelve-link one lost labels off the right edge. Whether that
    // happens is a layout question (the gate in
    // tests/render-gates/site/nav-mobile.spec.js measures it in two engines);
    // that the override is emitted at all is answerable here.
    assert!(
        rule.contains("flex-direction: column") && rule.contains("flex-wrap: nowrap"),
        ".nav-links on mobile must be a single non-wrapping column, got: {}",
        rule
    );

    // The open panel is sized by its links, capped only by the viewport — no
    // height number decides how many nav items an author may have.
    let open = get_css_rule_in_media(
        DEFAULT_CSS,
        "@media (max-width: 20rem)",
        ".nav-links.mobile-open",
    )
    .expect(".nav-links.mobile-open rule should exist");
    assert!(
        open.contains("max-height: calc(100dvh - 100%)"),
        "open .nav-links should be bounded by the viewport, not a fixed height, got: {}",
        open
    );
}

#[test]
fn test_css_nav_content_uses_flex_with_wrap() {
    // Nav content should use flexbox with wrap for natural responsive layout
    // Items stay on one row when they fit, wrap only when needed
    let rule =
        get_css_rule(DEFAULT_CSS, ".nav-content").expect(".nav-content CSS rule should exist");

    assert!(
        rule.contains("display: flex"),
        ".nav-content should use flexbox, got: {}",
        rule
    );
    assert!(
        rule.contains("flex-wrap: wrap"),
        ".nav-content should wrap when needed, got: {}",
        rule
    );
}

#[test]
fn test_css_nav_right_uses_flex() {
    // Nav-right should use flex to contain nav-links and nav-icons
    let rule = get_css_rule(DEFAULT_CSS, ".nav-right").expect(".nav-right CSS rule should exist");

    assert!(
        rule.contains("display: flex"),
        ".nav-right should use flexbox, got: {}",
        rule
    );
}

// =========================================================================
// Sidebar Responsive Layout Tests
// =========================================================================

#[test]
fn test_css_sidebar_responsive_width() {
    // Sidebar width should scale on wider screens using clamp()
    // Now on main.has-sidebar instead of .page-wrapper.has-sidebar
    let rule = get_css_rule(DEFAULT_CSS, "main.has-sidebar > .latest-sidebar")
        .expect(".latest-sidebar CSS rule should exist");
    assert!(
        rule.contains("clamp(") && rule.contains("vw"),
        "Sidebar should use clamp() with vw units for responsive width, got: {}",
        rule
    );
}

#[test]
fn test_css_sidebar_mobile_alignment() {
    // When sidebar moves below content on narrow screens, it should align with content
    // Now on main.has-sidebar instead of .page-wrapper.has-sidebar
    let rule = get_css_rule_in_media(
        DEFAULT_CSS,
        "@media (max-width: 64rem)",
        "main.has-sidebar > .latest-sidebar",
    )
    .expect(".latest-sidebar mobile CSS rule should exist");
    // Should center with the same measure as content. Logical, like every
    // consumer of the measure: physical max-width binds the block axis under
    // vertical-rl.
    assert!(
        rule.contains("max-inline-size: var(--moss-content-width)")
            && rule.contains("margin")
            && rule.contains("auto"),
        "Mobile sidebar should center align with content (max-inline-size + margin: auto), got: {}",
        rule
    );
}

#[test]
fn test_css_hero_follows_header_keeps_nav_padding() {
    // When hero follows header, nav-content should KEEP padding-bottom
    // (so nav links have breathing room) but remove border-bottom
    // (hero visually replaces the divider line)
    let rule = get_css_rule(DEFAULT_CSS, "header:has(+ .moss-hero) .nav-content")
        .expect("header:has(+ .moss-hero) .nav-content CSS rule should exist");

    // Should remove border but NOT padding. Same edge family as the rule it
    // cancels: a physical `border-bottom: none` no longer cancels a
    // `border-block-end` under vertical-rl.
    assert!(
        rule.contains("border-block-end: none"),
        "Hero-adjacent nav should remove border, got: {}",
        rule
    );
    assert!(
        !rule.contains("padding-block-end: 0") && !rule.contains("padding-block-end:0"),
        "Hero-adjacent nav should KEEP padding-block-end for nav link spacing, got: {}",
        rule
    );
}

#[test]
fn test_css_hero_img_fills_container() {
    // Hero image fills its container and crops via object-fit: cover.
    // The container (.moss-hero) caps the block size; the image stretches to
    // fill. Logical spellings only: a physical twin would size a different
    // axis under vertical typesetting.
    let rule =
        get_css_rule(DEFAULT_CSS, ".moss-hero img").expect(".moss-hero img CSS rule should exist");

    assert!(
        rule.contains("inline-size: 100%"),
        ".moss-hero img should fill the inline axis, got: {}",
        rule
    );
    assert!(
        rule.contains("block-size: 100%") && !rule.contains(" width") && !rule.contains(" height"),
        ".moss-hero img should fill the block axis with logical properties only, got: {}",
        rule
    );
    assert!(
        rule.contains("object-fit: cover") || rule.contains("object-fit:cover"),
        ".moss-hero img should use object-fit: cover, got: {}",
        rule
    );
}

#[test]
fn test_css_hero_scrim_only_under_overlay_text() {
    // The scrim exists to make overlay text legible. An image-only hero has
    // no `.moss-hero-content` at all (render_hero_html_typed returns early),
    // so gating on `:has()` keeps the author's image undarkened. Before this
    // gate the base rule was unconditional and dimmed every hero.
    let base = get_css_rule(DEFAULT_CSS, ".moss-hero::before")
        .expect(".moss-hero::before CSS rule should exist");
    assert!(
        base.contains("content: none"),
        "scrim must be off by default so image-only heroes stay undimmed, got: {}",
        base
    );

    let gated = get_css_rule(DEFAULT_CSS, ".moss-hero:has(.moss-hero-content)::before")
        .expect("scrim re-enable rule should exist");
    assert!(
        gated.contains("content: ''") || gated.contains("content:''"),
        "scrim must be re-enabled for heroes carrying overlay text, got: {}",
        gated
    );

    // The gradient has to be strongest where the text is. `.moss-hero-content`
    // is anchored bottom-left; a 135deg ramp put its maximum in the top-left
    // corner instead, leaving the text zone too pale to carry white type.
    assert!(
        base.contains("to top"),
        "scrim must ramp toward the bottom, where the overlay sits, got: {}",
        base
    );
}

#[test]
fn test_css_hero_light_tone_removes_the_scrim_last() {
    // A pale cover takes NO scrim: the text flips dark instead, so the
    // author's picture arrives unaltered. This rule used to darken harder
    // (0.78 black at the bottom) so that white type would survive, which is
    // the behaviour that read as a dirty band across the artwork.
    let light = get_css_rule(DEFAULT_CSS, ".moss-hero[data-hero-tone=\"light\"]::before")
        .expect("light-tone scrim rule should exist");
    assert!(
        light.contains("content: none"),
        "a pale cover must take no scrim at all, got: {}",
        light
    );
    assert!(
        !light.contains("background"),
        "the light rule removes the scrim; a background here means it is \
         still painting one, got: {}",
        light
    );

    // Source order is the ONLY thing holding this up. The mobile block's
    // `.moss-hero[data-mobile="overlay"]::before { content: '' }` is (0,2,1)
    // and so is this, so moving the light rule earlier silently gives an
    // overlay hero its scrim back below 48rem — which is exactly what
    // happened on the first attempt, and what the hero-tone render gate
    // caught. The gate needs a browser; this does not, so it runs on every PR.
    let overlay_at = DEFAULT_CSS
        .find(".moss-hero[data-mobile=\"overlay\"]::before")
        .expect("mobile-overlay scrim rule should exist");
    let light_at = DEFAULT_CSS
        .find(".moss-hero[data-hero-tone=\"light\"]::before")
        .expect("light-tone scrim rule should exist");
    assert!(
        light_at > overlay_at,
        "the light-tone rule must come after the mobile-overlay scrim rule — \
         they tie on specificity and order is the tiebreak"
    );
}

#[test]
fn test_css_hero_overlay_text_is_white_throughout() {
    // `h1..h6 { color: var(--moss-color-text) }` out-specifies a bare
    // `.moss-hero-content h1`, so hero headings used to repaint themselves
    // body-dark on top of the photo. Colour is owned by the container and
    // handed back by everything that carries its own.
    let container = get_css_rule(DEFAULT_CSS, ".moss-hero-content")
        .expect(".moss-hero-content CSS rule should exist");
    assert!(
        container.contains("color: #ffffff"),
        ".moss-hero-content should own the white overlay colour, got: {}",
        container
    );

    let inherit = get_css_rule(
        DEFAULT_CSS,
        ".moss-hero-content :is(h1, h2, h3, h4, h5, h6, blockquote, li, strong, em)",
    )
    .expect("hero overlay colour-inherit rule should exist");
    assert!(
        inherit.contains("color: inherit"),
        "hero overlay children must inherit the container colour, got: {}",
        inherit
    );
}

#[test]
fn test_css_hero_heading_treatment_is_not_h1_only() {
    // A hero whose opening line is `## Season 3` is ordinary markup — moss's
    // markup contract does not require an `h1` inside `:::hero`. When the size
    // and shadow rules keyed on `h1`, such a heading got the colour override
    // (white) and no shadow of its own: white text on a photograph with
    // nothing behind it. The treatment must follow whatever heading is there.
    let shared = get_css_rule(DEFAULT_CSS, ".moss-hero-content :is(h1, h2, h3, h4, h5, h6)")
        .expect("hero headings should be styled by level-agnostic selector");
    assert!(
        shared.contains("text-shadow: 0 2px 4px"),
        "every hero heading level needs the deeper shadow, got: {}",
        shared
    );
    assert!(
        shared.contains("font-size"),
        "every hero heading level needs a hero-scale size, got: {}",
        shared
    );

    // Hierarchy survives: the FIRST heading in the hero (any level) still
    // gets the display size, so `#` + `##` in one hero is not flattened.
    let lead = get_css_rule(
        DEFAULT_CSS,
        ".moss-hero-content :is(h1, h2, h3, h4, h5, h6):not(:is(h1, h2, h3, h4, h5, h6) ~ *)",
    )
    .expect("hero lead-heading rule should exist");
    assert!(
        lead.contains("font-size: 2.5rem"),
        "the lead hero heading keeps the display size, got: {}",
        lead
    );

    assert!(
        !DEFAULT_CSS.contains(".moss-hero-content h1 {"),
        "no hero rule may key on h1 alone any more"
    );
}

#[test]
fn test_css_footer_container_provides_default_chrome() {
    // The default footer chrome (divider above via ::before, padding,
    // muted typography) lives on `footer.container` directly — no
    // nested `.footer-content` wrapper. This keeps the HTML flat:
    // authored content (footer.md HTML or default link list) sits as
    // direct children of <footer>, allowing site CSS to use
    // `body > footer.container > selector` rules for custom designs
    // (the SoCiviC pattern).
    //
    // Footer renders at the default body font-size; sites that want a
    // smaller footer override `footer.container > * { font-size: ... }`
    // in their own custom.css.
    let rule = get_css_rule(DEFAULT_CSS, "footer.container")
        .expect("footer.container CSS rule should exist");
    assert!(
        rule.contains("color: var(--moss-color-muted)")
            || rule.contains("color:var(--moss-color-muted)"),
        "footer.container should use muted color, got: {}",
        rule
    );
    // Block padding only: the inline inset is `.container`'s, the one owner
    // every band inherits so header rule, listing and footer rule share one
    // extent under vertical-rl.
    assert!(
        rule.contains("padding-block:") && !rule.contains("padding-inline"),
        "footer.container should have block padding and inherit its inline inset, got: {}",
        rule
    );

    let before_rule = get_css_rule(DEFAULT_CSS, "footer.container::before")
        .expect("footer.container::before CSS rule should exist");
    // Logical, not `border-top`: under `writing-mode: vertical-rl` the divider
    // must sit on the footer's right edge, before the strip in reading order
    // (the vertical-measure render gate checks the rendered side).
    assert!(
        before_rule.contains("border-block-start"),
        "footer.container::before should draw the divider via border-block-start, got: {}",
        before_rule
    );
}

#[test]
fn test_css_footer_subscribe_flex_container() {
    // Regression guard for the footer subscribe-form layout, keyed on the
    // `:has(> .moss-subscribe)` selector that replaced the retired
    // `data-moss-shape="default"` attribute.
    //
    // A footer that directly contains a `.moss-subscribe` block (auto-
    // injected OR authored `:::subscribe`) becomes `display: flex` so the
    // link list and form sit side-by-side. The moment the footer is a flex
    // container its `::before` divider becomes a flex ITEM; with empty
    // `content:""` its width collapses to 0 (border-top invisible) and a
    // phantom column-gap shifts the link list right. The paired `::before`
    // rule pins `flex-basis: 100%` so the divider claims a full row.
    //
    // Pins the contract: the flex-container rule and its `::before` override
    // must both exist, and `data-moss-shape` must be gone. (The right-anchor
    // `margin-inline-start: auto` lives in email.css / @layer plugins, not
    // DEFAULT_CSS — verified by the browser measure gate, not here.)
    let flex_rule = get_css_rule(DEFAULT_CSS, "footer.container:has(> .moss-subscribe)")
        .expect("footer :has(> .moss-subscribe) flex rule should exist");
    assert!(
        flex_rule.contains("display: flex") || flex_rule.contains("display:flex"),
        "footer subscribe flex rule should set display:flex, got: {flex_rule}"
    );
    assert!(
        !flex_rule.contains("flex-basis"),
        "base flex rule must not contain flex-basis (that belongs to ::before): {flex_rule}"
    );

    let pseudo_override = get_css_rule(
        DEFAULT_CSS,
        "footer.container:has(> .moss-subscribe)::before",
    )
    .expect(
        "footer :has(> .moss-subscribe)::before override MUST exist — without \
                     `flex-basis: 100%` the divider collapses to a zero-width box and the \
                     link list shifts right by one column-gap.",
    );
    assert!(
        pseudo_override.contains("flex-basis: 100%") || pseudo_override.contains("flex-basis:100%"),
        "footer subscribe ::before override must pin flex-basis:100%. Got: {pseudo_override}"
    );

    assert!(
        !DEFAULT_CSS.contains("[data-moss-shape"),
        "no CSS selector may key on the retired data-moss-shape attribute \
             (a historical mention in a comment is fine)"
    );
}

#[test]
fn test_template_replaces_lang_placeholder() {
    let processor = ShellProcessor::new();
    let vars = ShellVars {
        lang: "zh-hans".to_string(),
        ..make_test_vars(false)
    };
    let result = processor.process(ShellType::Page, vars);
    assert!(
        result.contains(r#"lang="zh-hans""#),
        "Template should contain lang attribute with zh-hans"
    );
    assert!(
        !result.contains(r#"lang="{lang}""#),
        "Template should not contain unreplaced lang placeholder"
    );
}

#[test]
fn test_css_footer_link_underline_uses_text_muted() {
    let rule =
        get_css_rule(DEFAULT_CSS, ".footer-link").expect(".footer-link CSS rule should exist");
    assert!(
        rule.contains("text-decoration-color: var(--moss-color-muted)"),
        "Footer link underline should use --moss-color-muted, got: {}",
        rule
    );
}

#[test]
fn test_css_moss_btn_hover_uses_accent() {
    let rule = get_css_rule(DEFAULT_CSS, ".moss-btn:hover")
        .expect(".moss-btn:hover CSS rule should exist");
    // Task 2.5: .moss-btn:hover uses --moss-color-ui-accent (chrome token)
    // which defaults to var(--moss-color-accent) at :root and can be
    // overridden independently for quiet-chrome themes.
    assert!(
        rule.contains("background: var(--moss-color-ui-accent)"),
        "Button hover should use --moss-color-ui-accent (chrome accent seam), got: {}",
        rule
    );
    assert!(
        !rule.contains("opacity"),
        "Button hover should not use opacity, got: {}",
        rule
    );
}

// =========================================================================
// Content Width 2-Tier System Tests
// =========================================================================

#[test]
fn test_css_root_content_width_is_font_stable() {
    // Font-stable measure: a multiple of --moss-reading-size (a length),
    // not a `ch` value, so text, inline images, and figures resolve to the
    // same column at any element font-size. A `ch` cap drifts per element
    // font and desyncs text from figures when a theme lowers the prose font.
    // See docs/archive/2026-06-04-content-column-measure-consistency-design.md.
    //
    // Task 1.3: token definitions live in tokens.json (not DEFAULT_CSS), so
    // we check the full assembled CSS (generated prefix + DEFAULT_CSS).
    let css = full_site_css();
    assert!(
        css.contains("--moss-content-width: calc(42 * var(--moss-reading-size))"),
        ":root should set --moss-content-width to a font-stable reading-size multiple"
    );
    assert!(
        css.contains("--moss-content-width-sidebar: calc(39 * var(--moss-reading-size))"),
        ":root should set --moss-content-width-sidebar to a font-stable reading-size multiple"
    );
}

#[test]
fn test_css_has_sidebar_layout_does_not_override_content_width() {
    // html.has-sidebar-layout must NOT reassign --moss-content-width
    // (higher specificity would beat user :root overrides in custom.css)
    if let Some(rule) = get_css_rule(DEFAULT_CSS, "html.has-sidebar-layout") {
        assert!(
            !rule.contains("--moss-content-width"),
            "html.has-sidebar-layout must not reassign --moss-content-width, got: {}",
            rule
        );
    }
}

#[test]
fn test_css_sidebar_article_container_uses_sidebar_variable() {
    let rule = get_css_rule(DEFAULT_CSS, "main.has-sidebar > article.container")
        .expect("main.has-sidebar > article.container CSS rule should exist");
    assert!(
        rule.contains("var(--moss-content-width-sidebar)"),
        "Sidebar article should use var(--moss-content-width-sidebar), got: {}",
        rule
    );
}

#[test]
fn test_css_comment_section_matches_article_box_model() {
    // .moss-comments must use the same box model as article.container:
    // content-box (default) + container-padding. Both elements use max-width + padding
    // with content-box so their content areas align.
    let rule = get_css_rule(DEFAULT_CSS, "main > .moss-comments")
        .expect("main > .moss-comments CSS rule should exist");
    assert!(
            !rule.contains("border-box"),
            "main > .moss-comments should NOT use border-box (must match article.container's content-box), got: {}",
            rule
        );
    assert!(
        rule.contains("container-padding"),
        "main > .moss-comments should have container-padding (same as article), got: {}",
        rule
    );
}

#[test]
fn test_css_sidebar_no_overflow_scroll() {
    // Sidebar should not have overflow-y: auto (removed scrollbar)
    let rule = get_css_rule(DEFAULT_CSS, "main.has-sidebar > .latest-sidebar")
        .expect("main.has-sidebar > .latest-sidebar CSS rule should exist");
    assert!(
        !rule.contains("overflow-y"),
        "Sidebar should not have overflow-y (scrollbar removed), got: {}",
        rule
    );
    assert!(
        !rule.contains("max-height"),
        "Sidebar should not have max-height (scrollbar removed), got: {}",
        rule
    );
}

#[test]
fn test_css_sidebar_more_link_styles() {
    let rule = get_css_rule(DEFAULT_CSS, ".latest-sidebar .sidebar-more")
        .expect(".latest-sidebar .sidebar-more CSS rule should exist");
    assert!(
        rule.contains("display: inline-block"),
        ".sidebar-more should be inline-block, got: {}",
        rule
    );
    assert!(
        rule.contains("text-decoration: none"),
        ".sidebar-more should have no text-decoration, got: {}",
        rule
    );
    assert!(
        rule.contains("color: var(--moss-color-muted)"),
        ".sidebar-more should use muted color, got: {}",
        rule
    );
}

#[test]
fn test_css_sidebar_more_link_hover() {
    let rule = get_css_rule(DEFAULT_CSS, ".latest-sidebar .sidebar-more:hover")
        .expect(".latest-sidebar .sidebar-more:hover CSS rule should exist");
    assert!(
        rule.contains("color: var(--moss-color-text)"),
        ".sidebar-more:hover should use text color, got: {}",
        rule
    );
}

// =========================================================================
// Responsive: table, hero, grid card
// =========================================================================

#[test]
fn test_css_table_mobile_overflow() {
    // Wide tables scroll horizontally instead of overflowing the page. The
    // 2026-07-27 table redesign replaced the old mobile-only
    // `@media (max-width:48rem) table { display:block; overflow-x:auto }`
    // with an always-on `.moss-table-scroll` wrapper (with edge shadows), so
    // the guard now checks the wrapper carries the horizontal scroll.
    let rule = get_css_rule(DEFAULT_CSS, ".moss-table-scroll")
        .expect(".moss-table-scroll CSS rule should exist");
    assert!(
        rule.contains("overflow-x: auto"),
        ".moss-table-scroll should scroll horizontally (overflow-x: auto), got: {}",
        rule
    );
}

#[test]
fn test_css_mobile_reading_size_stays_above_cjk_floor() {
    // Mobile shrinks the reading size from the 18px desktop base, but must
    // not drop to (or below) the ~16px legibility floor — that reads too
    // small, especially for dense CJK ideographs. The mobile override now
    // re-points --moss-reading-size-base to 1.0625rem (= 17px): a small
    // viewport reduction from 18px that stays comfortably legible.
    let rule = get_css_rule_in_media(DEFAULT_CSS, "@media (max-width: 48rem)", ":root")
        .expect(":root override should exist in @media (max-width: 48rem)");
    assert!(
            rule.contains("--moss-reading-size-base: 1.0625rem"),
            "Mobile :root should set --moss-reading-size-base to 1.0625rem (17px), not the old 1rem (16px) floor, got: {}",
            rule
        );
    assert!(
        !rule.contains("--moss-reading-size-base: 1rem"),
        "Mobile :root must not shrink body copy back to the 1rem (16px) floor, got: {}",
        rule
    );
}

#[test]
fn test_css_cjk_body_size_and_leading_bump() {
    // CJK ideographs are dense and fill the em-square (no ascenders/
    // descenders), so best practice is a small size bump (~+1px over Latin)
    // with looser leading. The bump is declared on the multiplier inside
    // --moss-reading-size, not on `body { font-size }`: everything built on
    // that size — headings, captions, the content column — takes it too, which
    // is what keeps the hierarchy from compressing by 6% on the one script that
    // cannot show a small size step. Latin body line-height (1.75) is left
    // untouched — only CJK gets 1.8 here.
    let bump = get_css_rule(DEFAULT_CSS, "html[lang^=\"zh\"]")
        .expect("html[lang^=\"zh\"] CSS rule should exist");
    assert!(
        bump.contains("--moss-reading-script-scale: 1.06"),
        "the CJK size bump must live on --moss-reading-script-scale so it reaches the whole \
             reading scale and survives browser zoom, got: {}",
        bump
    );

    let rule = get_css_rule(DEFAULT_CSS, "html[lang^=\"zh\"] body")
        .expect("html[lang^=\"zh\"] body CSS rule should exist");
    assert!(
        rule.contains("line-height: 1.8"),
        "CJK body should use looser 1.8 leading, got: {}",
        rule
    );
    assert!(
        !rule.contains("font-size"),
        "CJK body must NOT re-declare a font-size — that is the collision the \
             reading scale removed, got: {}",
        rule
    );
}

#[test]
fn test_css_dark_mode_preserves_user_accent() {
    // Dark mode now DOES set --moss-color-accent to a legible green in the
    // generated tokens: #6a9a5a (~5.3:1 on --moss-color-bg) replaces the raw
    // #2d5a2d (~2.2:1, failing WCAG AA as text/border/icon) — see 5b6c7a071.
    // User customizations still persist across themes because the generated
    // token blocks live in `@layer tokens`, and the layer order
    // (`tokens` < `themes`) lets a user's `.moss/theme/style.css` (@layer
    // themes) and an unlayered `custom.css` both win the cascade.
    let css = full_site_css();

    // Layer order must place `tokens` before `themes`, so user overrides win.
    assert!(
        css.contains("@layer reset, tokens, base, layout, shortcodes, plugins, themes;"),
        "layer order must declare tokens before themes so user accent overrides win, got: {}",
        css.lines()
            .find(|l| l.contains("@layer"))
            .unwrap_or("<no @layer line>")
    );

    // Explicit toggle block — selector is `[data-theme="dark"]` (no :root prefix).
    let rule = get_css_rule(&css, "[data-theme=\"dark\"]")
        .expect("[data-theme=\"dark\"] CSS rule should exist");
    assert!(
        rule.contains("--moss-color-accent:") && rule.contains("#6a9a5a"),
        "[data-theme=\"dark\"] should set --moss-color-accent to the legible #6a9a5a, got: {}",
        rule
    );
}

#[test]
fn test_css_hero_mobile_stacks_overlay_below_image() {
    // On mobile, hero with an overlay (:has(.moss-hero-content)) stacks
    // the text below the image in a flex column. The earlier behavior
    // (aspect-ratio crop + object-fit:cover) cut wide images on both
    // sides on narrow viewports — see 67a26c540 "fix(hero): stack text
    // below image on mobile instead of cropping". The text now flows
    // below the image in a solid block; the image shows at full natural
    // width.
    //
    // Hero-responsive fix (#508) earlier narrowed the rule from bare
    // `.moss-hero` to `.moss-hero:has(.moss-hero-content)` so image-only
    // heroes keep their natural aspect ratio. That narrowing still
    // applies; only the with-overlay arm changed from crop to stack.
    //
    // Keep the selector at exactly this specificity (0-2-0): the mobile
    // overlay-mode block below relies on `.moss-hero[data-mobile="overlay"]`
    // tying with it and winning on source order.
    let rule = get_css_rule_in_media(
        DEFAULT_CSS,
        "@media (max-width: 48rem)",
        ".moss-hero:has(.moss-hero-content)",
    )
    .expect(".moss-hero:has(.moss-hero-content) mobile CSS rule should exist");
    assert!(
        rule.contains("flex-direction: column"),
        "Mobile hero-with-overlay should stack as flex column (no crop), got: {}",
        rule
    );
    assert!(
        !rule.contains("aspect-ratio"),
        "Mobile hero-with-overlay should NOT force aspect-ratio (would crop wide images), got: {}",
        rule
    );
}

#[test]
fn test_css_grid_card_link_no_underline() {
    // Grid LINK cards (`data-kind="link"` — the cell IS a link) strip the
    // underline from their inner text: cards are navigational, not inline
    // text links. This is SCOPED to `[data-kind="link"]` so that PROSE
    // cells (`<div class="moss-grid-card">` with no data-kind, e.g. a bio
    // paragraph containing a `[CV](…)` link) keep the standard `article a`
    // underline. Internal link-cards (render_link_card) and Block::LinkCard
    // both carry `data-kind="link"`, so their card affordance is preserved;
    // external link-previews have no nested <a> and use `.link-preview`.
    let rule = get_css_rule(DEFAULT_CSS, ".moss-grid-card[data-kind=\"link\"] a")
        .expect(".moss-grid-card[data-kind=\"link\"] a CSS rule should exist");
    assert!(
        rule.contains("text-decoration: none"),
        "Grid link-card inner links should have text-decoration: none, got: {}",
        rule
    );

    // The OLD unscoped `.moss-grid-card a` rule must be gone — it stripped
    // underlines from prose-cell inline links too (the bug this fixes).
    assert!(
        get_css_rule(DEFAULT_CSS, ".moss-grid-card a").is_none(),
        "Unscoped `.moss-grid-card a` rule must not exist — it would strip \
             underlines from inline links in :::grid prose cells"
    );
}

// =========================================================================
// Template: has-sidebar-layout class on <html>
// =========================================================================

/// Helper to build ShellVars with sensible defaults for testing.
fn make_test_vars(has_sidebar_layout: bool) -> ShellVars {
    ShellVars {
        title: "Test".to_string(),
        css_path: "style.css".to_string(),
        js_path: "theme.js".to_string(),
        lazy_chunk_attrs: String::new(),
        navigation: "".to_string(),
        nav_island: String::new(),
        homepage_content: "".to_string(),
        latest_list: None,
        latest_sidebar: None,
        favicon: None,
        rss_link: None,
        analytics: None,
        footer: None,
        body_attrs: String::new(),
        page_wrapper_class: String::new(),
        hero_section: None,
        share_cover_attr: String::new(),
        share_qr_attr: String::new(),
        main_class: String::new(),
        date: None,
        formatted_date: None,
        date_line: None,
        short_date: None,
        content: None,
        description: None,
        og_tags: None,
        twitter_tags: None,
        canonical_link: None,
        hreflang_links: None,
        apple_touch_icon: None,
        schema_json_ld: None,
        content_width_attr: String::new(),
        typesetting_attr: String::new(),
        comments_attr: String::new(),
        lang: "en".to_string(),
        ui_lang: crate::i18n::Language::En,
        user_css_link: None,
        user_js_tag: None,
        has_sidebar_layout,
        embed_head_assets: String::new(),
        post_article: String::new(),
        runtime_js_tags: String::new(),
        robots_meta: None,
    }
}

/// Minimal-vars helper for composition tests (no sidebar).
fn test_vars() -> ShellVars {
    make_test_vars(false)
}

#[test]
fn a_frontmatter_title_cannot_close_the_title_element() {
    // `<title>` is RCDATA: `</title>` ends it, and everything after is parsed
    // as markup. The same title is already escaped for `og:title`.
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.title = "Hi</title><script>alert(1)</script>".to_string();
    let result = processor.process(ShellType::Page, vars);
    assert!(!result.contains("<script>alert(1)</script>"), "title broke out");
    assert!(result.contains("&lt;/title&gt;"));
}

#[test]
fn a_language_tag_cannot_break_out_of_the_html_lang_attribute() {
    // Second lock behind the allowlist in `resolve_site_default_lang`: if a
    // future producer ever reaches `<html lang>` without passing it, the value
    // still cannot close the attribute and open a tag.
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.lang = r#"en" onload="alert(1)"#.to_string();
    let result = processor.process(ShellType::Page, vars);
    assert!(!result.contains(r#"onload="alert(1)"#), "attribute broke out: {}", &result[..400]);
    assert!(result.contains("&quot;"), "expected the quote to be escaped");
}

#[test]
fn test_template_has_sidebar_layout_true_adds_class() {
    let processor = ShellProcessor::new();
    let vars = make_test_vars(true);
    let result = processor.process(ShellType::Page, vars);
    assert!(
        result.contains(r#"class="has-sidebar-layout""#),
        "When has_sidebar_layout is true, <html> should have class=\"has-sidebar-layout\""
    );
}

#[test]
fn test_template_has_sidebar_layout_false_no_class() {
    let processor = ShellProcessor::new();
    let vars = make_test_vars(false);
    let result = processor.process(ShellType::Page, vars);
    assert!(
        !result.contains("has-sidebar-layout"),
        "When has_sidebar_layout is false, output should not contain has-sidebar-layout"
    );
}

#[test]
fn test_homepage_template_includes_description_and_og_tags() {
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.description = Some("看星星，食烟火。".to_string());
    vars.og_tags = Some(r#"<meta property="og:type" content="website">"#.to_string());
    let result = processor.process(ShellType::Page, vars);
    assert!(
        result.contains(r#"<meta name="description" content="看星星，食烟火。">"#),
        "Homepage should contain meta description when provided"
    );
    assert!(
        result.contains(r#"og:type" content="website"#),
        "Homepage should contain OG tags when provided"
    );
}

#[test]
fn test_template_emits_canonical_link_when_provided() {
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.canonical_link =
        Some(r#"<link rel="canonical" href="https://www.example.com/">"#.to_string());
    let result = processor.process(ShellType::Page, vars);
    assert!(
        result.contains(r#"<link rel="canonical" href="https://www.example.com/">"#),
        "should render canonical link tag when provided"
    );
}

#[test]
fn test_template_emits_twitter_tags_when_provided() {
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.twitter_tags =
        Some(r#"<meta name="twitter:card" content="summary_large_image">"#.to_string());
    let result = processor.process(ShellType::Page, vars);
    assert!(
        result.contains(r#"<meta name="twitter:card" content="summary_large_image">"#),
        "should render twitter card meta tag when provided"
    );
}

#[test]
fn test_template_has_sidebar_layout_article_template() {
    let processor = ShellProcessor::new();
    let vars = make_test_vars(true);
    let result = processor.process(ShellType::Article, vars);
    assert!(
        result.contains(r#"class="has-sidebar-layout""#),
        "Article template should also support has-sidebar-layout class"
    );
}

// =========================================================================
// Content width: data-content-width attribute on <body>
// =========================================================================

#[test]
fn test_template_content_width_wide() {
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.content_width_attr = r#" data-content-width="wide""#.to_string();
    let result = processor.process(ShellType::Page, vars);
    assert!(
        result.contains(r#"data-content-width="wide""#),
        "Page template should contain data-content-width attribute when set"
    );
}

#[test]
fn test_template_content_width_empty_by_default() {
    let processor = ShellProcessor::new();
    let vars = make_test_vars(false);
    let result = processor.process(ShellType::Page, vars);
    assert!(
        !result.contains("data-content-width"),
        "Page template should not contain data-content-width when empty"
    );
}

#[test]
fn test_template_content_width_article_template() {
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.content_width_attr = r#" data-content-width="full""#.to_string();
    let result = processor.process(ShellType::Article, vars);
    assert!(
        result.contains(r#"data-content-width="full""#),
        "Article template should contain data-content-width attribute when set"
    );
}

// =========================================================================
// body_attrs: data-moss-preview attribute
// =========================================================================

#[test]
fn test_body_attrs_preview_mode() {
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.body_attrs = " data-moss-preview".to_string();
    let result = processor.process(ShellType::Page, vars);
    assert!(
        result.contains("data-moss-preview"),
        "Page template should contain data-moss-preview when body_attrs is set"
    );
}

#[test]
fn test_body_attrs_production_mode() {
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.body_attrs = String::new();
    let result = processor.process(ShellType::Page, vars);
    assert!(
        !result.contains("data-moss-preview"),
        "Page template should not contain data-moss-preview in production"
    );
}

#[test]
fn test_body_attrs_article_template() {
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.body_attrs = " data-moss-preview".to_string();
    let result = processor.process(ShellType::Article, vars);
    assert!(
        result.contains("data-moss-preview"),
        "Article template should contain data-moss-preview when body_attrs is set"
    );
}

// =========================================================================
// Preview-mode chrome annotation: data-source-none
// =========================================================================

#[test]
fn template_marks_series_nav_as_source_none_in_preview_mode() {
    // series-nav is emitted as `<nav class="moss-series-nav">` into
    // {post_article}.  The existing replacen only catches the FIRST <nav
    // (the main-nav).  This test guards that series-nav also gets
    // data-source-none so users don't see a stale source annotation on
    // prev/next links.
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.body_attrs = " data-moss-preview".to_string();
    // Simulate the series-nav HTML that the build pipeline injects via
    // components::series_nav::render (exact opening tag must match).
    vars.post_article =
        r#"<nav class="moss-series-nav"><div class="moss-series-nav-links"></div></nav>"#
            .to_string();
    let result = processor.process(ShellType::Article, vars);
    assert!(
        result.contains(r#"<nav class="moss-series-nav" data-source-none"#)
            || result.contains(r#"<nav data-source-none class="moss-series-nav""#),
        "series-nav <nav> must carry data-source-none in preview mode. Got snippet: {}",
        &result[result
            .find("moss-series-nav")
            .map_or(0, |i| i.saturating_sub(20))
            ..result
                .find("moss-series-nav")
                .map_or(result.len(), |i| (i + 60).min(result.len()))]
    );
}

#[test]
fn template_series_nav_not_source_none_in_production_mode() {
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.body_attrs = String::new(); // no preview mode
    vars.post_article =
        r#"<nav class="moss-series-nav"><div class="moss-series-nav-links"></div></nav>"#
            .to_string();
    let result = processor.process(ShellType::Article, vars);
    // In production the series-nav must NOT carry data-source-none
    assert!(
        !result.contains(r#"<nav class="moss-series-nav" data-source-none"#)
            && !result.contains(r#"<nav data-source-none class="moss-series-nav""#),
        "series-nav must NOT have data-source-none in production mode"
    );
}

#[test]
fn template_marks_font_anchor_as_source_none_in_preview_mode() {
    // font-anchor is the reading-preferences widget (font-trigger button +
    // font-pill popup) injected as template chrome into the article
    // date-line.  In the build pipeline the date-line is spliced into
    // homepage_content which becomes vars.content for article pages.
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.body_attrs = " data-moss-preview".to_string();
    // Use vars.content — the article template expands {content}, not {homepage_content}.
    vars.content = Some(r#"<div class="date-line"><span class="date">2026</span><div class="font-anchor"><button class="font-trigger size-std"></button></div></div>"#.to_string());
    let result = processor.process(ShellType::Article, vars);
    assert!(
            result.contains(r#"<div class="font-anchor" data-source-none"#)
                || result.contains(r#"<div data-source-none class="font-anchor""#),
            "font-anchor must carry data-source-none in preview mode. Got snippet containing font-anchor: {}",
            result.find("font-anchor").map_or("(not found)", |i| &result[i.saturating_sub(10)..(i + 60).min(result.len())])
        );
}

#[test]
fn template_font_anchor_not_source_none_in_production_mode() {
    let processor = ShellProcessor::new();
    let mut vars = make_test_vars(false);
    vars.body_attrs = String::new();
    vars.content = Some(r#"<div class="date-line"><span class="date">2026</span><div class="font-anchor"><button class="font-trigger size-std"></button></div></div>"#.to_string());
    let result = processor.process(ShellType::Article, vars);
    assert!(
        !result.contains(r#"<div class="font-anchor" data-source-none"#)
            && !result.contains(r#"<div data-source-none class="font-anchor""#),
        "font-anchor must NOT have data-source-none in production mode"
    );
}

// =========================================================================
// CSS: content_width presets
// =========================================================================

#[test]
fn test_css_content_width_wide_preset() {
    let rule = get_css_rule(DEFAULT_CSS, r#"body[data-content-width="wide"]"#)
        .expect("CSS should have body[data-content-width=\"wide\"] rule");
    assert!(
            rule.contains("--moss-content-width: calc(50 * var(--moss-reading-size))"),
            "Wide preset should set --moss-content-width to a font-stable reading-size multiple, got: {}",
            rule
        );
}

#[test]
fn test_css_content_width_full_preset() {
    let rule = get_css_rule(DEFAULT_CSS, r#"body[data-content-width="full"]"#)
        .expect("CSS should have body[data-content-width=\"full\"] rule");
    assert!(
        rule.contains("--moss-content-width: var(--moss-site-max-width)"),
        "Full preset should set --moss-content-width to site max width, got: {}",
        rule
    );
}

// =========================================================================
// CSS: content-width escape (ADR-021 Corollary 2, data-width bands)
// =========================================================================

#[test]
fn test_css_content_width_escape_rules() {
    // main is the query container the escape resolves 100cqw against.
    assert!(
        DEFAULT_CSS.contains("main {\n  container-type: inline-size;\n}"),
        "main must be an inline-size query container for the data-width escape"
    );
    // container-type implies contain:layout, which would make `main` the
    // containing block for the position:fixed immersive iframe — this
    // override is the single highest-risk line of the escape; see the
    // comment block above the rule and the immersive check in
    // tests/render-gates/site/content-width-escape.spec.js.
    assert!(
        DEFAULT_CSS.contains(".immersive-fs-active main {\n  container-type: normal;\n}"),
        "immersive fullscreen must turn the query container off"
    );
    // The three band widths. `page` MUST be --moss-site-max-width itself
    // (not a copy) so the band stays aligned with nav/footer .container.
    assert!(
        DEFAULT_CSS
            .contains(r#"article.container > [data-width="wide"]   { --moss-escape: var(--moss-width-wide); }"#),
        "wide band missing"
    );
    assert!(
        DEFAULT_CSS
            .contains(r#"article.container > [data-width="page"]   { --moss-escape: var(--moss-site-max-width); }"#),
        "page band missing"
    );
    assert!(
        DEFAULT_CSS
            .contains(r#"article.container > [data-width="screen"] { --moss-escape: 100cqw; }"#),
        "screen band missing"
    );
    // The shared rule clamps to the container: cqw, never vw (100vw
    // includes a classic scrollbar's gutter and clips silently under
    // body{overflow-x:hidden} — the rejected option B failure mode).
    let rule = get_css_rule(DEFAULT_CSS, "article.container > [data-width]")
        .expect("shared data-width escape rule must exist");
    assert!(
        rule.contains("width: min(var(--moss-escape, 100%), 100cqw)"),
        "escape width must clamp to 100cqw, got: {rule}"
    );
    assert!(
        !rule.contains("100vw"),
        "the escape must never size against 100vw (scrollbar gutter), got: {rule}"
    );
}

#[test]
fn test_css_width_wide_token_is_clamped() {
    // --moss-width-wide must clamp to --moss-site-max-width: the reader
    // font-scale control scales --moss-reading-size (xlarge = 1.25×), and
    // an unclamped 56× multiple would overrun the nav alignment.
    let css = full_site_css();
    assert!(
        css.contains(
            "--moss-width-wide: min(calc(56 * var(--moss-reading-size)), var(--moss-site-max-width))"
        ),
        "--moss-width-wide token missing or missing its site-max clamp"
    );
}

#[test]
fn test_css_content_width_escape_collapses_on_mobile() {
    let rule = get_css_rule_in_media(
        DEFAULT_CSS,
        "@media (max-width: 48rem)",
        "article.container > [data-width]",
    )
    .expect("data-width escape must collapse below 48rem");
    assert!(
        rule.contains("width: auto") && rule.contains("transform: none"),
        "mobile collapse must undo the escape, got: {rule}"
    );
}

#[test]
fn test_css_default_content_width_is_font_stable() {
    // The :root default is a font-stable multiple of --moss-reading-size
    // (≈ the prior 67ch for the default font) so the column does not desync
    // from text when a theme changes the prose font-size.
    //
    // Task 1.3: token definitions live in tokens.json (not DEFAULT_CSS), so
    // we check the full assembled CSS (generated prefix + DEFAULT_CSS).
    let css = full_site_css();
    assert!(
        css.contains("--moss-content-width: calc(42 * var(--moss-reading-size))"),
        "Default --moss-content-width should be a font-stable reading-size multiple"
    );
}

// =========================================================================
// Nav width: decoupled from content width via --moss-nav-width
// =========================================================================

#[test]
fn test_css_nav_width_not_aliased_in_root() {
    // --moss-nav-width must NOT be pre-bound to content-width in :root.
    // Custom properties resolve at their declaring element, so a :root alias
    // `--moss-nav-width: var(--moss-content-width)` freezes at :root's value
    // (67ch) and the <body>-level content_width=wide preset never reaches the
    // nav. It is now an opt-in escape hatch: unset by default, settable in
    // custom.css. The nav tracks content via the fallback in .main-nav.
    //
    // Scope the check to the :root block — a bare `DEFAULT_CSS.contains`
    // would also match the .main-nav comment that mentions `--moss-nav-width:`
    // while explaining this history, and the fallback
    // `var(--moss-nav-width, ...)` references the name without declaring it.
    let root_block = get_css_rule(DEFAULT_CSS, ":root").expect(":root block should exist");
    assert!(
        !root_block.contains("--moss-nav-width:"),
        "--moss-nav-width must not be declared in :root by default (opt-in escape hatch); \
             a :root alias freezes it and breaks the wide preset, got :root: {}",
        root_block
    );
}

#[test]
fn test_css_main_nav_falls_back_to_content_width() {
    // The nav reads --moss-nav-width with --moss-content-width as the
    // fallback, so with --moss-nav-width unset the nav inherits the
    // (possibly per-page-overridden) content width and aligns with article
    // text. Setting --moss-nav-width in custom.css overrides it.
    let rule = get_css_rule(DEFAULT_CSS, ".main-nav").expect(".main-nav CSS rule should exist");
    assert!(
        rule.contains("var(--moss-nav-width, var(--moss-content-width))"),
        ".main-nav max-width should be var(--moss-nav-width, var(--moss-content-width)), got: {}",
        rule
    );
}

#[test]
fn test_css_footer_uses_content_width() {
    // footer.container mirrors article.container's box model exactly so
    // footer text aligns with body text above at every viewport. Both use
    // --moss-content-width directly; the divider is drawn by an inner
    // pseudo-element to land on the same x as body text.
    let rule = get_css_rule(DEFAULT_CSS, "footer.container")
        .expect("footer.container CSS rule should exist");
    assert!(
            rule.contains("--moss-content-width"),
            "footer.container should use --moss-content-width for max-width (mirroring article.container), got: {}",
            rule
        );
}

#[test]
fn test_css_content_width_wide_does_not_set_nav_width() {
    // body[data-content-width="wide"] sets --moss-content-width on <body>; it
    // must NOT also set --moss-nav-width. The nav reads
    // var(--moss-nav-width, var(--moss-content-width)) and inherits this
    // <body>-level content width through the fallback, so the nav tracks the
    // wider column automatically. Setting --moss-nav-width here would override
    // the user's opt-in escape hatch.
    let rule = get_css_rule(DEFAULT_CSS, r#"body[data-content-width="wide"]"#)
        .expect(r#"body[data-content-width="wide"] CSS rule should exist"#);
    assert!(
        !rule.contains("--moss-nav-width"),
        r#"body[data-content-width="wide"] should NOT set --moss-nav-width (nav inherits content-width via fallback), got: {}"#,
        rule
    );
}

#[test]
fn test_css_content_width_full_does_not_set_nav_width() {
    // body[data-content-width="full"] must NOT set --moss-nav-width — the nav
    // inherits --moss-content-width via the fallback in .main-nav.
    let rule = get_css_rule(DEFAULT_CSS, r#"body[data-content-width="full"]"#)
        .expect(r#"body[data-content-width="full"] CSS rule should exist"#);
    assert!(
        !rule.contains("--moss-nav-width"),
        r#"body[data-content-width="full"] should NOT set --moss-nav-width (nav inherits content-width via fallback), got: {}"#,
        rule
    );
}

// =========================================================================
// CSS Separation: site.css must be self-contained (no tokens.css dependency)
// =========================================================================

#[test]
fn test_default_css_does_not_contain_app_only_tokens() {
    // tokens.css defines app-only variables that should NOT appear in site CSS
    assert!(
        !DEFAULT_CSS.contains("--moss-hover-overlay"),
        "DEFAULT_CSS should not contain --moss-hover-overlay (app-only token from tokens.css)"
    );
    assert!(
        !DEFAULT_CSS.contains("--moss-active-overlay"),
        "DEFAULT_CSS should not contain --moss-active-overlay (app-only token from tokens.css)"
    );
    assert!(
        !DEFAULT_CSS.contains("--moss-success-subtle"),
        "DEFAULT_CSS should not contain --moss-success-subtle (app-only token from tokens.css)"
    );
    assert!(
        !DEFAULT_CSS.contains("--moss-accent-light"),
        "DEFAULT_CSS should not contain --moss-accent-light (app-only token from tokens.css)"
    );
}

#[test]
fn test_default_css_contains_primitives() {
    // site.css must include primitive component styles (previously from primitives.css)
    assert!(
        DEFAULT_CSS.contains(".moss-btn"),
        "DEFAULT_CSS should contain .moss-btn primitive"
    );
    assert!(
        DEFAULT_CSS.contains(".moss-input"),
        "DEFAULT_CSS should contain .moss-input primitive"
    );
    assert!(
        DEFAULT_CSS.contains(".moss-link"),
        "DEFAULT_CSS should contain .moss-link primitive"
    );
}

#[test]
fn test_default_css_contains_site_tokens() {
    // site.css must define all site-specific tokens
    assert!(
        DEFAULT_CSS.contains("--moss-font-body"),
        "DEFAULT_CSS should contain --moss-font-body"
    );
    assert!(
        DEFAULT_CSS.contains("--moss-color-accent"),
        "DEFAULT_CSS should contain --moss-color-accent"
    );
    assert!(
        DEFAULT_CSS.contains("--moss-code-background"),
        "DEFAULT_CSS should contain --moss-code-background (site-only token)"
    );
    assert!(
        DEFAULT_CSS.contains("--moss-space-md"),
        "DEFAULT_CSS should contain --moss-space-md (site-only token)"
    );
}

#[test]
fn test_css_collection_cover_row_uses_wide_gap() {
    // Desktop: collection cover row should use 4rem gap (64px) for breathing room
    let rule = get_css_rule(DEFAULT_CSS, ".moss-collection-cover-row")
        .expect(".moss-collection-cover-row CSS rule should exist");
    assert!(
            rule.contains("gap: 4rem"),
            ".moss-collection-cover-row should use 4rem gap for more space between cover and title, got: {}",
            rule
        );
}

#[test]
fn test_css_collection_cover_body_centers_vertically() {
    // Desktop: the title + lead block is vertically centered against the
    // cover — the row stretches and the body is a centered flex column.
    // This replaces the old fixed `padding-top: 2.5rem` offset so the title
    // rises as the lead grows (best-effort vertical alignment).
    let row = get_css_rule(DEFAULT_CSS, ".moss-collection-cover-row")
        .expect(".moss-collection-cover-row CSS rule should exist");
    assert!(
        row.contains("align-items: stretch"),
        ".moss-collection-cover-row should stretch so the body can center, got: {}",
        row
    );
    let body = get_css_rule(DEFAULT_CSS, ".moss-collection-cover-body")
        .expect(".moss-collection-cover-body CSS rule should exist");
    assert!(
        body.contains("justify-content: center"),
        ".moss-collection-cover-body should center the title + lead vertically, got: {}",
        body
    );
    assert!(
        !body.contains("padding-top"),
        ".moss-collection-cover-body should no longer use a fixed padding-top offset, got: {}",
        body
    );
}

#[test]
fn test_css_collection_cover_row_mobile_resets_gap() {
    // Mobile: collection cover row should reset to lg gap
    let rule = get_css_rule_in_media(
        DEFAULT_CSS,
        "@media (max-width: 48rem)",
        ".moss-collection-cover-row",
    )
    .expect(".moss-collection-cover-row mobile CSS rule should exist");
    assert!(
        rule.contains("gap: var(--moss-space-lg)"),
        ".moss-collection-cover-row on mobile should reset gap to lg, got: {}",
        rule
    );
}

#[test]
fn test_css_collection_cover_body_mobile_top_aligns() {
    // Mobile: the compact stack keeps the lead top-aligned against the
    // smaller cover (the desktop `justify-content: center` is overridden).
    let rule = get_css_rule_in_media(
        DEFAULT_CSS,
        "@media (max-width: 48rem)",
        ".moss-collection-cover-body",
    )
    .expect(".moss-collection-cover-body mobile CSS rule should exist");
    assert!(
        rule.contains("justify-content: flex-start"),
        ".moss-collection-cover-body on mobile should top-align the lead, got: {}",
        rule
    );
}

// ── select_shell_type tests ──────────────────────────────────────

fn make_doc(url_path: &str, is_index: bool) -> crate::build::types::ParsedDocument {
    let kind = if is_index {
        PageKind::Folder
    } else {
        PageKind::Article
    };
    crate::build::types::ParsedDocument {
        url_path: url_path.to_string(),
        kind,
        ..Default::default()
    }
}

#[test]
fn process_composes_shell_with_content_fragment() {
    let vars = ShellVars {
        content: Some("ARTICLE_BODY".into()),
        ..test_vars()
    };
    let out = ShellProcessor::new().process(ShellType::Article, vars);
    assert!(out.contains("<header>"));
    assert!(out.contains("ARTICLE_BODY"));
    assert!(!out.contains("{main_content}"));
}

#[test]
fn process_does_not_resplice_main_content_from_a_var_value() {
    let vars = ShellVars {
        content: Some("see {main_content} marker".into()),
        ..test_vars()
    };
    let out = ShellProcessor::new().process(ShellType::Article, vars);
    // The shell splice runs ONCE over pure template text; a var value
    // containing the token must not cause a re-splice AND must survive
    // verbatim — substituted values are never rescanned (#847).
    assert!(
        out.contains("see {main_content} marker"),
        "literal {{main_content}} in author content must survive verbatim"
    );
}

// =========================================================================
// #847: literal {token} text in AUTHOR content must never be eaten
// =========================================================================

#[test]
fn process_preserves_literal_tokens_in_article_content() {
    // Real-world shapes from the live docs site: inline code and prose
    // containing {name}, {title}, {value}, {main_content}. All four must
    // render literally — substituted values are never rescanned for
    // placeholders or stripped (#847).
    let body = concat!(
        "<p>Plugins live in <code>.moss/plugins/{name}/</code>.</p>",
        "<p>Use {title} in your template, and set {value} in the manifest.</p>",
        "<p>The shell slot is {main_content}.</p>",
    );
    let vars = ShellVars {
        content: Some(body.to_string()),
        ..test_vars()
    };
    let out = ShellProcessor::new().process(ShellType::Article, vars);
    assert!(
        out.contains(".moss/plugins/{name}/"),
        "inline-code {{name}} must survive (rendered as .moss/plugins//: the #847 bug)"
    );
    assert!(
        out.contains("Use {title} in your template"),
        "prose {{title}} must survive"
    );
    assert!(
        out.contains("set {value} in the manifest"),
        "prose {{value}} must survive"
    );
    assert!(
        out.contains("The shell slot is {main_content}."),
        "prose {{main_content}} must survive"
    );
}

#[test]
fn process_preserves_literal_tokens_in_homepage_content() {
    // Same guarantee on the Page template's {homepage_content} slot.
    let body = "<p>i18n example: strings like {name} and {value} interpolate at runtime.</p>";
    let vars = ShellVars {
        homepage_content: body.to_string(),
        ..test_vars()
    };
    let out = ShellProcessor::new().process(ShellType::Page, vars);
    assert!(out.contains("strings like {name} and {value} interpolate"));
}

/// #1013: the Page template used to drop both halves of the comment
/// contract on the floor — no `{comments_attr}`, no `<!-- slot:after-article
/// -->` — so moss accepted `comments:` on a folder page, computed the value,
/// and rendered nothing. Both templates must carry both, exactly once: a
/// second marker would inject the whole comment section twice.
#[test]
fn both_templates_carry_the_comment_attribute_and_slot() {
    for tt in [ShellType::Page, ShellType::Article] {
        let vars = ShellVars {
            comments_attr: r#" data-comments="true""#.to_string(),
            ..test_vars()
        };
        let out = ShellProcessor::new().process(tt, vars);
        assert_eq!(
            out.matches(r#"<article class="container" data-comments="true">"#).count(),
            1,
            "{tt:?} must place the comments attribute on <article> exactly once"
        );
        assert_eq!(
            out.matches("<!-- slot:after-article -->").count(),
            1,
            "{tt:?} must offer the after-article injection point exactly once"
        );
    }
}

/// The share card reads the page's cover off `<article class="container">`,
/// so the article template has to put it there — and put nothing there when
/// the page has no cover. A selector that matches nothing fails silently:
/// the page still renders, the cover strip just never appears (which is how
/// `.article-cover` survived for weeks).
#[test]
fn article_template_carries_the_share_cover_attribute() {
    let vars = ShellVars {
        share_cover_attr: r#" data-share-cover="/posts/a/assets/hero.jpg""#.to_string(),
        ..test_vars()
    };
    let out = ShellProcessor::new().process(ShellType::Article, vars);
    assert_eq!(
        out.matches(r#"<article class="container" data-share-cover="/posts/a/assets/hero.jpg">"#)
            .count(),
        1,
        "the article template must place the share cover on <article> exactly once"
    );
}

#[test]
fn article_without_a_cover_emits_no_share_cover_attribute() {
    let out = ShellProcessor::new().process(ShellType::Article, test_vars());
    assert!(!out.contains("data-share-cover"));
    assert!(out.contains(r#"<article class="container">"#));
}

/// Same contract, for the QR code: the page names the code that belongs to it,
/// and the card fetches what it is told. When the card derived the path itself
/// instead, every disagreement between the two derivations produced a code
/// pointing at the site root — or no code at all.
#[test]
fn both_templates_carry_the_share_qr_attribute() {
    for kind in [ShellType::Article, ShellType::Page] {
        let vars = ShellVars {
            share_qr_attr: r#" data-share-qr="/qr/posts-a.svg""#.to_string(),
            ..test_vars()
        };
        let out = ShellProcessor::new().process(kind, vars);
        assert_eq!(
            out.matches(r#" data-share-qr="/qr/posts-a.svg""#).count(),
            1,
            "{kind:?} must place the share QR on <article> exactly once"
        );
    }
}

#[test]
fn a_page_with_no_qr_emits_no_share_qr_attribute() {
    let out = ShellProcessor::new().process(ShellType::Article, test_vars());
    assert!(!out.contains("data-share-qr"));
}

#[test]
fn process_never_rescans_substituted_values_for_other_tokens() {
    // A value containing ANOTHER known placeholder must stay literal:
    // substitution reads template text only, values are emitted verbatim.
    let vars = ShellVars {
        content: Some("try {css_path} or {navigation} literally".into()),
        ..test_vars()
    };
    let out = ShellProcessor::new().process(ShellType::Article, vars);
    assert!(
        out.contains("try {css_path} or {navigation} literally"),
        "known-token literals inside a substituted value must not be re-substituted or stripped"
    );
}

#[test]
fn process_preserves_cjk_and_double_brace_tokens_in_content() {
    // {名称} (non-ASCII) and {{embed}} previously survived only by accident;
    // pin that they survive by construction now.
    let vars = ShellVars {
        content: Some("CJK {名称} and wiki {{embed}} stay".into()),
        ..test_vars()
    };
    let out = ShellProcessor::new().process(ShellType::Article, vars);
    assert!(out.contains("CJK {名称} and wiki {{embed}} stay"));
}

#[test]
fn select_shell_type_never_returns_visual() {
    let mut d = make_doc("gallery.html", false);
    d.hero_html = Some("<section class=\"moss-hero\">x</section>".into());
    d.html_content = String::new();
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&d), false, true),
        ShellType::Page
    );
}

#[test]
fn article_in_subfolder_uses_article_template() {
    let doc = make_doc("writings/hello/index.html", false);
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&doc), false, true),
        ShellType::Article
    );
}

#[test]
fn root_level_doc_uses_page_template() {
    let doc = make_doc("about.html", false);
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&doc), false, true),
        ShellType::Page
    );
}

#[test]
fn homepage_uses_page_template() {
    let doc = make_doc("index.html", true);
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&doc), true, true),
        ShellType::Page
    );
}

#[test]
fn folder_index_uses_page_template_not_article() {
    // Folder index pages like "writings/index.html" should NOT get
    // the article template — they don't need Aa panel or selection actions.
    let doc = make_doc("writings/index.html", true);
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&doc), false, true),
        ShellType::Page
    );
}

#[test]
fn layout_page_overrides_subfolder_non_index_to_page() {
    let mut doc = make_doc("writings/hello/index.html", false);
    doc.layout = Some("page".to_string());
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&doc), false, true),
        ShellType::Page
    );
}

#[test]
fn layout_article_overrides_root_level_to_article() {
    let mut doc = make_doc("manifesto.html", false);
    doc.layout = Some("article".to_string());
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&doc), false, true),
        ShellType::Article
    );
}

#[test]
fn layout_article_overrides_index_to_article() {
    let mut doc = make_doc("writings/index.html", true);
    doc.layout = Some("article".to_string());
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&doc), false, true),
        ShellType::Article
    );
}

#[test]
fn layout_unknown_ignored_falls_through_to_default() {
    let mut doc = make_doc("writings/hello/index.html", false);
    doc.layout = Some("unknown".to_string());
    // Non-index in subfolder → Article (default)
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&doc), false, true),
        ShellType::Article
    );
}

#[test]
fn no_doc_uses_page_template() {
    assert_eq!(
        ShellRegistry::select_shell_type(None, false, true),
        ShellType::Page
    );
}

#[test]
fn nested_nav_true_uses_page_template() {
    // The fix: a nested file explicitly pinned to the nav (nav: true) gets
    // the clean Page layout, not blog-Article chrome.
    let mut doc = make_doc("docs/intro/index.html", false);
    doc.nav = Some(true);
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&doc), false, true),
        ShellType::Page
    );
}

#[test]
fn nested_no_nav_uses_article_template() {
    // Unchanged: a nested file without nav: true is still an Article.
    let doc = make_doc("docs/intro/index.html", false);
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&doc), false, true),
        ShellType::Article
    );
}

#[test]
fn root_no_nav_organized_stays_article() {
    // Safety: nav-awareness is the ONLY change. A no-nav root page in
    // organized mode keeps its existing Article layout (its url_path is
    // `about/index.html`, so the auto-detect leaves it Article) — content
    // essays/articles are NOT flipped to Page.
    //
    // NB: `root_level_doc_uses_page_template` above passes `"about.html"`
    // (no `/`) to exercise the literal `!url_path.contains('/')` Page branch;
    // production never emits that form (compute_url_path → `about/index.html`).
    // Both are correct given the literal predicate.
    let doc = make_doc("about/index.html", false);
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&doc), false, true),
        ShellType::Article
    );
}

#[test]
fn flat_nonkeyword_nav_true_uses_page_template() {
    // nav: true wins in flat mode too, even for a non-keyword stem.
    let mut doc = make_doc("portfolio.html", false);
    doc.nav = Some(true);
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&doc), false, false),
        ShellType::Page
    );
}

#[test]
fn layout_article_wins_over_nav_true() {
    // `layout:` is checked before the nav-aware branch, so an explicit
    // `layout: article` keeps the Article layout even on a nav: true page.
    let mut doc = make_doc("docs/intro/index.html", false);
    doc.nav = Some(true);
    doc.layout = Some("article".to_string());
    assert_eq!(
        ShellRegistry::select_shell_type(Some(&doc), false, true),
        ShellType::Article
    );
}

// ── CSS minification tests ──────────────────────────────────

#[test]
fn minify_css_strips_comments() {
    let input = "/* This is a comment */\nbody { color: red; }";
    let result = minify_css(input);
    assert!(
        !result.contains("comment"),
        "Comments should be stripped. Got: {}",
        result
    );
    assert!(result.contains("body"), "Selectors should be preserved");
}

#[test]
fn minify_css_collapses_whitespace() {
    let input = "body  {\n  color:  red;\n  font-size:  16px;\n}";
    let result = minify_css(input);
    assert!(
        !result.contains('\n'),
        "Newlines should be removed. Got: {}",
        result
    );
    assert!(
        !result.contains("  "),
        "Double spaces should be collapsed. Got: {}",
        result
    );
}

#[test]
fn minify_css_removes_trailing_semicolon() {
    let input = "body { color: red; }";
    let result = minify_css(input);
    assert!(
        result.contains("color:red"),
        "Should remove space after colon. Got: {}",
        result
    );
    assert!(
        !result.contains("; }") && !result.contains(";}"),
        "Trailing semicolons should be removed. Got: {}",
        result
    );
}

#[test]
fn minify_css_preserves_functionality() {
    let input = r#":root {
  --moss-color-bg: #faf8f5;
  --moss-color-text: #2c2825;
}

/* Dark mode */
[data-theme="dark"] {
  --moss-color-bg: #1c1914;
}

@media (max-width: 48rem) {
  .nav-links { display: none; }
}"#;
    let result = minify_css(input);
    // All functional parts should be present
    assert!(
        result.contains("--moss-color-bg:#faf8f5"),
        "Custom properties preserved. Got: {}",
        result
    );
    assert!(
        result.contains("[data-theme=\"dark\"]"),
        "Attribute selectors preserved. Got: {}",
        result
    );
    assert!(
        result.contains("@media"),
        "Media queries preserved. Got: {}",
        result
    );
    assert!(
        result.contains("max-width:48rem"),
        "Media query values preserved. Got: {}",
        result
    );
}

#[test]
fn minify_css_is_smaller() {
    let result = minify_css(DEFAULT_CSS);
    assert!(
        result.len() < DEFAULT_CSS.len(),
        "Minified CSS ({}) should be smaller than original ({})",
        result.len(),
        DEFAULT_CSS.len()
    );
}

#[test]
fn minify_css_preserves_descendant_combinator_before_pseudo_class() {
    // Regression (#631): the minifier was stripping the space before ':' in
    // pseudo-classes when the pseudo-class was preceded by whitespace in the source.
    // ".foo :hover" (all :hover descendants of .foo) must NOT become ".foo:hover"
    // (.foo itself when hovered — a completely different set of elements).
    assert_eq!(
        minify_css(".foo :hover { color: red }"),
        ".foo :hover{color:red}",
        "space before pseudo-class must be preserved (descendant combinator)"
    );
    // Also verify the common inline case works correctly (no regression)
    assert_eq!(
        minify_css(".foo .bar:hover { color: red }"),
        ".foo .bar:hover{color:red}",
        "attached pseudo-class must be preserved"
    );
}

#[test]
fn test_css_vertical_body_uses_writing_mode() {
    let rule = get_css_rule(&site_css_with_partials(), r#"body[data-typesetting="vertical"]"#)
        .expect("body[data-typesetting='vertical'] CSS rule should exist");
    assert!(
        rule.contains("writing-mode: vertical-rl"),
        "Body should use vertical-rl writing mode"
    );
    assert!(
        rule.contains("text-orientation: mixed"),
        "Body should use mixed text orientation"
    );
    assert!(
        rule.contains("overflow-x: auto"),
        "Body should allow horizontal scrolling"
    );
}

#[test]
fn test_css_vertical_no_horizontal_override_for_header() {
    // The old CSS forced header/footer/colophon back to horizontal-tb.
    // The new CSS should NOT have this override — they inherit vertical-rl from body.
    let rule = get_css_rule(&site_css_with_partials(), r#"body[data-typesetting="vertical"] header"#);
    if let Some(r) = rule {
        assert!(
            !r.contains("writing-mode: horizontal-tb"),
            "Header should NOT be forced to horizontal-tb in vertical mode"
        );
    }
}

#[test]
fn test_css_vertical_block_code_stays_horizontal() {
    let rule = get_css_rule(&site_css_with_partials(), r#"body[data-typesetting="vertical"] pre code"#)
        .expect("Vertical pre code CSS rule should exist");
    assert!(
        rule.contains("writing-mode: horizontal-tb"),
        "Block code should stay horizontal"
    );
    // Inline code (without pre) should NOT have a horizontal-tb reset
    let inline_rule = get_css_rule(&site_css_with_partials(), r#"body[data-typesetting="vertical"] code"#);
    assert!(
        inline_rule.is_none() || !inline_rule.unwrap().contains("writing-mode: horizontal-tb"),
        "Inline code should inherit vertical writing mode"
    );
}

#[test]
fn test_css_vertical_hides_mobile_menu() {
    let rule = get_css_rule(
        &site_css_with_partials(),
        r#"body[data-typesetting="vertical"] .mobile-menu-button"#,
    )
    .expect("Vertical mobile-menu-button CSS rule should exist");
    assert!(
        rule.contains("display: none"),
        "Mobile menu should be hidden in vertical mode"
    );
}

#[test]
fn test_template_includes_colophon_text_placeholder() {
    // The shared shell owns the colophon and must use placeholders (not
    // hardcoded strings) so it can be localized.
    assert!(
        SHELL_TEMPLATE.contains("{colophon_text}") && SHELL_TEMPLATE.contains("{colophon_name}"),
        "Shell template should use the {{colophon_text}} and {{colophon_name}} placeholders"
    );
    assert!(
        !SHELL_TEMPLATE.contains("Published with moss"),
        "Shell template should not have hardcoded 'Published with moss'"
    );
    // Two strings, two jobs. The <svg> is aria-hidden, so without an
    // aria-label the visible wordmark would be the link's whole accessible
    // name — and "moss" alone does not say what the link does. The label
    // carries the full phrase; the span carries the half a reader sees.
    assert!(
        SHELL_TEMPLATE.contains(r#"aria-label="{colophon_text}""#),
        "The colophon link needs the full phrase as its accessible name — the mark is aria-hidden"
    );
    assert!(
        SHELL_TEMPLATE.contains(r#"<span class="moss-colophon-label moss-wordmark">{colophon_name}</span>"#),
        "The visible wordmark must stay wrapped in .moss-colophon-label (the CSS reveals it by class) \
         and carry .moss-wordmark (mark.css sets its face by it)"
    );
    // The link carries its own `lang`, because the wordmark's language and the
    // page's differ: a Japanese page emits `<html lang="ja">` while its chrome
    // falls back to the site's language, and it is the chrome's language that
    // decided whether this link says "moss" or "青苔". `:lang(zh)` in site.css
    // picks the face and the lead off this attribute, so keying it on the page
    // would set a Latin wordmark in the Chinese metrics.
    assert!(
        SHELL_TEMPLATE.contains(r#"lang="{colophon_lang}""#),
        "The colophon link must declare the wordmark's own language, not the page's"
    );
}

#[test]
fn test_template_skip_link_is_first_in_body_and_localized() {
    // Skip link (WCAG 2.4.1, moss#1047): must be the very first thing after
    // <body ...>, before {nav_island}, and localized via {skip_link_label}
    // rather than hardcoded — same reasoning as the colophon text above.
    let body_open = SHELL_TEMPLATE
        .find("<body")
        .expect("shell template must have a <body> tag");
    let body_tag_end = SHELL_TEMPLATE[body_open..]
        .find('>')
        .map(|i| body_open + i + 1)
        .expect("<body ...> tag must close");
    let after_body = &SHELL_TEMPLATE[body_tag_end..];
    assert!(
        after_body.starts_with(r##"<a class="moss-skip-link" href="#main-content">{skip_link_label}</a>"##),
        "Skip link must be the first element after <body ...>, got: {:?}",
        &after_body[..after_body.len().min(120)]
    );
    assert!(
        !SHELL_TEMPLATE.contains("Skip to content"),
        "Shell template should not have hardcoded skip-link text"
    );

    let processor = ShellProcessor::new();
    let vars = ShellVars {
        ui_lang: crate::i18n::Language::ZhHans,
        ..test_vars()
    };
    let result = processor.process(ShellType::Page, vars);
    assert!(
        result.contains(r##"<a class="moss-skip-link" href="#main-content">跳到内容</a>"##),
        "Skip link should render with the localized label"
    );
}

#[test]
fn test_template_main_has_id_for_skip_link_target() {
    // Both content fragments emit <main id="main-content" tabindex="-1"> — the
    // skip link's href target (WCAG 2.4.1, moss#1047).
    //
    // The tabindex is not decoration. WebKit moves the SCROLL position to a
    // fragment target but only moves FOCUS if the target can hold it, so
    // without it the reader lands looking at the content while tab order
    // resumes back at the nav — and WebKitGTK is the engine behind the
    // preview, so this is the browser moss itself renders in.
    let processor = ShellProcessor::new();
    for (label, result) in [
        ("Article", processor.process(ShellType::Article, test_vars())),
        ("Page", processor.process(ShellType::Page, test_vars())),
    ] {
        assert!(
            result.contains(r#"<main id="main-content" tabindex="-1""#),
            "{label} template's <main> must carry id=\"main-content\" and tabindex=\"-1\""
        );
    }
}

#[test]
fn test_css_colophon_label_is_hidden_by_opacity_not_display_none() {
    // The wording is invisible at rest and fades in on hover/focus. It must
    // hide by going transparent, which keeps the text in the accessibility
    // tree; `display: none` or `visibility: hidden` would remove it and take
    // the link's accessible name with it.
    let css = site_css_with_partials();
    let rest = get_css_rule(&css, ".moss-colophon-label").expect("Colophon label CSS rule should exist");
    assert!(
        rest.contains("opacity: 0"),
        "Label should hide by going transparent, got: {rest}"
    );
    assert!(
        !rest.contains("display: none") && !rest.contains("visibility: hidden"),
        "Label must stay in the accessibility tree, got: {rest}"
    );
    // Keyboard users get the reveal too, not just pointer users — and because
    // the pointer's rules are gated behind `@media (hover: hover)`, focus has
    // to be written outside that gate or a touch device with a keyboard
    // attached loses the reveal entirely.
    assert!(
        css.contains(".moss-colophon a:focus-visible .moss-colophon-label"),
        "Colophon label should reveal on keyboard focus, not hover alone"
    );
}

#[test]
fn test_css_colophon_wording_is_out_of_flow_and_its_space_reserved() {
    // The mark holds its place: the wording is positioned out of flow, centred
    // on the mark's own axis, so revealing it moves nothing. That is also what
    // makes the credit centre identically with the wording and without it.
    //
    // Out of flow means it adds no height, so the link has to reserve the line
    // box itself — otherwise the words render on top of whatever follows the
    // strip, or past the foot of a short page.
    let css = site_css_with_partials();
    let label = get_css_rule(&css, ".moss-colophon-label").expect("Colophon label CSS rule should exist");
    assert!(
        label.contains("position: absolute") && label.contains("inset-inline-start: 50%"),
        "Wording should be out of flow and centred on the mark's axis, got: {label}"
    );
    let link = get_css_rule(&css, ".moss-colophon a").expect("Colophon link CSS rule should exist");
    assert!(
        link.contains("1.15em"),
        "Colophon link should reserve the wording's line box in its padding, got: {link}"
    );
    assert!(
        label.contains("line-height: 1.15"),
        "The label's line box must be pinned to the height the link reserves, got: {label}"
    );
}

#[test]
fn test_css_colophon_gap_is_ink_to_ink_and_the_same_in_every_script() {
    // The air between the mark and the wording is measured ink to ink, not box
    // to box, because type starts below the top of its line box by an amount
    // that depends on the script and is not small: 4.8px of empty leading above
    // `moss` in the body stack at 12px, 2.1px above 青苔, which fills its em.
    // The mark's ink reaches its own box bottom exactly, so that leading is the
    // whole correction — see the derivation in site.css.
    //
    // It is subtracted out of a single stated gap, so the lead is a variable
    // rather than a number baked into the padding. Hardcode the padding again
    // and Chinese silently goes back to sitting nearer the mark than English —
    // which is the state this replaced.
    let css = site_css_with_partials();
    let link = get_css_rule(&css, ".moss-colophon a").expect("Colophon link CSS rule should exist");
    assert!(
        link.contains("--moss-colophon-lead:") && link.contains("var(--moss-colophon-lead)"),
        "The gap should be stated once as a lead and added to the reserve, got: {link}"
    );
    // The two halves of what makes the Chinese credit different: the wider
    // lead is the colophon's own, and the running hand comes with the shared
    // `.moss-wordmark` class the label carries (mark.css).
    let zh = get_css_rule(&css, ".moss-colophon a:lang(zh)")
        .expect("Chinese colophon CSS rule should exist");
    assert!(
        zh.contains("--moss-colophon-lead:"),
        "CJK needs more lead to reach the same ink gap; without the override it renders tighter, got: {zh}"
    );
    let face = get_css_rule(&css, ".moss-wordmark:lang(zh)")
        .expect("the shared wordmark face rule should be in the assembled sheet");
    assert!(
        face.contains("'Moss Long Cang'") && face.contains("var(--moss-font-body)"),
        "青苔 is set in the running hand, ahead of — not instead of — the body stack, got: {face}"
    );
    // The reserve is written logically (`padding-block`), so vertical pages
    // get the same gap from the same rule and need no override of their own.
    assert!(
        link.contains("padding-block:"),
        "The reserve must be logical so it transposes on its own, got: {link}"
    );
    assert!(
        get_css_rule(&css, r#"body[data-typesetting="vertical"] .moss-colophon a"#).is_none(),
        "vertical.css must not restate the colophon reserve"
    );
}

#[test]
fn test_css_vertical_content_height() {
    let rule = get_css_rule(&site_css_with_partials(), r#"body[data-typesetting="vertical"]"#)
        .expect("body[data-typesetting='vertical'] CSS rule should exist");
    assert!(
        rule.contains("height:"),
        "Vertical body should have a height constraint"
    );
    assert!(
        rule.contains("38em"),
        "Height should use 38em for column height"
    );
}

#[test]
fn test_css_vertical_breadcrumb_dot() {
    let rule = get_css_rule(
        &site_css_with_partials(),
        r#"body[data-typesetting="vertical"] .breadcrumb-separator"#,
    )
    .expect("Vertical breadcrumb separator CSS rule should exist");
    assert!(
        rule.contains("color: transparent"),
        "Breadcrumb separator should hide text via transparent color"
    );
}

#[test]
fn test_css_vertical_breadcrumb_dot_before() {
    let rule = get_css_rule(
        &site_css_with_partials(),
        r#"body[data-typesetting="vertical"] .breadcrumb-separator::before"#,
    )
    .expect("Vertical breadcrumb ::before CSS rule should exist");
    assert!(
        rule.contains("00B7") || rule.contains("·"),
        "Should use middle dot"
    );
}

#[test]
fn test_css_vertical_grid_card_layout_is_site_css() {
    // Same rule as the list card below, for the shelf: a `:::grid N` under
    // vertical typesetting is site.css's grid, rotated. The track count, the
    // gaps, the stack and the card's size all come from
    // `grid-template-columns` on the INLINE axis; vertical.css sizes nothing.
    // The measure-and-plate geometry that lived here for one day on
    // 2026-09-11 — a 13em card along the scroll, a plate 0.75 of it, a
    // `display: none` on the coverless card — is what this forbids coming
    // back (docs/archive/2026-09-11-vertical-cards-design.md).
    let css = site_css_with_partials();
    assert!(
        !css.contains("--moss-vertical-card-measure"),
        "the vertical card measure token is gone: the grid's own tracks size the card"
    );
    let grid = r#"[data-typesetting="vertical"] :is(.moss-cards[data-layout="grid"], .moss-grid)"#;
    for part in ["moss-card", "moss-card-cover", "moss-card-content"] {
        let rule = get_css_rule(&css, &format!("{grid} .{part}")).unwrap_or_default();
        // Declaration starts, not substrings: `min-block-size` is a floor being
        // removed, not a size being set.
        for decl in rule.lines().map(str::trim) {
            for sizing in ["inline-size:", "block-size:", "flex:", "flex-basis:"] {
                assert!(
                    !decl.starts_with(sizing),
                    "vertical.css sizes .{part} with `{sizing}`; the grid's own tracks already do, got: {rule}"
                );
            }
        }
    }
    assert!(
        get_css_rule(&css, r#"[data-typesetting="vertical"] .moss-grid .moss-card-no-cover"#).is_none(),
        "a coverless grid card is the horizontal colour band, turned — it needs no vertical rule"
    );
    // The plate's ratio is the one thing rotation cannot do: `aspect-ratio` is
    // width/height, with no flow-relative spelling. It must still defer to a
    // site's own hook, and it must be all that rule says.
    let cover = get_css_rule(&css, &format!("{grid} .moss-card-cover"))
        .expect("the turned plate ratio should be in the assembled sheet");
    assert!(
        cover.contains("aspect-ratio: var(--moss-card-cover-ratio, 3 / 4)"),
        "the turned default must still read the site hook, got: {cover}"
    );
    // And the shared cover box carries no percentage inline-size: a box sized
    // by `aspect-ratio` over a percentage contributes ZERO intrinsic size on
    // the other axis, which is what collapsed the plate to 0px the first time
    // a `:::grid` shelf was laid out down a column.
    let shared = get_css_rule(DEFAULT_CSS, ".moss-card-cover").expect("cover rule should exist");
    assert!(
        !shared.contains("inline-size"),
        "the cover fills its container by being a block box; a percentage here re-arms the zero-intrinsic-size collapse, got: {shared}"
    );
}

/// Pulls the `N / D` out of `aspect-ratio: var(--moss-card-cover-ratio, N / D)`.
/// Panics rather than returning `Option` — every caller already asserted the
/// rule it's handed exists, so a missing fallback here means the fixed literal
/// syntax changed and the test needs a look, not a graceful skip.
fn parse_card_cover_ratio_fallback(rule: &str) -> (u32, u32) {
    let marker = "--moss-card-cover-ratio, ";
    let start = rule
        .find(marker)
        .unwrap_or_else(|| panic!("no `{marker}` fallback in rule: {rule}"))
        + marker.len();
    let rest = &rule[start..];
    let end = rest
        .find(')')
        .unwrap_or_else(|| panic!("unterminated fallback in rule: {rule}"));
    let mut parts = rest[..end].split('/').map(|part| {
        part.trim()
            .parse::<u32>()
            .unwrap_or_else(|_| panic!("non-numeric ratio component in: {rule}"))
    });
    let n = parts.next().unwrap_or_else(|| panic!("missing numerator in: {rule}"));
    let d = parts.next().unwrap_or_else(|| panic!("missing denominator in: {rule}"));
    (n, d)
}

#[test]
fn test_css_vertical_card_cover_ratio_is_reciprocal_of_horizontal_default() {
    // The horizontal default (site.css, `4 / 3`) and the vertical override
    // (vertical.css, `3 / 4`) are independent hand-written literals with
    // nothing but a design doc saying the second must be the first inverted
    // — `aspect-ratio` is width/height with no logical spelling, so the
    // inversion can't be expressed once and shared. The golden-string tests
    // elsewhere pin each literal exactly but don't check they still agree;
    // this is the one that catches an edit to either number alone.
    let css = site_css_with_partials();
    let horizontal =
        get_css_rule(&css, ".moss-card-cover").expect("horizontal cover rule should exist");
    let (h_n, h_d) = parse_card_cover_ratio_fallback(&horizontal);
    let grid = r#"[data-typesetting="vertical"] :is(.moss-cards[data-layout="grid"], .moss-grid)"#;
    let vertical = get_css_rule(&css, &format!("{grid} .moss-card-cover"))
        .expect("vertical cover rule should exist");
    let (v_n, v_d) = parse_card_cover_ratio_fallback(&vertical);
    assert_eq!(
        h_n * v_n,
        h_d * v_d,
        "vertical's default ({v_n} / {v_d}) must be the exact reciprocal of horizontal's ({h_n} / {h_d})"
    );
}

#[test]
fn test_css_vertical_list_card_layout_is_site_css() {
    // The vertical list card is site.css's card rotated: the 120px cover
    // length at the inline end and the body's block-start alignment are
    // logical there, so vertical.css carries no layout rule for the card at
    // all — not even an `order` or `display: contents` reorder. The 140×140
    // square, the per-card hairline, the head-column flex, and
    // (2026-09-10 – 2026-09-11) the cover-leads-column and title-first
    // reorders all lived here once; a rule reappearing is the compensation
    // §1 forbids (docs/archive/2026-09-05-vertical-layout-design.md).
    let css = site_css_with_partials();
    for part in ["moss-card-cover", "moss-card-body", "moss-card-head"] {
        let sel = format!(r#"[data-typesetting="vertical"] .moss-cards[data-layout="list"] .{part}"#);
        assert!(
            get_css_rule(&css, &sel).is_none(),
            "vertical.css must not declare any rule for the list card's .{part}"
        );
    }
    let cover = get_css_rule(DEFAULT_CSS, r#".moss-cards[data-layout="list"] .moss-card-cover"#)
        .expect("list cover rule should exist");
    assert!(
        cover.contains("inline-size: 120px") && !cover.contains("block-size"),
        "the list cover fixes only its main-axis length; the cross extent stretches to the body's, got: {cover}"
    );
    let row = get_css_rule(DEFAULT_CSS, ".moss-card-row").expect("card row rule should exist");
    assert!(
        row.contains("align-items: stretch"),
        "body and cover share the row's block-start edge AND its cross extent (the brow leads, the foot sizes to the text), got: {row}"
    );
}

#[test]
fn test_css_vertical_card_title_inline() {
    let rule = get_css_rule(
        &site_css_with_partials(),
        r#"[data-typesetting="vertical"] .moss-cards[data-layout="list"] .moss-card-title"#,
    )
    .expect("Vertical card title CSS rule should exist");
    assert!(
        rule.contains("white-space: normal"),
        "Title should wrap normally in vertical mode"
    );
}

#[test]
fn test_css_vertical_card_desc_unclamped() {
    // A vertical card is a column-group whose block length is free, so the
    // horizontal two-line clamp is undone deliberately (design issue 1). The
    // base rule clamps via a logical `max-block-size` (not the legacy
    // `-webkit-line-clamp`, dropped 2026-09-12 because it forced
    // `writing-mode: horizontal-tb` on cards with no vertical-mode override,
    // e.g. a `:::grid` cell), so lifting the clamp here means lifting that
    // cap back to `none`.
    let rule = get_css_rule(
        &site_css_with_partials(),
        r#"[data-typesetting="vertical"] .moss-cards[data-layout="list"] .moss-card-description"#,
    )
    .expect("Vertical card description CSS rule should exist");
    assert!(
        rule.contains("max-block-size: none"),
        "Description should not be clamped to two columns in vertical mode"
    );
}

#[test]
fn test_css_vertical_no_physical_layout_props() {
    // Layout in vertical.css is expressed by rotating site.css's logical rules,
    // never by a physical override compensating for a physical site.css rule
    // (the 2026-09-05 hairline: `border-inline-end` re-spelling a
    // `border-bottom`). Anchored line patterns, not substrings, so
    // `padding-block-end` and `margin-inline` pass. The body sizing block, the
    // html viewport-centring block and the scrollbar are the named exclusions
    // (2026-09-11: vertical.css's own comment on that block says why the
    // centring trick has to be physical). The phone-width `.moss-card-content`
    // band is a fourth: it re-establishes its OWN `writing-mode: vertical-rl`
    // inside a card that opted back out to `horizontal-tb` (2026-09-14, "Grid
    // cards, phone width" block), so `min-block-size` on that element binds
    // its physical WIDTH, not height — the floor genuinely has to be spelled
    // `min-height` to mean height. Same shape as the other three: a rotation
    // rule for a box that isn't rotating along with its container.
    let vertical = crate::build::emit::stylesheet::CSS_PARTIALS
        .iter()
        .find(|p| p.name == "vertical")
        .expect("vertical partial")
        .source;
    let mut offenders = Vec::new();
    let mut selector = String::new();
    for (i, raw) in vertical.lines().enumerate() {
        let line = raw.trim_start();
        if line.ends_with('{') {
            selector = line.trim_end_matches('{').trim().to_string();
        }
        let excluded = selector == r#"body[data-typesetting="vertical"]"#
            || selector == r#"html:has(body[data-typesetting="vertical"])"#
            || selector.contains("::-webkit-scrollbar")
            || selector.ends_with(".moss-card-content");
        if excluded {
            continue;
        }
        let physical = ["border-", "padding-", "margin-"]
            .iter()
            .any(|p| line.starts_with(p) && (line[p.len()..].starts_with("top") || line[p.len()..].starts_with("bottom")))
            || line.starts_with("width:")
            || line.starts_with("height:")
            || line.starts_with("min-width:")
            || line.starts_with("min-height:");
        if physical {
            offenders.push(format!("{}: {}", i + 1, raw.trim()));
        }
    }
    assert!(
        offenders.is_empty(),
        "vertical.css carries physical layout properties; spell them logically in site.css instead:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn test_css_list_summary_ordinary_card_title_hover_accent() {
    // Regression guard for f0747cb4e ("decouple title and star hover").
    // That commit rescoped the list-summary title-hover rule from
    // `.moss-card:hover .moss-card-title` to `.moss-card-title-link:hover`,
    // which only exists on linkblog cards. Ordinary article/folder summary
    // cards (a bare `<h3 class="moss-card-title">` inside `<a class="moss-card">`)
    // therefore stopped turning accent-green on hover.
    //
    // The whole ordinary card is a single link, so hovering anywhere on it
    // is hovering the title's target — `.moss-card:not([data-linkblog]):hover`
    // restores the accent while keeping linkblog cards (two targets: title
    // link vs. `★` permalink) decoupled.
    let rule = get_css_rule(
            DEFAULT_CSS,
            r#".moss-cards[data-layout="list"] .moss-card:not([data-linkblog]):hover .moss-card-title"#,
        )
        .expect("Ordinary (non-linkblog) summary cards must have a title-hover accent rule (regression f0747cb4e)");
    assert!(
        rule.contains("var(--moss-color-accent)"),
        "Ordinary summary card title must turn accent-green on hover, got: {}",
        rule
    );
}

#[test]
fn test_css_list_layout_container_has_section_break_margin() {
    // Bug: a page's own body content sat flush against its children/list
    // layout (data-layout="list") with no separation — unlike the grid
    // layout, which always carries a --moss-space-2xl margin. The margin
    // lives on .moss-cards-container (not the nested .moss-cards[list]
    // element) because .moss-cards-container establishes layout
    // containment (container-type: inline-size), which stops a child's
    // margin from collapsing with whatever precedes the container.
    //
    // `:where(:not(:first-child))` excludes a listing that is the very
    // first thing in its parent (a bare embed/listing page with nothing
    // above it) — see tests/render-gates/site/list-layout-section-break.spec.js for the
    // rendered-layout proof that this margin would otherwise collapse
    // through <article> into the page chrome above it.
    let rule = get_css_rule(
        DEFAULT_CSS,
        r#".moss-cards-container:where(:not(:first-child)):has(> .moss-cards[data-layout="list"])"#,
    )
    .expect("List-layout container should carry a section-break margin rule");
    // Block-start, not top: under vertical-rl the section break is the gap to
    // the RIGHT of the listing, and a physical `margin-top` pushed the cards
    // 96px down the column instead (zhu-da home, 2026-09-05).
    assert!(
        rule.contains("margin-block-start: var(--moss-space-2xl)"),
        "List-layout container should add a 2xl block-start margin (section break), got: {}",
        rule
    );
}

#[test]
fn test_css_list_layout_cancel_rules_stay_tight() {
    // The section-break margin above must NOT apply when the immediate
    // previous sibling already owns/provides the list's separation.
    // Literal-selector-text checks can't prove the actual rendered gap
    // (that requires real margin-collapse layout — see
    // tests/render-gates/site/list-layout-section-break.spec.js for the Playwright-driven
    // proof across all three shapes below plus the first-child case).
    // This test just guards the CSS source against accidental deletion
    // or a typo'd property value.
    let combined_selector = concat!(
            ":is(h1, h2, h3, h4, h5, h6) + .moss-cards-container:has(> .moss-cards[data-layout=\"list\"]),\n",
            ".moss-cards-container:has(> .moss-cards[data-layout=\"grid\"]) + .moss-cards-container:has(> .moss-cards[data-layout=\"list\"]),\n",
            ".moss-cards-container:has(> .moss-cards[data-layout=\"minimal\"] > .moss-cards-minimal-year-group:last-child) + .moss-cards-container:has(> .moss-cards[data-layout=\"list\"]),\n",
            ".moss-collection-cover-row + .moss-cards-container:has(> .moss-cards[data-layout=\"list\"])",
        );
    let rule = get_css_rule(DEFAULT_CSS, combined_selector).expect(
            "Cancel-rules for heading/grid-sibling/year-grouped-minimal-sibling/cover-row before a list container should exist",
        );
    assert!(
        rule.contains("margin-block-start: 0"),
        "Cancel-rules must zero the added margin to preserve the tight bind, got: {}",
        rule
    );

    // Individually confirm each of the four selectors is present in
    // the combined rule (catches a selector silently dropped from the
    // group without changing the rule's overall behavior detectably
    // above, since the others would still zero the same property).
    assert!(
            DEFAULT_CSS.contains(r#":is(h1, h2, h3, h4, h5, h6) + .moss-cards-container:has(> .moss-cards[data-layout="list"])"#),
            "Heading (any level) cancel-rule missing"
        );
    assert!(
            DEFAULT_CSS.contains(r#".moss-cards-container:has(> .moss-cards[data-layout="grid"]) + .moss-cards-container:has(> .moss-cards[data-layout="list"])"#),
            "Grid-sibling cancel-rule missing — without it, a grid listing directly followed by a list listing double-adds the section-break margin (grid's own trailing 2xl is trapped inside its containment box)"
        );
    assert!(
            DEFAULT_CSS.contains(r#".moss-cards-container:has(> .moss-cards[data-layout="minimal"] > .moss-cards-minimal-year-group:last-child) + .moss-cards-container:has(> .moss-cards[data-layout="list"])"#),
            "Year-grouped-minimal-sibling cancel-rule missing. NOTE: this must stay scoped to `> .moss-cards-minimal-year-group:last-child`, NOT generalized to `:not([data-layout=\"list\"])` — a FLAT (ungrouped) minimal listing (folder_embed.rs: folders always render flat; articles go flat whenever children_group isn't \"year\") has no .moss-cards-minimal-year-group wrapper and thus no trailing margin of its own to cancel against; a blanket not-list selector would wrongly zero the section-break gap for that shape and reintroduce the original bug"
        );
    assert!(
            DEFAULT_CSS.contains(r#".moss-collection-cover-row + .moss-cards-container:has(> .moss-cards[data-layout="list"])"#),
            "Collection cover-row cancel-rule missing — without it, a folder page with a cover image (folder_cover.rs) gets the section-break margin added on top of the cover-row's own lg margin"
        );
}

#[test]
fn test_css_minimal_year_group_title_hover_accent() {
    // Sibling regression to the list-summary one above: f0747cb4e also
    // rescoped the minimal year-group rule to `.moss-card-title-link:hover`,
    // a class the minimal rows never emit (they render via
    // `child_list::render` as `<p class="moss-card"><a class="moss-prefix-link">
    // …<span class="...title">`). The rule matched nothing, so hovering the
    // row outside its short link no longer lit the title. There is no
    // linkblog/star variant in minimal year-groups, so the simple
    // whole-row `.moss-card:hover .title` is the correct restoration.
    let rule = get_css_rule(
        DEFAULT_CSS,
        r#".moss-cards-minimal-year-group.minimal .moss-card:hover .title"#,
    )
    .expect(
        "Minimal year-group rows must light the title on whole-row hover (regression f0747cb4e)",
    );
    assert!(
        rule.contains("var(--moss-color-accent)"),
        "Minimal year-group title must turn accent-green on hover, got: {}",
        rule
    );
}

/// `runtime_js_tags` is a whole-BLOCK var: shell.html drops it in verbatim,
/// and an empty one leaves no residue at all. Residue matters more than
/// presence here — a leftover `{runtime_js_tags}` literal, or a dead
/// `<script>` pointing at a bundle this build never emitted, 404s on every
/// page load of a site that opted the feature out.
///
/// One test, not one per script: which scripts the block contains, in what
/// order, and with which `defer`, is decided by the `SITE_SCRIPTS` table and
/// covered at that layer (`emit::scripts_tests::shell_tags_*`). This one owns
/// only the template's half of the contract.
#[test]
fn test_shell_html_runtime_js_tags_var() {
    let processor = ShellProcessor::new();
    let vars = ShellVars {
        runtime_js_tags: "\n    <script src=\"/_moss/js/preview.abc123.js\"></script>\n    \
             <script src=\"/_moss/js/search.def456.js\" defer></script>"
            .to_string(),
        ..test_vars()
    };
    let result = processor.process(ShellType::Page, vars);
    assert!(
        result.contains(r#"src="/_moss/js/preview.abc123.js""#)
            && result.contains(r#"src="/_moss/js/search.def456.js" defer"#),
        "shell.html should drop {{runtime_js_tags}} in verbatim, defer included"
    );

    let processor = ShellProcessor::new();
    let off = processor.process(ShellType::Page, test_vars());
    assert!(
        !off.contains("runtime_js_tags") && !off.contains("/_moss/js/preview"),
        "an empty runtime_js_tags must leave no residue in the page"
    );
}

// ---- The blueprint placeholder is PREVIEW-ONLY ----

/// A built page carries no placeholder at all — no script, no marker class, no
/// CSS rule.
///
/// It used to: shell.html inlined a `#moss-img-fallback` error listener into
/// every page, so a published site could show a reader a blueprint grid where a
/// photo should be. That is the wrong contract. A published site can never
/// contain a broken image, because moss refuses to deploy one; the placeholder
/// is a working state, and the work is local. The preview server injects it
/// (`preview::iframe_bridge::inject_placeholder_into_head`) and nothing else
/// does.
#[test]
fn a_built_page_ships_no_blueprint_placeholder() {
    let processor = ShellProcessor::new();
    let result = processor.process(ShellType::Page, test_vars());
    for token in ["moss-img-fallback", "thumb-swap", "asset-placeholder"] {
        assert!(
            !result.contains(token),
            "published output must not carry '{token}' — the placeholder is preview-only"
        );
    }
    assert!(
        !DEFAULT_CSS.contains("moss-img-fallback"),
        "site.css must not style a class that only exists in the preview"
    );
}


// ---- Floating surfaces ----

/// The selection popover's `left` is its real left edge, not its centre.
///
/// `selection-actions.ts` subtracts half the popover's measured width itself
/// and clamps the result against the visible band. A `transform:
/// translateX(-50%)` here would silently re-introduce the centring the script
/// already applied AND hide the true left edge from it — which is exactly how
/// half the share/copy bar ended up off the left of a phone screen. See
/// docs/reference/design/floating-surfaces.md.
#[test]
fn the_selection_popover_is_not_centred_by_a_transform() {
    let rule = get_css_rule(DEFAULT_CSS, ".sel-popover")
        .expect("site.css must style .sel-popover");
    assert!(
        !rule.contains("transform"),
        ".sel-popover must carry no transform — `left` is its real left edge, \
         set and clamped by selection-actions.ts:\n{rule}"
    );
}

/// A surface hidden with `opacity: 0` takes no clicks.
///
/// `.moss-preview-popup` stays laid out when hidden — up to 360px of it, parked
/// wherever the last hovered link was. With `pointer-events: auto` there it is
/// an invisible lid over the text. `auto` belongs on `.visible`, where the card
/// is genuinely under the cursor and genuinely clickable.
#[test]
fn an_invisible_preview_card_takes_no_pointer_events() {
    let hidden = get_css_rule(DEFAULT_CSS, ".moss-preview-popup")
        .expect("site.css must style .moss-preview-popup");
    assert!(
        hidden.contains("pointer-events: none"),
        "the hidden preview card must not intercept clicks:\n{hidden}"
    );

    let visible = get_css_rule(DEFAULT_CSS, ".moss-preview-popup.visible")
        .expect("site.css must style .moss-preview-popup.visible");
    assert!(
        visible.contains("pointer-events: auto"),
        "the shown preview card must be clickable — it navigates:\n{visible}"
    );
}

/// The hint pill must be reachable once shown, and inert until then.
///
/// WCAG 1.4.13 "hoverable" says content shown on hover must survive the pointer
/// moving onto it. That needs two things a reader would never guess are
/// related: the pill has to accept pointer events (a `::after` is hit-tested as
/// its host, so this keeps `:hover` alive), and the air between host and pill
/// has to be a transparent BORDER rather than an offset — as a gap it is dead
/// space where `:hover` drops and the pill disappears mid-journey.
///
/// The pill is laid out at rest (it hides by `opacity`), so the same
/// `pointer-events: auto` on the base rule would be an invisible lid up to a
/// screenful wide under every host — the bug
/// `an_invisible_preview_card_takes_no_pointer_events` guards next door.
#[test]
fn a_hint_pill_is_reachable_when_shown_and_inert_when_not() {
    let rest = get_css_rule(DEFAULT_CSS, "[data-tooltip]::after")
        .expect("site.css must style the hint pill");
    assert!(
        rest.contains("pointer-events: none"),
        "the hidden hint pill must not intercept clicks:\n{rest}"
    );
    assert!(
        rest.contains("border-top: 6px solid transparent"),
        "the air under the host must be a transparent border, not a gap — a gap \
         un-hovers the host before the pointer reaches the pill:\n{rest}"
    );
    assert!(
        !rest.contains("top: calc("),
        "`top` must be a bare 100%; offsetting it re-opens the dead zone the \
         border exists to close:\n{rest}"
    );
    assert!(
        rest.contains("background-clip: padding-box"),
        "the fill must stay inside the padding box or the pill looks 6px \
         taller than it should:\n{rest}"
    );

    for selector in ["[data-tooltip]:hover::after", "[data-tooltip]:focus-visible::after"] {
        let shown = get_css_rule(DEFAULT_CSS, selector)
            .unwrap_or_else(|| panic!("site.css must style {selector}"));
        assert!(
            shown.contains("pointer-events: auto"),
            "a shown hint pill must be hoverable (WCAG 1.4.13):\n{selector} {{{shown}}}"
        );
    }
}
