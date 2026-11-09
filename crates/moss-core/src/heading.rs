//! Everything moss knows about a heading, in one place — not four separate
//! subjects, but four steps of one pipeline:
//!
//! ```text
//!   inline nodes / parser events ──[text]──> plain text
//!                                              │
//!                                              ├─[anchor]──> the <hN id="…">
//!                                              └─[extract]─> autocomplete rows
//!   file path + frontmatter ───────[state]───> the auto-injected <h1>
//! ```
//!
//! The keystone invariant of the whole cluster is **byte-identity**: the
//! slug `extract` reports, the `id` the renderer emits, and the raw-line
//! slug the wikilink scanner computes in `build/scan/scan.rs` must agree
//! character for character, or a `[[Page#Heading]]` link resolves to a
//! fragment the page does not have. Co-locating the four steps in one module
//! is what keeps that identity from drifting when one of them is edited in
//! isolation.

pub mod anchor;
pub mod anchor_rule;
pub mod attrs;
pub mod extract;
pub mod opening;
pub mod range;
pub mod state;
pub mod text;

pub use anchor::obsidian_heading_anchor;
pub use opening::opening_words;
pub use extract::{extract_headings, extract_headings_with_config, HeadingInfo};
pub use state::{
    article_title_outcome, body_heading_replacing_title, body_supplies_title, opening_heading_beyond_title, hero_at_top_owns_title, title_outcome, compute, TitleOutcome, filename_text, filename_text_with_root, is_untitled_stamp, HeadingInputs,
    HeadingSource, HeadingState,
};

/// Prepend a moss-injected article title `<h1>` to a rendered body.
///
/// Used by the article-page branch of the markdown pipeline when the body has
/// no `# H1` of its own. The injected heading carries the stable class
/// `moss-article-title` so themes can target it (e.g. to `display: none` on a
/// specific page that uses a hero image as its visual title instead).
///
/// The title text is HTML-escaped — moss receives it as plain text from the
/// frontmatter cascade or the title-cased filename, so we treat it as
/// untrusted relative to the HTML output context.
///
/// Injection is article-only, and the `moss-article-title` class is a stable
/// contract.
pub fn inject_article_title_h1(html_body: &str, title: &str, emit_source_lines: bool, visually_hidden: bool) -> String {
    let escaped = crate::media::html_escape(title);
    let escaped = crate::heading::range::keep_ranges_together(&escaped);
    let fm_attr = if emit_source_lines { r#" data-source-fm="title""# } else { "" };
    let class = if visually_hidden { "moss-article-title visually-hidden" } else { "moss-article-title" };
    if html_body.is_empty() {
        format!("<h1 class=\"{class}\"{fm_attr}>{escaped}</h1>\n")
    } else {
        format!("<h1 class=\"{class}\"{fm_attr}>{escaped}</h1>\n{html_body}")
    }
}
