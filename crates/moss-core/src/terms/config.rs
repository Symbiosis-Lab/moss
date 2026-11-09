//! Pure reading of term kinds and their name-list fields.
use std::collections::BTreeMap;
use crate::page::site_config::{read, ConfigValue};
use super::{AUTHOR_NS, TAGS_NS};

/// The two built-in namespaces and the field each one carries by default.
/// One table, because two readers have to agree about it: config builds the
/// built-in kinds from it, and the build decides a field has moved by
/// comparing the kinds table against it.
pub const BUILTIN_DEFAULT_FIELDS: [(&str, &str); 2] = [(AUTHOR_NS, "author"), (TAGS_NS, "tags")];

/// One `[terms.<key>]` table, or a built-in kind no table declared, before
/// the build checks its `fields` and resolves its `title`.
#[derive(Debug, Clone)]
pub struct RawKind {
    /// URL namespace / pseudo-folder prefix (`authors`, `tags`, or a
    /// declared key like `people`).
    pub key: String,
    /// Frontmatter field names as configured, not yet checked against the
    /// built-in schema (see [`filter_fields`]).
    pub fields: Vec<String>,
    /// The table's `title`, a string or a table of `language tag = title`, as
    /// `(language tag, title)` pairs. Empty for the two built-ins (the build
    /// names them in the site language) and for a declared kind that didn't
    /// set one (its title is its key).
    pub titles: Vec<(String, String)>,
    /// The table's `count`, in the same `(language tag, text)` shape as
    /// `titles`: the wording of a term's card count, with `{n}` for the number
    /// and the first `|` splitting the singular form. `count = false` is the
    /// empty text, which hides the count. Empty when the key is absent and for
    /// the two built-ins.
    pub counts: Vec<(String, String)>,
    /// The declared `type = "..."` key, unvalidated: `"place"` is the only
    /// value the build recognizes, and anything else is a diagnostic there.
    /// `None` for the two built-ins and for a declared kind that didn't set one.
    pub kind_type: Option<String>,
    /// `[terms.<key>] explorer`. The build keeps it only for the place-typed
    /// kind, the same way `type` itself is only meaningful there. `None` for
    /// the two built-ins and for a declared kind that didn't set the key.
    pub explorer: Option<bool>,
    /// `[terms.<key>] line`, kept by the build only for the place-typed kind,
    /// like `explorer`.
    pub line: Option<bool>,
    /// `[terms.<key>] cards`. `None` when the key is absent and for the two built-ins.
    pub cards: Option<bool>,
}

impl RawKind {
    /// A table with no `fields` array is skipped with that one warning: a kind
    /// that names no fields would derive nothing, and is more likely a typo
    /// (`field = [...]`) than an intentional no-op.
    pub(crate) fn read(key: &str, table: &BTreeMap<String, ConfigValue>, warnings: &mut Vec<String>) -> Option<Self> {
        let path = format!("[terms.{key}]");
        let Some(fields) = table.get("fields").and_then(ConfigValue::array) else {
            warnings.push(format!("{path} has no `fields` array; skipping"));
            return None;
        };
        let fields = fields.iter().filter_map(|value| read(Some(value), |v| v.text().cloned(),
            &format!("{path} fields entry"), "text", warnings)).collect();
        let titles = read(table.get("title"), ConfigValue::labels,
            &format!("{path} title"), "text or a table of text per language", warnings);
        let counts = read(table.get("count"), |value| match value {
            ConfigValue::Bool(false) => Some(vec![("en".to_string(), String::new())]),
            other => other.labels(),
        }, &format!("{path} count"), "text, a table of text per language, or false", warnings);
        let flag = |name: &str, warnings: &mut Vec<String>| read(table.get(name), ConfigValue::boolean,
            &format!("{path} {name}"), "true or false", warnings);
        Some(Self {
            key: key.to_string(),
            fields,
            titles: titles.unwrap_or_default(),
            counts: counts.unwrap_or_default(),
            kind_type: read(table.get("type"), |v| v.text().cloned(), &format!("{path} type"), "text", warnings),
            explorer: flag("explorer", warnings),
            line: flag("line", warnings),
            cards: flag("cards", warnings),
        })
    }
}

/// The declared kinds, after a built-in kind for each of `authors`/`tags` that
/// no declared table already used.
///
/// A declared `[terms.authors]` or `[terms.tags]` table IS that kind: its own
/// `fields` and `title` win outright, rather than layering on the built-in
/// default. Two kinds sharing one key would seed the same pseudo-folder twice
/// and double-count every member.
pub(crate) fn with_builtins(declared: &[RawKind], disabled: &[String]) -> Vec<RawKind> {
    let mut kinds = Vec::with_capacity(declared.len() + 2);
    for (ns, field) in BUILTIN_DEFAULT_FIELDS {
        if declared.iter().any(|kind| kind.key == ns) {
            continue;
        }
        kinds.push(RawKind {
            key: ns.to_string(),
            fields: if disabled.iter().any(|d| d == field) { Vec::new() } else { vec![field.to_string()] },
            titles: Vec::new(),
            counts: Vec::new(),
            kind_type: None,
            explorer: None, line: None, cards: None,
        });
    }
    kinds.extend(declared.iter().cloned());
    kinds
}

/// Why a name in a `[terms.<key>] fields` list cannot be a name-list field, or
/// `None` when it can. A built-in field that is not a name list is refused,
/// since reading its value as a list of names would break it. So is a name
/// whose claim key (`<name>_page`) is a built-in's claim key while a kind that
/// does not list the name lists that built-in: `tag` would claim `tag_page:`,
/// which `tags` already owns, so one `tag_page:` would claim in two kinds.
/// `place` shares `location`'s `place_page:`, which is harmless until a
/// different kind also lists `location`.
pub fn name_list_refusal(field: &str, kinds: &[RawKind]) -> Option<String> {
    match crate::schema_fields::BUILTIN_FIELDS.iter().find(|b| b.name == field) {
        Some(b) if !b.name_list => Some("a built-in field that is not a name list".to_string()),
        Some(_) => None,
        None => {
            let claim = super::claim_field_key(field);
            let owner = crate::schema_fields::BUILTIN_FIELDS
                .iter()
                .find(|b| b.name_list && super::claim_field_key(b.name) == claim)?;
            kinds
                .iter()
                .find(|k| k.fields.iter().any(|f| f == owner.name) && !k.fields.iter().any(|f| f == field))
                .map(|k| {
                    format!(
                        "its claim key {claim} is also {}'s, and [terms.{}] lists {}, so one {claim}: would claim in both kinds",
                        owner.name, k.key, owner.name
                    )
                })
        }
    }
}

pub fn declared_name_list_fields(kinds: &[RawKind]) -> (Vec<String>, Vec<String>) {
    let mut out: Vec<String> = Vec::new();
    let (filtered, warnings) = filter_fields(kinds);
    for (_, fields) in &filtered {
        for f in fields {
            if !out.contains(f) {
                out.push(f.clone());
            }
        }
    }
    (out, warnings)
}

/// Filter against the complete original kind set, preserving field order.
pub fn filter_fields(kinds: &[RawKind]) -> (Vec<(RawKind, Vec<String>)>, Vec<String>) {
    let mut warnings = Vec::new();
    let filtered = kinds.iter().map(|kind| {
        let fields = kind.fields.iter().filter(|field| {
            if let Some(reason) = name_list_refusal(field, kinds) {
                warnings.push(format!("[terms.{}] names field \"{}\", which cannot be a name-list field ({}); ignoring", kind.key, field, reason));
                false
            } else {
                true
            }
        }).cloned().collect();
        (kind.clone(), fields)
    }).collect();
    (filtered, warnings)
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
