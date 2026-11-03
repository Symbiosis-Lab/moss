//! `:::form`: a request form written as a list.
//!
//! The fence body is read as a markdown tree, not as lines: each top-level list
//! item is a field, its first line is the label (with a trailing `*` for
//! required) followed by an optional `{…}` block, a nested list is the choices,
//! and the rest of the item is the help text. The `{…}` belongs to the form, so
//! it is read here with the attribute parser rather than by markdown.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

use super::attrs::parse_attrs;

/// The kinds of field a form list item can be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    Email,
    Tel,
    Textarea,
    Select,
    Radio,
    Checkbox,
    Date,
}

/// One field of a form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormField {
    pub label: String,
    pub name: String,
    pub kind: FieldKind,
    pub required: bool,
    pub help: Option<String>,
    pub choices: Vec<String>,
    pub autocomplete: Option<String>,
}

/// Arguments and fields of [`super::Shortcode::Form`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormShortcode {
    pub name: Option<String>,
    pub next: Option<String>,
    pub button: Option<String>,
    pub fields: Vec<FormField>,
}

/// The `[a-z][a-z0-9_]{0,39}` shape of a field name.
fn is_field_name(n: &str) -> bool {
    let mut chars = n.chars();
    n.len() <= 40
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The `[a-z0-9-]{1,40}` shape of a form name.
fn is_form_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 40 && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A site path: starts with one `/`, no scheme, no spaces.
fn is_site_path(p: &str) -> bool {
    p.starts_with('/') && !p.starts_with("//") && !p.contains(char::is_whitespace) && !p.contains('\\')
}

/// A list item as the markdown tree gives it: its paragraphs as source ranges
/// and its nested items.
#[derive(Default)]
struct Item {
    paragraphs: Vec<Range<usize>>,
    open: Option<Range<usize>>,
    children: Vec<Item>,
}

impl Item {
    fn flush(&mut self) {
        if let Some(r) = self.open.take() {
            self.paragraphs.push(r);
        }
    }
    fn extend(&mut self, r: Range<usize>) {
        self.open = Some(match self.open.take() {
            Some(o) => o.start.min(r.start)..o.end.max(r.end),
            None => r,
        });
    }
}

fn read_items(body: &str) -> Vec<Item> {
    let mut top: Vec<Item> = Vec::new();
    let mut stack: Vec<Item> = Vec::new();
    // allow:no-wikilinks a form label is plain text; `[[x]]` stays as typed
    for (event, range) in Parser::new_ext(body, Options::empty()).into_offset_iter() {
        match event {
            Event::Start(Tag::Item) => stack.push(Item::default()),
            Event::End(TagEnd::Item) => {
                let Some(mut done) = stack.pop() else { continue };
                done.flush();
                match stack.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => top.push(done),
                }
            }
            Event::Start(Tag::List(_)) | Event::End(TagEnd::List(_)) | Event::End(TagEnd::Paragraph) | Event::Start(Tag::Paragraph) => {
                if let Some(item) = stack.last_mut() {
                    item.flush();
                }
            }
            // Inline text of any kind, math included, widens the paragraph's source range.
            Event::Text(_)
            | Event::Code(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::Start(_)
            | Event::End(_) => {
                if let Some(item) = stack.last_mut() {
                    item.extend(range);
                }
            }
            Event::FootnoteReference(_) | Event::Rule | Event::TaskListMarker(_) => {}
        }
    }
    top
}

/// The words of a paragraph's source, one string, lines joined by a space.
fn flow(text: &str) -> String {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Label line and the help that follows it, from the item's first paragraph.
fn first_line(item: &Item, body: &str) -> (String, Vec<String>) {
    let mut help = Vec::new();
    let Some(first) = item.paragraphs.first() else { return (String::new(), help) };
    let text = &body[first.clone()];
    let (line, rest) = text.split_once('\n').unwrap_or((text, ""));
    let rest = flow(rest);
    if !rest.is_empty() {
        help.push(rest);
    }
    for p in item.paragraphs.iter().skip(1) {
        let t = flow(&body[p.clone()]);
        if !t.is_empty() {
            help.push(t);
        }
    }
    (line.trim().to_string(), help)
}

/// Splits `Label * {a=b}` into the label text and the parsed `{…}` block.
fn split_attrs(line: &str) -> (&str, Option<&str>) {
    let trimmed = line.trim_end();
    if trimmed.ends_with('}') {
        if let Some(open) = trimmed.rfind('{') {
            if open == 0 || trimmed[..open].ends_with(char::is_whitespace) {
                return (trimmed[..open].trim_end(), Some(&trimmed[open..]));
            }
        }
    }
    (trimmed, None)
}

/// Name from a label: ASCII words in lowercase joined by `_`.
fn derive_name(label: &str) -> String {
    label
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("_")
}

fn kind_of(word: &str) -> Option<FieldKind> {
    Some(match word {
        "text" => FieldKind::Text,
        "email" => FieldKind::Email,
        "tel" => FieldKind::Tel,
        "textarea" => FieldKind::Textarea,
        "select" => FieldKind::Select,
        "radio" => FieldKind::Radio,
        "checkbox" => FieldKind::Checkbox,
        "date" => FieldKind::Date,
        _ => return None,
    })
}

/// Reads `:::form {…}` and its list. The second value is the build problems,
/// each a sentence naming what to change; a field with a problem that leaves
/// it unsendable is left out.
pub fn parse_form(args: &str, body: &str) -> (FormShortcode, Vec<String>) {
    let mut problems = Vec::new();
    let block = parse_attrs(args).unwrap_or_default();
    let mut form = FormShortcode {
        name: block.id.clone(),
        next: block.get("next").map(str::to_string),
        button: block.get("button").filter(|b| !b.is_empty()).map(str::to_string),
        fields: Vec::new(),
    };
    match form.name.as_deref() {
        None => problems.push("`:::form` needs a name to post to: write it as `:::form {#booking next=/booking/received/}`.".to_string()),
        Some(n) if !is_form_name(n) => problems.push(format!(
            "`:::form {{#{n}}}` is not a valid form name: use 1 to 40 lowercase letters, digits and hyphens."
        )),
        Some(_) => {}
    }
    let who = form.name.clone().unwrap_or_default();
    match form.next.as_deref() {
        None => problems.push(format!("`:::form` {who} needs `next=` , the page the reader lands on after sending, such as `next=/booking/received/`.")),
        Some(n) if !is_site_path(n) => problems.push(format!(
            "`:::form` {who} has `next={n}`, which is not a path in the site: write it with a leading `/`, such as `next=/booking/received/`."
        )),
        Some(_) => {}
    }

    for item in read_items(body) {
        let (line, mut help) = first_line(&item, body);
        let (text, attr_src) = split_attrs(&line);
        let attrs = match attr_src.map(parse_attrs) {
            Some(Ok(a)) => a,
            Some(Err(_)) => {
                problems.push(format!("`:::form` {who}: the `{{…}}` after \"{text}\" does not parse, so it is ignored."));
                Default::default()
            }
            None => Default::default(),
        };
        let (label, required) = match text.strip_suffix('*') {
            Some(rest) if rest.is_empty() || rest.ends_with(char::is_whitespace) => (rest.trim_end(), true),
            _ => (text, false),
        };
        if label.is_empty() {
            problems.push(format!("`:::form` {who} has a list item with no label."));
            continue;
        }
        let kind = match attrs.get("type") {
            None => FieldKind::Text,
            Some(word) => kind_of(word).unwrap_or_else(|| {
                problems.push(format!(
                    "`:::form` {who}: \"{label}\" has `type={word}`, which is not a field type. Use text, email, tel, textarea, select, radio, checkbox or date; it is read as text."
                ));
                FieldKind::Text
            }),
        };
        let name = match attrs.get("name") {
            Some(n) => n.to_string(),
            None if !label.chars().any(|c| c.is_ascii_alphabetic()) => {
                problems.push(format!(
                    "`:::form` {who}: \"{label}\" has no Latin letters to make a field name from; add `{{name=…}}`."
                ));
                continue;
            }
            None => derive_name(label),
        };
        if name == "_next" || name == "_hp" {
            problems.push(format!("`:::form` {who}: `{name}` is reserved for the form itself; pick another `name=` for \"{label}\"."));
            continue;
        }
        if !is_field_name(&name) {
            problems.push(format!(
                "`:::form` {who}: \"{label}\" would be named `{name}`, which is not a field name (a lowercase letter, then up to 39 lowercase letters, digits or `_`); set `{{name=…}}`."
            ));
            continue;
        }
        if form.fields.iter().any(|f| f.name == name) {
            problems.push(format!("`:::form` {who}: the field name `{name}` is used twice; set `{{name=…}}` on one of them."));
            continue;
        }
        let mut choices: Vec<String> = item
            .children
            .iter()
            .map(|c| first_line(c, body).0)
            .filter(|c| !c.is_empty())
            .collect();
        match kind {
            FieldKind::Select | FieldKind::Radio if choices.len() < 2 => {
                problems.push(format!("`:::form` {who}: \"{label}\" is a {} and needs at least two choices as a nested list.", if kind == FieldKind::Select { "select" } else { "radio" }));
            }
            FieldKind::Select | FieldKind::Radio | FieldKind::Checkbox => {}
            _ if !choices.is_empty() => {
                problems.push(format!("`:::form` {who}: \"{label}\" has a nested list, but only select, radio and checkbox take choices; the list is ignored."));
                choices.clear();
            }
            _ => {}
        }
        let autocomplete = match attrs.get("autocomplete") {
            Some(a) if !a.is_empty() => Some(a.to_string()),
            _ => match kind {
                FieldKind::Email => Some("email".to_string()),
                FieldKind::Tel => Some("tel".to_string()),
                _ => None,
            },
        };
        form.fields.push(FormField {
            label: label.to_string(),
            name,
            kind,
            required,
            help: (!help.is_empty()).then(|| help.drain(..).collect::<Vec<_>>().join("\n\n")),
            choices,
            autocomplete,
        });
    }
    (form, problems)
}

#[cfg(test)]
#[path = "extract_form_tests.rs"]
mod tests;
