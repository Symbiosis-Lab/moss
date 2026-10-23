//! Reader for a Douban database backup (a `tofu-db-backup.zip`).
//!
//! The archive holds `database.json` (`{"local": {"files": [...]}}`) and one
//! `<name>.json` per backed-up user, each `{"<table>": [row, ...]}`. A row
//! wraps the Douban mobile-API object for one item. Every item becomes its
//! own page; each kind's folder gets a collection `index.md` grouped by year.
//!
//! | table      | page                                                   |
//! |------------|--------------------------------------------------------|
//! | `note`     | `note/<id>.md`                                         |
//! | `review`   | `review/<id>.md`, `review_of` = the subject's URL      |
//! | `status`   | `status/<id>.md`                                       |
//! | `interest` | `<subject type>/<subject id>.md`, body = the comment   |
//!
//! A page with images becomes a bundle (`<kind>/<id>/index.md` beside its
//! images). Tables not listed above are counted and ignored.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::Path;

use serde_json::Value;

use super::scrape::converter::extract_image_urls_in_markdown;
use super::scrape::fetch::{download_asset, strip_query};
use super::scrape::run::{write_note, ScrapeResult};
use super::scrape::service::{default_user_agent, escape_yaml_string};
use super::scrape::writer::{rename_for_collision, sanitize_filename};

/// Query parameters Douban appends to links to track where a click came from.
const TRACKING_PARAMS: &[&str] = &["_spm_id", "_i"];

/// Douban's own words for a mark's progress (done, doing, wanted), by the
/// subject types that share them. The last row is the fallback.
const MARK_WORDS: &[(&[&str], [&str; 3])] = &[
    (&["book"], ["读过", "在读", "想读"]),
    (&["music"], ["听过", "在听", "想听"]),
    (&["game"], ["玩过", "在玩", "想玩"]),
    (&["movie", "tv", "drama"], ["看过", "在看", "想看"]),
];

/// Whether `path` is a zip whose `database.json` lists per-user backup files.
pub(crate) fn is_tofu_backup(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        && open_archive(path)
            .and_then(|mut z| read_json(&mut z, "database.json"))
            .is_ok_and(|db| db["local"]["files"].is_array())
}

/// What one import did, per kind of page.
#[derive(Debug, Default)]
pub(crate) struct TofuReport {
    pub pages: BTreeMap<String, usize>,
    /// Pages that only had an abstract; a fuller export can replace them.
    pub partial: usize,
    /// Rows without an id or subject, which cannot be given a page.
    pub skipped: usize,
    pub unknown_tables: BTreeMap<String, usize>,
    pub media_downloaded: usize,
    pub media_failed: usize,
    /// Bodies whose HTML could not be converted; the abstract stands in.
    pub html_failed: usize,
}

impl TofuReport {
    pub(crate) fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .pages
            .iter()
            .map(|(k, n)| format!("{k}/: {n} page(s)"))
            .collect();
        if self.partial > 0 {
            out.push(format!(
                "{} page(s) have only an abstract (partial)",
                self.partial
            ));
        }
        if self.skipped > 0 {
            out.push(format!(
                "{} row(s) skipped (no id, subject or body, or a table that is not a list)",
                self.skipped
            ));
        }
        for (t, n) in &self.unknown_tables {
            out.push(format!("ignored table `{t}`: {n} row(s)"));
        }
        if self.html_failed > 0 {
            out.push(format!(
                "{} page(s) kept their abstract because the HTML body did not convert",
                self.html_failed
            ));
        }
        if self.media_downloaded + self.media_failed > 0 {
            out.push(format!(
                "media: {} downloaded, {} failed",
                self.media_downloaded, self.media_failed
            ));
        }
        out
    }
}

/// A frontmatter value.
enum Field {
    Str(String),
    Int(i64),
    List(Vec<String>),
}

/// One page, before its images are fetched.
struct Page {
    folder: String,
    slug: String,
    fields: Vec<(&'static str, Field)>,
    body: String,
    /// The item's own address, which images are fetched on behalf of.
    page_url: Option<String>,
    partial: bool,
    html_failed: bool,
}

impl Page {
    /// Each image reference as written, and the absolute URL to fetch it from.
    fn images(&self) -> Vec<(String, String)> {
        extract_image_urls_in_markdown(&self.body)
            .into_iter()
            .filter_map(|raw| match raw.strip_prefix("//") {
                Some(rest) => Some((raw.clone(), format!("https://{rest}"))),
                None if raw.starts_with("http") => Some((raw.clone(), raw)),
                None => None,
            })
            .collect()
    }
}

pub(crate) async fn import_tofu_backup(
    path: &Path,
    output_dir: &Path,
) -> Result<(ScrapeResult, TofuReport), String> {
    let mut archive = open_archive(path)?;
    let db = read_json(&mut archive, "database.json")?;
    let names: Vec<String> = db["local"]["files"]
        .as_array()
        .ok_or("database.json lists no backup files")?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();

    let mut report = TofuReport::default();
    let mut pages = Vec::new();
    for name in &names {
        let tables = read_json(&mut archive, &format!("{name}.json"))?;
        for (table, rows) in tables.as_object().into_iter().flatten() {
            let Some(rows) = rows.as_array() else {
                report.skipped += 1;
                continue;
            };
            for row in rows {
                match convert_row(table, row) {
                    Row::Page(p) => {
                        report.html_failed += usize::from(p.html_failed);
                        pages.push(p);
                    }
                    Row::Skipped => report.skipped += 1,
                    Row::Unknown => *report.unknown_tables.entry(table.clone()).or_default() += 1,
                }
            }
        }
    }

    let user_agent = default_user_agent();
    for page in &mut pages {
        let images = page.images();
        let bundle = !images.is_empty();
        let mut dir = if bundle {
            format!("{}/{}", page.folder, page.slug)
        } else {
            page.folder.clone()
        };
        if bundle {
            // A second page with the same id must not write into the first's folder.
            let base = dir.clone();
            let mut n = 2;
            while output_dir.join(&dir).exists() {
                dir = format!("{base} {n}");
                n += 1;
            }
        }
        let mut local: HashMap<String, String> = HashMap::new();
        if bundle {
            std::fs::create_dir_all(output_dir.join(&dir))
                .map_err(|e| format!("Failed to create directory: {e}"))?;
        }
        for (raw, url) in images {
            match download_asset(
                &url,
                &output_dir.join(&dir),
                &user_agent,
                page.page_url.as_deref(),
            )
            .await
            {
                Ok(file) => {
                    report.media_downloaded += 1;
                    local.insert(raw, format!("./{file}"));
                }
                Err(e) => {
                    report.media_failed += 1;
                    log::warn!("import: image download failed, keeping remote URL: {url}: {e}");
                }
            }
        }
        page.body = super::scrape::converter::rewrite_image_links(&page.body, &local);
        let rel = if bundle {
            format!("{dir}/index.md")
        } else {
            format!("{}/{}.md", page.folder, page.slug)
        };
        let rel = rename_for_collision(output_dir, &rel);
        write_note(output_dir, &rel, &render(&page.fields, &page.body))?;
        *report.pages.entry(page.folder.clone()).or_default() += 1;
        report.partial += usize::from(page.partial);
    }

    for folder in report.pages.keys() {
        let rel = format!("{folder}/index.md");
        if !output_dir.join(&rel).exists() {
            let fields = [
                ("title", Field::Str(collection_title(folder))),
                ("children_group", Field::Str("year".into())),
            ];
            write_note(output_dir, &rel, &render(&fields, ""))?;
        }
    }

    let total = report.pages.values().sum();
    Ok((
        ScrapeResult {
            success: true,
            total_pages: total,
            failed_pages: 0,
            skipped_pages: 0,
            duplicate_pages: 0,
            unreachable_variants: 0,
            unreachable_files: 0,
            linked_files_downloaded: 0,
            oversized_files: 0,
            widgets_carried: 0,
            widget_gaps: Vec::new(),
            capped: false,
            remaining_urls: 0,
            sitemap_urls: 0,
            sitemap_truncated: false,
            rate_limited_hosts: Vec::new(),
            chrome: Default::default(),
            error: None,
        },
        report,
    ))
}

type Archive = zip::ZipArchive<std::io::Cursor<Vec<u8>>>;

/// The user picked this file, so it may be cloud-evicted: read it the way the
/// other local-file import does.
fn open_archive(path: &Path) -> Result<Archive, String> {
    let bytes = crate::build::cloud_readiness::read_with_materialize_wait(
        path,
        crate::build::cloud_readiness::INTERACTIVE_DEADLINE,
    )
    .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("{} is not a zip: {e}", path.display()))
}

fn read_json(archive: &mut Archive, name: &str) -> Result<Value, String> {
    let mut entry = archive
        .by_name(name)
        .map_err(|e| format!("{name} missing from the backup: {e}"))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read {name}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{name} is not valid JSON: {e}"))
}

enum Row {
    Page(Page),
    Skipped,
    Unknown,
}

fn convert_row(table: &str, row: &Value) -> Row {
    let page = match table {
        "note" => note_page(row),
        "review" => review_page(row),
        "status" => status_page(row),
        "interest" => interest_page(row),
        _ => return Row::Unknown,
    };
    page.map_or(Row::Skipped, Row::Page)
}

/// A string or number as text; empty and null are `None`.
fn text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A source id as one safe path segment.
fn slug(v: &Value) -> Option<String> {
    text(v)
        .map(|s| sanitize_filename(&s))
        .filter(|s| !s.is_empty())
}

/// A link with Douban's click-tracking parameters removed.
fn clean_url(u: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(u) else {
        return u.to_string();
    };
    let kept: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| !TRACKING_PARAMS.contains(&k.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        parsed.set_query(None);
    } else {
        parsed.query_pairs_mut().clear().extend_pairs(kept);
    }
    parsed.to_string()
}

fn link(v: &Value) -> Option<String> {
    text(v).map(|u| clean_url(&u))
}

/// Douban's `YYYY-MM-DD HH:MM:SS` (Asia/Shanghai) as RFC 3339 at +08:00, so
/// items on one day keep their order.
fn date_of(v: &Value) -> Option<String> {
    let s = text(v)?;
    if s.len() == 19 && s.as_bytes()[10] == b' ' {
        Some(format!("{}T{}+08:00", &s[..10], &s[11..]))
    } else {
        s.get(..10).map(str::to_string)
    }
}

fn stars(v: &Value) -> Option<i64> {
    v["value"]
        .as_f64()
        .map(|n| n.round() as i64)
        .filter(|n| (1..=5).contains(n))
}

fn push<T>(
    fields: &mut Vec<(&'static str, Field)>,
    key: &'static str,
    v: Option<T>,
    make: impl FnOnce(T) -> Field,
) {
    if let Some(v) = v {
        fields.push((key, make(v)));
    }
}

/// The fields every item with its own address shares.
fn common(
    obj: &Value,
    title: Option<String>,
    origin: Option<String>,
) -> Vec<(&'static str, Field)> {
    let mut fields = Vec::new();
    push(&mut fields, "title", title, Field::Str);
    push(&mut fields, "date", date_of(&obj["create_time"]), Field::Str);
    push(&mut fields, "origin", origin, Field::Str);
    fields
}

/// A note or review body: the HTML through the importer's htmd path, else the
/// abstract as written. Returns `(body, abstract_only, html_failed)`, or
/// `None` when the row carries no body at all.
fn article_body(item: &Value) -> Option<(String, bool, bool)> {
    let mut html_failed = false;
    if let Some(html) = text(&item["fulltext"]) {
        match super::widgets::html_to_markdown(&html) {
            Ok(md) if !md.trim().is_empty() => return Some((md.trim().to_string(), false, false)),
            Ok(_) => {}
            Err(e) => {
                log::warn!("import: HTML body did not convert, using the abstract: {e}");
                html_failed = true;
            }
        }
    }
    text(&item["abstract"]).map(|a| (a, true, html_failed))
}

fn note_page(row: &Value) -> Option<Page> {
    let id = slug(&row["id"])?;
    let note = &row["note"];
    let (body, partial, html_failed) = article_body(note)?;
    let origin = link(&note["url"]);
    Some(Page {
        folder: "note".into(),
        slug: id,
        fields: common(note, text(&note["title"]), origin.clone()),
        body,
        page_url: origin,
        partial,
        html_failed,
    })
}

fn review_page(row: &Value) -> Option<Page> {
    let id = slug(&row["id"])?;
    let review = &row["review"];
    let (body, partial, html_failed) = article_body(review)?;
    let origin = link(&review["url"]);
    let mut fields = common(review, text(&review["title"]), origin.clone());
    push(&mut fields, "review_of", link(&review["subject"]["url"]), Field::Str);
    push(&mut fields, "rating", stars(&review["rating"]), Field::Int);
    Some(Page {
        folder: "review".into(),
        slug: id,
        fields,
        body,
        page_url: origin,
        partial,
        html_failed,
    })
}

/// Douban's word for a mark's progress (`done`, `doing`, `mark`) on a subject of `kind`.
fn mark_tag(kind: &str, status: &str) -> Option<String> {
    let at = match status {
        "done" => 0,
        "doing" => 1,
        "mark" => 2,
        _ => return None,
    };
    let words = MARK_WORDS
        .iter()
        .find(|(kinds, _)| kinds.contains(&kind))
        .map_or(&MARK_WORDS[3].1, |(_, w)| w);
    Some(words[at].to_string())
}

fn interest_page(row: &Value) -> Option<Page> {
    let interest = &row["interest"];
    let subject = &interest["subject"];
    let kind = text(&row["type"]).or_else(|| text(&subject["type"]))?;
    let slug = slug(&subject["id"]).or_else(|| slug(&row["subject"]))?;
    // A mark has no address of its own, so there is no `origin` and no Referer.
    let mut fields = common(interest, text(&subject["title"]), None);
    push(&mut fields, "review_of", link(&subject["url"]), Field::Str);
    push(&mut fields, "rating", stars(&interest["rating"]), Field::Int);
    let progress = text(&row["status"]).or_else(|| text(&interest["status"]));
    let tags: Vec<String> = progress
        .and_then(|st| mark_tag(&kind, &st))
        .into_iter()
        .chain(interest["tags"].as_array().into_iter().flatten().filter_map(text))
        .collect();
    push(&mut fields, "tags", (!tags.is_empty()).then_some(tags), Field::List);
    let folder = Some(sanitize_filename(&kind))
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| "other".into());
    Some(Page {
        folder,
        slug,
        fields,
        body: text(&interest["comment"]).unwrap_or_default(),
        page_url: None,
        partial: false,
        html_failed: false,
    })
}

fn status_page(row: &Value) -> Option<Page> {
    let status = &row["status"];
    let id = slug(&status["id"]).or_else(|| slug(&row["id"]))?;
    let own_text = linkify(status["text"].as_str().unwrap_or(""), &status["entities"]);

    let mut parts = Vec::new();
    if !own_text.trim().is_empty() {
        parts.push(own_text.trim().to_string());
    }
    if let Some(reshared) = status.get("reshared_status").filter(|r| r.is_object()) {
        parts.push(reshared_block(reshared));
    }
    // The activity is the status's verb ("read", "want to read"); with no text it is all there is.
    let verb = text(&status["activity"]);
    let card = status
        .get("card")
        .filter(|c| c.is_object())
        .and_then(|c| link(&c["url"]).map(|url| (c, url)));
    match card {
        Some((card, url)) => {
            let label = text(&card["title"])
                .or_else(|| text(&card["subtitle"]))
                .unwrap_or_else(|| url.clone());
            let anchor = format!("[{}]({url})", escape_link_text(&label));
            parts.push(match verb {
                Some(v) => format!("{v} {anchor}"),
                None => anchor,
            });
        }
        None => parts.extend(verb),
    }
    for image in status["images"].as_array().into_iter().flatten() {
        if let Some(url) = text(&image["large"]["url"]).or_else(|| text(&image["normal"]["url"])) {
            parts.push(format!("![]({url})"));
        }
    }
    if parts.is_empty() {
        return None;
    }

    // `url` is the permanent address; `sharing_url` is an app dispatch link.
    let origin = text(&status["url"]).map(|u| strip_query(&u));
    Some(Page {
        folder: "status".into(),
        slug: id,
        fields: common(status, None, origin.clone()),
        body: parts.join("\n\n"),
        page_url: origin,
        partial: false,
        html_failed: false,
    })
}

/// A reshared status as a blockquote: author, text, then a link to it.
fn reshared_block(r: &Value) -> String {
    let author = &r["author"];
    let mut lines = Vec::new();
    let body = linkify(r["text"].as_str().unwrap_or(""), &r["entities"]);
    match (text(&author["name"]), link(&author["url"])) {
        (Some(name), Some(url)) => lines.push(format!(
            "[{}]({url}): {}",
            escape_link_text(&name),
            body.trim()
        )),
        (Some(name), None) => lines.push(format!("**{name}**: {}", body.trim())),
        _ => lines.push(body.trim().to_string()),
    }
    if let Some(url) = link(&r["sharing_url"]).or_else(|| link(&r["url"])) {
        lines.push(format!("[{url}]({url})"));
    }
    lines
        .iter()
        .flat_map(|l| l.lines())
        .map(|l| format!("> {l}"))
        .collect::<Vec<_>>()
        .join("\n>\n")
}

fn escape_link_text(s: &str) -> String {
    s.replace('[', "\\[").replace(']', "\\]")
}

/// Status text with its `entities` (links, mentions, topics) as markdown links.
/// An entity is located by its `start`/`end` character offsets when they cut
/// out its title, otherwise by the first free occurrence of the title.
fn linkify(text_in: &str, entities: &Value) -> String {
    let chars: Vec<char> = text_in.chars().collect();
    let mut spans: Vec<(usize, usize, String, String)> = Vec::new();
    for e in entities.as_array().into_iter().flatten() {
        let Some(url) = link(&e["url"]).filter(|u| u.starts_with("http")) else {
            continue;
        };
        let at = |k: &str| e[k].as_u64().map(|n| n as usize);
        let by_offset = match (at("start"), at("end")) {
            (Some(s), Some(en)) if s < en && en <= chars.len() => Some((s, en)),
            _ => None,
        };
        let title = text(&e["title"]);
        let range = by_offset
            .filter(|&(s, en)| {
                title
                    .as_ref()
                    .is_none_or(|t| chars[s..en].iter().collect::<String>() == *t)
            })
            .or_else(|| {
                let t: Vec<char> = title.as_ref()?.chars().collect();
                (0..chars.len().saturating_sub(t.len()) + 1)
                    .find(|&s| {
                        chars[s..s + t.len()] == t[..]
                            && spans.iter().all(|&(a, b, ..)| s + t.len() <= a || s >= b)
                    })
                    .map(|s| (s, s + t.len()))
            });
        if let Some((s, en)) = range {
            if spans.iter().all(|&(a, b, ..)| en <= a || s >= b) {
                spans.push((s, en, chars[s..en].iter().collect(), url));
            }
        }
    }
    spans.sort_by_key(|s| s.0);
    let mut out = String::new();
    let mut at = 0;
    for (s, en, label, url) in spans {
        out.extend(&chars[at..s]);
        out.push_str(&format!("[{}]({url})", escape_link_text(&label)));
        at = en;
    }
    out.extend(&chars[at..]);
    out
}

fn collection_title(folder: &str) -> String {
    match folder {
        "note" => "Notes".into(),
        "review" => "Reviews".into(),
        "status" => "Statuses".into(),
        "book" => "Books".into(),
        "movie" => "Movies".into(),
        "music" => "Music".into(),
        "drama" => "Dramas".into(),
        "game" => "Games".into(),
        "tv" => "TV".into(),
        other => {
            let mut cs = other.chars();
            cs.next()
                .map_or_else(String::new, |c| c.to_uppercase().chain(cs).collect())
        }
    }
}

/// Written by hand, not through `moss_core::frontmatter::serialize`: that walks a
/// `HashMap`, so its key order changes between runs.
fn render(fields: &[(&str, Field)], body: &str) -> String {
    let mut out = String::from("---\n");
    for (key, value) in fields {
        match value {
            Field::Str(s) => {
                out.push_str(&format!("{key}: \"{}\"\n", escape_yaml_string(s.trim())))
            }
            Field::Int(n) => out.push_str(&format!("{key}: {n}\n")),
            Field::List(items) => {
                let items: Vec<String> = items
                    .iter()
                    .map(|i| format!("\"{}\"", escape_yaml_string(i)))
                    .collect();
                out.push_str(&format!("{key}: [{}]\n", items.join(", ")));
            }
        }
    }
    out.push_str("---\n\n");
    out.push_str(body.trim());
    out.push('\n');
    out
}

#[cfg(test)]
#[path = "douban_tests.rs"]
mod tests;
