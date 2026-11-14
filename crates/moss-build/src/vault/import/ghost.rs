//! Ghost exports adapt into the same typed pages, media and destination table
//! as HTML imports. Only allowlisted content fields enter the project.
use std::{collections::HashMap, path::Path};
use serde_json::Value;
use moss_core::{frontmatter_typed::FrontMatter, frontmatter::members::Members, terms::TermClaim};
use super::items::{base_of, write_items, ImportedPages, Progress};
use super::page::{ImportedPage, PageStyle, media::MediaWriter};
use super::source::{ImportOptions, ImportResult};
use super::scrape::{crawl_state::{AssetMap, HostPacer}, fetch::FileProgress, run::{ScrapeProgress, ScrapeResult, ASSETS_PREFIX, ASSETS_SUBDIR}, writer::PendingPage};
pub(crate) mod bundle;
mod handoff;
mod metadata;

pub(crate) async fn import(mut bundle: bundle::Bundle, folder: &Path, options: ImportOptions, held: &mut ImportedPages, progress: Progress<'_>) -> Result<ImportResult, String> {
    let root = std::fs::canonicalize(folder).map_err(|e| e.to_string())?;
    if bundle.root().starts_with(&root) || root.starts_with(bundle.root()) {
        return Err("Keep the export bundle outside the destination project".into());
    }
    let mut report = Vec::new();
    let site = bundle.site_url()
        .and_then(|u| url::Url::parse(&u).ok())
        .filter(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some());
    if site.is_none() {
        report.push("The export has no site address; original addresses and remote images could not be recovered.".into());
    }
    let base = base_of(site.as_ref());
    let rows = |key: &str| bundle.data[key].as_array().cloned().unwrap_or_default();
    let posts = rows("posts");
    let tags = rows("tags");
    let users = rows("users");
    let post_tags = rows("posts_tags");
    let post_authors = rows("posts_authors");
    let users: Vec<_> = users.into_iter().filter(|user| post_authors.iter().any(|link| link["author_id"] == user["id"])).collect();
    let mut pages = Vec::new();
    let mut ids = HashMap::new();
    let mut assets = AssetMap::default();
    let mut pacer = HostPacer::default();
    let mut restricted = 0;
    let mut paid = 0;
    for post in posts {
        let slug = text(&post, "slug").unwrap_or_else(|| text(&post, "id").unwrap_or("post"));
        let source = base.join(&format!("{slug}/")).map_err(|e| e.to_string())?;
        if source.origin() != base.origin() { return Err("Post slug leaves the site's origin".into()); }
        let access = match text(&post, "visibility").unwrap_or("public") {
            "public" => None, "members" => Some(Members::Subscribers), _ => Some(Members::Paid),
        };
        if access.is_some() { restricted += 1; }
        if access == Some(Members::Paid) { paid += 1; }
        let html = text(&post, "html").unwrap_or("");
        if html.is_empty() { report.push(format!("{slug}: the export has no HTML body; saved an empty page.")); }
        let mut page = ImportedPage {
            frontmatter: FrontMatter {
                uid: text(&post, "uuid").map(str::to_string),
                title: text(&post, "title").map(str::to_string),
                date: text(&post, "published_at").or_else(|| text(&post, "created_at")).map(str::to_string),
                description: text(&post, "custom_excerpt").map(str::to_string),
                cover: text(&post, "feature_image").map(|s| absolute(s, &base)),
                origin: site.as_ref().map(|_| source.to_string()),
                draft: (text(&post, "status") != Some("published")).then_some(true),
                members: access,
                tags: Some(relations(&post, &post_tags, "tag_id", &tags, "name")),
                author: Some(relations(&post, &post_authors, "author_id", &users, "name")),
                ..Default::default()
            },
            body: convert(html, &base)?,
        };
        localize(&mut page, &mut bundle, folder, &base, site.is_some(), &mut assets, &mut pacer, &mut report).await?;
        // Ghost's feed gives each entry its post id as the guid.
        if let (Some(uid), Some(id)) = (text(&post, "uuid"), text(&post, "id")) {
            ids.insert(source.to_string(), (uid.to_string(), id.to_string()));
        }
        let current_url = site.as_ref().map(|_| source.to_string());
        progress(ScrapeProgress { pages_scraped: pages.len(), pages_failed: 0, current_url, complete: false });
        pages.push(PendingPage::new(source.as_str(), &page.render(PageStyle::Html), true)?);
    }
    for tag in tags.iter().filter(|v| text(v, "visibility") != Some("internal")) {
        let (Some(slug), Some(name)) = (text(tag, "slug"), text(tag, "name")) else { continue };
        let source = base.join(&format!("tag/{slug}/")).map_err(|e| e.to_string())?;
        let description = text(tag, "description");
        let page = ImportedPage {
            frontmatter: FrontMatter {
                title: Some(name.into()),
                tag_page: Some(TermClaim::Name(name.into())),
                description: description.map(str::to_string),
                origin: site.as_ref().map(|_| source.to_string()),
                ..Default::default()
            },
            body: description.unwrap_or("").into(),
        };
        pages.push(PendingPage::new(source.as_str(), &page.render(PageStyle::Html), true)?);
    }
    let home = ImportedPage {
        frontmatter: FrontMatter {
            title: bundle.setting("title"),
            description: bundle.setting("description"),
            origin: site.as_ref().map(|_| base.to_string()),
            ..Default::default()
        },
        body: String::new(),
    };
    pages.push(PendingPage::new(base.as_str(), &home.render(PageStyle::Html), true)?);
    let subscribers = handoff::retain(&mut bundle, folder, options.private_members_dir.as_deref())?;
    let rows = metadata::rows(&mut bundle, &users, &mut report)?;
    if restricted > 0 {
        report.push(format!("{restricted} restricted posts retain their full source; builds publish only their public previews."));
    }
    if paid > 0 { report.push(format!("{paid} paid posts remain restricted; paid access is not available yet.")); }
    report.push(match subscribers {
        Some(_) => "Reader files are waiting in private app data for Import a list after publishing; consume or discard them there.".into(),
        None => "No reader list was included.".into(),
    });
    for name in ["comments.json", "routes.yaml", "redirects.yaml"] {
        if bundle.read(name)?.is_none() { report.push(format!("The bundle did not include {name}.")); }
    }
    report.push(match bundle.names.iter().any(|n| n.contains("themes/")) {
        true => "The Ghost theme is not a moss theme and was not installed.".into(),
        false => "No Ghost theme was included.".into(),
    });
    report.push("Newsletter settings, tiers and comments are not imported into pages.".into());
    let (mut result, paths) = write_items(folder, site.as_ref(), held, &pages, rows, report, ScrapeResult::default(), progress)?;
    metadata::ledger(folder, &paths, &ids)?;
    result.subscribers = subscribers;
    Ok(result)
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> { v[key].as_str().filter(|s| !s.is_empty()) }
fn relations(post: &Value, joins: &[Value], foreign: &str, table: &[Value], field: &str) -> Vec<String> {
    let mut links: Vec<_> = joins.iter().filter(|r| r["post_id"] == post["id"]).collect();
    links.sort_by_key(|r| r["sort_order"].as_i64().unwrap_or(0));
    links.iter()
        .filter_map(|r| table.iter().find(|v| v["id"] == r[foreign]))
        .filter_map(|v| text(v, field))
        .filter(|s| !s.starts_with('#'))
        .map(str::to_string)
        .collect()
}
fn absolute(value: &str, base: &url::Url) -> String {
    let value = value.replace("__GHOST_URL__", base.as_str().trim_end_matches('/'));
    base.join(&value).map(|u| u.to_string()).unwrap_or(value)
}
fn convert(html: &str, base: &url::Url) -> Result<String, String> {
    let marker = moss_core::frontmatter::members::MARKER;
    // Convert the halves separately: HTML converters discard comments.
    html.split(marker).map(|part| {
        let (html, _) = super::widgets::carry_widgets(&part.replace("__GHOST_URL__", base.as_str().trim_end_matches('/')), base);
        super::widgets::html_to_markdown(&html).map_err(|e| e.to_string())
    }).collect::<Result<Vec<_>, _>>().map(|parts| parts.join(&format!("\n\n{marker}\n\n")))
}
async fn localize(
    page: &mut ImportedPage, bundle: &mut bundle::Bundle, folder: &Path, base: &url::Url,
    fetch: bool, assets: &mut AssetMap, pacer: &mut HostPacer, report: &mut Vec<String>,
) -> Result<(), String> {
    let mut images: Vec<_> = super::scrape::converter::extract_image_urls_in_markdown(&page.body)
        .into_iter()
        .map(|s| {
            let url = absolute(&s, base);
            (s, url)
        })
        .collect();
    if let Some(cover) = &page.frontmatter.cover {
        images.push((cover.clone(), cover.clone()));
    }
    let dir = folder.join(ASSETS_SUBDIR);
    let prefix = ASSETS_PREFIX;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut remote = Vec::new();
    for (raw, address) in images {
        if assets.is_settled(&raw) { continue; }
        let url = url::Url::parse(&address).ok();
        let local = url.as_ref()
            .filter(|u| u.origin() == base.origin())
            .map(|u| u.path().trim_start_matches('/'))
            .filter(|p| p.starts_with("content/images/"));
        let bytes = match local { Some(name) => bundle.read(name)?, None => None };
        if let Some(bytes) = bytes {
            let extension = Path::new(local.unwrap()).extension().and_then(|e| e.to_str()).unwrap_or("bin");
            let local = MediaWriter { directory: &dir, prefix, user_agent: "", referer: None }.bytes(&address, extension, &bytes)?;
            assets.insert(raw, local);
        } else if fetch {
            remote.push((raw, address));
        } else {
            report.push("An image was absent from the export and its source address is unknown.".into());
        }
    }
    let user_agent = super::scrape::service::default_user_agent();
    let downloads = MediaWriter { directory: &dir, prefix, user_agent: &user_agent, referer: Some(base.as_str()) };
    let mut files = FileProgress::new(remote.len());
    let result = downloads.download(&remote, assets, Some((pacer, &mut files))).await;
    if result.media_failed > 0 {
        report.push(format!("{} images could not be downloaded; their remote links remain.", result.media_failed));
    }
    page.localize(&assets.local_map());
    Ok(())
}
