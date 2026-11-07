//! Country maps: a page whose markdown table is drawn as a world map with each
//! country coloured by one column, plus a legend, an as-of stamp and the rows
//! as text.
//!
//! The page's `country_map:` frontmatter names the column and its ordered
//! levels ([`spec`]); the table is read from the page body; each
//! row's country is matched against the embedded shapes ([`shapes`],
//! [`matching`]); and [`resolve`] turns all of that into a [`resolve::Model`]
//! plus the build problems an author must see. [`render`] draws the model in
//! Equal Earth ([`projection`]), which keeps areas true so a colour never
//! claims more of the page than the country holds.

pub(crate) mod matching;
pub(crate) mod projection;
pub(crate) mod render;
pub(crate) mod resolve;
pub(crate) mod search;
pub(crate) mod shapes;
pub(crate) mod spec;

/// Every country-map page's problems, as build problems (a failure under `--strict`).
pub(crate) fn warn_problems(documents: &[crate::build::types::ParsedDocument]) {
    for problem in documents.iter().filter_map(|d| d.country_map.as_ref()).flat_map(|m| &m.problems) {
        crate::build::cli_output::log_warn_problem!("{problem}");
    }
}
