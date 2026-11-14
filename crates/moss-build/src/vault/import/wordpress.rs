//! A WordPress site: its address first, then its export file. The address
//! brings every published page as visitors see it, page-builder bodies
//! included; the export adds drafts, private pages, custom post types, dates,
//! terms, the menu, the media library and old addresses. A page the folder
//! already has gains only the fields it lacks; a page it lacks is written from
//! the export's stored HTML.
use std::{collections::{BTreeMap, HashMap}, path::Path, sync::LazyLock};
use moss_core::frontmatter_typed::FrontMatter;
use super::items::{base_of, write_items, ImportedPages, Progress};
use super::page::{ImportedPage, PageStyle, media::MediaWriter};
use super::source::ImportResult;
use super::scrape::{
    converter::rewrite_file_links, crawl_state::{AssetMap, HostPacer}, design::{self, norm_url, NavEntry},
    fetch::{Downloaded, FileProgress, LINKED_FILE_MAX_BYTES},
    run::{scrape_into, ScrapeProgress, ScrapeResult, ASSETS_PREFIX, ASSETS_SUBDIR}, scope::{bare_host, UrlScope},
    service::{default_user_agent, ScrapeConfig}, writer::PendingPage,
};
pub(crate) mod wxr;
use wxr::{Item, RECORDS};

/// One export item that becomes a page, and where it lands.
struct Place<'a> {
    item: &'a Item,
    link: &'a str,
    source: url::Url,
    published: bool,
}

/// Import the export at `path` into `folder`, after crawling `address` (or the
/// site the export names) so published pages keep the body the site renders.
pub(crate) async fn import(path: &Path, address: Option<&str>, folder: &Path, held: &mut ImportedPages, progress: Progress<'_>) -> Result<ImportResult, String> {
    let export = wxr::read(path)?;
    let site = match address {
        Some(address) => {
            let address = url::Url::parse(address).map_err(|e| e.to_string())?;
            if let Some(named) = export.site.as_ref().filter(|named| host(named) != host(&address)) {
                return Err(format!("This export is from {}, not {}.", host(named), host(&address)));
            }
            Some(address)
        }
        None => export.site.clone(),
    };
    let base = base_of(site.as_ref());
    let mut report = Vec::new();
    let (places, unplaced) = places(&export.items, &base);
    let menu = menu(&export.items, &places);

    let quiet = |p: ScrapeProgress| if !p.complete { progress(p) };
    let crawl = match &site {
        Some(site) => {
            let mut config = ScrapeConfig::new(site.as_str(), folder);
            config.recursive = true;
            config.menu = menu.clone();
            match scrape_into(config, held, &quiet).await {
                Ok(crawl) if crawl.result.total_pages > 0 => Some(crawl),
                _ => {
                    held.remove_stubs_of_this_run(folder);
                    report.push(format!("{} could not be reached; every page was written from the export.", host(site)));
                    None
                }
            }
        }
        None => {
            report.push("The export names no site address; every page was written from it.".into());
            None
        }
    };
    let crawled = crawl.is_some();
    let (scrape, mut assets, mut pacer) = crawl.map_or_else(Default::default, |c| (c.result, c.assets, c.pacer));

    let dir = folder.join(ASSETS_SUBDIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let user_agent = default_user_agent();
    let media = MediaWriter { directory: &dir, prefix: ASSETS_PREFIX, user_agent: &user_agent, referer: Some(base.as_str()) };
    let (landed, remote) = fetch_library(&export.items, &media, &mut assets, &mut pacer).await;
    if landed + remote > 0 {
        report.push(format!("Media library files downloaded: {landed}; left at their addresses: {remote}."));
    }

    let scope = UrlScope::new(base.as_str()).map_err(|e| e.to_string())?;
    let attachments: HashMap<&str, &str> = export.items.iter()
        .filter_map(|i| Some((i.id.as_str(), i.attachment_url.as_deref()?))).collect();
    let mut pages = Vec::new();
    let mut rows = Vec::new();
    let mut types: BTreeMap<&str, usize> = BTreeMap::new();
    for place in &places {
        let item = place.item;
        *types.entry(item.post_type.as_str()).or_default() += 1;
        let fresh = held.path_of(place.link).is_none();
        let mut page = ImportedPage {
            frontmatter: FrontMatter {
                title: Some(item.title.clone()).filter(|t| !t.is_empty()),
                date: item.date.clone().filter(|_| item.post_type != "page"),
                author: item.author.clone().map(|a| vec![a]),
                description: Some(item.excerpt.trim().to_string()).filter(|e| !e.is_empty() && !e.contains('<')),
                cover: item.meta("_thumbnail_id").and_then(|id| attachments.get(id)).map(|u| u.to_string()),
                tags: Some(tags(item)).filter(|t| !t.is_empty()),
                origin: site.as_ref().map(|_| place.link.to_string()),
                draft: (!place.published).then_some(true),
                ..Default::default()
            },
            body: if fresh { convert(&item.content, &base)? } else { String::new() },
        };
        localize(&mut page, &media, &mut assets, &mut pacer, &place.source, &scope).await;
        if place.published {
            let source = &place.source;
            rows.extend(item.meta.iter().filter(|(k, v)| k == "_wp_old_slug" && !v.is_empty()).map(|(_, slug)| (former(source, slug), source.to_string())));
            rows.push((source.path().to_string(), source.to_string()));
        }
        progress(ScrapeProgress { pages_scraped: scrape.total_pages + pages.len(), pages_failed: 0, current_url: Some(place.link.to_string()), complete: false });
        pages.push(PendingPage::new(place.source.as_str(), &page.render(PageStyle::Html), true)?);
    }

    let kinds: Vec<String> = types.iter().map(|(kind, n)| format!("{kind} {n}")).collect();
    report.push(format!("Items in the export by type: {}.", kinds.join(", ")));
    let unpublished = places.iter().filter(|p| !p.published).count();
    if unpublished > 0 { report.push(format!("Drafts, private, scheduled and password-protected items, marked draft and not published: {unpublished}.")); }
    if unplaced > 0 { report.push(format!("Items with no address of their own (a plugin's records), not imported: {unplaced}.")); }
    let comments: usize = export.items.iter().map(|i| i.comments).sum();
    if comments > 0 { report.push(format!("Comments counted, not imported: {comments}.")); }
    let (result, paths) = write_items(folder, site.as_ref(), held, &pages, rows, report, scrape, progress)?;
    if !crawled && !menu.is_empty() {
        let written: HashMap<String, String> = pages.iter()
            .filter_map(|p| Some((norm_url(&p.source)?, paths.get(&p.source)?.clone())))
            .filter(|(_, path)| !paths.kept(path)).collect();
        design::place_menu(folder, &written, &menu)?;
    }
    Ok(result)
}

fn host(url: &url::Url) -> String {
    bare_host(url.host_str().unwrap_or_default())
}

/// The items that become pages, each with where it lands, and how many were
/// left out because they have no address of their own.
fn places<'a>(items: &'a [Item], base: &url::Url) -> (Vec<Place<'a>>, usize) {
    let mut places = Vec::new();
    let mut unplaced = 0;
    for item in items.iter().filter(|i| !i.post_type.is_empty() && !RECORDS.contains(&i.post_type.as_str())) {
        if matches!(item.status.as_str(), "trash" | "auto-draft" | "inherit") { continue; }
        let published = item.status == "publish" && !item.protected;
        match item.link.as_deref().and_then(|link| Some((link, address_of(item, link, base, published)?))) {
            Some((link, source)) => places.push(Place { item, link, source, published }),
            None => unplaced += 1,
        }
    }
    (places, unplaced)
}

/// Where the page lands: its own address once published, else a name from
/// its slug (a custom type in a folder of its own), since an unpublished
/// item's link is only `?p=<id>`. A published record of a plugin's own type
/// whose link is only a query is not a page of the site.
fn address_of(item: &Item, link: &str, base: &url::Url, published: bool) -> Option<url::Url> {
    // The export may name the site with or without `www.` or under the other scheme.
    let pretty = url::Url::parse(link).ok().filter(|l| l.query().is_none() && host(l) == host(base));
    let ordinary = matches!(item.post_type.as_str(), "post" | "page");
    match pretty {
        Some(link) if published => return base.join(link.path()).ok(),
        None if published && !ordinary => return None,
        _ => {}
    }
    let slug = if item.slug.is_empty() { format!("{}-{}", item.post_type, item.id) } else { item.slug.clone() };
    base.join(&if ordinary { format!("{slug}/") } else { format!("{}/{slug}/", item.post_type) }).ok()
}

/// Categories and tags by name; WordPress's default "Uncategorized" is not one a reader chose.
fn tags(item: &Item) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for term in &item.terms {
        let chosen = term.taxonomy == "post_tag" || (term.taxonomy == "category" && term.slug != "uncategorized");
        if chosen && !term.name.is_empty() && !tags.contains(&term.name) { tags.push(term.name.clone()); }
    }
    tags
}

/// The export's largest menu, top level only, in its order, as the nav
/// entries of the pages it links to.
fn menu(items: &[Item], places: &[Place]) -> Vec<NavEntry> {
    let mut menus: BTreeMap<&str, Vec<&Item>> = BTreeMap::new();
    for item in items.iter().filter(|i| i.post_type == "nav_menu_item") {
        if let Some(term) = item.terms.iter().find(|t| t.taxonomy == "nav_menu") {
            menus.entry(term.slug.as_str()).or_default().push(item);
        }
    }
    let Some(mut entries) = menus.into_values().reduce(|a, b| if b.len() > a.len() { b } else { a }) else { return Vec::new() };
    entries.sort_by_key(|i| i.menu_order);
    entries.iter()
        .filter(|i| i.meta("_menu_item_menu_item_parent").is_none_or(|p| p == "0") && i.meta("_menu_item_type") == Some("post_type"))
        .filter_map(|i| places.iter().find(|p| Some(p.item.id.as_str()) == i.meta("_menu_item_object_id")))
        .map(|p| NavEntry { label: p.item.title.clone(), url: p.source.to_string() })
        .collect()
}

/// `link` with its last segment replaced by an earlier slug.
fn former(link: &url::Url, slug: &str) -> String {
    let path = link.path().trim_end_matches('/');
    let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
    format!("{parent}/{slug}/")
}

static CAPTION: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\[/?caption[^\]]*\]").expect("caption regex"));
static PARAGRAPH: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"(?i)<p[\s>]|<!-- wp:").expect("paragraph regex"));

/// The export's stored HTML as markdown, converted as a crawled page is. The
/// classic editor stores paragraphs as blank lines and wraps a captioned
/// picture in `[caption]`.
fn convert(content: &str, base: &url::Url) -> Result<String, String> {
    let content = CAPTION.replace_all(content, "");
    let html = if PARAGRAPH.is_match(&content) {
        content.into_owned()
    } else {
        content.split("\n\n").map(str::trim).filter(|p| !p.is_empty())
            .map(|p| format!("<p>{}</p>", p.replace('\n', "<br>\n"))).collect()
    };
    let (html, _) = super::widgets::carry_widgets(&html, base);
    super::widgets::html_to_markdown(&html).map_err(|e| e.to_string())
}

/// Download the media library through the crawl's own record of what it
/// saved: pictures named by their address's hash like the crawl's, other
/// files under their own names, capped like a page's linked files.
/// Returns how many landed and how many stayed at their addresses.
async fn fetch_library(items: &[Item], media: &MediaWriter<'_>, assets: &mut AssetMap, pacer: &mut HostPacer) -> (usize, usize) {
    let library: Vec<&Item> = items.iter().filter(|i| i.attachment_url.as_deref().is_some_and(|u| !assets.is_settled(u))).collect();
    let (pictures, files): (Vec<&Item>, Vec<&Item>) = library.into_iter().partition(|i| i.mime.starts_with("image/"));
    let pictures: Vec<(String, String)> = pictures.iter().filter_map(|i| i.attachment_url.clone()).map(|u| (u.clone(), u)).collect();
    let mut progress = FileProgress::new(pictures.len() + files.len());
    let done = media.download(&pictures, assets, Some((pacer, &mut progress))).await;
    let (mut landed, mut remote) = (done.media_downloaded, done.media_failed);
    for url in files.iter().filter_map(|i| i.attachment_url.as_deref()) {
        match media.linked(url, assets, pacer, LINKED_FILE_MAX_BYTES).await {
            Ok(Downloaded::Saved(_)) => landed += 1,
            _ => remote += 1,
        }
    }
    (landed, remote)
}

/// Point the page's pictures, cover and linked files at local copies,
/// downloading the pictures the library did not hold.
async fn localize(page: &mut ImportedPage, media: &MediaWriter<'_>, assets: &mut AssetMap, pacer: &mut HostPacer, source: &url::Url, scope: &UrlScope) {
    let mut images = page.images();
    if let Some(cover) = &page.frontmatter.cover { images.push((cover.clone(), cover.clone())); }
    let mut progress = FileProgress::new(images.len());
    media.download(&images, assets, Some((pacer, &mut progress))).await;
    let local = assets.local_map();
    page.localize(&local);
    page.body = rewrite_file_links(&page.body, source.as_str(), scope, &local);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(post_type: &str, slug: &str) -> Item {
        Item { id: "7".into(), post_type: post_type.into(), slug: slug.into(), ..Default::default() }
    }

    #[test]
    fn a_published_item_keeps_its_path_on_the_site_whatever_host_spelling_the_export_used() {
        let base = url::Url::parse("https://lanterns.example/").unwrap();
        let address = |item: &Item, link: &str, published| address_of(item, link, &base, published).map(|u| u.to_string());
        assert_eq!(address(&item("post", "kiln"), "http://www.lanterns.example/2024/05/kiln/", true).unwrap(), "https://lanterns.example/2024/05/kiln/");
        assert_eq!(address(&item("post", "kiln"), "https://lanterns.example/?p=7", false).unwrap(), "https://lanterns.example/kiln/");
        assert_eq!(address(&item("project", ""), "https://lanterns.example/?p=7", false).unwrap(), "https://lanterns.example/project/project-7/");
        assert_eq!(address(&item("wpcf7_contact_form", "contact"), "https://lanterns.example/?post_type=wpcf7_contact_form&p=7", true), None);
        assert_eq!(former(&url::Url::parse("https://lanterns.example/2024/05/kiln/").unwrap(), "first-kiln"), "/2024/05/first-kiln/");
    }

    #[tokio::test]
    async fn a_media_library_on_loopback_or_link_local_is_left_at_its_addresses_unrequested() {
        let (loopback, hits) = crate::vault::import::scrape::fetch::unregistered_loopback_server();
        let link_local = "http://169.254.0.1/wp-content/uploads/price-sheet.pdf";
        let attachment = |id: &str, mime: &str, url: &str| format!(
            "<item><title>{id}</title><wp:post_id>{id}</wp:post_id><wp:status>inherit</wp:status><wp:post_type>attachment</wp:post_type>\
             <wp:post_mime_type>{mime}</wp:post_mime_type><wp:attachment_url>{url}</wp:attachment_url></item>");
        let export = format!(
            "<?xml version=\"1.0\"?><rss version=\"2.0\" xmlns:wp=\"http://wordpress.org/export/1.2/\"><channel><title>Lantern Studio</title>{}{}</channel></rss>",
            attachment("20", "image/png", &format!("{loopback}/harbour.png")),
            attachment("21", "application/pdf", link_local),
        );
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("export.xml");
        std::fs::write(&path, export).unwrap();
        let folder = tmp.path().join("site");
        std::fs::create_dir_all(&folder).unwrap();

        let result = import(&path, None, &folder, &mut ImportedPages::default(), &|_| {}).await.unwrap();

        assert!(result.report.iter().any(|l| l == "Media library files downloaded: 0; left at their addresses: 2."), "{:?}", result.report);
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
        // No listener can count the link-local request: the library's file
        // download is the address check's refusal, where a request would fail to connect.
        let media = MediaWriter { directory: &folder, prefix: ASSETS_PREFIX, user_agent: "test", referer: None };
        let refused = media.linked(link_local, &mut AssetMap::default(), &mut HostPacer::default(), LINKED_FILE_MAX_BYTES).await.err().unwrap();
        assert!(refused.is_refused(), "{refused}");
    }

    #[test]
    fn a_classic_post_with_a_code_block_keeps_its_paragraphs() {
        let base = url::Url::parse("https://lanterns.example/").unwrap();
        let md = convert("First paragraph.\n\nSecond paragraph.\n\n<pre>let x = 1;</pre>", &base).unwrap();
        assert!(md.contains("First paragraph.\n\nSecond paragraph."), "{md}");
    }
}
