//! `moss describe --css <needle>` — print the default rules a theme override
//! has to displace.
//!
//! ## Why a query and not the sheet
//!
//! The emitted stylesheet is ~89 KB minified. Telling an agent to read it is
//! telling it to spend most of its context on vertical-Chinese typesetting and
//! immersive fullscreen in order to change a hero. okagaki's whole 136-line
//! theme fights about 50 rules. So: print the matching ones.
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

/// One matched rule, with the context needed to reproduce its effect.
struct Match<'a> {
    /// The `@media`/`@supports`/`@layer` conditions the rule sits inside,
    /// outermost first. Without these a copied rule silently applies at every
    /// width — the single most common way a theme override goes wrong.
    conditions: Vec<&'a str>,
    /// The comment block immediately above the rule, if any.
    comment: Option<&'a str>,
    /// Selector list plus declaration block, verbatim.
    rule: &'a str,
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
fn mentions(haystack: &str, needle: &str) -> bool {
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

/// Walk a stylesheet by brace depth, collecting rules whose selector mentions
/// `needle` along with the at-rule conditions enclosing them.
///
/// Deliberately not a real CSS parser. It has one job — slice out source spans
/// — and a parser would drop the comments, which are half the value.
// `i` walks bytes and can sit mid-character, but every slice below is taken at a
// point where `bytes[i]` matched an ASCII delimiter (`/` `*` `{` `}` `;`) or at
// `bytes.len()`. An ASCII byte never occurs inside a multi-byte UTF-8 sequence,
// so those indices — and `prelude_start`, which is only ever assigned one of
// them — are always char boundaries. None of these can panic.
#[allow(clippy::string_slice)]
fn scan<'a>(css: &'a str, needle: &str, out: &mut Vec<Match<'a>>) {
    let bytes = css.as_bytes();
    // Stack of open at-rule preludes (`@media (max-width: 32rem)`), so a nested
    // rule reports every condition it is under.
    let mut conditions: Vec<&'a str> = Vec::new();
    // Start of the current prelude — selector list or at-rule header.
    let mut prelude_start = 0usize;
    // End of the last comment block seen at the current position, and its span.
    let mut pending_comment: Option<&'a str> = None;
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let start = i;
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                // Only a comment sitting alone above a rule is context for it.
                // One trailing a declaration (`.a { … } /* note */`) belongs to
                // the line it is on, and hoisting it above the next rule would
                // put unrelated prose in front of half the output. The tell is
                // a newline between the previous `}` and the comment.
                let gap = &css[prelude_start..start];
                if gap.trim().is_empty() && (prelude_start == 0 || gap.contains('\n')) {
                    pending_comment = Some(css[start..i].trim_end());
                    prelude_start = i;
                }
            }
            b'{' => {
                let prelude = css[prelude_start..i].trim();
                if prelude.starts_with('@') {
                    // An at-rule block: descend, remembering the condition.
                    conditions.push(prelude);
                    i += 1;
                    prelude_start = i;
                    pending_comment = None;
                    continue;
                }
                // A style rule: find its matching close brace. Declaration
                // blocks do not nest in moss's sheets, so depth-1 suffices.
                let mut depth = 1;
                i += 1;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                // A custom property lives in the declarations, not the
                // selector — and "what reads --moss-grid-image-ratio?" is
                // exactly the question an agent asks before setting one. So
                // `--`-prefixed needles search the whole rule.
                let haystack = if needle.starts_with("--") {
                    &css[prelude_start..i]
                } else {
                    prelude
                };
                if mentions(haystack, needle) {
                    out.push(Match {
                        conditions: conditions.clone(),
                        comment: pending_comment,
                        rule: css[prelude_start..i].trim(),
                    });
                }
                prelude_start = i;
                pending_comment = None;
            }
            b'}' => {
                conditions.pop();
                i += 1;
                prelude_start = i;
                pending_comment = None;
            }
            b';' => {
                // A statement at this level (`@import`, `@layer a, b;`).
                i += 1;
                prelude_start = i;
                pending_comment = None;
            }
            _ => i += 1,
        }
    }
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
        "site/vertical.css" => "shipped only when some page uses vertical typesetting",
        "comments.css" => "linked only when the site has synced comments",
        "email.css" => {
            "linked only when the email channel is installed or a page has :::subscribe / :::apply"
        }
        "review.css" => "linked only when the site has review data",
        _ => "shipping conditions unknown — add a row to shipping_note()",
    }
}

/// Render every default rule mentioning `needle`, grouped by stylesheet.
pub fn rules_matching(needle: &str) -> String {
    let mut out = String::new();
    for (name, css) in sheets() {
        let mut matches = Vec::new();
        scan(css, needle, &mut matches);
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
