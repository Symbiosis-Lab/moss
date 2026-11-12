//! The public view of a members page, before any body-derived information is
//! computed. Every reader gets the same view, including transclusion.

use std::borrow::Cow;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use super::{frontmatter_map, frontmatter_span};

pub const MARKER: &str = "<!--members-only-->";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Members {
    Public,
    Subscribers,
    Paid,
}

impl Members {
    /// Invalid declarations stay restricted, including blank, null and false.
    /// Schema validation reports the mistake separately; it cannot expose text.
    pub fn from_value(value: &Value) -> Self {
        match value.as_str().map(str::trim) {
            Some("public") => Self::Public,
            Some("paid") => Self::Paid,
            _ => Self::Subscribers,
        }
    }

    /// A restricted level named in a shortcode argument; anything else is none.
    pub fn restricted(value: &str) -> Option<Self> {
        match value.trim() {
            "subscribers" => Some(Self::Subscribers),
            "paid" => Some(Self::Paid),
            _ => None,
        }
    }
}

pub fn deserialize_members<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Members>, D::Error> {
    Ok(Some(Members::from_value(&Value::deserialize(d)?)))
}

/// A malformed block cannot be parsed for its access declaration. Recognize
/// the field's spelling conservatively rather than letting parse recovery
/// turn a restricted page into a public one.
pub(super) fn declares_members(fields: &str) -> bool {
    fields.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|word| word.eq_ignore_ascii_case("members"))
}

/// The front matter mentions `members` but cannot be read with certainty:
/// unparsable, never closed, or spelled loosely. Every reader of access,
/// a page's own and a folder's cascade, treats this as restricted.
pub(super) fn access_uncertain(content: &str) -> bool {
    let Some(span) = frontmatter_span(content) else {
        let candidate = content.trim_start_matches('\u{feff}').trim_start();
        let first = candidate.lines().next().unwrap_or("").trim();
        let prefix = candidate.lines().skip(1).take_while(|line| line.trim() != "---")
            .collect::<Vec<_>>().join("\n");
        let starts_fields = crate::frontmatter_typed::is_frontmatter_field_line(first);
        let apparent_key = first.split([':', ' ', '\t']).next().unwrap_or("")
            .trim_matches(['\'', '"', '{']);
        return (declares_members(first) || declares_members(&prefix))
            && (starts_fields || first.starts_with("---") || apparent_key.eq_ignore_ascii_case("members"));
    };
    if !declares_members(content.get(span.fields).unwrap_or("")) { return false; }
    let map = frontmatter_map(content);
    super::parse(content).frontmatter_error.is_some()
        || (!map.contains_key("members") && map.keys().any(|key| declares_members(key)))
}

/// Idempotent: retain the boundary marker in the public view, so another
/// consumer can apply this policy without losing the public preview or
/// duplicating its subscribe box. Original source bytes remain on disk only.
pub fn public_source(content: &str) -> Cow<'_, str> {
    if access_uncertain(content) { return restricted_fallback(None); }
    let Some(span) = frontmatter_span(content) else { return Cow::Borrowed(content) };
    let map = frontmatter_map(content);
    let Some(value) = map.get("members") else { return Cow::Borrowed(content) };
    if !matches!(value.as_str().map(str::trim), Some("public" | "paid" | "subscribers")) {
        return restricted_fallback(map.get("title").and_then(Value::as_str));
    }
    let access = Members::from_value(value);
    if access == Members::Public { return Cow::Borrowed(content); }
    let body = content.get(span.body..).unwrap_or("");
    let preview = body.split_once(MARKER).map_or("", |(public, _)| public);
    Cow::Owned(format!("{}{preview}{MARKER}{}", content.get(..span.body).unwrap_or(""), notice(access)))
}

pub(super) fn restricted_fallback(title: Option<&str>) -> Cow<'static, str> {
    let title = title.map(|title| format!("title: {}\n", serde_json::Value::String(title.into())))
        .unwrap_or_default();
    Cow::Owned(format!("---\n{title}members: subscribers\n---\n{MARKER}{}", notice(Members::Subscribers)))
}

fn notice(access: Members) -> &'static str {
    match access {
        Members::Public => "",
        Members::Subscribers => "\n\n<div class=\"moss-members\">\n\n:::subscribe {members=\"subscribers\"}\n:::\n\n</div>\n",
        Members::Paid => "\n\n<div class=\"moss-members\">\n\n:::subscribe {members=\"paid\"}\n:::\n\n</div>\n",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn members_projection_is_fail_closed_and_idempotent_in_both_dialects() {
        for head in ["---\nmembers: subscribers\n---\n", "members: paid\n---\n"] {
            for (body, public) in [("private-unique-secret", false), ("public preview\n<!--members-only-->private-unique-secret", true)] {
                let source = format!("{head}{body}");
                let out = public_source(&source);
                assert!(!out.contains("private-unique-secret"));
                assert_eq!(out.contains("public preview"), public);
                assert_eq!(public_source(&out), out);
            }
        }
        assert_eq!(public_source("ordinary public body"), "ordinary public body");
    }

    #[test]
    fn malformed_members_never_become_public_through_recovery() {
        for value in ["wrong", "false", "null", "[]", "", "{bad"] {
            let source = format!("---\nmembers: {value}\nweight: wrong\n---\nprivate-unique-secret");
            assert!(!public_source(&source).contains("private-unique-secret"), "{value}");
        }
        for source in ["---\nmembers: paid\nprivate-unique-secret", "---\n'members': [\n---\nprivate-unique-secret", "---\n{members: paid, bad: [}\n---\nprivate-unique-secret", "members: paid\nbad line\n---\nprivate-unique-secret"] {
            assert!(!public_source(source).contains("private-unique-secret"));
        }
    }

    #[test]
    fn uncertain_members_keep_only_a_trusted_title() {
        for value in ["null", "false", "[]", "unknown", "'unknown'"] {
            let source = format!("---\ntitle: Public title\nmembers: {value}\ndescription: secret\ncover: secret\n---\nsecret preview\n{MARKER}secret");
            let projected = public_source(&source);
            assert!(!projected.contains("secret"), "{value}");
            assert!(projected.contains("Public title"));
            assert_eq!(Members::from_value(&frontmatter_map(&projected)["members"]), Members::Subscribers);
            assert_eq!(public_source(&projected), projected);
        }
        for source in ["--- \nmembers: paid\nsecret", "members\n---\nsecret", "'members': paid\n---\nsecret", "members:paid\n---\nsecret", "MEMBERS: public\n---\nsecret", "  ---\n 'members' : public\nbad: [\n---\nsecret", "\u{feff}--- \r\nmembers: paid\r\nsecret", "---\nmembers paid\n---\nsecret"] {
            let projected = public_source(source);
            assert!(!projected.contains("secret"), "{source}");
            assert!(projected.contains("members: subscribers"));
        }
    }

    #[test]
    fn typed_members_survive_an_invalid_neighbour_and_null() {
        for value in ["subscribers", "paid", "null", "false", "[]"] {
            let yaml = format!("members: {value}\nweight: wrong");
            let map = serde_yaml::from_str(&yaml).unwrap();
            let (fm, _) = crate::frontmatter_typed::project_typed(&map);
            assert!(fm.members.is_some(), "{value}");
            let simplified = format!("members: {value}\n---\nbody");
            assert!(crate::frontmatter_typed::parse_simplified_frontmatter(&simplified).0.members.is_some());
        }
    }
}
