//! Reads a WordPress export file (WXR, Tools → Export): the site it names, its
//! items with their terms and meta, and authors by display name. An author's
//! email address is never read.
use std::{collections::HashMap, io::Read, path::Path};
use roxmltree::Node;

/// The largest export read: the parser holds the whole file in memory at
/// several times its size, so this keeps an import under about a gigabyte.
pub(crate) const MAX_EXPORT_BYTES: u64 = 128 * 1024 * 1024;
/// Post types WordPress keeps its own records in; none of them is a page.
pub(crate) const RECORDS: &[&str] = &[
    "attachment", "nav_menu_item", "revision", "customize_changeset", "oembed_cache", "user_request",
    "wp_block", "wp_template", "wp_template_part", "wp_global_styles", "wp_navigation",
    "wp_font_family", "wp_font_face", "custom_css",
];
/// The only post meta the import reads; a page builder's layout data and
/// the like stay in the file.
const META: &[&str] = &["_thumbnail_id", "_wp_old_slug", "_menu_item_type", "_menu_item_menu_item_parent", "_menu_item_object_id"];
/// Every WXR version's namespace starts with this.
const NAMESPACE: &str = "http://wordpress.org/export/";
const CONTENT: &str = "http://purl.org/rss/1.0/modules/content/";
const HEAD_BYTES: u64 = 64 * 1024;

pub(crate) struct Export {
    pub site: Option<url::Url>,
    pub items: Vec<Item>,
}

#[derive(Default)]
pub(crate) struct Item {
    pub id: String,
    pub title: String,
    pub link: Option<String>,
    pub post_type: String,
    pub status: String,
    pub slug: String,
    pub menu_order: i32,
    pub date: Option<String>,
    /// The author's display name; none when the export gives only a login.
    pub author: Option<String>,
    pub content: String,
    pub excerpt: String,
    pub protected: bool,
    pub attachment_url: Option<String>,
    /// An attachment's media type (`image/jpeg`).
    pub mime: String,
    pub terms: Vec<Term>,
    pub meta: Vec<(String, String)>,
    pub comments: usize,
}

/// A category, tag, menu (`nav_menu`) or other term an item carries.
pub(crate) struct Term {
    pub taxonomy: String,
    pub slug: String,
    pub name: String,
}

impl Item {
    pub fn meta(&self, key: &str) -> Option<&str> {
        self.meta.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
    }
}

/// What the first part of a file says, when it is a WordPress export.
pub(crate) struct Head {
    pub site: Option<String>,
    pub title: Option<String>,
}

/// Recognise an export by its content, reading only the start of the file.
pub(crate) fn sniff(path: &Path) -> Option<Head> {
    if !path.is_file() { return None; }
    let mut head = Vec::new();
    // allow:raw_read the export file the person named, an import source rather than a page of the site
    std::fs::File::open(path).ok()?.take(HEAD_BYTES).read_to_end(&mut head).ok()?;
    let head = String::from_utf8_lossy(&head);
    if !head.contains("<rss") || !head.contains(&format!("\"{NAMESPACE}")) { return None; }
    let between = |open: &str, close: &str| {
        let start = head.find(open)? + open.len();
        let text = head.get(start..)?.split(close).next()?;
        let text = text.trim().trim_start_matches("<![CDATA[").trim_end_matches("]]>").trim();
        (!text.is_empty()).then(|| text.to_string())
    };
    Some(Head { site: between("<wp:base_site_url>", "</").or_else(|| between("<link>", "</")), title: between("<title>", "</title>") })
}

pub(crate) fn read(path: &Path) -> Result<Export, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_EXPORT_BYTES {
        return Err(format!(
            "This WordPress export is {} MB; moss reads exports up to {} MB. Export one content type at a time (Tools → Export) and import each file.",
            size.div_ceil(1024 * 1024), MAX_EXPORT_BYTES / (1024 * 1024),
        ));
    }
    // allow:raw_read the export file the person named, an import source rather than a page of the site
    let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read the WordPress export: {e}"))?;
    parse(&text)
}

pub(crate) fn parse(text: &str) -> Result<Export, String> {
    let doc = roxmltree::Document::parse(text).map_err(|e| format!("The WordPress export is not valid XML: {e}"))?;
    let channel = doc.root_element().children().find(|n| n.has_tag_name("channel")).ok_or("The WordPress export has no channel")?;
    let mut authors = HashMap::new();
    for author in channel.children().filter(|n| is_wp(n, "author")) {
        // A display name can be the author's email address; that is never written.
        if let (Some(login), Some(name)) = (child(author, "author_login"), child(author, "author_display_name").filter(|n| !n.contains('@'))) {
            authors.insert(login, name);
        }
    }
    let site = child(channel, "base_site_url").or_else(|| plain(channel, "link"))
        .and_then(|u| url::Url::parse(&u).ok())
        .filter(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some());
    let items = channel.children().filter(|n| n.has_tag_name("item")).map(|node| item(node, &authors)).collect();
    Ok(Export { site, items })
}

fn item(node: Node, authors: &HashMap<String, String>) -> Item {
    let mut item = Item {
        id: child(node, "post_id").unwrap_or_default(),
        title: plain(node, "title").unwrap_or_default(),
        link: plain(node, "link"),
        post_type: child(node, "post_type").unwrap_or_default(),
        status: child(node, "status").unwrap_or_default(),
        slug: child(node, "post_name").unwrap_or_default(),
        menu_order: child(node, "menu_order").and_then(|o| o.parse().ok()).unwrap_or(0),
        date: date(child(node, "post_date"), child(node, "post_date_gmt")),
        author: node.children().find(|n| n.tag_name().name() == "creator").map(text).and_then(|login| authors.get(&login).cloned()),
        protected: child(node, "post_password").is_some(),
        attachment_url: child(node, "attachment_url"),
        mime: child(node, "post_mime_type").unwrap_or_default(),
        ..Default::default()
    };
    let record = RECORDS.contains(&item.post_type.as_str());
    for n in node.children().filter(Node::is_element) {
        let (name, ns) = (n.tag_name().name(), n.tag_name().namespace().unwrap_or(""));
        match name {
            "encoded" if ns == CONTENT && !record => item.content = text(n),
            "encoded" if ns.starts_with(NAMESPACE) && !record => item.excerpt = text(n),
            "category" if ns.is_empty() => item.terms.push(Term {
                taxonomy: n.attribute("domain").unwrap_or("category").into(), slug: n.attribute("nicename").unwrap_or("").into(), name: text(n).trim().into(),
            }),
            "postmeta" if is_wp(&n, name) => {
                if let Some(key) = child(n, "meta_key").filter(|k| META.contains(&k.as_str())) { item.meta.push((key, child(n, "meta_value").unwrap_or_default())); }
            }
            "comment" if is_wp(&n, name) => item.comments += usize::from(child(n, "comment_approved").as_deref() == Some("1")),
            _ => {}
        }
    }
    item
}

/// Text of a node, its CDATA sections joined.
fn text(node: Node) -> String {
    node.children().filter(Node::is_text).filter_map(|n| n.text()).collect()
}
fn is_wp(node: &Node, name: &str) -> bool {
    node.tag_name().name() == name && node.tag_name().namespace().is_some_and(|ns| ns.starts_with(NAMESPACE) && !ns.contains("excerpt"))
}
fn child(node: Node, name: &str) -> Option<String> {
    node.children().find(|n| is_wp(n, name)).map(text).map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}
fn plain(node: Node, name: &str) -> Option<String> {
    node.children().find(|n| n.tag_name().name() == name && n.tag_name().namespace().is_none())
        .map(text).map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}
/// The day the site shows (`2024-03-09`), as the crawl writes a date; the UTC
/// day when the site's own is unset.
fn date(local: Option<String>, gmt: Option<String>) -> Option<String> {
    [local, gmt].into_iter().flatten().find(|d| !d.starts_with("0000"))
        .and_then(|d| d.get(..10).map(str::to_string))
}
