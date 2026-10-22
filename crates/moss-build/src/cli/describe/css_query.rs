//! `moss describe --css <needle>` — print the default rules a theme override
//! has to displace.
//!
//! ## Why a query and not the sheet
//!
//! The emitted stylesheet is ~89 KB minified. Telling an agent to read it is
//! telling it to spend most of its context on vertical-Chinese typesetting and
//! immersive fullscreen in order to change a hero. So: print the matching ones.
//!
//! ## Why the source, not the emitted CSS
//!
//! Comments. `site.css`'s comments are where the reasoning lives — which cap a
//! rule is imposing, what the responsive default assumes, which selector
//! already overrides it. That is most of what an agent needs and none of it
//! survives minification. Rules are printed with the comment block immediately
//! above them attached.
//!
//! Searches every stylesheet moss ships: `site.css`, the feature sheets
//! (`email.css`, `comments.css`, `review.css`), and each gated partial in
//! `build::emit::stylesheet::CSS_PARTIALS`. A rule is no less relevant for
//! living in a sheet the scanner skipped — `email.css` is where
//! `.moss-subscribe` is actually styled, and it is injected into a *later*
//! cascade layer than `site.css`, which is exactly the kind of thing a theme
//! author gets wrong unseen. The partials are searched unconditionally: the
//! question is "what does moss style by default", not "what did the last build
//! happen to emit".

use crate::build::theme_lint::{style_rules, StyleRule};

/// The always-embedded stylesheets, in cascade order. `site.css` first because
/// it is what nearly every query wants; the rest carry the feature surfaces.
///
/// The gated partials in `build::emit::stylesheet::CSS_PARTIALS` are appended
/// to this list by [`sheets`] — a query must reach a rule whether or not *this*
/// site happens to ship it, since the question being asked is "what does moss
/// style by default", not "what did the last build emit".
fn fixed_sheets() -> Vec<(&'static str, &'static str)> {
    let mut sheets = vec![(
        "site.css",
        crate::build::emit::stylesheet::CORE_CSS,
    )];
    sheets.extend(
        crate::build::emit::feature_styles::FEATURE_STYLES
            .iter()
            .map(|s| (s.dev_path.rsplit('/').next().unwrap_or(s.dev_path), s.source)),
    );
    sheets
}

/// Every stylesheet the query scans: the fixed set plus each registered partial.
fn sheets() -> Vec<(String, &'static str)> {
    fixed_sheets()
        .into_iter()
        .map(|(n, c)| (n.to_string(), c))
        .chain(
            crate::build::emit::stylesheet::CSS_PARTIALS
                .iter()
                .map(|p| (format!("site/{}.css", p.name), p.source)),
        )
        .collect()
}

/// Whether `haystack` mentions `needle` as a whole CSS token.
///
/// Substring matching would make `.moss-card` return all 60 `.moss-card-*`
/// rules, which is the dump this command exists to avoid. A match must end at
/// a CSS token boundary — so `.moss-card` matches `.moss-card:hover` and
/// `.moss-card > img` but not `.moss-card-cover`.
// Both slices start at an index `str::find` returned (a char boundary) plus the
// length of the needle matched there, which lands on the boundary just past the
// match — neither can split a UTF-8 character.
#[allow(clippy::string_slice)]
pub(crate) fn mentions(haystack: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(i) = haystack[from..].find(needle) {
        let start = from + i;
        let end = start + needle.len();
        let next = haystack[end..].chars().next();
        // `-` and `_` continue an identifier; alphanumerics do too. Anything
        // else (`,` `{` `:` ` ` `>` `[` end-of-string) ends the token.
        let boundary = match next {
            None => true,
            Some(c) => !(c.is_alphanumeric() || c == '-' || c == '_'),
        };
        if boundary {
            return true;
        }
        from = end;
    }
    false
}

/// Whether `rule` is one a query for `needle` returns. A custom property lives
/// in the declarations, not the selector, and "what reads
/// --moss-grid-image-ratio?" is exactly what an agent asks before setting one,
/// so `--`-prefixed needles search the whole rule.
fn rule_mentions(rule: &StyleRule<'_>, needle: &str) -> bool {
    mentions(if needle.starts_with("--") { rule.rule } else { rule.prelude }, needle)
}

/// One line telling the reader whether a given site actually ships this sheet.
///
/// The whole reason a theme author reads this output is to know what they have
/// to displace. A rule in a sheet the site never loads is not a rule they have
/// to fight — and since 2026-08-04 that is a real distinction rather than a
/// theoretical one, because most sheets here are gated. Saying so is the
/// cheapest way to publish the partition: it lands in the tool people already
/// run, instead of a table someone has to remember to consult.
fn shipping_note(sheet: &str) -> &'static str {
    match sheet {
        "site.css" => "always shipped",
        "site/callouts.css" => "shipped only when some page has a callout",
        "site/search.css" => "shipped only when [site].search is on (off by default)",
        "site/sidenotes.css" => {
            "shipped only when some page defines a footnote; the rules themselves \
             render nothing below a 76rem viewport"
        }
        "site/field-places.css" => "shipped only when some page draws a declared field (a `place` in [fields])",
        "site/vertical.css" => "shipped only when some page uses vertical typesetting",
        "site/places-explorer.css" => {
            "shipped only when the site has a place-typed namespace root with the explorer on"
        }
        "comments.css" => "linked only when the site has synced comments",
        "email.css" => {
            "linked only when the email channel is installed or a page has :::subscribe"
        }
        "review.css" => "linked only when the site has review data",
        _ => "shipping conditions unknown — add a row to shipping_note()",
    }
}

/// Render every default rule mentioning `needle`, grouped by stylesheet.
pub fn rules_matching(needle: &str) -> String {
    let mut out = String::new();
    for (name, css) in sheets() {
        let matches: Vec<StyleRule<'_>> = style_rules(css).into_iter().filter(|r| rule_mentions(r, needle)).collect();
        if matches.is_empty() {
            continue;
        }
        out.push_str(&format!("/* ===== {name} — {} rules ===== */\n", matches.len()));
        out.push_str(&format!("/* {} */\n\n", shipping_note(&name)));
        for m in &matches {
            if let Some(c) = m.comment {
                out.push_str(c);
                out.push('\n');
            }
            for cond in &m.conditions {
                out.push_str(&format!("{cond} {{\n"));
            }
            out.push_str(m.rule);
            out.push('\n');
            for _ in &m.conditions {
                out.push_str("}\n");
            }
            out.push('\n');
        }
    }
    out
}

/// Print the matching rules. Returns a CLI exit code.
///
/// An unmatched selector is not an error: an agent probing the vocabulary
/// should not have to distinguish "no such component" from "the command
/// failed", and a nonzero exit invites a retry loop.
pub fn run(needle: &str) -> i32 {
    let out = rules_matching(needle);
    if out.trim().is_empty() {
        println!("no rules match {needle}");
        println!("Try `moss describe --json` for the class and custom-property vocabulary.");
    } else {
        print!("{out}");
    }
    0
}

#[cfg(test)]
#[path = "css_query_tests.rs"]
mod tests;
