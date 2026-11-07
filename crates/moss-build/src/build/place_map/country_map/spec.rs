//! The `country_map:` frontmatter block.

use serde::{Deserialize, Serialize};
use serde_yaml::Value;

/// More ordered steps than this cannot be told apart by colour on a map.
pub(crate) const MAX_LEVELS: usize = 7;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Level {
    pub name: String,
    /// The advice shown for a country at this level whose row has no extra columns.
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Action {
    pub label: String,
    /// May hold `{country}`, replaced by the URL-encoded country name.
    pub link: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Spec {
    pub by: String,
    pub levels: Vec<Level>,
    pub unrated: Option<String>,
    pub action: Option<Action>,
    /// The search field above the map; on unless the author sets `search: false`.
    pub search: bool,
}

impl Default for Spec {
    fn default() -> Self {
        Spec { by: String::new(), levels: Vec::new(), unrated: None, action: None, search: true }
    }
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub(crate) fn parse(value: &Value) -> Result<Spec, String> {
    let map = value.as_mapping().ok_or("country_map must be a block with `by` and `levels`")?;
    let get = |key: &str| map.get(Value::String(key.to_string()));
    let by = get("by").and_then(text_of).filter(|s| !s.is_empty()).ok_or("country_map needs `by`, the name of the column that colours the countries")?;
    let list = get("levels").and_then(Value::as_sequence).filter(|l| !l.is_empty()).ok_or("country_map needs `levels`, the column's values lowest first")?;
    let mut levels = Vec::new();
    for item in list {
        let level = match item {
            Value::Mapping(m) => {
                let name = m.get(Value::String("name".into())).and_then(text_of).filter(|s| !s.is_empty());
                let text = m.get(Value::String("text".into())).and_then(text_of).filter(|s| !s.is_empty());
                name.map(|name| Level { name, text })
            }
            other => text_of(other).filter(|s| !s.is_empty()).map(|name| Level { name, text: None }),
        };
        levels.push(level.ok_or("each of country_map's `levels` is a name or a {name, text} pair")?);
    }
    if levels.len() > MAX_LEVELS {
        return Err(format!("country_map has {} levels; the most it can colour is {MAX_LEVELS}", levels.len()));
    }
    let action = match get("action") {
        None | Some(Value::Null) => None,
        Some(Value::Mapping(m)) => {
            let field = |k: &str| m.get(Value::String(k.into())).and_then(text_of).filter(|s| !s.is_empty());
            Some(Action {
                label: field("label").ok_or("country_map's `action` needs a `label`")?,
                link: field("link").ok_or("country_map's `action` needs a `link`")?,
            })
        }
        Some(_) => return Err("country_map's `action` is {label, link}".into()),
    };
    let search = match get("search") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(on)) => *on,
        Some(_) => return Err("country_map's `search` is true or false".into()),
    };
    Ok(Spec { by, levels, unrated: get("unrated").and_then(text_of).filter(|s| !s.is_empty()), action, search })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_yaml(yaml: &str) -> Result<Spec, String> {
        parse(&serde_yaml::from_str(yaml).unwrap())
    }

    #[test]
    fn reads_plain_and_described_levels_unrated_and_action() {
        let spec = parse_yaml(
            "by: Level\nlevels:\n  - Low\n  - {name: High, text: Avoid travel.}\nunrated: Not assessed.\naction: {label: Ask, link: '/ask/?to={country}'}",
        )
        .unwrap();
        assert_eq!(spec.by, "Level");
        assert_eq!(spec.levels[0], Level { name: "Low".into(), text: None });
        assert_eq!(spec.levels[1].text.as_deref(), Some("Avoid travel."));
        assert_eq!(spec.unrated.as_deref(), Some("Not assessed."));
        assert_eq!(spec.action.unwrap().link, "/ask/?to={country}");
    }

    #[test]
    fn search_is_on_unless_turned_off() {
        assert!(parse_yaml("by: x\nlevels: [a]").unwrap().search);
        assert!(parse_yaml("by: x\nlevels: [a]\nsearch: true").unwrap().search);
        assert!(!parse_yaml("by: x\nlevels: [a]\nsearch: false").unwrap().search);
        assert!(parse_yaml("by: x\nlevels: [a]\nsearch: maybe").unwrap_err().contains("`search`"));
    }

    #[test]
    fn more_than_seven_levels_are_refused() {
        assert!(parse_yaml("by: x\nlevels: [a, b, c, d, e, f, g, h]").unwrap_err().contains("8 levels; the most it can colour is 7"));
        assert!(parse_yaml("by: x\nlevels: [a, b, c, d, e, f, g]").is_ok());
    }

    #[test]
    fn a_block_without_by_or_levels_is_refused_with_the_missing_key() {
        assert!(parse_yaml("levels: [a]").unwrap_err().contains("`by`"));
        assert!(parse_yaml("by: x").unwrap_err().contains("`levels`"));
        assert!(parse_yaml("by: x\nlevels: [a]\naction: {label: Go}").unwrap_err().contains("`link`"));
    }
}
