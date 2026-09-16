//! Pure logic for the page-templates feature: what kind a captured page is,
//! and how a template's frontmatter is reset when it's instantiated into a
//! new page.
//!
//! Template storage and file I/O live in src-tauri (`.moss/templates/`) —
//! this module never touches the filesystem. Frontmatter here is the same
//! untyped `HashMap<String, serde_yaml::Value>` `crate::frontmatter` reads
//! and writes, not the vault's typed `FrontMatter`/`BUILTIN_FIELDS` schema:
//! a template's fields are copied verbatim, including ones the schema
//! doesn't know about, so round-tripping through the typed struct (which has
//! no catch-all field) would silently drop them.

use std::collections::HashMap;
use serde_yaml::Value;

/// What a saved template produces when instantiated. Decided at capture time
/// from the tree's own home-file election (a folder's home file captures as
/// `Folder`), so the template store and the tree never disagree about it.
/// Serialized as `page` / `folder` in the store and across the Tauri seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum TemplateKind {
    /// A single new file.
    Page,
    /// A new folder with a seeded, self-named home file.
    Folder,
}

/// Reset a template's frontmatter for a new instance: `title` is cleared
/// (the untitled-first flow fills it in when the user commits the H1), a
/// `date` the template carries is re-stamped to `now`, and every field that
/// names THIS page rather than describes it is dropped — `uid` (the
/// comments/redirects join key), `url` (the pinned address), `author_page` /
/// `tag_page` (a term claim; two claimants resolve to the first `url_path`,
/// so a copy could steal the original's term page), `translationKey` (a copy
/// makes the pair "one page's translations" and links them), and
/// `syndicated` (where the captured page was published, written by the
/// matters plugin). Every other field — layout, tags, cascade, and anything
/// else the template carries — is copied verbatim, since that's the point of
/// templating them.
///
/// A template without a `date` produces an instance without one (2026-09-05,
/// user report): the captured page's author chose not to date it, and a
/// dateless page is how moss renders an undated section or note; inventing
/// the field would put a date line on every page made from that template.
///
/// `now` is a caller-supplied `YYYY-MM-DD` string (moss-core has no `chrono`
/// dependency by convention — see `date.rs`) so this stays pure and
/// deterministic; the Tauri I/O boundary reads the clock, not this function.
pub fn instantiate_template_frontmatter(
    mut frontmatter: HashMap<String, Value>,
    now: &str,
) -> HashMap<String, Value> {
    frontmatter.remove("title");
    frontmatter.remove("uid");
    // `url:` pins the captured page's slug. Copied, the instance publishes to
    // the SAME address, the build's slug dedup moves it to `<slug>-2/`, and
    // the preview waits on the path-derived URL that never arrives (seen in
    // the 2026-09-05 log: `測試獎.md` sent to /awards/writing-2/).
    frontmatter.remove("url");
    for claim in ["author_page", "tag_page", "translationKey", "syndicated"] {
        frontmatter.remove(claim);
    }
    if frontmatter.contains_key("date") {
        frontmatter.insert("date".to_string(), Value::String(now.to_string()));
    }
    frontmatter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instantiate_clears_identity_fields_and_stamps_date() {
        let mut fm = HashMap::new();
        for (k, v) in [
            ("title", "Old Title"),
            ("uid", "abc123"),
            ("url", "writing"),
            ("author_page", "guo"),
            ("tag_page", "essays"),
            ("translationKey", "about"),
            ("date", "2020-01-01"),
        ] {
            fm.insert(k.to_string(), Value::String(v.to_string()));
        }
        fm.insert("syndicated".to_string(), Value::Sequence(vec![Value::String("https://m/x".to_string())]));

        let out = instantiate_template_frontmatter(fm, "2026-09-03");

        for identity in ["title", "uid", "url", "author_page", "tag_page", "translationKey", "syndicated"] {
            assert_eq!(out.get(identity), None, "`{identity}` names the captured page, not the instance");
        }
        assert_eq!(out.get("date"), Some(&Value::String("2026-09-03".to_string())));
    }

    #[test]
    fn instantiate_preserves_other_fields_verbatim() {
        let mut fm = HashMap::new();
        fm.insert("layout".to_string(), Value::String("article".to_string()));
        fm.insert("tags".to_string(), Value::Sequence(vec![Value::String("a".to_string())]));
        let mut cascade = serde_yaml::Mapping::new();
        cascade.insert(Value::String("nav".to_string()), Value::Bool(true));
        fm.insert("cascade".to_string(), Value::Mapping(cascade.clone()));

        let out = instantiate_template_frontmatter(fm, "2026-09-03");

        assert_eq!(out.get("layout"), Some(&Value::String("article".to_string())));
        assert_eq!(
            out.get("tags"),
            Some(&Value::Sequence(vec![Value::String("a".to_string())]))
        );
        assert_eq!(out.get("cascade"), Some(&Value::Mapping(cascade)));
    }

    #[test]
    fn instantiate_adds_no_date_when_the_template_has_none() {
        let mut fm = HashMap::new();
        fm.insert("layout".to_string(), Value::String("page".to_string()));
        let out = instantiate_template_frontmatter(fm, "2026-09-03");
        assert_eq!(out.get("date"), None);
    }
}
