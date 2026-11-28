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

/// One folder's work: the index of its self-named home page, and every other
/// page sharing its folder, in the caller's order.
pub struct WorkGroup {
    pub home_idx: usize,
    pub companions: Vec<usize>,
}

/// The site's works, by folder: a folder whose home is its self-named file
/// (not an index stem, not a `home: true` file of another name) and names a
/// place (`location` non-empty). Its other pages are the work's companions.
pub fn work_groups<P: super::site_page::SitePage>(pages: &[P], location: impl Fn(&P) -> &[String]) -> std::collections::BTreeMap<String, WorkGroup> {
    let mut by_folder: std::collections::BTreeMap<String, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, page) in pages.iter().enumerate() {
        let Some(path) = page.source_path() else { continue };
        by_folder.entry(path.rsplit_once('/').map(|(dir, _)| dir.to_string()).unwrap_or_default()).or_default().push(i);
    }
    let mut works = std::collections::BTreeMap::new();
    for (folder, indices) in by_folder {
        let folder_name = folder.rsplit('/').next().unwrap_or("");
        if indices.len() < 2 || folder_name.is_empty() {
            continue;
        }
        let Some(home_idx) = indices.iter().copied().find(|&i| pages[i].is_folder_index()) else { continue };
        if location(&pages[home_idx]).is_empty() {
            continue;
        }
        let home_file = pages[home_idx].source_path().and_then(|p| p.rsplit('/').next()).unwrap_or("");
        let home_stem = std::path::Path::new(home_file).file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if crate::home::is_index_stem_any_lang(home_stem) || crate::content_graph::generate_slug(home_stem) != crate::content_graph::generate_slug(folder_name) {
            continue;
        }
        let companions = indices.into_iter().filter(|&i| i != home_idx).collect();
        works.insert(folder, WorkGroup { home_idx, companions });
    }
    works
}

/// Maps read a page's places from `location`, so the names a place-typed kind
/// takes from any other field are added there too.
pub fn mirror_place_fields_into_location<P: super::site_page::SitePage>(pages: &mut [P], kinds: &[crate::terms::kinds::TermKind], location_mut: impl Fn(&mut P) -> &mut Vec<String>) {
    let fields: Vec<&str> = kinds.iter().filter(|k| k.is_place).flat_map(|k| k.fields.iter().map(String::as_str)).filter(|f| *f != "location").collect();
    for page in pages.iter_mut() {
        for field in &fields {
            for name in page.term_values(field) {
                let location = location_mut(page);
                if !location.contains(&name) {
                    location.push(name);
                }
            }
        }
    }
}

/// Each work's companions with no `location:` of their own take the home's.
pub fn inherit_work_locations<P: super::site_page::SitePage>(pages: &mut [P], location: impl Fn(&P) -> &[String], location_mut: impl Fn(&mut P) -> &mut Vec<String>) {
    for work in work_groups(pages, &location).into_values() {
        let home = location(&pages[work.home_idx]).to_vec();
        for i in work.companions {
            let own = location_mut(&mut pages[i]);
            if own.is_empty() {
                own.clone_from(&home);
            }
        }
    }
}

#[cfg(test)]
#[path = "inherit_tests.rs"]
mod tests;
