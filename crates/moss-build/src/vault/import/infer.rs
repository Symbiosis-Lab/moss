//! Generic component-tree walker: JSON nodes → block IR, driven by a
//! [`JsonDialect`](super::dialects::JsonDialect) row.
//!
//! The mechanics are builder-agnostic (typed nodes, container recursion,
//! active-child wrappers, sentinel image URLs with a CDN fallback); builder
//! specifics live in the dialect row.

use super::blocks::Block;
use super::dialects::{JsonDialect, NodeRule};
use serde_json::Value;

/// Walk result: blocks in reading order plus every unknown component type
/// encountered (skipped, but never silently — callers log them).
pub(crate) struct WalkOutput {
    pub blocks: Vec<Block>,
    pub skipped: Vec<String>,
}

/// Walk a section tree (an array of typed component nodes) into blocks.
/// `res_id` feeds the dialect's CDN template for sentinel image URLs; when
/// empty, such images are dropped rather than emitted as broken links.
pub(crate) fn walk_tree(sections: &Value, dialect: &JsonDialect, res_id: &str) -> WalkOutput {
    let mut w = Walker {
        dialect,
        res_id,
        blocks: Vec::new(),
        skipped: Vec::new(),
    };
    if let Some(arr) = sections.as_array() {
        for section in arr {
            w.walk(section);
        }
    }
    WalkOutput {
        blocks: w.blocks,
        skipped: w.skipped,
    }
}

struct Walker<'a> {
    dialect: &'a JsonDialect,
    res_id: &'a str,
    blocks: Vec<Block>,
    skipped: Vec<String>,
}

impl Walker<'_> {
    /// Dispatch one node: EVERY rule matching its `type` applies, in table
    /// order (Blog.Section carries both a `Child` and a `Container` row).
    /// A type with no rule is counted into `skipped` when the dialect's
    /// predicate says it is a genuine component.
    fn walk(&mut self, node: &Value) {
        let Some(obj) = node.as_object() else {
            return;
        };
        let Some(ty) = obj.get("type").and_then(Value::as_str) else {
            return;
        };
        let mut matched = false;
        for (t, rule) in self.dialect.rules {
            if *t == ty {
                matched = true;
                self.apply(rule, obj);
            }
        }
        if !matched && (self.dialect.unknown_is_component)(ty) {
            self.skipped.push(ty.to_string());
        }
    }

    fn apply(&mut self, rule: &NodeRule, obj: &serde_json::Map<String, Value>) {
        match rule {
            NodeRule::Text { field, unescape } => {
                if let Some(v) = str_field(obj, field) {
                    let html = if *unescape {
                        unescape_html_entities(v)
                    } else {
                        v.to_string()
                    };
                    self.blocks.push(Block::RichText { html });
                }
            }
            NodeRule::Quote { field } => {
                if let Some(v) = str_field(obj, field) {
                    self.blocks.push(Block::Quote {
                        html: v.to_string(),
                    });
                }
            }
            NodeRule::Image { gate } => {
                let gated =
                    gate.is_some_and(|(f, skip)| obj.get(f).and_then(Value::as_bool) == Some(skip));
                if !gated {
                    if let Some(block) = self.image_block(obj) {
                        self.blocks.push(block);
                    }
                }
            }
            NodeRule::Video { field } => {
                if let Some(url) = str_field(obj, field).filter(|u| !u.is_empty()) {
                    self.blocks.push(Block::Video {
                        url: url.to_string(),
                    });
                }
            }
            // Field names deliberately unparameterized until a second
            // producer needs different ones.
            NodeRule::Button => {
                let text = str_field(obj, "text").map(str::trim).unwrap_or("");
                let url = str_field(obj, "url").map(str::trim).unwrap_or("");
                if !text.is_empty() && !url.is_empty() {
                    self.blocks.push(Block::Button {
                        text: text.to_string(),
                        url: url.to_string(),
                    });
                }
            }
            NodeRule::Separator => self.blocks.push(Block::Separator),
            NodeRule::Child(field) => {
                if let Some(child) = obj.get(*field) {
                    self.walk(child);
                }
            }
            NodeRule::Container(fields) => {
                for field in *fields {
                    self.walk_collection(obj.get(*field));
                }
            }
            NodeRule::ActiveChild {
                field,
                slots,
                default_slot,
            } => {
                let current = obj.get(*field).and_then(Value::as_str);
                let slot = current
                    .and_then(|c| slots.iter().find(|(v, _)| *v == c).map(|(_, s)| *s))
                    .unwrap_or(default_slot);
                if let Some(child) = obj.get(slot) {
                    self.walk(child);
                }
            }
            NodeRule::Chrome => {}
        }
    }

    /// Iterate a collection of components: a map keyed by slot name
    /// (Slide's `components`) or a list (BlockComponent.items,
    /// Repeatable.list, …). Non-component values inside (strings, untyped
    /// maps) fall out in `walk`'s early returns. A SINGLE component under a
    /// singular field is `NodeRule::Child`, not a collection.
    fn walk_collection(&mut self, children: Option<&Value>) {
        match children {
            Some(Value::Object(map)) => {
                for child in map.values() {
                    self.walk(child);
                }
            }
            Some(Value::Array(list)) => {
                for child in list {
                    self.walk(child);
                }
            }
            _ => {}
        }
    }

    /// Image block for an image-role component. A real URL wins; the
    /// dialect's sentinel routes to the CDN template (dropped when no
    /// res_id is known).
    fn image_block(&self, obj: &serde_json::Map<String, Value>) -> Option<Block> {
        let d = self.dialect;
        let src = match str_field(obj, d.image_url_field)
            .filter(|u| !u.is_empty() && *u != d.image_url_sentinel)
        {
            Some(real) => real.to_string(),
            None => {
                let cdn = d.cdn.as_ref()?;
                if self.res_id.is_empty() {
                    return None;
                }
                let key = str_field(obj, cdn.key_field).filter(|k| !k.is_empty())?;
                let format = str_field(obj, cdn.format_field)
                    .filter(|f| !f.is_empty())
                    .unwrap_or(cdn.default_format);
                format!(
                    "{}{}{}{}.{}",
                    cdn.prefix, self.res_id, cdn.infix, key, format
                )
            }
        };
        let alt = d
            .image_caption_fields
            .iter()
            .find_map(|f| str_field(obj, f))
            .unwrap_or("")
            .to_string();
        Some(Block::Image { src, alt })
    }
}

fn str_field<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(Value::as_str)
}

/// Minimal entity unescape for values that arrive HTML-entity-escaped
/// inside builder JSON (Strikingly `HtmlComponent.value`).
fn unescape_html_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::super::dialects::STRIKINGLY;
    use super::*;
    use serde_json::json;

    fn walk(sections: serde_json::Value) -> WalkOutput {
        walk_tree(&sections, &STRIKINGLY, "resx")
    }

    #[test]
    fn child_and_container_rows_both_apply_in_table_order() {
        // Blog.Section carries a singular `component` AND a `components`
        // map; both must walk, singular first (two rules, one type).
        let out = walk(json!([
            {"type": "Blog.Section", "component":
                {"type": "RichText", "value": "<p>first</p>"},
             "components": {"a": {"type": "RichText", "value": "<p>second</p>"}}}
        ]));
        assert_eq!(
            out.blocks,
            vec![
                Block::RichText { html: "<p>first</p>".into() },
                Block::RichText { html: "<p>second</p>".into() },
            ]
        );
    }

    #[test]
    fn untyped_map_under_child_field_is_dropped() {
        // Old-renderer parity: walk_opt on an untyped map early-returns;
        // it must NOT be treated as a collection and have its values walked.
        let out = walk(json!([
            {"type": "Blog.Section", "component":
                {"note": {"type": "RichText", "value": "<p>hidden</p>"}}}
        ]));
        assert!(out.blocks.is_empty(), "got: {:?}", out.blocks);
        assert!(out.skipped.is_empty());
    }

    #[test]
    fn collection_map_with_type_metadata_entry_still_iterates_values() {
        // A components map carrying a literal "type" string as metadata is
        // still a collection: values walk (the string value early-returns),
        // and nothing is silently lost or counted.
        let out = walk(json!([
            {"type": "Slide", "components": {
                "type": "columns",
                "text1": {"type": "RichText", "value": "<p>kept</p>"}
            }}
        ]));
        assert_eq!(
            out.blocks,
            vec![Block::RichText { html: "<p>kept</p>".into() }]
        );
        assert!(out.skipped.is_empty(), "skipped: {:?}", out.skipped);
    }
}
