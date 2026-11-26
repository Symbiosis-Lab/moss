//! The site's navigation bar and breadcrumb trail: which pages each lists, in
//! what order, and their markup. The build and every host draw both from here.

use super::site_page::SitePage;
use crate::frontmatter_typed::BreadcrumbSetting;
use crate::i18n::Language;
use crate::media::{html_escape, html_escape_attr};

/// Filenames that put a root-level file in the nav bar of a site with no
/// content folders.
pub fn is_nav_keyword(stem: &str) -> bool {
    matches!(stem.to_lowercase().as_str(), "about" | "关于" | "關於")
}

/// Whether any of the site's page files sits in a top-level folder the walk keeps,
/// which puts the site's top-level pages in the nav bar.
pub fn has_content_folders<'a>(page_files: impl IntoIterator<Item = &'a str>) -> bool {
    page_files.into_iter().any(|f| f.split_once('/').is_some_and(|(dir, _)| !crate::source_edit::policy::is_excluded_dir_name(dir)))
}

/// Whether a page reads as a nav bar entry by position and name alone, before
/// any `nav:`: a root-level non-index file where the site has content folders,
/// or an `about` file where it has none. `is_root_index` is the site home.
pub fn is_navigational_page(
    is_root_level: bool,
    is_root_index: bool,
    clean_stem: &str,
    has_content_folders: bool,
) -> bool {
    is_root_level && !is_root_index && (has_content_folders || is_nav_keyword(clean_stem))
}

/// The frontmatter and slot facts that decide nav membership before position does.
#[derive(Debug, Clone, Copy, Default)]
pub struct NavFacts {
    /// Explicit `nav:` opt-in / opt-out.
    pub nav: Option<bool>,
    pub draft: bool,
    pub slot_only: bool,
    /// `listed: false`.
    pub unlisted: bool,
}

/// Does this page render as an item in the top navigation bar?
///
/// A slot file and a draft never do, even with `nav: true`. Without `nav:`, an
/// unlisted page is never promoted by position; `nav: true` still lists it.
/// A subfolder's index is excluded by `is_root_level`, the site home by
/// `is_root_index`.
pub fn is_nav_bar_item(
    facts: NavFacts,
    is_root_level: bool,
    is_root_index: bool,
    clean_stem: &str,
    has_content_folders: bool,
) -> bool {
    let NavFacts { nav, draft, slot_only, unlisted } = facts;
    if slot_only || draft {
        return false;
    }
    match nav {
        Some(v) => v,
        None => !unlisted && is_navigational_page(is_root_level, is_root_index, clean_stem, has_content_folders),
    }
}

/// An absolute http(s) address.
pub fn is_valid_external_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// The language whose chrome a page shows: its own only when it lives under a
/// language tree (`en/…`), else the site's. Content detection alone must not
/// move it: an English page on a Chinese site with no `en/` tree would link
/// home to an `/en/` that does not exist and match no nav items.
pub fn chrome_lang(url_path: Option<&str>, page_lang: Language, site_lang: Language) -> Language {
    if url_path.and_then(crate::home::lang_tree_prefix).is_some() { page_lang } else { site_lang }
}

/// Where the site-name link points: `/` for the site's language, `/<tree>/`
/// for a page in an edition or a language tree. The tree is the folder the page
/// lives in, never a code spelled from the language: `en-us/` has no `Language` code.
pub fn home_path(url_path: Option<&str>, edition: &str, page_lang: Language, site_lang: Language) -> String {
    match url_path.and_then(crate::home::lang_tree_prefix) {
        Some(tree) if !edition.is_empty() || chrome_lang(url_path, page_lang, site_lang) != site_lang => {
            format!("/{tree}/") // allow:served-path-url-construct (nav href to language-root page, not a framework asset)
        }
        _ => "/".to_string(),
    }
}

/// A page's `external_url:` as its nav bar entry's target, or `None` when it
/// has none or it is not an [`is_valid_nav_target`], so the entry links to the page.
pub fn nav_link(external_url: Option<&str>) -> Option<&str> {
    external_url.map(str::trim).filter(|u| is_valid_nav_target(u))
}

/// Whether `url` can be a nav bar entry's target: an absolute http(s) URL,
/// `mailto:`, `tel:`, an in-page `#anchor`, or a site path. Wider than
/// [`is_valid_external_url`] on purpose: a nav item may point at an address or
/// a section, a card or sitemap entry may not. `javascript:`, `data:` and a
/// protocol-relative `//host` fail.
pub fn is_valid_nav_target(url: &str) -> bool {
    is_valid_external_url(url)
        || url.starts_with("mailto:")
        || url.starts_with("tel:")
        || url.starts_with('#')
        || (url.starts_with('/') && !url.starts_with("//"))
}

/// [`is_nav_bar_item`] for a page. Not filtered by language or edition: each
/// bar picks its own pages with [`nav_bar_pages`]. A partial with `nav: true`
/// and a nav link is a header link with no page behind it, so it is listed
/// although it fills a slot.
pub fn is_nav_bar_page<P: SitePage + ?Sized>(page: &P, has_content_folders: bool) -> bool {
    if page.partial() && page.nav() == Some(true) && page.nav_link().is_some() {
        return true;
    }
    is_nav_bar_item(
        NavFacts {
            nav: page.nav(),
            draft: page.draft() == Some(true),
            slot_only: page.slot_only(),
            unlisted: page.listed() == Some(false) || page.search_engines() == Some(false),
        },
        page.is_root_level(),
        crate::home::is_language_root(page.url_path()),
        page.clean_stem(),
        has_content_folders,
    )
}

/// Whether `edition` has any page its nav bar lists. Each edition has its own
/// bar, so another edition's pages do not count.
pub fn has_nav_items<P: SitePage>(pages: &[P], edition: &str, has_content_folders: bool) -> bool {
    pages.iter().any(|p| p.edition() == edition && is_nav_bar_page(p, has_content_folders))
}

/// The pages the nav bar of a page in `lang` and `edition` lists, in site order.
pub fn nav_bar_pages<'a, P: SitePage>(pages: &'a [P], lang: Language, edition: &str, has_content_folders: bool) -> Vec<&'a P> {
    pages.iter()
        .filter(|p| p.lang() == lang && p.edition() == edition && is_nav_bar_page(*p, has_content_folders))
        .collect()
}

/// One header entry: its sort keys and its link or group.
pub struct NavItem<'a> {
    pub weight: Option<i32>,
    /// Plain-text label, the alphabetical tiebreak.
    pub label: &'a str,
    pub html: String,
}

/// Header order, for both levels: weighted entries first by weight, then the
/// rest, ties by label, so the order does not follow the folder walk.
pub fn sort_nav_items(items: &mut [NavItem<'_>]) {
    items.sort_by(|a, b| match (a.weight, b.weight) {
        (Some(aw), Some(bw)) => aw.cmp(&bw).then_with(|| a.label.cmp(b.label)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.label.cmp(b.label),
    });
}

/// The nav bar's page entries for the page at `current_url`, a header page
/// inside a header folder grouped under that folder's entry, unsorted at the
/// top level so a caller can add entries of its own before
/// [`sort_nav_items`]. `pages` is [`nav_bar_pages`]. `nav_mode` is
/// `[site].header = "nav"`, which has no trail, so the entry of the section
/// the page is in is marked current too.
pub fn nav_items<'a, P: SitePage>(pages: &[&'a P], current_url: Option<&str>, nav_mode: bool, lang: Language) -> Vec<NavItem<'a>> {
    let parents = parent_indices(pages);
    let items = pages.iter().enumerate().map(|(i, &page)| {
        let href = match page.nav_link() {
            Some(url) => html_escape(url),
            None => format!("/{}", crate::resolve::output_url::to_pretty_url(page.url_path()).trim_start_matches('/')), // allow:served-path-url-construct (nav href to user content page, not a framework asset)
        };
        let is_parent = parents.contains(&Some(i));
        let attrs = if current_url == Some(page.url_path()) {
            r#" class="active" aria-current="page""#
        } else if (nav_mode || is_parent) && is_current_section(page, current_url) {
            r#" class="active" aria-current="true""#
        } else {
            ""
        };
        // The entry's settings live in that page's frontmatter, not the open file's.
        let source = page.source_path()
            .map(|path| format!(r#" data-source-file="{}" data-source-fm="nav""#, html_escape(path)))
            .unwrap_or_default();
        NavItem { weight: page.weight(), label: page.nav_text(), html: format!(r#"<a href="{href}"{attrs}{source}>{}</a>"#, html_escape(page.nav_text())) }
    }).collect();
    nest(items, &parents, lang)
}

/// Whether the current page is inside the folder entry `page`, at any depth,
/// but not the entry itself, which is `aria-current="page"` instead.
fn is_current_section<P: SitePage + ?Sized>(page: &P, current_url: Option<&str>) -> bool {
    let Some(current) = current_url.filter(|c| page.is_folder_index() && *c != page.url_path()) else {
        return false;
    };
    page.url_path().strip_suffix("index.html").is_some_and(|folder| current.starts_with(folder))
}

/// For each header page, the index of the header folder it is listed under:
/// the outermost one (same edition, not a language root) whose directory holds
/// it, so a third level joins the same list.
fn parent_indices<P: SitePage>(pages: &[&P]) -> Vec<Option<usize>> {
    fn folder_dir<P: SitePage + ?Sized>(p: &P) -> Option<&str> {
        if !p.is_folder_index() || crate::home::is_language_root(p.url_path()) {
            return None;
        }
        p.url_path().strip_suffix("index.html").filter(|dir| !dir.is_empty())
    }
    pages.iter()
        .map(|page| {
            (0..pages.len())
                .filter(|&j| pages[j].edition() == page.edition() && pages[j].url_path() != page.url_path())
                .filter_map(|j| folder_dir(pages[j]).map(|dir| (j, dir)))
                .filter(|(_, dir)| page.url_path().starts_with(dir))
                .min_by_key(|(_, dir)| dir.len())
                .map(|(j, _)| j)
        })
        .collect()
}

/// Top-level items, each parent's link replaced by its group.
fn nest<'a>(items: Vec<NavItem<'a>>, parents: &[Option<usize>], lang: Language) -> Vec<NavItem<'a>> {
    let mut slots: Vec<Option<NavItem<'a>>> = items.into_iter().map(Some).collect();
    let mut kids: Vec<Vec<NavItem<'a>>> = (0..slots.len()).map(|_| Vec::new()).collect();
    for (i, parent) in parents.iter().enumerate() {
        let Some(p) = *parent else { continue };
        if let Some(item) = slots.get_mut(i).and_then(Option::take) {
            kids[p].push(item);
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

/// The parent link, its disclosure button and the hidden list. Both stay
/// `hidden` until script unhides the button, so a reader without script never
/// meets a dead control; the parent link still works.
fn group_html(parent: &NavItem<'_>, children: &[NavItem<'_>], n: usize, lang: Language) -> String {
    let id = format!("nav-sub-{n}");
    let label = format!("{}: {}", html_escape(parent.label), crate::i18n::t(lang, "nav_more"));
    let list: String = children.iter().map(|c| format!("<li>{}</li>", c.html)).collect();
    format!(
        r#"<span class="moss-nav-group">{link}<button class="moss-nav-sub-toggle" type="button" hidden aria-expanded="false" aria-controls="{id}" aria-label="{label}"></button><ul class="moss-nav-sub" id="{id}" hidden>{list}</ul></span>"#,
        link = parent.html,
    )
}

/// One segment of a breadcrumb trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreadcrumbSegment {
    pub title: String,
    /// Empty for the current page.
    pub url: String,
    pub is_current: bool,
}

/// The home page's `breadcrumb:`, which decides the setting for the site.
pub fn home_breadcrumb_setting<P: SitePage>(pages: &[P]) -> Option<BreadcrumbSetting> {
    pages.iter().find(|p| p.url_path() == "index.html").and_then(|p| p.breadcrumb())
}

/// The breadcrumb trail of `page`, or `None` where it shows none.
///
/// It shows where the home page says `breadcrumb: true`, or where the page's
/// edition has no nav items and the home page says nothing (or `force` is set);
/// never where the page says `breadcrumb: false`, nor on a home page. The first
/// segment is the site name, the last the page itself, unlinked.
///
/// `force` is for a page whose own title is hidden, so a site that keeps the
/// trail off would name it nowhere; where the edition has nav items the bar
/// orients the reader and the trail stays off.
///
/// A folder whose index translates the site home is not a segment: the site
/// name links to it instead.
pub fn breadcrumb_segments<P: SitePage>(
    page: &P,
    pages: &[P],
    site_title: &str,
    has_content_folders: bool,
    force: bool,
) -> Option<Vec<BreadcrumbSegment>> {
    let has_nav = has_nav_items(pages, page.edition(), has_content_folders);
    let enabled = match home_breadcrumb_setting(pages) {
        Some(BreadcrumbSetting::On) => true,
        Some(BreadcrumbSetting::Off | BreadcrumbSetting::Label) => force && !has_nav,
        None => !has_nav,
    };
    if !enabled || page.breadcrumb() == Some(BreadcrumbSetting::Off) {
        return None;
    }
    let (home_url, tail) = trail_tail(page, pages)?;
    let mut segments = vec![BreadcrumbSegment { title: site_title.to_string(), url: home_url, is_current: false }];
    segments.extend(tail);
    Some(segments)
}

/// The link to the folder holding `page`, shown above its title where the home
/// page says `breadcrumb: label`. `None` on a home or top-level page and where
/// the page says `breadcrumb: false`.
pub fn parent_label_segment<P: SitePage>(
    page: &P,
    pages: &[P],
    home_setting: Option<BreadcrumbSetting>,
) -> Option<BreadcrumbSegment> {
    if home_setting != Some(BreadcrumbSetting::Label) || page.breadcrumb() == Some(BreadcrumbSetting::Off) {
        return None;
    }
    let (_, mut tail) = trail_tail(page, pages)?;
    // [parent folder, ..., current page]: a parent exists from two on.
    let parent = tail.len().checked_sub(2)?;
    Some(tail.swap_remove(parent))
}

/// A folder with no page of its own is named as the build names its generated
/// page: by the folder's name on disk, read from a page inside it.
fn folder_name<P: SitePage>(pages: &[P], url_folder: &str) -> Option<String> {
    let depth = url_folder.split('/').count();
    let prefix = format!("{url_folder}/");
    pages.iter().filter(|p| p.url_path().starts_with(&prefix))
        .find_map(|p| p.source_path()?.split('/').nth(depth - 1).map(crate::heading::filename_text))
}

/// The trail below the site name, each folder above the page and then the
/// page, with the address the site name links to (a translation root, else `/`).
fn trail_tail<P: SitePage>(page: &P, pages: &[P]) -> Option<(String, Vec<BreadcrumbSegment>)> {
    let parts: Vec<&str> = page.url_path().split('/').collect();
    let (filename, folder_parts) = parts.split_last()?;
    let find = |url_path: &str| pages.iter().find(|p| p.url_path() == url_path);

    let lang_prefix = folder_parts.first()
        .filter(|first| find(&format!("{first}/index.html")).is_some_and(|p| p.translates_home()))
        .copied();
    let effective = if lang_prefix.is_some() { folder_parts.get(1..).unwrap_or_default() } else { folder_parts };
    let home_url = lang_prefix.map_or_else(|| "/".to_string(), |prefix| format!("/{prefix}/")); // allow:served-path-url-construct (breadcrumb home href to translation root, not a framework asset)
    if effective.is_empty() {
        return None;
    }
    let full = |path: &str| lang_prefix.map_or_else(|| path.to_string(), |prefix| format!("{prefix}/{path}"));

    let mut segments = Vec::new();
    for i in 0..effective.len() {
        let path = effective[..=i].join("/");
        let index = format!("{}/index.html", full(&path));
        let folder = find(&index);
        let title = folder.map_or_else(|| folder_name(pages, &full(&path)).unwrap_or_else(|| titlecase_segment(effective[i])), |p| p.label().to_string());
        // A passthrough folder gets no listing page, so an ancestor inside one
        // with no page of its own is left out.
        let has_page = folder.is_some()
            || !page.passthrough_root().is_some_and(|root| crate::resolve::folder_within(root, &full(&path)));
        let is_current_folder = i + 1 == effective.len() && *filename == "index.html";
        if is_current_folder {
            segments.push(BreadcrumbSegment { title, url: String::new(), is_current: true });
        } else if has_page {
            segments.push(BreadcrumbSegment { title, url: format!("/{}/", full(&path)), is_current: false }); // allow:served-path-url-construct (breadcrumb href to user content folder, not a framework asset)
        }
    }
    if *filename != "index.html" {
        segments.push(BreadcrumbSegment { title: page.label().to_string(), url: String::new(), is_current: true });
    }
    Some((home_url, segments))
}

/// A path segment as a title: hyphens to spaces, each word capitalised.
fn titlecase_segment(segment: &str) -> String {
    segment
        .split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `/` between crumbs, in the masthead and the island. Hidden from
/// assistive technology: the links already say where each level is. As a
/// visual delimiter it is held to non-text contrast (3:1).
pub const SEPARATOR: &str = r#"<span class="breadcrumb-separator" aria-hidden="true">/</span>"#;

/// The `…` that opens a trail's folded middle, and the separator after it, for
/// the masthead and the island alike. Both ship `hidden` until the fold script
/// has had to fold. No `aria-haspopup="menu"`: a menu promises arrow-key focus,
/// which a popover of plain links does not have.
pub fn fold_more_fragment(class: &str, more_label: &str) -> String {
    format!(
        r#"<button type="button" class="{class}" hidden aria-expanded="false" aria-label="{more_label}">…</button><span class="breadcrumb-separator" aria-hidden="true" data-trail-more-separator hidden>/</span>"#
    )
}

/// What the masthead's trail is drawn around.
pub struct TrailMarkup<'a> {
    /// Where the site name links, compared with each segment's address.
    pub home_path: &'a str,
    pub logo_html: &'a str,
    /// Draw the current page's crumb, for a page whose own title is hidden.
    pub current_visible: bool,
    /// Editor preview: name the field that toggles the trail.
    pub source_fm: bool,
    pub lang: Language,
}

/// The masthead's `.nav-left` for a non-empty trail.
///
/// The current page's crumb is skipped unless `current_visible`: the page's
/// own `<h1>` already says where the reader is. A folder segment's label is in
/// its own `<span>`, which clips with an ellipsis, so the `<a>`'s hint can
/// paint outside it; its full text is in `data-hint-label`, which the theme
/// script turns into a tooltip only while the label is cut off. Every crumb
/// carries `data-trail-crumb`, the fold script's handle. A trail of three or
/// more crumbs folds its middle behind a `…`, whose panel is a sibling of
/// `.nav-left` because `.nav-left` clips its own overflow.
pub fn trail_html(segments: &[BreadcrumbSegment], m: &TrailMarkup<'_>) -> String {
    let mut parts = Vec::new();
    for segment in segments {
        if segment.is_current {
            if m.current_visible {
                parts.push(format!(
                    r#"<span class="breadcrumb-segment breadcrumb-current" aria-current="page" data-trail-crumb><span class="breadcrumb-label">{}</span></span>"#,
                    segment.title
                ));
            }
            continue;
        }
        let is_home = segment.url == m.home_path;
        let (class, hint, content) = if is_home {
            ("site-name", String::new(), format!("{}{}", m.logo_html, segment.title))
        } else {
            (
                "breadcrumb-segment",
                format!(r#" data-hint-label="{}""#, html_escape_attr(&segment.title)),
                format!(r#"<span class="breadcrumb-label">{}</span>"#, segment.title),
            )
        };
        parts.push(format!(r#"<a href="{}" class="{class}" data-trail-crumb{hint}>{content}</a>"#, segment.url));
    }
    let fm_attr = if m.source_fm { r#" data-source-fm="breadcrumb""# } else { "" };
    match parts.split_first() {
        Some((first, rest)) if parts.len() >= 3 => {
            let more_label = html_escape_attr(crate::i18n::t(m.lang, "nav_hidden_levels"));
            let more = fold_more_fragment("moss-breadcrumb-more", &more_label);
            format!(
                r#"<div class="nav-left"{fm_attr}>{first}{SEPARATOR}{more}{}</div><div class="moss-breadcrumb-menu" hidden></div>"#,
                rest.join(SEPARATOR)
            )
        }
        _ => format!(r#"<div class="nav-left"{fm_attr}>{}</div>"#, parts.join(SEPARATOR)),
    }
}

#[cfg(test)]
#[path = "nav_tests.rs"]
mod tests;
