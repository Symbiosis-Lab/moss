//! Sidebar navigation component for "Latest" article list
//!
//! Renders a sidebar with dated articles, used when the `sidebar` field is set.
//! Format: "Latest" header + list of `{year.month} {title}` items.

use super::child_list::ArticleListItemProps;
use crate::build::media::cover::html_escape;

/// Renders a sidebar with "Latest" header and article list.
///
/// # Arguments
/// * `articles` - Article props sorted by date (newest first), already filtered to dated only
/// * `more_url` - Optional URL for a "More" link (cross-referencing sidebar only)
/// * `lang` - Site language for UI string localization
///
/// # Returns
/// HTML string with sidebar navigation including "Latest" header
pub fn render(articles: &[ArticleListItemProps], more_url: Option<&str>, lang: crate::i18n::Language) -> String {
    if articles.is_empty() {
        return String::new();
    }

    let items: Vec<String> = articles
        .iter()
        .map(|article| {
            format!(
                r#"<li><a href="{}" class="moss-prefix-link"><span class="moss-prefix-link-prefix">{}</span><span class="moss-prefix-link-title">{}</span></a></li>"#,
                html_escape(&article.url),
                html_escape(&article.date_display),
                html_escape(&article.title)
            )
        })
        .collect();

    let more_link = match more_url {
        Some(url) => format!(
            r#"<a href="{}" class="sidebar-more">{}</a>"#,
            html_escape(url),
            crate::i18n::t(lang, "more_link"),
        ),
        None => String::new(),
    };

    format!(
        r#"<nav class="latest-sidebar"><h3>{}</h3><ul>{}</ul>{}</nav>"#,
        crate::i18n::t(lang, "site_latest"),
        items.join("\n"),
        more_link
    )
}

#[cfg(test)]
#[path = "sidebar_nav_tests.rs"]
mod tests;
