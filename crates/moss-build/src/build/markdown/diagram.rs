//! Fenced `mermaid` blocks drawn to SVG at build time, so the published page
//! carries the drawing and no script.
//!
//! The renderer is pele, an MIT-licensed Mermaid renderer with no DOM
//! dependency, vendored at an exact version (`data/pele/`) and run in moss's
//! own QuickJS engine on a dedicated thread ([`engine`]).

mod engine;
mod sanitize;

use std::cell::RefCell;
use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::i18n::{self as strings, Language};

/// Width the narrow layout is drawn for; the stylesheet switches to it under
/// the renderer's own 640px breakpoint.
const NARROW_WIDTH: u32 = 360;
/// Smallest text, in CSS px, a drawing may shrink its labels to before its
/// frame scrolls instead.
const FLOOR_PX: f64 = 11.0;

/// Why a diagram was not drawn. Every case shows the source with the reason
/// above it, and warns.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DiagramRefusal {
    /// The renderer refused the source; `line` is within the diagram, 0 when unknown.
    Source { line: u32, why: Why },
    TimedOut,
    Unsafe(String),
    Engine(String),
}

/// What the renderer objected to, in a form each page language can say.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Why {
    Empty,
    Unsupported(String),
    /// A syntax error, with the source line it points at when the renderer quoted one.
    Syntax(Option<String>),
    /// The renderer's own sentence ("Invalid date:2024-13-45").
    Said(String),
}

impl DiagramRefusal {
    /// The plain reason, with its line in the diagram.
    pub(crate) fn reason(&self, lang: Language) -> String {
        match self.line() {
            Some(line) => strings::t(lang, "diagram_line").replace("{n}", &line.to_string()).replace("{what}", &self.what(lang)),
            None => self.what(lang),
        }
    }

    fn what(&self, lang: Language) -> String {
        let t = |key| strings::t(lang, key).to_string();
        match self {
            DiagramRefusal::Source { why: Why::Empty, .. } => t("diagram_empty"),
            DiagramRefusal::Source { why: Why::Unsupported(kind), .. } => t("diagram_unsupported").replace("{type}", kind),
            DiagramRefusal::Source { why: Why::Syntax(Some(near)), .. } => t("diagram_syntax_near").replace("{text}", near),
            DiagramRefusal::Source { why: Why::Syntax(None), .. } => t("diagram_syntax"),
            DiagramRefusal::Source { why: Why::Said(said), .. } => said.clone(),
            DiagramRefusal::TimedOut => t("diagram_timed_out"),
            DiagramRefusal::Unsafe(_) => t("diagram_unsafe"),
            DiagramRefusal::Engine(_) => t("diagram_failed"),
        }
    }

    /// The line within the diagram the reason points at, if any.
    pub(crate) fn line(&self) -> Option<u32> {
        match self {
            DiagramRefusal::Source { line, .. } if *line > 0 => Some(*line),
            _ => None,
        }
    }
}

/// Resolves the note an Obsidian `internal-link` node names to its page's
/// address, the way a `[[wikilink]]` to it would; `None` when no page matches.
/// Absent where a render has no site to resolve against (a fragment): links
/// are then drawn as plain text, without a warning.
pub(crate) type NoteLinks<'a> = Option<&'a dyn Fn(&str) -> Option<String>>;

/// One drawn diagram, the note names among its links that matched no page,
/// and why its narrow layout was left out when the renderer or the allowlist
/// refused it.
#[derive(Debug)]
pub(crate) struct Figure {
    pub html: String,
    pub unresolved: Vec<String>,
    pub narrow_refused: Option<DiagramRefusal>,
}

/// Draws one diagram as its `<figure>`, both layouts in it. `id` names the
/// drawing within its page and prefixes every id inside it.
pub(crate) fn render(
    source: &str,
    id: &str,
    source_line: Option<usize>,
    lang: Language,
    notes: NoteLinks<'_>,
) -> Result<Figure, DiagramRefusal> {
    let mut unresolved = Vec::new();
    let wide = draw(source, &format!("{id}w"), 0, lang, notes, &mut unresolved)?;
    // The narrow layout is an improvement for phones; without it the wide one
    // shrinks and scrolls as on any other screen.
    let mut narrow_refused = None;
    let narrow = if wide.width > f64::from(NARROW_WIDTH) {
        match draw(source, &format!("{id}n"), NARROW_WIDTH, lang, notes, &mut unresolved) {
            Ok(narrow) => Some(narrow).filter(|n| n.width < wide.width),
            // The wide layout drew, so the source is fine: this is the
            // renderer or the allowlist, which should not go unseen.
            Err(refusal @ (DiagramRefusal::Unsafe(_) | DiagramRefusal::Engine(_))) => {
                narrow_refused = Some(refusal);
                None
            }
            Err(_) => None,
        }
    } else {
        None
    };
    unresolved.sort();
    unresolved.dedup();
    let mut html = String::from("<figure class=\"moss-diagram\"");
    if narrow.is_some() {
        html.push_str(" data-narrow");
    }
    html.push_str(" tabindex=\"0\"");
    if let Some(line) = source_line {
        html.push_str(&format!(" data-source-line=\"{line}\""));
    }
    html.push('>');
    push_layout(&mut html, &wide, false);
    if let Some(narrow) = narrow {
        push_layout(&mut html, &narrow, true);
    }
    html.push_str("</figure>\n");
    Ok(Figure { html, unresolved, narrow_refused })
}

/// One layout in its wrapper. The wrapper carries the drawing's natural width
/// and the narrowest it may shrink to, the scale that keeps its smallest text
/// at [`FLOOR_PX`]; the stylesheet sizes the drawing between them, and past
/// that the figure scrolls.
/// The site search reads the wide layout only, so a label is not indexed twice.
fn push_layout(html: &mut String, drawn: &engine::Drawn, narrow: bool) {
    let smallest = font_sizes(&drawn.svg).fold(16.0_f64, f64::min);
    let floor = (drawn.width * (FLOOR_PX / smallest).min(1.0) * 10.0).round() / 10.0;
    let (class, search) = if narrow { ("moss-diagram-narrow", " data-pagefind-ignore=\"\"") } else { ("moss-diagram-wide", "") };
    html.push_str(&format!("<div class=\"{class}\"{search} style=\"--diagram-w:{}px;--diagram-min:{floor}px\">", drawn.width));
    html.push_str(&drawn.svg);
    html.push_str("</div>");
}

/// A page's diagrams as a feed carries them: the wide layout only. A feed
/// reader has no container query to choose a layout, so it would show both.
pub(crate) fn for_feeds(html: &str) -> String {
    const OPEN: &str = "<div class=\"moss-diagram-narrow\"";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        // The allowlist admits no <div> inside a drawing, so the next close is the wrapper's.
        let Some(close) = rest[at..].find("</div>") else { break };
        rest = &rest[at + close + "</div>".len()..];
    }
    out.push_str(rest);
    out
}

/// The build's warning for a refused diagram. A preview build knows the
/// fence's line; `moss build` parses without positions, so there the diagram
/// is named by its first line.
pub(crate) fn warning(page: &str, source: &str, source_line: Option<usize>, refusal: &DiagramRefusal) -> String {
    let (which, reason) = match source_line {
        Some(fence) => (format!(" line {}: diagram", fence + refusal.line().map_or(0, |l| l as usize)), refusal.what(Language::En)),
        None => (format!(" diagram “{}”", first_line(source)), refusal.reason(Language::En)),
    };
    let detail = match refusal {
        DiagramRefusal::Unsafe(why) | DiagramRefusal::Engine(why) => format!(" ({why})"),
        _ => String::new(),
    };
    format!("[{page}]{which} not drawn: {reason}{detail}")
}

fn first_line(source: &str) -> String {
    source.lines().next().unwrap_or("").trim().chars().take(40).collect()
}

/// The figure a refused diagram leaves: the reason, then the source as the
/// code block it would otherwise have been.
pub(crate) fn refused_html(refusal: &DiagramRefusal, lang: Language, fallback_html: &str) -> String {
    let message = strings::t(lang, "diagram_not_drawn").replace("{reason}", &refusal.reason(lang));
    format!(
        "<figure class=\"moss-diagram moss-diagram-error\"><figcaption class=\"moss-diagram-message\">{}</figcaption>{}</figure>\n",
        crate::build::features::html_escape(&message),
        fallback_html.trim_end()
    )
}

/// One layout, named and linked, and held to the allowlist.
fn draw(
    source: &str,
    prefix: &str,
    max_width: u32,
    lang: Language,
    notes: NoteLinks<'_>,
    unresolved: &mut Vec<String>,
) -> Result<engine::Drawn, DiagramRefusal> {
    let mut drawn = engine::render(source, prefix, max_width).map_err(|e| match e {
        engine::EngineError::Refused(r) => DiagramRefusal::Source { line: r.line, why: why(&r.code, &r.message, source) },
        engine::EngineError::TimedOut => DiagramRefusal::TimedOut,
        engine::EngineError::Engine(e) => DiagramRefusal::Engine(e),
    })?;
    drawn.svg = link_notes(&named(&drawn, lang), notes, unresolved);
    sanitize::check(&drawn.svg, prefix).map_err(DiagramRefusal::Unsafe)?;
    Ok(drawn)
}

/// A drawing without `accTitle` is named by its front matter title, or else
/// by its type, so it is never an unnamed graphic.
fn named(drawn: &engine::Drawn, lang: Language) -> String {
    let open = &drawn.svg[..drawn.svg.find('>').unwrap_or(0)];
    if open.contains(" aria-labelledby=") {
        return drawn.svg.clone();
    }
    let name = title(&drawn.svg).unwrap_or_else(|| strings::diagram_type_name(lang, &drawn.kind).to_string());
    format!("<svg aria-label=\"{}\"{}", name.replace('"', "&quot;"), &drawn.svg[4..])
}

/// Points each `internal-link` anchor at its note's page. An anchor whose
/// note matches no page is removed, its contents kept, and the name recorded.
fn link_notes(svg: &str, notes: NoteLinks<'_>, unresolved: &mut Vec<String>) -> String {
    const OPEN: &str = "<a class=\"internal-link\" href=\"";
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        let after = &rest[at + OPEN.len()..];
        let (Some(quote), Some(tag_end)) = (after.find('"'), after.find('>')) else {
            out.push_str(&rest[at..]);
            return out;
        };
        let name = unescape_xml(&after[..quote]);
        let body = &after[tag_end + 1..];
        match notes.and_then(|resolve| resolve(&name)) {
            Some(href) => {
                out.push_str(OPEN);
                out.push_str(&crate::build::features::html_escape(&href));
                out.push_str(&after[quote..=tag_end]);
                rest = body;
            }
            None => {
                // The renderer never nests anchors, so the next close is this one's.
                let close = body.find("</a>").unwrap_or(body.len());
                out.push_str(&body[..close]);
                rest = body.get(close + 4..).unwrap_or("");
                if notes.is_some() {
                    unresolved.push(name);
                }
            }
        }
    }
    out.push_str(rest);
    out
}

fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'").replace("&amp;", "&")
}

fn font_sizes(svg: &str) -> impl Iterator<Item = f64> + '_ {
    svg.split("font-size=\"").skip(1).filter_map(|rest| rest.split('"').next()?.parse().ok())
}

/// The diagram's front matter title: the text of the renderer's title
/// element, already escaped, its line `<tspan>`s joined with spaces.
fn title(svg: &str) -> Option<String> {
    let at = svg.find("class=\"pele-title\"")?;
    let element = &svg[at..];
    let element = &element[element.find('>')? + 1..element.find("</text>")?];
    let words: Vec<&str> = element.split('<').filter_map(|part| part.split_once('>').map_or(Some(part), |(_, text)| Some(text))).collect();
    let text = words.iter().map(|w| w.trim()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    Some(text).filter(|t| !t.is_empty())
}

/// What a renderer refusal objects to.
fn why(code: &str, message: &str, source: &str) -> Why {
    let body = without_front_matter_and_comments(source);
    if body.trim().is_empty() {
        return Why::Empty;
    }
    if code == "unsupported-diagram" {
        return Why::Unsupported(body.split_whitespace().next().unwrap_or("").chars().take(40).collect());
    }
    // A parse error reads "Parse error on line N:", then the source line, a
    // caret line and the parser's expected tokens; the source line is the
    // part an author can act on.
    let lines: Vec<&str> = message
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.chars().all(|c| c == '-' || c == '^'))
        .collect();
    match lines.as_slice() {
        [first, rest @ ..] if first.starts_with("Parse error on line") || first.starts_with("Lexical error on line") => {
            Why::Syntax(rest.first().map(|near| near.chars().take(60).collect()))
        }
        _ => Why::Said(lines.last().unwrap_or(&"unknown error").to_string()),
    }
}

fn without_front_matter_and_comments(source: &str) -> String {
    let mut lines = source.lines().peekable();
    let mut out = String::new();
    if lines.peek().is_some_and(|l| l.trim() == "---") {
        lines.next();
        for line in lines.by_ref() {
            if line.trim() == "---" {
                break;
            }
        }
    }
    for line in lines {
        let line = line.split("%%").next().unwrap_or("");
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The diagrams of one page render. Each drawing's ids are prefixed from its
/// source, so a page's other diagrams cannot change its bytes; a repeat of
/// the same source on the page gets a counter so no id appears twice.
pub(crate) struct PageDiagrams<'a> {
    /// The page's source path, named in warnings.
    page: &'a str,
    lang: Language,
    notes: NoteLinks<'a>,
    seen: RefCell<HashMap<String, u32>>,
}

impl<'a> PageDiagrams<'a> {
    pub(crate) fn new(page: &'a str, lang: Language, notes: NoteLinks<'a>) -> Self {
        PageDiagrams { page, lang, notes, seen: RefCell::default() }
    }

    /// Writes the figure for one fenced block, or its source with the reason
    /// it was not drawn, which also warns (and fails `--strict`), as does a
    /// note link that matches no page.
    pub(crate) fn render(&self, out: &mut String, source: &str, source_line: Option<usize>, fallback_html: &str) {
        let (page, lang, notes) = (self.page, self.lang, self.notes);
        let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
        let id = {
            let mut seen = self.seen.borrow_mut();
            let n = seen.entry(hash[..8].to_string()).or_insert(0);
            *n += 1;
            if *n == 1 { format!("d{}", &hash[..8]) } else { format!("d{}-{n}", &hash[..8]) }
        };
        match render(source, &id, source_line, lang, notes) {
            Ok(figure) => {
                if let Some(DiagramRefusal::Unsafe(why) | DiagramRefusal::Engine(why)) = &figure.narrow_refused {
                    log::warn!("[{page}] diagram “{}”: narrow layout left out ({why})", first_line(source));
                }
                for name in &figure.unresolved {
                    crate::build::cli_output::log_warn_problem!(
                        "[{page}] diagram link “{name}” matches no page; drawn without the link"
                    );
                }
                out.push_str(&figure.html);
            }
            Err(refusal) => {
                crate::build::cli_output::log_warn_problem!("{}", warning(page, source, source_line, &refusal));
                out.push_str(&refused_html(&refusal, lang, fallback_html));
            }
        }
    }
}

#[cfg(test)]
#[path = "diagram_tests.rs"]
mod tests;
