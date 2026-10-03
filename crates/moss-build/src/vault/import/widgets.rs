//! Interactive widgets on an imported page: what a static site can keep of them.
//!
//! The generic extractor strips every `iframe`, `form`, `button`, `input`,
//! `object` and `embed` before scoring, so a video embed, a donation widget
//! or a contact form used to vanish without a trace. [`carry_widgets`] runs
//! first and classifies each one against [`ROWS`]:
//!
//! - `Embed` — a video host: the iframe becomes moss's remote-embed form
//!   (`![[url]]`), which the build renders as a player.
//! - `Carry` — a hosted page (donation, scheduling, forms, any other `https`
//!   iframe): a link to it where the iframe stood, the honest static form.
//! - `Chrome` — chat bubbles, comment embeds, newsletter vendors: dropped
//!   silently, as the clutter strip always did.
//!
//! A `<form>` is classified by its inputs: one email input and no textarea
//! is a newsletter signup (chrome); anything else is a contact form, which is
//! replaced by its contact route (a `mailto:` found in the page, the page's
//! JSON-LD `email`, or a hosted form action). A widget with no static form
//! is dropped and counted, never marked in the page. Widgets under a chrome
//! ancestor ([`is_chrome_element`]) are skipped uncounted.

use ego_tree::{NodeId, NodeRef};
use scraper::node::{Element, Text};
use htmd::element_handler::Handlers;
use htmd::HtmlToMarkdown;
use scraper::{ElementRef, Html, Node, Selector};
use url::Url;

use super::scrape::extractor::{is_chrome_element, looks_hidden, LAZY_SRC_ATTRS};
use super::scrape::metadata::schema_email;

/// What the pre-pass did with the widgets on one page.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WidgetCount {
    /// Replaced by a link (a hosted page, or a contact address).
    pub carried: usize,
    /// No static form existed; removed.
    pub dropped: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Marker {
    /// Host of an iframe `src` that is a video player.
    EmbedHost,
    /// Host of any other iframe `src`.
    IframeSrcHost,
    /// Host of a `<form action>`.
    FormActionHost,
    /// A whole class token.
    AttrToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Embed,
    Carry,
    Chrome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Href {
    Src,
    Action,
    Attr(&'static str),
}

pub(crate) struct Row {
    pub marker: Marker,
    pub pattern: &'static str,
    pub role: Role,
    pub href: Href,
    /// English link text when the element names none.
    pub label: &'static str,
}

const fn row(marker: Marker, pattern: &'static str, role: Role, href: Href, label: &'static str) -> Row {
    Row { marker, pattern, role, href, label }
}

use Href::{Action, Attr, Src};
use Marker::{AttrToken, EmbedHost, FormActionHost, IframeSrcHost};
use Role::{Carry, Chrome, Embed};

/// Host markers match by suffix (`player.vimeo.com` matches `vimeo.com`); the
/// first matching row wins, so a narrower host sits above its parent domain.
/// The `Embed` hosts are Readability's video allow-list.
pub(crate) const ROWS: &[Row] = &[
    row(EmbedHost, "dailymotion.com", Embed, Src, "Dailymotion video"),
    row(EmbedHost, "youtube.com", Embed, Src, "YouTube video"),
    row(EmbedHost, "youtube-nocookie.com", Embed, Src, "YouTube video"),
    row(EmbedHost, "player.vimeo.com", Embed, Src, "Vimeo video"),
    row(EmbedHost, "v.qq.com", Embed, Src, "Tencent video"),
    row(EmbedHost, "bilibili.com", Embed, Src, "Bilibili video"),
    row(EmbedHost, "live.bilibili.com", Embed, Src, "Bilibili video"),
    row(EmbedHost, "archive.org", Embed, Src, "Internet Archive video"),
    row(EmbedHost, "upload.wikimedia.org", Embed, Src, "Wikimedia video"),
    row(EmbedHost, "player.twitch.tv", Embed, Src, "Twitch video"),
    row(IframeSrcHost, "js.stripe.com", Chrome, Src, ""),
    row(IframeSrcHost, "googletagmanager.com", Chrome, Src, ""),
    row(IframeSrcHost, "doubleclick.net", Chrome, Src, ""),
    row(IframeSrcHost, "intercom.io", Chrome, Src, ""),
    row(IframeSrcHost, "crisp.chat", Chrome, Src, ""),
    row(IframeSrcHost, "tawk.to", Chrome, Src, ""),
    row(IframeSrcHost, "drift.com", Chrome, Src, ""),
    row(IframeSrcHost, "driftt.com", Chrome, Src, ""),
    row(IframeSrcHost, "disqus.com", Chrome, Src, ""),
    row(IframeSrcHost, "list-manage.com", Chrome, Src, ""),
    row(IframeSrcHost, "mailchimp.com", Chrome, Src, ""),
    row(IframeSrcHost, "substack.com", Chrome, Src, ""),
    row(IframeSrcHost, "donorbox.org", Carry, Src, "Donate"),
    row(IframeSrcHost, "givebutter.com", Carry, Src, "Donate"),
    row(IframeSrcHost, "paypal.com", Carry, Src, "Donate"),
    row(IframeSrcHost, "calendly.com", Carry, Src, "Book a time"),
    row(IframeSrcHost, "eventbrite.com", Carry, Src, "Tickets"),
    row(IframeSrcHost, "typeform.com", Carry, Src, "Form"),
    row(IframeSrcHost, "jotform.com", Carry, Src, "Form"),
    row(IframeSrcHost, "tally.so", Carry, Src, "Form"),
    row(IframeSrcHost, "airtable.com", Carry, Src, "Form"),
    row(FormActionHost, "list-manage.com", Chrome, Action, ""),
    row(FormActionHost, "mailchimp.com", Chrome, Action, ""),
    row(FormActionHost, "substack.com", Chrome, Action, ""),
    row(FormActionHost, "convertkit.com", Chrome, Action, ""),
    row(FormActionHost, "buttondown.email", Chrome, Action, ""),
    row(FormActionHost, "paypal.com", Carry, Action, "Donate"),
    row(FormActionHost, "donorbox.org", Carry, Action, "Donate"),
    row(FormActionHost, "givebutter.com", Carry, Action, "Donate"),
    row(FormActionHost, "typeform.com", Carry, Action, "Form"),
    row(FormActionHost, "jotform.com", Carry, Action, "Form"),
    row(FormActionHost, "tally.so", Carry, Action, "Form"),
    row(AttrToken, "calendly-inline-widget", Carry, Attr("data-url"), "Book a time"),
];

fn host_matches(host: &str, pattern: &str) -> bool {
    host == pattern || host.strip_suffix(pattern).is_some_and(|rest| rest.ends_with('.'))
}

fn host_row(markers: &[Marker], host: &str) -> Option<&'static Row> {
    ROWS.iter().find(|r| markers.contains(&r.marker) && host_matches(host, r.pattern))
}

fn contains_ci(html: &str, needle: &str) -> bool {
    html.as_bytes().windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Cheap pre-check: a page with none of these substrings has no widget, and
/// skips the parse entirely so its output is untouched.
fn may_hold_widget(html: &str) -> bool {
    contains_ci(html, "<iframe")
        || contains_ci(html, "<form")
        || ROWS.iter().any(|r| r.marker == AttrToken && contains_ci(html, r.pattern))
}

enum Kind {
    Iframe,
    Form,
    Token(&'static Row),
}

#[derive(Default)]
struct Scan {
    widgets: Vec<(NodeId, Kind)>,
    body_mailto: Option<String>,
    chrome_mailto: Option<String>,
}

fn attr<'a>(el: &'a Element, name: &str) -> &'a str {
    el.attr(name).map(str::trim).unwrap_or("")
}

fn is_chrome_el(el: &Element) -> bool {
    let hook = ["data-testid", "data-test-id", "data-component", "data-hook"]
        .iter()
        .find_map(|a| el.attr(a))
        .unwrap_or("");
    is_chrome_element(el.name(), attr(el, "class"), attr(el, "id"), hook, attr(el, "role"))
        || looks_hidden(attr(el, "class"), attr(el, "style"))
        // Unlike the stripper, the pre-pass also skips an element marked
        // `hidden` and zero-sized iframes (tracking frames): neither is a widget.
        || el.attr("hidden").is_some()
        || (el.name() == "iframe" && (attr(el, "width") == "0" || attr(el, "height") == "0"))
}

fn mailto_address(href: &str) -> Option<String> {
    let href = href.trim();
    let rest = href.get(7..).filter(|_| href[..7].eq_ignore_ascii_case("mailto:"))?;
    let addr = rest.split('?').next()?.trim();
    (!addr.is_empty()).then(|| addr.to_string())
}

fn kind_of(el: &Element) -> Option<Kind> {
    match el.name() {
        "iframe" => Some(Kind::Iframe),
        "form" => Some(Kind::Form),
        _ => ROWS
            .iter()
            .filter(|r| r.marker == AttrToken)
            .find(|r| el.classes().any(|c| c == r.pattern))
            .map(Kind::Token),
    }
}

fn scan(node: NodeRef<Node>, in_chrome: bool, in_widget: bool, out: &mut Scan) {
    for child in node.children() {
        let Node::Element(el) = child.value() else { continue };
        let chrome = in_chrome || is_chrome_el(el);
        if el.name() == "a" {
            if let Some(addr) = mailto_address(attr(el, "href")) {
                let slot = if chrome { &mut out.chrome_mailto } else { &mut out.body_mailto };
                slot.get_or_insert(addr);
            }
        }
        let mut nested = in_widget;
        if !chrome && !in_widget {
            if let Some(kind) = kind_of(el) {
                out.widgets.push((child.id(), kind));
                nested = true;
            }
        }
        scan(child, chrome, nested, out);
    }
}

fn http_url(base: &Url, raw: &str) -> Option<Url> {
    let url = base.join(raw.trim()).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

/// First non-empty of the element's own names for itself, then the row's
/// label, then the link host.
fn link_label(el: &Element, row: Option<&Row>, url: &Url) -> String {
    [attr(el, "title"), attr(el, "aria-label"), row.map_or("", |r| r.label)]
        .into_iter()
        .find(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| url.host_str().unwrap_or("link").to_string())
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn link_html(href: &str, label: &str) -> String {
    format!("<p><a href=\"{}\">{}</a></p>", escape(href), escape(label))
}

/// Tag the pre-pass leaves for an embed; [`html_to_markdown`] turns it into
/// `![[url]]`. (Plain text `![[url]]` would be escaped by the converter.)
const EMBED_TAG: &str = "moss-embed";

/// htmd with one extra handler: `<moss-embed data-url="…">` becomes the
/// remote-embed wikilink on its own paragraph.
pub(crate) fn html_to_markdown(html: &str) -> std::io::Result<String> {
    HtmlToMarkdown::builder()
        .add_handler(vec![EMBED_TAG], |_: &dyn Handlers, el: htmd::Element| {
            let url = el.attrs.iter().find(|a| &*a.name.local == "data-url")?;
            Some(format!("\n\n![[{}]]\n\n", url.value).into())
        })
        .build()
        .convert(html)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The URL form moss renders as a player: YouTube watch URLs and `vimeo.com/ID`;
/// every other video host keeps its own iframe URL, which moss embeds as-is.
fn embed_target(url: &Url) -> String {
    let host = url.host_str().unwrap_or("");
    let mut segs = url.path_segments().into_iter().flatten();
    let target = match (host.trim_start_matches("www."), segs.next(), segs.next()) {
        ("youtube.com" | "youtube-nocookie.com", Some("embed"), Some(id)) if valid_id(id) => {
            format!("https://www.youtube.com/watch?v={id}")
        }
        ("player.vimeo.com", Some("video"), Some(id)) if url.query().is_none() && valid_id(id) => {
            format!("https://vimeo.com/{id}")
        }
        _ => url.as_str().to_string(),
    };
    target.replace('(', "%28").replace(')', "%29")
}

enum Outcome {
    Replace(String),
    Leave,
    Carried(String),
    Dropped(String),
}

fn iframe_outcome(el: &Element, base: &Url) -> Outcome {
    let raw = std::iter::once("src")
        .chain(LAZY_SRC_ATTRS.iter().copied())
        .map(|a| attr(el, a))
        .find(|v| !v.is_empty());
    let Some(raw) = raw else { return Outcome::Leave };
    // `about:blank`, `javascript:` and `data:` sources, and relative ones on
    // a page with no URL of its own, name nothing to carry.
    let Some(url) = http_url(base, raw) else { return Outcome::Leave };
    if url.scheme() != "https" {
        return Outcome::Dropped("iframe".into());
    }
    let row = host_row(&[EmbedHost, IframeSrcHost], url.host_str().unwrap_or(""));
    match row.map(|r| r.role) {
        Some(Embed) => Outcome::Replace(format!(
            "<{EMBED_TAG} data-url=\"{}\"></{EMBED_TAG}>",
            escape(&embed_target(&url))
        )),
        Some(Chrome) => Outcome::Leave,
        Some(Carry) | None => Outcome::Carried(link_html(url.as_str(), &link_label(el, row, &url))),
    }
}

fn form_outcome(node: NodeRef<Node>, el: &Element, base: &Url, doc: &Html, scan: &Scan) -> Outcome {
    let action = http_url(base, attr(el, "action"));
    let action_row = action
        .as_ref()
        .and_then(|u| host_row(&[FormActionHost], u.host_str().unwrap_or("")));
    if action_row.is_some_and(|r| r.role == Chrome) || attr(el, "role") == "search" {
        return Outcome::Leave;
    }
    let form = ElementRef::wrap(node).expect("an element node");
    let count = |css: &str| form.select(&Selector::parse(css).expect("static selector")).count();
    let emails = count("input[type=email], input[name*=mail i]");
    if count("input[type=search]") > 0 || (emails == 1 && count("textarea") == 0) {
        return Outcome::Leave;
    }
    if scan.body_mailto.is_some() {
        return Outcome::Leave;
    }
    if let Some(addr) = scan.chrome_mailto.clone().or_else(|| schema_email(doc)) {
        return Outcome::Carried(link_html(&format!("mailto:{addr}"), &addr));
    }
    match (action_row, &action) {
        (Some(row), Some(url)) if row.role == Carry => {
            Outcome::Carried(link_html(url.as_str(), &link_label(el, Some(row), url)))
        }
        _ => Outcome::Dropped("contact form".into()),
    }
}

fn token_outcome(el: &Element, row: &Row, base: &Url) -> Outcome {
    let Href::Attr(name) = row.href else { return Outcome::Leave };
    match http_url(base, attr(el, name)) {
        Some(url) => Outcome::Carried(link_html(url.as_str(), &link_label(el, Some(row), &url))),
        None => Outcome::Dropped(row.pattern.to_string()),
    }
}

/// Classify every widget on a generic page before the extractor's strip
/// removes it. Returns the page HTML (byte-identical when no widget needed
/// rewriting) and what was carried or dropped.
pub fn carry_widgets(html: &str, base_url: &Url) -> (String, WidgetCount) {
    if !may_hold_widget(html) {
        return (html.to_string(), WidgetCount::default());
    }
    let mut doc = Html::parse_document(html);
    let mut found = Scan::default();
    scan(doc.tree.root(), false, false, &mut found);

    let mut count = WidgetCount::default();
    let mut replacements: Vec<(NodeId, String)> = Vec::new();
    for (id, kind) in &found.widgets {
        let node = doc.tree.get(*id).expect("scanned node");
        let Node::Element(el) = node.value() else { continue };
        let outcome = match kind {
            Kind::Iframe => iframe_outcome(el, base_url),
            Kind::Form => form_outcome(node, el, base_url, &doc, &found),
            Kind::Token(row) => token_outcome(el, row, base_url),
        };
        match outcome {
            Outcome::Replace(h) => replacements.push((*id, h)),
            Outcome::Carried(h) => {
                count.carried += 1;
                replacements.push((*id, h));
            }
            Outcome::Dropped(label) => {
                count.dropped += 1;
                log::warn!("import: {base_url}: widget with no static form dropped: {label}");
            }
            Outcome::Leave => {}
        }
    }
    if replacements.is_empty() {
        return (html.to_string(), count);
    }
    // A text token stands in for each replacement while the tree is
    // serialized, then is swapped for its markup: building element nodes
    // here would need html5ever's name types, a dependency scraper keeps private.
    let token = |n: usize| format!("MOSSWIDGETTOKEN{n}END");
    for (n, (id, _)) in replacements.iter().enumerate() {
        let mut node = doc.tree.get_mut(*id).expect("scanned node");
        node.insert_after(Node::Text(Text { text: token(n).into() }));
        node.detach();
    }
    let mut out = doc.html();
    for (n, (_, markup)) in replacements.iter().enumerate() {
        out = out.replace(&token(n), markup);
    }
    (out, count)
}

#[cfg(test)]
#[path = "widgets_tests.rs"]
mod tests;
