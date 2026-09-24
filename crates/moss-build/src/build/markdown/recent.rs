//! :::recent shortcode renderer.
//!
//! Queries the build's post set for recent posts (filtered by date and
//! scope), emits HTML (`<ul class="moss-recent">...`) for web rendering
//! and plain-text equivalent for email body_text. Same source data; two
//! emitters keep web and email exact peers.
//!
//! The shortcode AST node (`moss_core::ast::Shortcode::Recent`) is
//! dispatched by `pipeline::render_shortcode` (see Task 4.4).

use chrono::{DateTime, Duration, Utc};
use moss_core::ast::RecentShortcode;

use crate::build::types::ParsedDocument;

/// The web-HTML rendering of a `:::recent` block.
///
/// Despite this module's name, on a web page this does **not** query posts: the
/// per-page processor carries no aggregate `ParsedDocument` slice, so all it can
/// do is render the authored fallback markdown as ordinary content. The live
/// `<ul class="moss-recent">` list ([`render_html`]) is reachable only from the
/// email-digest path (`infra/newsletter.rs`), where the post slice is in scope.
///
/// An empty fallback therefore collapses to nothing, and that silence is the
/// worst outcome available: the block parsed cleanly, so none of the usual
/// "shortcode did not render" causes apply and the author gets no signal at all.
/// `:::recent {count=5}` with no body is the obvious way to write this, so it
/// warns rather than vanishing. `moss describe --json` carries the same caveat
/// on the `moss-recent` entry.
///
/// moss#915: the fallback render must go through `render_markdown_to_html_with`.
/// It was a bare `pulldown_cmark::Parser::new`, which silently mishandled
/// footnotes, wikilinks and tables (ADR-036's "parse once" bug class).
pub fn render_web(
    args: &RecentShortcode,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    heading_anchors: bool,
) -> String {
    if args.fallback_markdown.is_empty() {
        log::warn!(
            target: "build",
            "`:::recent` on a web page renders its fallback body, not a live post list \
             (the live list is emitted only in email digests). This block has an empty \
             body, so it produced nothing. Add fallback content between the fences, or \
             rely on the automatic child listing a folder home page emits."
        );
        return String::new();
    }
    crate::build::markdown::pipeline::render_markdown_to_html_with(
        &args.fallback_markdown,
        &|s: &str| s.to_string(),
        media_lookup,
        heading_anchors,
    )
}

/// Resolved query parameters. Public for testing.
#[derive(Debug, PartialEq)]
pub struct ResolvedQuery {
    pub since: Option<DateTime<Utc>>,
    pub count: usize,
}

const DEFAULT_COUNT: usize = 10;

/// Resolve the AST node's params to a runtime query. Errors collapse to
/// safe defaults (the renderer never panics on bad input).
pub fn resolve_query(args: &RecentShortcode, now: DateTime<Utc>) -> ResolvedQuery {
    let mut since: Option<DateTime<Utc>> = None;
    if let Some(s) = &args.since {
        if let Ok(d) = DateTime::parse_from_rfc3339(&format!("{s}T00:00:00Z")) {
            since = Some(d.with_timezone(&Utc));
        }
    }
    if let Some(last) = &args.last {
        if let Some(dur) = parse_duration(last) {
            // If both since and last are set, take the LATER cutoff (more restrictive)
            let last_cutoff = now - dur;
            since = match since {
                Some(s) => Some(s.max(last_cutoff)),
                None => Some(last_cutoff),
            };
        }
    }
    let count = args.count.map(|c| c as usize).unwrap_or(DEFAULT_COUNT);
    ResolvedQuery { since, count }
}

fn parse_duration(s: &str) -> Option<Duration> {
    match s {
        "day" | "1d" => Some(Duration::days(1)),
        "week" | "7d" => Some(Duration::days(7)),
        "month" | "30d" => Some(Duration::days(30)),
        s if s.ends_with('d') => s
            .trim_end_matches('d')
            .parse::<i64>()
            .ok()
            .map(Duration::days),
        _ => None,
    }
}

/// Filter + sort the post set for the resolved query and page scope.
///
/// Scope matching: when `page_scope` is non-empty, only return posts in
/// `/{page_scope}/...`. When empty (single-lang site), return all posts.
///
/// `posts` is the same `&[ParsedDocument]` slice already in scope in the
/// shortcode dispatcher. We avoid taking it by value.
pub fn query_posts<'a>(
    posts: &'a [ParsedDocument],
    query: &ResolvedQuery,
    page_scope: &str,
) -> Vec<&'a ParsedDocument> {
    let mut filtered: Vec<&ParsedDocument> = posts
        .iter()
        .filter(|d| d.is_listable())
        .filter(|d| {
            // Skip the page rendering this shortcode (would self-link).
            // Practical heuristic: don't include index.html pages OR the
            // current page itself. Caller doesn't pass current url_path
            // in v1 — accepted limitation; v2 plumbs it.
            !d.url_path.ends_with("/index.html") && d.url_path != "index.html"
        })
        .filter(|d| {
            if page_scope.is_empty() {
                true
            } else {
                d.url_path
                    .trim_start_matches('/')
                    .starts_with(&format!("{page_scope}/"))
            }
        })
        .filter(|d| match (query.since, parse_doc_date(d, DateBound::Latest)) {
            (Some(cutoff), Some(date)) => date >= cutoff,
            (Some(_), None) => false, // dateless posts excluded when filtering
            (None, _) => true,
        })
        .collect();

    // Sort newest-first. Posts without dates sort last.
    filtered.sort_by(|a, b| {
        let da = parse_doc_date(a, DateBound::Earliest);
        let db = parse_doc_date(b, DateBound::Earliest);
        db.cmp(&da)
    });
    filtered.truncate(query.count);
    filtered
}

/// Which instant a partial-precision date (`parse_doc_date`'s case 4) resolves
/// to. A full `YYYY-MM-DD` (or datetime) date is exact and ignores this —
/// only a month- or year-only date has more than one instant to choose from.
#[derive(Clone, Copy)]
enum DateBound {
    /// Day 1 / January — what sorting uses, so a month-dated post sorts
    /// alongside other posts as if written on the 1st.
    Earliest,
    /// The last day of the month / December 31st — what a `since:` cutoff
    /// uses, so a post dated only to the month isn't excluded just because
    /// its assumed day-of-month happens to be too early.
    Latest,
}

fn parse_doc_date(d: &ParsedDocument, bound: DateBound) -> Option<DateTime<Utc>> {
    // ParsedDocument carries date as String (existing convention — see
    // build/types.rs). Try a few common shapes seen in moss site frontmatter.
    let raw = d.date.as_deref()?;
    // 1. RFC 3339 with TZ: "2026-05-27T12:00:00Z" or "...+09:00".
    if let Ok(d) = DateTime::parse_from_rfc3339(raw) {
        return Some(d.with_timezone(&Utc));
    }
    // 2. Date-only: "2026-05-27" → pad to T00:00:00Z.
    if let Ok(d) = DateTime::parse_from_rfc3339(&format!("{raw}T00:00:00Z")) {
        return Some(d.with_timezone(&Utc));
    }
    // 3. Space-separated datetime: "2026-05-27 12:00:00" (Hugo, Jekyll,
    //    and many manual frontmatter styles). NaiveDateTime → assume UTC.
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    // 4. Partial precision ("1795-06", "1795") — the frontmatter schema
    //    allows dating a page to the month or year alone. `bound` picks which
    //    end of the range the missing month/day resolves to; sorting and the
    //    `since:` filter want different ends (see `DateBound`).
    let pd = crate::build::components::date::parse_partial_date(raw)?;
    let naive_date = match bound {
        DateBound::Earliest => pd.earliest(),
        DateBound::Latest => pd.latest(),
    }?;
    Some(DateTime::from_naive_utc_and_offset(naive_date.and_hms_opt(0, 0, 0)?, Utc))
}

/// Render the resolved posts as an HTML list. Empty input returns empty
/// string — caller renders the AST node's fallback_markdown in that case.
pub fn render_html(posts: &[&ParsedDocument]) -> String {
    if posts.is_empty() {
        return String::new();
    }
    let mut out = String::from("<ul class=\"moss-recent\">\n");
    for p in posts {
        let date_str = parse_doc_date(p, DateBound::Earliest)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        out.push_str(&format!(
            "  <li><a href=\"/{href}\">{title}</a><div class=\"moss-recent__date\">{date}</div><div class=\"moss-recent__desc\">{desc}</div></li>\n",
            href = p.url_path.trim_start_matches('/'),
            title = html_escape(&p.title),
            date = date_str,
            desc = html_escape(p.description.as_deref().unwrap_or("")),
        ));
    }
    out.push_str("</ul>\n");
    out
}

/// Render the resolved posts as plain text (for email body_text).
pub fn render_plaintext(posts: &[&ParsedDocument]) -> String {
    if posts.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for p in posts {
        let date_str = parse_doc_date(p, DateBound::Earliest)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        out.push_str(&format!(
            "- {title} ({date}) - /{href}\n  {desc}\n\n",
            title = p.title,
            date = date_str,
            href = p.url_path.trim_start_matches('/'),
            desc = p.description.as_deref().unwrap_or(""),
        ));
    }
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::types::ParsedDocument;

    fn doc(url: &str, title: &str, date: Option<&str>, desc: Option<&str>) -> ParsedDocument {
        let mut d = ParsedDocument::default();
        d.url_path = url.to_string();
        d.title = title.to_string();
        d.date = date.map(|s| s.to_string());
        d.description = desc.map(|s| s.to_string());
        d
    }

    fn fixed_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-27T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn resolve_query_defaults_to_count_10_no_since() {
        let r = resolve_query(&RecentShortcode::default(), fixed_now());
        assert_eq!(r.count, 10);
        assert!(r.since.is_none());
    }

    #[test]
    fn resolve_query_parses_since_and_count() {
        let args = RecentShortcode {
            since: Some("2026-04-01".to_string()),
            count: Some(5),
            ..Default::default()
        };
        let r = resolve_query(&args, fixed_now());
        assert!(r.since.is_some());
        assert_eq!(r.count, 5);
    }

    #[test]
    fn resolve_query_last_month_subtracts_30d() {
        let args = RecentShortcode {
            last: Some("month".to_string()),
            ..Default::default()
        };
        let r = resolve_query(&args, fixed_now());
        assert_eq!(r.since.unwrap(), fixed_now() - Duration::days(30));
    }

    #[test]
    fn query_filters_to_page_scope() {
        let docs = vec![
            doc("en/posts/a.html", "A", Some("2026-04-01"), None),
            doc("zh/posts/b.html", "B", Some("2026-04-02"), None),
            doc("en/posts/c.html", "C", Some("2026-04-03"), None),
        ];
        let q = ResolvedQuery {
            since: None,
            count: 10,
        };
        let result = query_posts(&docs, &q, "en");
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|d| d.url_path.starts_with("en/")));
    }

    #[test]
    fn query_returns_newest_first() {
        let docs = vec![
            doc("a.html", "A", Some("2026-04-01"), None),
            doc("c.html", "C", Some("2026-05-01"), None),
            doc("b.html", "B", Some("2026-04-15"), None),
        ];
        let q = ResolvedQuery {
            since: None,
            count: 10,
        };
        let result = query_posts(&docs, &q, "");
        assert_eq!(result[0].title, "C");
        assert_eq!(result[1].title, "B");
        assert_eq!(result[2].title, "A");
    }

    #[test]
    fn query_respects_count_limit_and_since_cutoff() {
        let docs = vec![
            doc("a.html", "A", Some("2026-03-01"), None),
            doc("b.html", "B", Some("2026-04-15"), None),
            doc("c.html", "C", Some("2026-05-01"), None),
        ];
        let cutoff = DateTime::parse_from_rfc3339("2026-04-10T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let q = ResolvedQuery {
            since: Some(cutoff),
            count: 1,
        };
        let result = query_posts(&docs, &q, "");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "C");
    }

    #[test]
    fn render_html_emits_list_with_links_dates_descs() {
        let d = doc("posts/a.html", "Hello", Some("2026-05-01"), Some("a summary"));
        let html = render_html(&[&d]);
        assert!(html.contains(r#"<a href="/posts/a.html">Hello</a>"#));
        assert!(html.contains("2026-05-01"));
        assert!(html.contains("a summary"));
        assert!(html.contains("moss-recent"));
    }

    #[test]
    fn render_html_empty_returns_empty_string() {
        assert_eq!(render_html(&[]), "");
    }

    #[test]
    fn render_plaintext_emits_dashed_list() {
        let d = doc("posts/a.html", "Hello", Some("2026-05-01"), Some("summary"));
        let text = render_plaintext(&[&d]);
        assert!(text.contains("- Hello (2026-05-01)"));
        assert!(text.contains("/posts/a.html"));
        assert!(text.contains("summary"));
    }

    #[test]
    fn recent_excludes_drafts() {
        let mut draft = doc("p/wip/wip.html", "WIP Post", Some("2026-01-02"), None);
        draft.draft = Some(true);
        let live = doc("p/live/live.html", "Live Post", Some("2026-01-01"), None);
        let posts = vec![live, draft];
        let q = ResolvedQuery { since: None, count: 10 };
        let got = query_posts(&posts, &q, "");
        assert!(got.iter().all(|d| d.draft != Some(true)), "drafts must be excluded from :::recent");
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn parse_doc_date_accepts_space_separated_datetime() {
        // Hugo/Jekyll-style "2026-05-27 12:00:00" — NOT RFC 3339.
        let d = doc("posts/a.html", "A", Some("2026-05-27 12:00:00"), None);
        assert!(
            parse_doc_date(&d, DateBound::Earliest).is_some(),
            "space-separated datetime should parse"
        );
    }

    #[test]
    fn parse_doc_date_returns_none_for_garbage() {
        let d = doc("posts/a.html", "A", Some("not a date"), None);
        assert!(parse_doc_date(&d, DateBound::Earliest).is_none());
    }

    #[test]
    fn parse_doc_date_accepts_month_precision() {
        // "1795-06" (a real site's dating precision) used to fail
        // every shape this function tries and come back None, which the
        // caller reads as dateless.
        let d = doc("posts/a.html", "A", Some("1795-06"), None);
        let parsed = parse_doc_date(&d, DateBound::Earliest);
        assert!(parsed.is_some(), "month-precision date should parse");
        assert_eq!(
            parsed.unwrap().format("%Y-%m-%d").to_string(),
            "1795-06-01"
        );
    }

    #[test]
    fn since_filter_passes_a_partial_date_when_its_latest_possible_day_clears_the_cutoff() {
        // today 2026-09-13, since: 7d -> cutoff 2026-09-06. A month-only date
        // ("2026-09") defaulted its day to 1 for the filter too, which put it
        // at 2026-09-01 -- before the cutoff -- and dropped a page that, for
        // all the frontmatter says, could have been written any day this
        // month, including today. The filter must ask whether the LATEST day
        // the date could denote clears the cutoff; sorting still uses the
        // earliest, so ordering among same-month posts is unaffected.
        let cutoff = DateTime::parse_from_rfc3339("2026-09-06T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let docs = vec![
            doc("a.html", "MonthMatch", Some("2026-09"), None),
            doc("b.html", "MonthTooOld", Some("2026-08"), None),
            doc("c.html", "FullTooOld", Some("2026-09-05"), None),
        ];
        let q = ResolvedQuery { since: Some(cutoff), count: 10 };
        let result = query_posts(&docs, &q, "");
        let titles: Vec<&str> = result.iter().map(|d| d.title.as_str()).collect();
        assert_eq!(titles, vec!["MonthMatch"], "got: {:?}", titles);
    }

    #[test]
    fn query_posts_includes_and_sorts_month_precision_dates_under_since_filter() {
        // since: excludes a post whose date fails to parse — so this only
        // stays green if "1795-06" actually parses (and sorts before a
        // cutoff-satisfying 1700 post, after a satisfying 2026 one).
        let docs = vec![
            doc("a.html", "Old", Some("1795-06"), None),
            doc("b.html", "New", Some("2026-04-01"), None),
        ];
        let cutoff = DateTime::parse_from_rfc3339("1700-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let q = ResolvedQuery {
            since: Some(cutoff),
            count: 10,
        };
        let result = query_posts(&docs, &q, "");
        assert_eq!(result.len(), 2, "a month-dated post must not be treated as dateless");
        assert_eq!(result[0].title, "New");
        assert_eq!(result[1].title, "Old");
    }
}
