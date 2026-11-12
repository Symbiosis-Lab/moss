//! Effective access from source hierarchy, resolved before body-derived data.
//!
//! Every reader of page source that derives public output goes through
//! [`MemberCascade::public_source`]; a new derived output is added to
//! moss-build's `members_public_test`, which searches all output for a
//! private sentinel.
use std::borrow::Cow;
use std::collections::BTreeMap;
use serde_yaml::Value;
use super::members::{Members, public_source};

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MemberCascade {
    // Declaring path -> (descendant source prefix, access).
    declarations: BTreeMap<String, (String, Members)>,
}

/// The access a page's `cascade:` gives the pages beneath it. Front matter
/// that cannot be read with certainty restricts them, and so does a
/// `cascade:` naming members that is not a mapping: the simplified dialect
/// reads `{members: paid}` as text, and a list carries no cascade.
pub fn declared_cascade(content: &str) -> Option<Members> {
    if super::members::access_uncertain(content) { return Some(Members::Subscribers); }
    let fields = super::frontmatter_map(content);
    let cascade = fields.get("cascade")?;
    if let Some(access) = cascade.get("members") { return Some(Members::from_value(access)); }
    let names_members = match cascade {
        Value::Mapping(map) => map.keys().filter_map(Value::as_str).any(super::members::declares_members),
        Value::String(text) => super::members::declares_members(text),
        other => serde_yaml::to_string(other).is_ok_and(|text| super::members::declares_members(&text)),
    };
    names_members.then_some(Members::Subscribers)
}

impl MemberCascade {
    /// `homes` are the folder homes the build elected, each with what its
    /// [`declared_cascade`] says; the host passes `Some(Subscribers)` for a
    /// home it could not read. A home's declaration covers its folder's
    /// subtree, never the home itself, and no other page's `cascade:` has a
    /// scope.
    pub fn from_homes<'a>(homes: impl IntoIterator<Item = (&'a str, Option<Members>)>) -> Self {
        let declarations = homes.into_iter().filter_map(|(home, access)| {
            let prefix = home.rsplit_once('/').map_or(String::new(), |(folder, _)| format!("{folder}/"));
            Some((home.to_string(), (prefix, access?)))
        }).collect();
        Self { declarations }
    }

    pub fn inherited(&self, path: &str) -> Option<Members> {
        self.declarations.iter().filter(|(source, (prefix, _))| {
            source.as_str() != path && path.starts_with(prefix)
        }).max_by_key(|(_, (prefix, _))| prefix.len()).map(|(_, (_, access))| *access)
    }

    /// Explicit page access wins, including public. A declaration never applies
    /// to its own page. Reprojection retains the public boundary marker.
    pub fn public_source<'a>(&self, path: &str, content: &'a str) -> Cow<'a, str> {
        let projected = public_source(content);
        let mut fields: BTreeMap<_, _> = super::frontmatter_map(&projected).into_iter().collect();
        if fields.contains_key("members") { return projected; }
        let Some(access) = self.inherited(path) else { return projected };
        if access == Members::Public { return projected; }
        if super::parse(&projected).frontmatter_error.is_some() {
            return super::members::restricted_fallback(None);
        }
        fields.insert("members".into(), serde_yaml::Value::String(if access == Members::Paid { "paid" } else { "subscribers" }.into()));
        let Ok(header) = serde_yaml::to_string(&fields) else { return super::members::restricted_fallback(None) };
        let source = format!("---\n{header}---\n{}", super::body(&projected));
        Cow::Owned(public_source(&source).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn homes(sources: &[(&str, String)]) -> MemberCascade {
        MemberCascade::from_homes(sources.iter().map(|(path, text)| (*path, declared_cascade(text))))
    }

    #[test]
    fn nearest_ancestor_and_explicit_page_access_win() {
        let sources = [
            ("index.MD", "---\ncascade: {members: subscribers}\n---\nRoot"),
            ("notes/index.markdown", "---\ncascade: {members: paid}\n---\nParent"),
            ("notes/open/index.mdown", "---\ncascade: {members: public}\n---\nParent"),
        ].map(|(path, text)| (path, text.to_string()));
        let policy = homes(&sources);
        assert_eq!(policy.inherited("index.MD"), None);
        assert_eq!(policy.inherited("notes/index.markdown"), Some(Members::Subscribers));
        assert_eq!(policy.inherited("notes/article.mkd"), Some(Members::Paid));
        assert_eq!(policy.inherited("notes/open/article.md"), Some(Members::Public));
        let explicit = "---\nmembers: public\n---\nprivate-word";
        assert_eq!(policy.public_source("notes/article.md", explicit), explicit);
        assert!(!policy.public_source("notes/article.md", "private-word").contains("private-word"));
        assert!(!policy.public_source("notes/open/article.md", "---\nmembers: paid\n---\nprivate-word").contains("private-word"));
    }

    #[test]
    fn malformed_ancestor_members_fail_closed_and_valid_overrides_win() {
        for fields in [
            "broken: [\ncascade: {members: subscribers}",
            "broken: [\nmembers: paid",
            "broken: [\ndescription: MEMBERS",
            "cascade: {members: public}\nbroken: [",
        ] {
            let sources = [
                ("notes/index.md", format!("---\n{fields}\n---\nParent")),
                ("notes/open/index.md", "---\ncascade: {members: public}\n---\nOpen".to_string()),
            ];
            let policy = homes(&sources);
            assert_eq!(policy.inherited("notes/article.md"), Some(Members::Subscribers), "{fields}");
            assert_eq!(policy.inherited("notes/index.md"), None);
            assert_eq!(policy.inherited("elsewhere/article.md"), None);
            let child = "Visible preview\n<!--members-only-->private-word";
            let projected = policy.public_source("notes/article.md", child);
            assert!(projected.contains("Visible preview"));
            assert!(!projected.contains("private-word"));
            for access in ["public", "paid", "subscribers"] {
                let explicit = format!("---\nmembers: {access}\n---\n{child}");
                let projected = policy.public_source("notes/article.md", &explicit);
                assert_eq!(projected.contains("private-word"), access == "public");
                assert_eq!(super::super::frontmatter_map(&projected)["members"].as_str(), Some(access));
            }
            assert_eq!(policy.public_source("notes/open/article.md", child), child);
        }
    }

    #[test]
    fn unclosed_ancestor_members_fail_closed() {
        for source in [
            "---\ncascade: {members: subscribers}\nParent",
            "--- \ncascade: {members: public}\nParent",
            "cascade: {members: subscribers}",
        ] {
            let policy = homes(&[("notes/index.md", source.to_string())]);
            assert_eq!(policy.inherited("notes/article.md"), Some(Members::Subscribers), "{source}");
            let projected = policy.public_source("notes/article.md", "Visible preview\n<!--members-only-->private-word");
            assert!(!projected.contains("private-word"), "{source}");
        }
    }

    #[test]
    fn absent_members_and_body_mentions_do_not_create_a_cascade() {
        for source in [
            "---\nbroken: [\n---\nMembers in the body",
            "---\ntitle: Public\n---\nmembers: subscribers",
            "A body mentioning members\ncascade: {members: subscribers}",
        ] {
            let policy = homes(&[("index.md", source.to_string())]);
            assert_eq!(policy.inherited("article.md"), None, "{source}");
            assert_eq!(policy.public_source("article.md", "Public body"), "Public body");
        }
    }

    #[test]
    fn an_unread_home_and_a_textual_cascade_restrict_the_folder() {
        let unread = MemberCascade::from_homes([("notes/index.md", Some(Members::Subscribers))]);
        assert!(!unread.public_source("notes/article.md", "private-word").contains("private-word"));
        for source in [
            "cascade: {members: public}\n---\nSimplified",
            "cascade: {Members: public}\n---\nSimplified",
            "---\ncascade: {Members: public}\n---\nYaml",
            "---\ncascade: [{members: public}]\n---\nYaml",
        ] {
            assert_eq!(declared_cascade(source), Some(Members::Subscribers), "{source}");
        }
        assert_eq!(declared_cascade("cascade: {nav: false}\n---\nSimplified"), None);
        assert_eq!(declared_cascade("---\ncascade: {members: paid}\n---\nYaml"), Some(Members::Paid));
    }
}
