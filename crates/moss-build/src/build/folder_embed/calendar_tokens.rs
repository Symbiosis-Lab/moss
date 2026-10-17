//! Where an embedded listing's calendar links go.
//!
//! A body embed of a folder that has a home page leaves a token right after its
//! listing. Once every embed on the page is expanded, the last token of each
//! folder becomes that folder's calendar links and the others are dropped, so
//! a folder embedded twice gets its links once, under the second listing.

use crate::build::markdown::body_plan::BodyPlan;
use crate::build::feeds::calendar::CalendarPlan;
use crate::build::site_url::SiteUrl;
use crate::build::types::ParsedDocument;
use std::collections::HashMap;

const TOKEN: &str = "<!--MOSS_EMBED_CALENDAR:";
const END: &str = "-->";

/// The token for embed number `ordinal` of the folder whose home page is `home_url_path`.
pub(super) fn token(ordinal: usize, home_url_path: &str) -> String {
    format!("{TOKEN}{ordinal}:{home_url_path}{END}")
}

/// The links an embedded folder gets on `host`: none without a calendar plan,
/// and none for the host's own folder when the host already ends with them
/// (`prints_folder_calendar_links`), so they are never printed twice.
pub(super) fn links_for<'a>(
    calendar: Option<(&'a CalendarPlan, &'a SiteUrl)>,
    host: &ParsedDocument,
    now: chrono::NaiveDateTime,
) -> impl Fn(&str) -> String + 'a {
    let (own_url, prints_own, lang) = (host.url_path.clone(), host.prints_folder_calendar_links(), host.lang);
    move |folder| match calendar {
        Some((cal, site_url)) if !(prints_own && folder == own_url) => {
            cal.embed_links_html(folder, site_url, lang, now)
        }
        _ => String::new(),
    }
}

/// Calls `at(ordinal, folder)` for each token in `html` and keeps the text
/// between them, with every token replaced by what `at` returns. A token that
/// never closes is ordinary text; one whose body is not `ordinal:folder` is dropped.
fn replace(html: &str, mut at: impl FnMut(usize, &str) -> String) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some((before, after)) = rest.split_once(TOKEN) {
        out.push_str(before);
        let Some((body, tail)) = after.split_once(END) else {
            out.push_str(TOKEN);
            rest = after;
            continue;
        };
        if let Some((ordinal, folder)) = body.split_once(':').and_then(|(o, f)| Some((o.parse().ok()?, f))) {
            out.push_str(&at(ordinal, folder));
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Replaces the tokens of a page: the last one of each folder (in page order)
/// becomes `links_for(folder)`, every other one becomes nothing.
pub(super) fn settle(plan: &mut BodyPlan, links_for: &dyn Fn(&str) -> String) {
    let mut last: HashMap<String, usize> = HashMap::new();
    plan.for_each_html(&mut |h| {
        replace(h, |ordinal, folder| {
            last.insert(folder.to_string(), ordinal);
            String::new()
        });
    });
    plan.map_html(&|h| {
        replace(h, |ordinal, folder| if last.get(folder) == Some(&ordinal) { links_for(folder) } else { String::new() })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settled(parts: &[&str]) -> String {
        let mut plan = BodyPlan::opaque(parts.concat());
        settle(&mut plan, &|folder| format!("[links {folder}]"));
        plan.to_html()
    }

    #[test]
    fn the_last_embed_of_each_folder_keeps_the_links() {
        let (a0, b1, a2) = (token(0, "a/index.html"), token(1, "b/index.html"), token(2, "a/index.html"));
        assert_eq!(settled(&["x", &a0, "y", &b1, "z", &a2, "w"]), "xy[links b/index.html]z[links a/index.html]w");
    }

    #[test]
    fn the_last_embed_wins_across_segments() {
        let mut plan = BodyPlan::opaque(format!("one{}", token(0, "a/index.html")));
        plan.segments.push(crate::build::markdown::body_plan::BodySegment::Html(format!("two{}", token(1, "a/index.html"))));
        settle(&mut plan, &|_| "[links]".to_string());
        assert_eq!(plan.to_html(), "onetwo[links]");
    }

    #[test]
    fn an_unterminated_token_stays_text_and_a_malformed_one_is_dropped() {
        assert_eq!(settled(&["a<!--MOSS_EMBED_CALENDAR:0:x"]), "a<!--MOSS_EMBED_CALENDAR:0:x");
        assert_eq!(settled(&["a<!--MOSS_EMBED_CALENDAR:nope-->b"]), "ab");
    }
}
