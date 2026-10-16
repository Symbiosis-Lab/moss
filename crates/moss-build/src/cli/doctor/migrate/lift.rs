//! `moss migrate --lift <class> --as <field>`: a classed div that restates a
//! fact about its page becomes a frontmatter value of a declared field, which
//! moss then draws.
//!
//! Only a div that is exactly `::: {.class}`, one paragraph of plain text and
//! `:::` is lifted. Anything else on a page (two such divs, a link or emphasis
//! inside, a different value already in the frontmatter) leaves that page as
//! written and says why, because splitting or reformatting the author's text
//! is a judgement this command cannot make.

use moss_core::fields::{FieldDecl, PageFormat, Place};
use moss_core::frontmatter::{edit_fields, FieldEdit};

use super::{FileChange, Skipped};

/// What lifting `class` into `field` would change.
#[derive(Debug, Default)]
pub struct LiftPlan {
    pub pages: Vec<FileChange>,
    /// Pages whose div was wholly in italics or bold, which the value drops.
    pub unstyled: usize,
    /// The config text with the field declared, when it has to change.
    pub config: Option<String>,
    pub skipped: Vec<Skipped>,
}

pub struct LiftRequest<'a> {
    pub class: &'a str,
    pub field: &'a str,
    /// `--place`: `None` when not given, `Some(None)` for `none`.
    pub place: Option<Option<Place>>,
}

/// The div's lines, 0-based and inclusive, and its text.
struct Div {
    opener: usize,
    closer: usize,
    text: String,
    /// The paragraph was one italic or bold span, now dropped.
    unstyled: bool,
}

/// Plan the lift over `pages` (site-relative path, file text), against the
/// config text (empty when the site has none) and its declarations.
pub fn plan(
    request: &LiftRequest,
    pages: &[(String, String)],
    config_text: &str,
    decls: &[FieldDecl],
    format: PageFormat,
) -> Result<LiftPlan, String> {
    let declared = decls.iter().find(|d| d.name == request.field);
    let builtin = moss_core::schema_fields::builtin_field(request.field).is_some();
    if builtin && !moss_core::schema_fields::PLACEABLE_BUILTINS.contains(&request.field) {
        return Err(format!(
            "`{}` is a moss field; a classed div can be lifted into a field of your own, or into {}",
            request.field,
            moss_core::schema_fields::PLACEABLE_BUILTINS.join(" or ")
        ));
    }

    let mut plan = LiftPlan::default();
    for (rel, content) in pages {
        let skip = |line: usize, reason: String| Skipped { path: rel.clone(), line: Some(line), reason };
        let openers: Vec<usize> = crate::cli::doctor::page_css_regions(content)
            .into_iter()
            .filter(|r| r.classes.iter().any(|c| c == request.class))
            .map(|r| r.line)
            .collect();
        let Some(&first) = openers.first() else { continue };
        if content.contains("\r\n") {
            plan.skipped.push(skip(first, "the file has Windows line endings".to_string()));
            continue;
        }
        if openers.len() > 1 {
            plan.skipped.push(skip(first, format!("the page has {} of these divs, and a field holds one value", openers.len())));
            continue;
        }
        let div = match lone_paragraph(content, first - 1, request.class) {
            Ok(div) => div,
            Err(reason) => {
                plan.skipped.push(skip(first, reason));
                continue;
            }
        };
        if let Some(decl) = declared {
            if let Err(e) = moss_core::fields::parse_value(decl, &serde_json::Value::String(div.text.clone()), format) {
                plan.skipped.push(skip(first, format!("\"{}\" does not fit the declared field: {e}", div.text)));
                continue;
            }
        }
        match moss_core::frontmatter::frontmatter_map(content).remove(request.field) {
            None => {}
            Some(serde_yaml::Value::String(value)) if value == div.text => {}
            Some(other) => {
                let said = moss_core::frontmatter::value_as_string(&other).unwrap_or_else(|| "a value that is not text".to_string());
                plan.skipped.push(skip(first, format!("the frontmatter already gives {} {said}", request.field)));
                continue;
            }
        }

        let without_div = remove_lines(content, div.opener, div.closer);
        let set = FieldEdit::Set { key: request.field.to_string(), value: serde_yaml::Value::String(div.text) };
        match edit_fields(&without_div, &[set]) {
            Ok(after) => {
                plan.unstyled += usize::from(div.unstyled);
                plan.pages.push(FileChange { path: rel.clone(), before: content.clone(), after });
            }
            Err(reason) => plan.skipped.push(skip(first, reason)),
        }
    }

    if !plan.pages.is_empty() {
        plan.config = declare(config_text, request, declared)?;
    }
    Ok(plan)
}

/// The div opening on 0-based line `opener` of `content`, when it is the class
/// alone around one paragraph of plain text.
fn lone_paragraph(content: &str, opener: usize, class: &str) -> Result<Div, String> {
    let lines: Vec<&str> = content.lines().collect();
    let head = lines.get(opener).map(|l| l.trim()).unwrap_or_default();
    let bare = head.trim_start_matches(':').trim();
    if bare != format!("{{.{class}}}") {
        return Err(format!("the opener `{head}` carries more than the class"));
    }
    let Some(closer) = (opener + 1..lines.len()).find(|&i| lines[i].trim().starts_with(":::")) else {
        return Err("the div is not closed".to_string());
    };
    let inner = &lines[opener + 1..closer];
    if lines[closer].trim().trim_start_matches(':') != "" {
        return Err("another div opens inside it".to_string());
    }
    if inner.is_empty() || inner.iter().any(|l| l.trim().is_empty()) {
        return Err("it holds more or less than one paragraph".to_string());
    }
    let text = inner.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
    let (text, unstyled) = plain_text(&text).ok_or("it holds formatting or a link, which a field value cannot carry")?;
    Ok(Div { opener, closer, text, unstyled })
}

/// The text of markdown `source` when it is one paragraph of plain text, or
/// of plain text wholly in one italic or bold span (then `true`), which a
/// theme can set on the field instead.
fn plain_text(source: &str) -> Option<(String, bool)> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    if source.contains(['\\', '[', '{']) {
        return None;
    }
    // Paragraph, then text inside at most one italic or bold span that
    // covers it all; anything else (a link, code, a `$…$` span) is formatting.
    let (mut text, mut depth, mut spans, mut text_outside) = (String::new(), 0usize, 0usize, false);
    for (i, event) in Parser::new_ext(source, moss_core::ast::parser_options(true)).enumerate() {
        match event {
            Event::Start(Tag::Paragraph) if i == 0 => {}
            Event::End(TagEnd::Paragraph) => {}
            Event::Start(Tag::Emphasis | Tag::Strong) if depth == 0 && text.is_empty() => {
                depth += 1;
                spans += 1;
            }
            Event::End(TagEnd::Emphasis | TagEnd::Strong) if depth == 1 => depth -= 1,
            Event::Text(t) => {
                text_outside |= depth == 0;
                text.push_str(&t);
            }
            Event::InlineMath(_) | Event::DisplayMath(_) => return None,
            _ => return None,
        }
    }
    match (spans, text_outside) {
        (0, _) => Some((text, false)),
        (1, false) => Some((text, true)),
        _ => None,
    }
}

/// `content` without lines `from..=to`, and without one of the blank lines
/// that would then sit together.
fn remove_lines(content: &str, from: usize, to: usize) -> String {
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let blank = |i: usize| lines.get(i).is_some_and(|l| l.trim().is_empty());
    let above_blank = from == 0 || blank(from - 1);
    let end = if above_blank && blank(to + 1) { to + 2 } else { to + 1 };
    let mut out: String = lines[..from].concat();
    out.push_str(&lines[end.min(lines.len())..].concat());
    out
}

/// The config text with `[fields.<field>]` declared as the lift needs, or
/// `None` when it already is.
fn declare(config_text: &str, request: &LiftRequest, declared: Option<&FieldDecl>) -> Result<Option<String>, String> {
    if declared.is_some_and(|d| request.place.is_none_or(|place| d.place == place)) {
        return Ok(None);
    }
    let mut table: toml::Table = toml::from_str(config_text).map_err(|e| format!("could not read .moss/config.toml: {e}"))?;
    let fields = table.entry("fields").or_insert_with(|| toml::Value::Table(Default::default()));
    let Some(fields) = fields.as_table_mut() else {
        return Err("[fields] in .moss/config.toml is not a table".to_string());
    };
    let decl = fields.entry(request.field).or_insert_with(|| toml::Value::Table(Default::default()));
    let Some(decl) = decl.as_table_mut() else {
        return Err(format!("fields.{} in .moss/config.toml is not a table", request.field));
    };
    let builtin = moss_core::schema_fields::builtin_field(request.field).is_some();
    if declared.is_none() && !builtin {
        decl.insert("type".to_string(), toml::Value::String("text".to_string()));
    }
    match request.place {
        Some(Some(place)) => {
            decl.insert("place".to_string(), toml::Value::String(place.name().to_string()));
        }
        Some(None) => {
            decl.remove("place");
        }
        None => {}
    }
    crate::infra::toml_rewrite::apply_changes(config_text, &table).map(Some)
}

#[cfg(test)]
#[path = "lift_tests.rs"]
mod tests;
