//! The header's second level: a header page inside a header folder is listed
//! under that folder's entry. Hooks (`.nav-group`, `.nav-sub-toggle`,
//! `.nav-sub`) are styled in site.css and driven by `js-src/site/nav/submenu.ts`.

use super::{sort_nav_items, NavItem};
use crate::build::features::html_escape;
use crate::build::types::ParsedDocument;
use crate::i18n::Language;
use moss_core::PageKind;

/// For each header page, the index of the header folder it is listed under.
///
/// The parent is the outermost header folder (same edition, not a language
/// root) whose directory holds the page, so a third level joins the same list.
/// A page whose folder is not itself in the header has no parent and stays a
/// top-level link.
pub(super) fn parent_indices(docs: &[&ParsedDocument]) -> Vec<Option<usize>> {
    fn folder_dir(d: &ParsedDocument) -> Option<&str> {
        if d.kind != PageKind::Folder || crate::build::render::lang_roots::is_language_root(&d.url_path) {
            return None;
        }
        d.url_path.strip_suffix("index.html").filter(|dir| !dir.is_empty())
    }
    docs.iter()
        .map(|doc| {
            (0..docs.len())
                .filter(|&j| docs[j].edition == doc.edition && docs[j].url_path != doc.url_path)
                .filter_map(|j| folder_dir(docs[j]).map(|dir| (j, dir)))
                .filter(|(_, dir)| doc.url_path.starts_with(dir))
                .min_by_key(|(_, dir)| dir.len())
                .map(|(j, _)| j)
        })
        .collect()
}

/// Top-level items, each parent's link replaced by its group. `items` is
/// aligned with `parents`.
pub(super) fn nest<'a>(items: Vec<NavItem<'a>>, parents: &[Option<usize>], lang: Language) -> Vec<NavItem<'a>> {
    let mut slots: Vec<Option<NavItem<'a>>> = items.into_iter().map(Some).collect();
    let mut kids: Vec<Vec<NavItem<'a>>> = (0..slots.len()).map(|_| Vec::new()).collect();
    for (i, parent) in parents.iter().enumerate() {
        if let Some(p) = *parent {
            if let Some(item) = slots[i].take() {
                kids[p].push(item);
            }
        }
    }
    let mut group = 0;
    let mut top = Vec::new();
    for (mut item, mut children) in slots.into_iter().zip(kids).filter_map(|(s, k)| s.map(|s| (s, k))) {
        if !children.is_empty() {
            sort_nav_items(&mut children);
            group += 1;
            item.html = group_html(&item, &children, group, lang);
        }
        top.push(item);
    }
    top
}

/// The parent link, its disclosure button and the hidden list. The button and
/// the list stay `hidden` until script unhides the button, so a reader without
/// script never meets a dead control; the parent link still works.
fn group_html(parent: &NavItem<'_>, children: &[NavItem<'_>], n: usize, lang: Language) -> String {
    let id = format!("nav-sub-{n}");
    let label = format!("{}: {}", html_escape(parent.label), crate::i18n::t(lang, "nav_more"));
    let list: String = children.iter().map(|c| format!("<li>{}</li>", c.html)).collect();
    format!(
        r#"<span class="nav-group">{link}<button class="nav-sub-toggle" type="button" hidden aria-expanded="false" aria-controls="{id}" aria-label="{label}"></button><ul class="nav-sub" id="{id}" hidden>{list}</ul></span>"#,
        link = parent.html,
    )
}
