//! What a page takes from the folder homes above it: their `cascade:` values
//! and a folder's `search_engines: false`. Both work on output addresses
//! (`url_path`, such as `notes/index.html`), so the build and a host agree on
//! which pages sit beneath which folder.

/// One folder home's `cascade:` and the pages it reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CascadeStep {
    /// The index of the page whose `cascade:` this is.
    pub source: usize,
    /// The folder's address with a leading slash, such as `/notes/`, or `/` for the site root.
    pub from: String,
    /// Every page beneath the folder, the source itself excepted.
    pub targets: Vec<usize>,
}

/// Plan the cascade over pages given as `(url_path, has_cascade)`. Only a
/// folder home (`index.html`) cascades. Steps come deepest folder first, so a
/// host that fills only unset values lets the nearest folder win.
pub fn cascade_plan<'a>(pages: impl IntoIterator<Item = (&'a str, bool)>) -> Vec<CascadeStep> {
    let pages: Vec<(&str, bool)> = pages.into_iter().collect();
    let mut steps: Vec<(&str, usize)> = pages
        .iter()
        .enumerate()
        .filter(|(_, (_, has))| *has)
        .filter_map(|(i, (url, _))| url.strip_suffix("index.html").filter(|p| p.is_empty() || p.ends_with('/')).map(|prefix| (prefix, i)))
        .collect();
    steps.sort_by_key(|(prefix, _)| std::cmp::Reverse(prefix.matches('/').count()));
    steps
        .into_iter()
        .map(|(prefix, source)| {
            let home = pages[source].0;
            let targets = pages.iter().enumerate().filter(|(_, (url, _))| url.starts_with(prefix) && *url != home).map(|(i, _)| i).collect();
            CascadeStep { source, from: format!("/{prefix}"), targets }
        })
        .collect()
}

/// For each page, the nearest folder above it (`notes/`, with its slash) whose
/// home sets `search_engines: false`. Pages come as `(url_path, hides)`, where
/// `hides` is true for a folder home that sets it. A folder's own home is not
/// beneath it.
pub fn hidden_homes<'a>(pages: impl IntoIterator<Item = (&'a str, bool)>) -> Vec<Option<String>> {
    let pages: Vec<(&str, bool)> = pages.into_iter().collect();
    let homes: Vec<String> = pages
        .iter()
        .filter(|(_, hides)| *hides)
        .filter_map(|(url, _)| url.strip_suffix("/index.html").map(|dir| format!("{dir}/")))
        .collect();
    pages
        .iter()
        .map(|(url, _)| {
            homes
                .iter()
                .filter(|home| url.strip_prefix(home.as_str()).is_some_and(|rest| rest != "index.html"))
                .max_by_key(|home| home.len())
                .cloned()
        })
        .collect()
}

#[cfg(test)]
#[path = "inherit_tests.rs"]
mod tests;
