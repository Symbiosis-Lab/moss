//! The shared imported page: typed metadata, ordered YAML and source body.
//! Readers choose the established layout; values always come from FrontMatter.

use std::collections::HashMap;
use std::path::Path;
use moss_core::frontmatter_typed::FrontMatter;
use serde_yaml::Value;

pub(crate) mod media;

/// Existing readers' file conventions, including their body whitespace.
#[derive(Clone, Copy)]
pub(crate) enum PageStyle {
    Html,
    Backup,
}

impl PageStyle {
    fn order(self) -> &'static [&'static str] {
        match self {
            Self::Html => &["title", "date", "author", "publisher", "lang", "description",
                "cover", "tags", "start", "end", "location", "status", "tickets", "online", "origin"],
            Self::Backup => &["title", "date", "origin", "review_of", "rating", "tags", "children_group"],
        }
    }
}

#[derive(Default)]
pub(crate) struct ImportedPage {
    pub frontmatter: FrontMatter,
    pub body: String,
}

impl ImportedPage {
    pub fn render(&self, style: PageStyle) -> String {
        let mut out = render_frontmatter(&self.frontmatter, style);
        match style {
            PageStyle::Html => out.push_str(&self.body),
            PageStyle::Backup => {
                out.push_str(self.body.trim());
                out.push('\n');
            }
        }
        out
    }

    pub fn images(&self) -> Vec<(String, String)> {
        super::scrape::converter::extract_image_urls_in_markdown(&self.body)
            .into_iter()
            .filter_map(|raw| match raw.strip_prefix("//") {
                Some(rest) => Some((raw.clone(), format!("https://{rest}"))),
                None if raw.starts_with("http") => Some((raw.clone(), raw)),
                None => None,
            })
            .collect()
    }

    pub fn localize(&mut self, local: &HashMap<String, String>) {
        self.body = super::scrape::converter::rewrite_image_links(&self.body, local);
        self.body = super::scrape::converter::rewrite_embed_links(&self.body, local);
        if let Some(path) = self.frontmatter.cover.as_ref().and_then(|u| local.get(u)) {
            self.frontmatter.cover = Some(path.clone());
        }
    }

    pub fn write(&self, root: &Path, relative: &str, style: PageStyle) -> Result<(), String> {
        write_note(root, relative, &self.render(style))
    }
}

/// Keep the original field order for each reader; append new typed fields in
/// declaration order. Never iterate a HashMap to serialize an imported page.
pub(crate) fn render_frontmatter(fields: &FrontMatter, style: PageStyle) -> String {
    render_frontmatter_with(fields, &[], style)
}

/// [`render_frontmatter`] followed by `record` fields an importer keeps that
/// are no page field, escaped the same way.
pub(crate) fn render_frontmatter_with(fields: &FrontMatter, record: &[(&str, &str)], style: PageStyle) -> String {
    let mut values = field_values(fields);
    values.extend(record.iter().map(|(key, value)| (key.to_string(), Value::String(value.to_string()))));
    let mut out = String::from("---\n");
    for key in style.order() {
        if let Some(at) = values.iter().position(|(k, _)| k == key) {
            let (key, value) = values.remove(at);
            out.push_str(&field_line(&key, &value));
        }
    }
    for (key, value) in values {
        out.push_str(&field_line(&key, &value));
    }
    out.push_str("---\n\n");
    out
}

fn field_values(fields: &FrontMatter) -> Vec<(String, Value)> {
    // FrontMatter has only string-keyed maps and serializable values.
    let Value::Mapping(map) = serde_yaml::to_value(fields).expect("typed frontmatter serializes") else {
        unreachable!("FrontMatter serializes as a mapping")
    };
    map.into_iter().filter_map(|(k, v)| {
        (!v.is_null()).then(|| (k.as_str().expect("field name").to_string(), v))
    }).collect()
}

pub(crate) fn escape_yaml_string(s: &str) -> String {
    moss_core::frontmatter::strip_control_chars_str(s)
        .replace('\\', "\\\\").replace('"', "\\\"")
        .replace('\n', "\\n").replace('\r', "\\r")
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(s) => format!("\"{}\"", escape_yaml_string(s)),
        Value::Sequence(items) => format!("[{}]", items.iter().map(scalar).collect::<Vec<_>>().join(", ")),
        // Only compound fields need serde's block format.
        _ => serde_yaml::to_string(value).expect("field value serializes").trim_end().to_string(),
    }
}

fn field_line(key: &str, value: &Value) -> String {
    let value = match value {
        Value::String(s) => Value::String(s.trim().to_string()),
        other => other.clone(),
    };
    if matches!(value, Value::Mapping(_)) {
        let indented = scalar(&value).lines().map(|l| format!("  {l}\n")).collect::<String>();
        format!("{key}:\n{indented}")
    } else {
        format!("{key}: {}\n", scalar(&value))
    }
}

/// Add typed fields without changing any existing field or body byte.
pub(crate) fn with_fields_absent(content: &str, fields: &FrontMatter) -> Option<String> {
    let span = moss_core::frontmatter::frontmatter_span(content)?;
    if span.kind != moss_core::frontmatter::FrontmatterKind::Yaml { return None; }
    let present = moss_core::frontmatter::frontmatter_map(content);
    let mut values = field_values(fields);
    // Chrome adds nav before weight and description before logo.
    let order = ["nav", "weight", "description", "logo"];
    values.sort_by_key(|(k, _)| order.iter().position(|name| name == k).unwrap_or(order.len()));
    let lines = values.into_iter()
        .filter(|(key, _)| !present.contains_key(key))
        .map(|(key, value)| field_line(&key, &value)).collect::<String>();
    let (before, after) = content.split_at(span.fields.end);
    (!lines.is_empty()).then(|| format!("{before}{lines}{after}"))
}

/// Replace the entire title value, including a multiline YAML scalar. Other
/// fields and the body retain their original bytes and ordering.
pub(crate) fn with_title(content: &str, title: &str) -> Option<String> {
    let span = moss_core::frontmatter::frontmatter_span(content)?;
    if span.kind != moss_core::frontmatter::FrontmatterKind::Yaml { return None; }
    let fields = content.get(span.fields.clone())?;
    let mut offset = span.fields.start;
    let mut start = None;
    let mut end = span.fields.end;
    for line in fields.split_inclusive('\n') {
        let top_level = !line.starts_with(char::is_whitespace) && !line.starts_with('#');
        if start.is_some() && top_level && line.contains(':') {
            end = offset;
            break;
        }
        if line.starts_with("title:") { start = Some(offset); }
        offset += line.len();
    }
    let fields = FrontMatter { title: Some(title.to_string()), ..Default::default() };
    let line = field_values(&fields).into_iter().map(|(k, v)| field_line(&k, &v)).collect::<String>();
    Some(format!("{}{line}{}", content.get(..start?)?, content.get(end..)?))
}

/// One write boundary for imported source notes; paths are reserved by the
/// reader's output-path owner before reaching here.
pub(crate) fn write_note(output_dir: &Path, relative: &str, content: &str) -> Result<(), String> {
    let file_path = output_dir.join(relative);
    if let Some(dir) = file_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create directory: {e}"))?;
    }
    // allow:raw_write the author's imported source note, never generated build output
    std::fs::write(&file_path, content).map_err(|e| format!("Failed to write file: {e}"))
}

#[cfg(test)]
#[path = "page/tests.rs"]
mod tests;
