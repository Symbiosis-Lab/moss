//! `:::form`: a request form written as a list.
//!
//! The fence body is read as a markdown tree, not as lines: each top-level list
//! item is a field, its first line is the label (with a trailing `*` for
//! required) followed by an optional `{…}` block, a nested list is the choices,
//! and the rest of the item is the help text. The `{…}` belongs to the form, so
//! it is read here with the attribute parser rather than by markdown.

use std::collections::HashSet;
use std::ops::Range;

use pulldown_cmark::{Event, Options, Tag, TagEnd};
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
    /// A box to type in that belongs to one choice ("Other: ___"), written as
    /// a one-line list under that choice; shown when the choice is picked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other: Option<OtherBox>,
}

/// The box under one choice of a group: the choice's index and the box's label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OtherBox {
    pub choice: usize,
    pub label: String,
    /// The box's own field name, `<field>_other`.
    pub name: String,
}

/// Arguments and fields of [`super::Shortcode::Form`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormShortcode {
    pub name: Option<String>,
    pub next: Option<String>,
    pub button: Option<String>,
    pub fields: Vec<FormField>,
}

/// The `[a-z][a-z0-9_]{0,39}` shape of a field name, or a label with no
/// Latin letters used as its own key (at most 40 characters, no spaces).
fn is_field_name(n: &str) -> bool {
    let mut chars = n.chars();
    let ascii = n.len() <= 40
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    ascii || is_label_key(n)
}

fn is_label_key(n: &str) -> bool {
    !n.is_empty()
        && n.chars().count() <= 40
        && !n.chars().any(|c| c.is_ascii_alphabetic() || c.is_whitespace() || c.is_control() || "\"'<>&=".contains(c))
}

/// A GFM task-list item's text without its `[ ]`, and whether it had one.
fn task_choice(text: &str) -> (&str, bool) {
    for mark in ["[ ] ", "[x] ", "[X] "] {
        if let Some(rest) = text.strip_prefix(mark) {
            return (rest.trim_start(), true);
        }
    }
    (text, false)
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
    for (event, range) in super::pulldown(body, Options::empty()).into_offset_iter() {
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
    let text = body.get(first.clone()).unwrap_or_default();
    let (line, rest) = text.split_once('\n').unwrap_or((text, ""));
    let rest = flow(rest);
    if !rest.is_empty() {
        help.push(rest);
    }
    for p in item.paragraphs.iter().skip(1) {
        let t = flow(body.get(p.clone()).unwrap_or_default());
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
            let (label, block) = trimmed.split_at(open);
            if label.is_empty() || label.ends_with(char::is_whitespace) {
                return (label.trim_end(), Some(block));
            }
        }
    }
    (trimmed, None)
}

/// Name from a label: ASCII words in lowercase joined by `_`.
fn derive_name(label: &str) -> String {
    // A long question keeps its first words, so it still names itself.
    let mut name = String::new();
    for word in label.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
        let sep = usize::from(!name.is_empty());
        if !name.is_empty() && name.len() + sep + word.len() > 40 {
            break;
        }
        if !name.is_empty() {
            name.push('_');
        }
        name.push_str(&word.to_ascii_lowercase());
    }
    name
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
    form.fields = parse_fields(&format!("`:::form` {who}"), &["_next", "_hp"], body, &mut problems);
    (form, problems)
}

/// The fields of a form's list. `at` names the block in a problem's sentence;
/// `reserved` are the names the block itself posts.
pub fn parse_fields(at: &str, reserved: &[&str], body: &str, problems: &mut Vec<String>) -> Vec<FormField> {
    let mut fields: Vec<FormField> = Vec::new();
    for item in read_items(body) {
        let (line, mut help) = first_line(&item, body);
        let (text, attr_src) = split_attrs(&line);
        let attrs = match attr_src.map(parse_attrs) {
            Some(Ok(a)) => a,
            Some(Err(_)) => {
                problems.push(format!("{at}: the `{{…}}` after \"{text}\" does not parse, so it is ignored."));
                Default::default()
            }
            None => Default::default(),
        };
        let (label, required) = match text.strip_suffix('*') {
            Some(rest) if rest.is_empty() || rest.ends_with(char::is_whitespace) => (rest.trim_end(), true),
            _ => (text, false),
        };
        if label.is_empty() {
            problems.push(format!("{at} has a list item with no label."));
            continue;
        }
        // `- [ ] choice` items make a pick-several group with no `type=`.
        let mut tasks = false;
        let mut choices = Vec::new();
        let mut other = None;
        for c in &item.children {
            let (raw, _) = first_line(c, body);
            let (choice, task) = task_choice(&raw);
            tasks |= task;
            if choice.is_empty() {
                continue;
            }
            if let Some(boxed) = c.children.first() {
                let (box_label, _) = first_line(boxed, body);
                if !box_label.is_empty() {
                    if other.is_some() {
                        problems.push(format!("{at}: \"{label}\" has a second box to type in, under \"{choice}\"; a question takes one, so it is ignored."));
                    } else {
                        other = Some(OtherBox { choice: choices.len(), label: box_label, name: String::new() });
                    }
                }
            }
            choices.push(choice.to_string());
        }
        let kind = match attrs.get("type") {
            None if tasks => FieldKind::Checkbox,
            None => FieldKind::Text,
            Some(word) => kind_of(word).unwrap_or_else(|| {
                problems.push(format!(
                    "{at}: \"{label}\" has `type={word}`, which is not a field type. Use text, email, tel, textarea, select, radio, checkbox or date; it is read as text."
                ));
                FieldKind::Text
            }),
        };
        let name = match attrs.get("name") {
            Some(n) => n.to_string(),
            None if !label.chars().any(|c| c.is_ascii_alphabetic()) => label.to_string(),
            None => derive_name(label),
        };
        if reserved.contains(&name.as_str()) {
            problems.push(format!("{at}: `{name}` is reserved for the form itself; pick another `name=` for \"{label}\"."));
            continue;
        }
        if !is_field_name(&name) {
            problems.push(format!(
                "{at}: \"{label}\" would be named `{name}`, which is not a field name (a lowercase letter, then up to 39 lowercase letters, digits or `_`, or a label of at most 40 characters with no Latin letters or spaces); set `{{name=…}}`."
            ));
            continue;
        }
        if fields.iter().any(|f| f.name == name) {
            problems.push(format!("{at}: the field name `{name}` is used twice; set `{{name=…}}` on one of them."));
            continue;
        }
        match kind {
            FieldKind::Select | FieldKind::Radio if choices.len() < 2 => {
                problems.push(format!("{at}: \"{label}\" is a {} and needs at least two choices as a nested list.", if kind == FieldKind::Select { "select" } else { "radio" }));
            }
            FieldKind::Select => other = None,
            FieldKind::Radio | FieldKind::Checkbox => {}
            _ if !choices.is_empty() => {
                problems.push(format!("{at}: \"{label}\" has a nested list, but only select, radio and checkbox take choices; the list is ignored."));
                choices.clear();
                other = None;
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
        if let Some(o) = other.as_mut() {
            o.name = format!("{name}_other");
        }
        fields.push(FormField {
            label: label.to_string(),
            name,
            kind,
            required,
            help: (!help.is_empty()).then(|| help.drain(..).collect::<Vec<_>>().join("\n\n")),
            choices,
            autocomplete,
            other,
        });
    }
    let names: HashSet<String> = fields.iter().map(|f| f.name.clone()).collect();
    for f in &mut fields {
        if f.other.as_ref().is_some_and(|o| names.contains(&o.name)) {
            problems.push(format!("{at}: the box under \"{}\" would post as `{}_other`, which another field is named; set `{{name=…}}` on one of them. The box is ignored.", f.label, f.name));
            f.other = None;
        }
    }
    fields
}

#[cfg(test)]
#[path = "extract_form_tests.rs"]
mod tests;
