//! Year-grouped article list component
//!
//! Renders articles grouped by year with headings, used for:
//! - Auto-generated home pages (directories without index files)
//! - Collection index pages (optional)

use super::child_list::{self as child_list_item, ArticleListItemProps};
use super::date as date_formatters;

/// Renders a list of articles grouped by year.
///
/// Articles are sorted by date (newest first) and grouped under year headings.
///
/// # Output (standard mode)
/// ```html
/// <section class="moss-cards-minimal-year-group">
///   <h2>2025</h2>
///   <p><span class="date">2025 · 11</span>&nbsp;&nbsp;<a href="...">Title</a></p>
///   <p><span class="date">2025 · 09</span>&nbsp;&nbsp;<a href="...">Title</a></p>
/// </section>
/// ```
///
/// # Output (minimal mode - month only, flexbox structure)
/// ```html
/// <section class="moss-cards-minimal-year-group minimal">
///   <h2>2025</h2>
///   <p class="moss-card"><span class="date">11</span><a href="...">Title</a></p>
///   <p class="moss-card"><span class="date">09</span><a href="...">Title</a></p>
/// </section>
/// ```
///
/// # Arguments
/// * `articles` - List of article properties to render
/// * `minimal` - If true, uses minimal styling: month-only dates, no underlines, flexbox layout
///
/// # Returns
/// HTML string with moss-cards-minimal-year-grouped article list
pub fn render(
    articles: &[ArticleListItemProps],
    minimal: bool,
    lang: crate::i18n::Language,
    typesetting: Option<&str>,
) -> String {
    let section_class = if minimal { "moss-cards-minimal-year-group minimal" } else { "moss-cards-minimal-year-group" };
    render_internal(articles, section_class, minimal, lang, typesetting)
}

/// Internal render function with all options.
///
/// # Arguments
/// * `articles` - List of article properties to render
/// * `section_class` - CSS class(es) to apply to section elements
/// * `use_minimal_items` - If true, use render_minimal for article items
///
/// # Returns
/// HTML string with moss-cards-minimal-year-grouped article list
fn render_internal(
    articles: &[ArticleListItemProps],
    section_class: &str,
    use_minimal_items: bool,
    lang: crate::i18n::Language,
    typesetting: Option<&str>,
) -> String {
    // Dated rows go newest first through the one date-axis comparator, so two
    // rows on the same date fall in the order the folder's series links walk.
    // Undated rows lead, as folder listings keep them, ordered by their
    // display string and then title.
    let mut sorted_articles = articles.to_vec();
    sorted_articles.sort_by(|a, b| match (&a.date_raw, &b.date_raw) {
        (Some(_), Some(_)) => moss_core::sort::cmp_date_axis(&a.date_sort_key(), &b.date_sort_key()),
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, None) => b.date_display.cmp(&a.date_display)
            .then_with(|| moss_core::sort::cmp_labels(&a.title, &b.title)),
    });

    // Group by year
    let mut current_year: Option<i32> = None;
    let mut sections: Vec<String> = Vec::new();
    let mut current_section_items: Vec<String> = Vec::new();

    for article in &sorted_articles {
        let year = year_of(article);

        if year != current_year {
            // Finish previous section if exists
            if !current_section_items.is_empty() {
                if let Some(y) = current_year {
                    sections.push(format!(
                        r#"<section class="{}">
<h2>{}</h2>
{}
</section>"#,
                        section_class,
                        date_formatters::format_year_heading(y, lang, typesetting),
                        current_section_items.join("\n")
                    ));
                } else {
                    // Previous section had unparseable years - render without heading
                    sections.push(format!(
                        r#"<section class="{}">
{}
</section>"#,
                        section_class,
                        current_section_items.join("\n")
                    ));
                }
                current_section_items.clear();
            }
            current_year = year;
        }

        // Render article item with minimal flag controlling date format
        let item_html = child_list_item::render(article, use_minimal_items, false, lang, typesetting);
        current_section_items.push(item_html);
    }

    // Finish last section
    if !current_section_items.is_empty() {
        if let Some(y) = current_year {
            sections.push(format!(
                r#"<section class="{}">
<h2>{}</h2>
{}
</section>"#,
                section_class,
                date_formatters::format_year_heading(y, lang, typesetting),
                current_section_items.join("\n")
            ));
        } else {
            // Articles without parseable year - render without heading
            sections.push(format!(
                r#"<section class="{}">
{}
</section>"#,
                section_class,
                current_section_items.join("\n")
            ));
        }
    }

    sections.join("\n")
}

/// The year a row belongs under: the raw ISO date first, the display string
/// only as a fallback.
///
/// It used to read `date_display` alone, and `extract_year` only understands
/// four leading ASCII digits — so on a vertical CJK page, where the display
/// date is already `一七〇三年十二月`, EVERY row returned `None`, the whole
/// listing collapsed into one headingless section, and 59 works lost their
/// chronology (zhu-da home, 2026-09-11). The raw date is the same ISO string
/// the sort above already trusts.
fn year_of(article: &ArticleListItemProps) -> Option<i32> {
    article
        .date_raw
        .as_deref()
        .and_then(date_formatters::extract_year)
        .or_else(|| date_formatters::extract_year(&article.date_display))
}

#[cfg(test)]
#[path = "year_group_tests.rs"]
mod tests;
