//! Property cascade/inheritance between parent and child documents.
//!
//! This module handles frontmatter cascade — parent pages pushing values
//! to descendants.
//!
//! - `apply_cascade()` — propagates cascade key-value pairs from parent to
//!   descendant documents that don't already define those keys.
//! - `apply_cascade_values()` — applies individual cascade fields to a document.
//!
//! Sort dispatch (formerly `sort_by_explicit_order`) moved to
//! `moss_core::sort::sort_by_resolved`, which operates on `ResolvedSort`
//! values populated by the scan pass.

use crate::build::types::ParsedDocument;

/// Every key cascade carries. `apply_cascade_values` has an arm for each one; the
/// test `test_cascade_keys_match_the_arms` holds the two in step.
const CASCADE_KEYS: &[&str] = &[
    "nav",
    "breadcrumb",
    "footer",
    "children",
    "children_style",
    "children_group",
    "children_depth",
    "tags",
    "author",
    "description",
    "content_width",
    "title_block",
    "reader_controls",
    "sort",
    "timezone",
    "syndicate",
];

/// Apply frontmatter cascade: parent pages push values to descendants.
///
/// Documents with a `cascade` field propagate key-value pairs to all descendant
/// documents that don't already define those keys. Each arm only fills a value
/// still unset, so the deepest cascade applies first and the nearest folder wins.
///
/// This runs as a pre-processing pass after all documents are parsed and before
/// HTML generation, modifying the `Vec<ParsedDocument>` in place.
pub(crate) fn apply_cascade(docs: &mut Vec<ParsedDocument>, fields: &[moss_core::fields::FieldDecl]) {
    for doc in docs.iter() {
        if let Some(c) = &doc.cascade {
            report_cascade_values(doc, c, fields);
        }
    }
    let plan = moss_core::page::inherit::cascade_plan(docs.iter().map(|d| (d.url_path.as_str(), d.cascade.is_some())));
    for step in plan {
        let source = &docs[step.source];
        let page = source.source_path.as_deref().unwrap_or(source.url_path.as_str()).to_string();
        let Some(cascade) = source.cascade.clone() else { continue };
        for i in step.targets {
            apply_cascade_values(&mut docs[i], &page, &step.from, &cascade, fields);
        }
    }

    // A cascaded value can change which fields a page draws, so place them once per page, after every cascade has applied.
    if fields.iter().any(|d| d.place.is_some()) {
        for doc in docs.iter_mut() {
            let format = moss_core::fields::PageFormat {
                lang: doc.lang.as_bcp47_attr(),
                decimal: crate::build::components::field_text::decimal_mark(doc.lang),
            };
            // The page's own `date`, which a group's `date` part reads.
            let frontmatter: std::collections::HashMap<String, serde_yaml::Value> =
                doc.date.iter().map(|d| ("date".to_string(), serde_yaml::Value::String(d.clone()))).collect();
            doc.placed_fields = crate::build::components::field_places::placed_for_page(
                fields,
                doc.source_path.as_deref().unwrap_or_default(),
                &doc.title,
                Some(&frontmatter),
                format,
                &doc.declared,
                &doc.field_errors,
                doc.kind == moss_core::PageKind::Folder || doc.slot_only,
            );
        }
    }
}

/// The declared field a cascade key names. A group holds no value, so its name
/// is not a key cascade carries.
fn cascadable_field<'a>(fields: &'a [moss_core::fields::FieldDecl], key: &str) -> Option<&'a moss_core::fields::FieldDecl> {
    fields.iter().find(|d| d.name == key && d.kind != moss_core::fields::FieldKind::Group)
}

/// Report what in a source page's cascade cannot take effect. This runs once per
/// source page, not once per descendant: an unknown key has no effect, and a
/// `timezone:` that is not a zone name is not applied to any event.
fn report_cascade_values(
    doc: &ParsedDocument,
    cascade: &std::collections::BTreeMap<String, serde_yaml::Value>,
    fields: &[moss_core::fields::FieldDecl],
) {
    let page = doc.source_path.as_deref().unwrap_or(doc.url_path.as_str());
    for key in cascade.keys().filter(|k| k.as_str() != "members" && !CASCADE_KEYS.contains(&k.as_str()) && cascadable_field(fields, k.as_str()).is_none()) {
        log::warn!(
            "[{page}] cascade key \"{key}\" is not a key cascade carries ({}) or a declared field; it has no effect",
            CASCADE_KEYS.join(", ")
        );
    }
    if let Some(value) = cascade.get("timezone") {
        // A blank value is absent, as a page's own blank `timezone:` is.
        let text = value.as_str().map(str::trim);
        if text != Some("") && !text.is_some_and(crate::build::feeds::calendar::zone::is_known) {
            let shown = value.as_str().map_or_else(
                || serde_yaml::to_string(value).unwrap_or_default().trim().to_string(),
                str::to_string,
            );
            log::warn!(
                "`timezone:` is not a time zone name (use Area/City), so these events' calendar times are floating: `{shown}` (cascaded from {page})"
            );
        }
    }
    if let Some(value) = cascade.get("syndicate") {
        if let Err(e) = serde_json::to_value(value).map_err(|e| e.to_string()).and_then(|v| crate::syndication::parse_choice(&v)) {
            log::warn!("{e}, so the pages beneath post no copies (cascaded from {page})");
        }
    }
    if let Some(value) = cascade.get("author") {
        if moss_core::frontmatter_union::normalize_name_list(value).is_err() {
            log::warn!("`author:` is not a name or a list of names, so it names no one on the pages beneath (cascaded from {page})");
        }
    }
}

/// Apply individual cascade key-value pairs to a document.
///
/// Only sets values for fields that are `None` on the target document,
/// preserving any explicitly-set frontmatter values.
fn apply_cascade_values(
    doc: &mut ParsedDocument,
    page: &str,
    from: &str,
    cascade: &std::collections::BTreeMap<String, serde_yaml::Value>,
    fields: &[moss_core::fields::FieldDecl],
) {
    for (key, value) in cascade {
        if !CASCADE_KEYS.contains(&key.as_str()) {
            if let Some(decl) = cascadable_field(fields, key) {
                if apply_declared_field(doc, decl, value, page) {
                    record_inherited(doc, key, value, from);
                }
            }
            continue;
        }
        // Each arm says whether it set a value, so `record_inherited` runs once for them all.
        let took = match key.as_str() {
            "nav" => doc.nav.is_none() && serde_yaml::from_value::<bool>(value.clone()).map(|v| doc.nav = Some(v)).is_ok(),
            "breadcrumb" => {
                doc.breadcrumb.is_none()
                    && serde_yaml::from_value::<moss_core::frontmatter_typed::BreadcrumbSetting>(value.clone()).map(|v| doc.breadcrumb = Some(v)).is_ok()
            }
            "footer" => doc.footer.is_none() && serde_yaml::from_value::<bool>(value.clone()).map(|v| doc.footer = Some(v)).is_ok(),
            "children" => doc.children.is_none() && serde_yaml::from_value::<bool>(value.clone()).map(|v| doc.children = Some(v)).is_ok(),
            "children_style" => {
                doc.children_style.is_none()
                    && serde_yaml::from_value::<String>(value.clone()).map(|v| doc.children_style = Some(moss_core::Resolved::cascade(v))).is_ok()
            }
            "children_group" => {
                doc.children_group.is_none()
                    && serde_yaml::from_value::<String>(value.clone()).map(|v| doc.children_group = Some(moss_core::Resolved::cascade(v))).is_ok()
            }
            "children_depth" => doc.children_depth.is_none() && serde_yaml::from_value::<String>(value.clone()).map(|v| doc.children_depth = Some(v)).is_ok(),
            "tags" => {
                doc.tags.is_none()
                    && serde_yaml::from_value::<Vec<String>>(value.clone())
                        .map(|v| {
                            // Cascaded tags are declared metadata (folder
                            // frontmatter), so they also derive term-page
                            // membership — `fm_tags` is what `derive_terms`
                            // reads, and `doc.tags == None` implies it is unset.
                            doc.fm_tags = Some(v.clone());
                            doc.tags = Some(v);
                        })
                        .is_ok()
            }
            "author" => {
                // Read as the page's own `author:` is, so a cascaded byline links to its term page
                // like a written one. A key the page writes, blank or not, keeps its own value.
                doc.author.is_empty()
                    && !doc.raw_frontmatter.contains_key("author")
                    && moss_core::frontmatter_union::normalize_name_list(value).map(|v| doc.author = v).is_ok()
            }
            "description" => doc.description.is_none() && serde_yaml::from_value::<String>(value.clone()).map(|v| doc.description = Some(v)).is_ok(),
            "content_width" => {
                doc.content_width.is_none()
                    && serde_yaml::from_value::<String>(value.clone())
                        .ok()
                        .filter(|v| matches!(v.as_str(), "wide" | "full" | "screen"))
                        .map(|v| doc.content_width = Some(v))
                        .is_some()
            }
            "title_block" => {
                let allowed = moss_core::schema_fields::builtin_field("title_block").and_then(|f| f.enum_values).unwrap_or_default();
                doc.title_block.is_none()
                    && serde_yaml::from_value::<String>(value.clone())
                        .ok()
                        .filter(|v| allowed.contains(&v.as_str()))
                        .map(|v| doc.title_block = Some(v))
                        .is_some()
            }
            "reader_controls" => {
                doc.reader_controls.is_none() && serde_yaml::from_value::<bool>(value.clone()).map(|v| doc.reader_controls = Some(v)).is_ok()
            }
            "sort" => {
                doc.sort.is_none() && serde_yaml::from_value::<moss_core::sort::SortField>(value.clone()).map(|v| doc.sort = Some(v)).is_ok()
            }
            "syndicate" => {
                // A page's own `syndicate:`, even an invalid one, is the page's say.
                doc.syndicate.is_none()
                    && !doc.raw_frontmatter.contains_key("syndicate")
                    && serde_json::to_value(value)
                        .map_err(|e| e.to_string())
                        .and_then(|v| crate::syndication::parse_choice(&v))
                        .map(|v| doc.syndicate = Some(v))
                        .is_ok()
            }
            "timezone" => {
                // Only an event that names no zone of its own takes the folder's, and
                // only a zone name; `report_cascade_values` reports the rest.
                doc.event.as_mut().filter(|e| e.timezone.is_none()).is_some_and(|event| {
                    serde_yaml::from_value::<String>(value.clone()).ok().filter(|v| crate::build::feeds::calendar::zone::is_known(v.trim())).map(|v| event.timezone = Some(v.trim().to_string())).is_some()
                })
            }
            // `typesetting` is intentionally excluded from cascade — it's a site-level
            // setting (config.toml) applied globally via LayoutConfig, not something
            // that should propagate through the page hierarchy.
            // `sidebar` is intentionally excluded from cascade — it's a per-page
            // declaration of which folder feeds the sidebar, not a property that
            // should inherit from parent to children.
            // `draft` and `listed` are intentionally excluded from cascade —
            // same policy: visibility is a per-page decision, not inherited.
            // (If folder-level off-feed is wanted later, add an arm here.)
            _ => false, // Ignore unknown cascade keys
        };
        if took {
            record_inherited(doc, key, value, from);
        }
    }
}

/// Note a value the page took from the folder at `from`, for the inventory.
fn record_inherited(doc: &mut ParsedDocument, key: &str, value: &serde_yaml::Value, from: &str) {
    if let Ok(value) = serde_json::to_value(value) {
        doc.inherited.push(crate::build::emit::inventory::Inherited { name: key.to_string(), value, from: from.to_string() });
    }
}

/// Carry a cascaded declared field to one descendant, checked as the page's own
/// value is. A value the page writes itself, blank or not, wins. So does a value
/// an outer folder states, even one that does not fit, so a nested folder's value
/// fills in only where no folder above has stated the field.
fn apply_declared_field(doc: &mut ParsedDocument, decl: &moss_core::fields::FieldDecl, value: &serde_yaml::Value, page: &str) -> bool {
    if doc.declared_written.contains(&decl.name) || doc.declared.contains_key(&decl.name) || doc.field_errors.contains_key(&decl.name) {
        return false;
    }
    let file = doc.source_path.clone().unwrap_or_default();
    let lang = doc.lang;
    let format = moss_core::fields::PageFormat {
        lang: lang.as_bcp47_attr(),
        decimal: crate::build::components::field_text::decimal_mark(lang),
    };
    match moss_core::fields::declare_one(decl, &file, Some(value), format) {
        Some(Ok(typed)) => {
            doc.declared.insert(decl.name.clone(), typed);
            true
        }
        Some(Err(reason)) => {
            crate::build::cli_output::cli_warn!("[{file}] frontmatter: field '{}': {reason} (cascaded from {page})", decl.name);
            doc.field_errors.insert(decl.name.clone(), reason);
            false
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;
    use moss_core::PageKind;
    use std::collections::BTreeMap as HashMap;

    /// Helper to create a minimal ParsedDocument for cascade testing.
    fn make_doc(title: &str, url_path: &str) -> ParsedDocument {
        ParsedDocument {
            title: title.to_string(),
            label: title.to_string(),
            url_path: url_path.to_string(),
            reading_time: 1,
            slug: title.to_lowercase().replace(' ', "-"),
            permalink: format!("/{}", url_path), // allow:served-path-url-construct (test fixture — permalink field, not HTML-emitted URL)
            lang: Language::En,
            kind: PageKind::Article,
            ..Default::default()
        }
    }

    /// Basic cascade: parent with `cascade: { nav: true }` propagates to descendants.
    #[test]
    fn test_basic_cascade_nav() {
        let mut parent = make_doc("Docs", "docs/index.html");
        let mut cascade = HashMap::new();
        cascade.insert(
            "nav".to_string(),
            serde_yaml::to_value(true).unwrap(),
        );
        parent.cascade = Some(cascade);

        let child = make_doc("Getting Started", "docs/getting-started/index.html");
        let grandchild = make_doc("Install", "docs/getting-started/install/index.html");

        let mut docs = vec![parent, child, grandchild];
        apply_cascade(&mut docs, &[]);

        // Child should inherit nav: true
        assert_eq!(
            docs[1].nav,
            Some(true),
            "Child should inherit nav: true from parent cascade"
        );
        // Grandchild should inherit nav: true
        assert_eq!(
            docs[2].nav,
            Some(true),
            "Grandchild should inherit nav: true from parent cascade"
        );
    }

    /// A page names what it took from a folder and from which, and nothing it set itself.
    #[test]
    fn a_page_records_the_values_it_took_and_the_folder_they_came_from() {
        let mut posts = make_doc("Posts", "posts/index.html");
        posts.cascade = Some(HashMap::from([
            ("nav".to_string(), serde_yaml::to_value(false).unwrap()),
            ("description".to_string(), serde_yaml::to_value("From posts").unwrap()),
            ("content_width".to_string(), serde_yaml::to_value("wide").unwrap()),
        ]));
        let mut heron = make_doc("Heron", "posts/heron/index.html");
        heron.description = Some("Its own".to_string());

        let mut docs = vec![posts, heron, make_doc("About", "about/index.html")];
        apply_cascade(&mut docs, &[]);

        assert_eq!(
            docs[1].inherited,
            vec![
                crate::build::emit::inventory::Inherited { name: "content_width".into(), value: serde_json::json!("wide"), from: "/posts/".into() },
                crate::build::emit::inventory::Inherited { name: "nav".into(), value: serde_json::json!(false), from: "/posts/".into() },
            ]
        );
        assert!(docs[0].inherited.is_empty() && docs[2].inherited.is_empty());
    }

    /// Descendant with explicit value is NOT overridden by cascade.
    #[test]
    fn test_cascade_does_not_override_explicit_value() {
        let mut parent = make_doc("Docs", "docs/index.html");
        let mut cascade = HashMap::new();
        cascade.insert(
            "nav".to_string(),
            serde_yaml::to_value(true).unwrap(),
        );
        parent.cascade = Some(cascade);

        let mut child = make_doc("Hidden Page", "docs/hidden/index.html");
        child.nav = Some(false); // Explicitly opt out

        let mut docs = vec![parent, child];
        apply_cascade(&mut docs, &[]);

        assert_eq!(
            docs[1].nav,
            Some(false),
            "Explicit nav: false should not be overridden by cascade"
        );
    }

    /// Cascade does NOT modify the source document itself.
    #[test]
    fn test_cascade_does_not_modify_source() {
        let mut parent = make_doc("Docs", "docs/index.html");
        parent.nav = None; // Source has no explicit nav
        let mut cascade = HashMap::new();
        cascade.insert(
            "nav".to_string(),
            serde_yaml::to_value(true).unwrap(),
        );
        parent.cascade = Some(cascade);

        let child = make_doc("Child", "docs/child/index.html");

        let mut docs = vec![parent, child];
        apply_cascade(&mut docs, &[]);

        // Source document should NOT get its own cascade applied
        assert_eq!(
            docs[0].nav,
            None,
            "Cascade source should not have cascade values applied to itself"
        );
    }

    /// Deeper cascade overrides shallower cascade for the same key.
    ///
    /// root cascade: children_style = "list"
    /// docs/ cascade: children_style = "summary"
    /// The nearer folder wins for the page; docs/ itself takes its parent's.
    #[test]
    fn test_deeper_cascade_overrides_shallower() {
        let mut root = make_doc("Home", "index.html");
        root.cascade = Some(HashMap::from([("children_style".to_string(), serde_yaml::to_value("list").unwrap())]));

        let mut folder = make_doc("Docs", "docs/index.html");
        folder.cascade = Some(HashMap::from([("children_style".to_string(), serde_yaml::to_value("summary").unwrap())]));

        let child = make_doc("Page", "docs/page/index.html");

        let mut docs = vec![root, folder, child];
        apply_cascade(&mut docs, &[]);

        assert_eq!(docs[2].children_style, Some(moss_core::Resolved::cascade("summary".to_string())));
        assert_eq!(docs[1].children_style, Some(moss_core::Resolved::cascade("list".to_string())));
    }

    /// Three-level hierarchy: root → folder → subfolder.
    /// Each level cascades different fields.
    #[test]
    fn test_three_level_cascade_different_fields() {
        let mut root = make_doc("Home", "index.html");
        let mut root_cascade = HashMap::new();
        root_cascade.insert(
            "breadcrumb".to_string(),
            serde_yaml::to_value(true).unwrap(),
        );
        root.cascade = Some(root_cascade);

        let mut docs_index = make_doc("Docs", "docs/index.html");
        let mut docs_cascade = HashMap::new();
        docs_cascade.insert(
            "children_style".to_string(),
            serde_yaml::to_value("summary").unwrap(),
        );
        docs_index.cascade = Some(docs_cascade);

        let child = make_doc("Guide", "docs/guide/index.html");

        let mut docs = vec![root, docs_index, child];
        apply_cascade(&mut docs, &[]);

        // Guide should inherit breadcrumb from root and children_style from docs/
        assert_eq!(
            docs[2].breadcrumb,
            Some(moss_core::frontmatter_typed::BreadcrumbSetting::On),
            "Guide should inherit breadcrumb: true from root cascade"
        );
        assert_eq!(
            docs[2].children_style,
            Some(moss_core::Resolved::cascade("summary".to_string())),
            "Guide should inherit children_style: summary from docs/ cascade"
        );
    }

    /// Cascade applies all supported field types.
    #[test]
    fn test_cascade_all_supported_fields() {
        let mut parent = make_doc("Docs", "docs/index.html");
        let mut cascade = HashMap::new();
        cascade.insert("nav".to_string(), serde_yaml::to_value(true).unwrap());
        cascade.insert("breadcrumb".to_string(), serde_yaml::to_value(false).unwrap());
        cascade.insert("children".to_string(), serde_yaml::to_value(true).unwrap());
        cascade.insert("children_style".to_string(), serde_yaml::to_value("summary").unwrap());
        cascade.insert("children_depth".to_string(), serde_yaml::to_value("all").unwrap());
        cascade.insert("tags".to_string(), serde_yaml::to_value(vec!["docs", "guide"]).unwrap());
        cascade.insert("description".to_string(), serde_yaml::to_value("Inherited description").unwrap());
        parent.cascade = Some(cascade);

        let child = make_doc("Page", "docs/page/index.html");

        let mut docs = vec![parent, child];
        apply_cascade(&mut docs, &[]);

        assert_eq!(docs[1].nav, Some(true));
        assert_eq!(docs[1].breadcrumb, Some(moss_core::frontmatter_typed::BreadcrumbSetting::Off));
        assert_eq!(docs[1].children, Some(true));
        assert_eq!(docs[1].children_style, Some(moss_core::Resolved::cascade("summary".to_string())));
        assert_eq!(docs[1].children_depth, Some("all".to_string()));
        assert_eq!(docs[1].tags, Some(vec!["docs".to_string(), "guide".to_string()]));
        assert_eq!(docs[1].description, Some("Inherited description".to_string()));
    }

    /// Cascade does NOT apply to sibling folders (only descendants).
    #[test]
    fn test_cascade_does_not_apply_to_siblings() {
        let mut docs_index = make_doc("Docs", "docs/index.html");
        let mut cascade = HashMap::new();
        cascade.insert("nav".to_string(), serde_yaml::to_value(true).unwrap());
        docs_index.cascade = Some(cascade);

        let blog_page = make_doc("Blog Post", "blog/post/index.html");

        let mut docs = vec![docs_index, blog_page];
        apply_cascade(&mut docs, &[]);

        assert_eq!(
            docs[1].nav,
            None,
            "Blog page should NOT inherit cascade from docs/ (sibling folder)"
        );
    }

    /// Root cascade applies to all pages.
    #[test]
    fn test_root_cascade_applies_globally() {
        let mut root = make_doc("Home", "index.html");
        let mut cascade = HashMap::new();
        cascade.insert(
            "breadcrumb".to_string(),
            serde_yaml::to_value(true).unwrap(),
        );
        root.cascade = Some(cascade);

        let page_a = make_doc("About", "about/index.html");
        let page_b = make_doc("Guide", "docs/guide/index.html");

        let mut docs = vec![root, page_a, page_b];
        apply_cascade(&mut docs, &[]);

        assert_eq!(
            docs[1].breadcrumb,
            Some(moss_core::frontmatter_typed::BreadcrumbSetting::On),
            "About should inherit breadcrumb from root cascade"
        );
        assert_eq!(
            docs[2].breadcrumb,
            Some(moss_core::frontmatter_typed::BreadcrumbSetting::On),
            "Docs/guide should inherit breadcrumb from root cascade"
        );
        // Root itself should NOT be modified
        assert_eq!(
            docs[0].breadcrumb,
            None,
            "Root should not inherit its own cascade"
        );
    }

    /// Non-index documents with cascade are ignored (they have no descendants).
    #[test]
    fn test_non_index_cascade_is_ignored() {
        let mut leaf = make_doc("Leaf Page", "docs/leaf/index.html");
        // This is actually an index file, so let's use a non-index path
        let mut non_index = make_doc("Article", "docs/article.html");
        let mut cascade = HashMap::new();
        cascade.insert("nav".to_string(), serde_yaml::to_value(true).unwrap());
        non_index.cascade = Some(cascade);

        leaf.nav = None;

        let mut docs = vec![non_index, leaf];
        apply_cascade(&mut docs, &[]);

        assert_eq!(
            docs[1].nav,
            None,
            "Non-index cascade source should not propagate to other docs"
        );
    }

    /// An unknown cascade key has no effect, and the build reports it once per source page.
    #[test]
    fn test_unknown_cascade_key_has_no_effect_and_is_reported() {
        // The first call installs the capturing logger, so it must run before the warning.
        crate::build::cache::tests::logged_count(log::Level::Warn, "");
        let mut parent = make_doc("Docs", "docs/index.html");
        let mut cascade = HashMap::new();
        cascade.insert(
            "unknown_field".to_string(),
            serde_yaml::to_value("some value").unwrap(),
        );
        cascade.insert(
            "nav".to_string(),
            serde_yaml::to_value(true).unwrap(),
        );
        parent.cascade = Some(cascade);

        let child = make_doc("Page", "docs/page/index.html");

        let mut docs = vec![parent, child];
        apply_cascade(&mut docs, &[]);

        // Known key should still work
        assert_eq!(docs[1].nav, Some(true));
        // The unknown key is not applied to any field...
        assert_eq!(docs[1].description, None);
        // ...and is reported, not dropped without a word.
        assert_eq!(
            crate::build::cache::tests::logged_count(log::Level::Warn, "cascade key \"unknown_field\""),
            1
        );
    }

    /// A cascaded `author:` that is not a name or a list of names is reported once, as a page's own is not applied.
    #[test]
    fn a_cascaded_author_that_is_not_a_name_is_reported() {
        // The first call installs the capturing logger, so it must run before the warning.
        crate::build::cache::tests::logged_count(log::Level::Warn, "");
        let mut parent = make_doc("Prints", "prints/index.html");
        let mut mapping = serde_yaml::Mapping::new();
        mapping.insert(serde_yaml::Value::String("first".into()), serde_yaml::Value::String("Ada".into()));
        parent.cascade = Some(HashMap::from([("author".to_string(), serde_yaml::Value::Mapping(mapping))]));
        let child = make_doc("Print", "prints/one/index.html");
        let mut docs = vec![parent, child];
        apply_cascade(&mut docs, &[]);
        assert!(docs[1].author.is_empty());
        assert_eq!(
            crate::build::cache::tests::logged_count(log::Level::Warn, "`author:` is not a name or a list of names"),
            1
        );
    }

    /// A valid sample value for each key in `CASCADE_KEYS`.
    fn sample_for(key: &str) -> serde_yaml::Value {
        match key {
            "nav" | "footer" | "children" => serde_yaml::Value::Bool(true),
            "breadcrumb" => serde_yaml::Value::Bool(false),
            "children_style" | "children_group" | "children_depth" | "description" => {
                serde_yaml::Value::String("summary".to_string())
            }
            "tags" => serde_yaml::to_value(vec!["docs"]).unwrap(),
            "author" => serde_yaml::Value::String("Ada Example".to_string()),
            "content_width" => serde_yaml::Value::String("wide".to_string()),
            "title_block" => serde_yaml::Value::String("beside-picture".to_string()),
            "reader_controls" => serde_yaml::Value::Bool(false),
            "sort" => serde_yaml::Value::String("title".to_string()),
            "timezone" => serde_yaml::Value::String("Europe/Oslo".to_string()),
            "syndicate" => serde_yaml::to_value(vec!["example-network"]).unwrap(),
            other => panic!("CASCADE_KEYS names `{other}` but the test has no sample value for it"),
        }
    }

    /// Cascading a key changes an event page exactly when the key is in `CASCADE_KEYS`.
    #[test]
    fn test_cascade_keys_match_the_arms() {
        for key in CASCADE_KEYS {
            let mut parent = make_doc("Events", "events/index.html");
            parent.cascade = Some(HashMap::from([(key.to_string(), sample_for(key))]));
            let mut docs = vec![parent, make_event_doc("Concert", "events/concert/index.html")];
            let before = format!("{:?}", docs[1]);
            apply_cascade(&mut docs, &[]);
            assert_ne!(format!("{:?}", docs[1]), before, "`{key}` is in CASCADE_KEYS but changes nothing");
        }
        let declared = [moss_core::fields::FieldDecl {
            name: "kind".into(),
            kind: moss_core::fields::FieldKind::Text,
            ..Default::default()
        }];
        // Keys cascade does not carry, including the ones excluded on purpose, change nothing.
        for key in ["colour", "typesetting", "sidebar", "draft", "listed", "start"] {
            for value in [serde_yaml::Value::Bool(true), serde_yaml::Value::String("wide".to_string())] {
                let mut parent = make_doc("Events", "events/index.html");
                parent.cascade = Some(HashMap::from([(key.to_string(), value)]));
                let mut docs = vec![parent, make_event_doc("Concert", "events/concert/index.html")];
                let before = format!("{:?}", docs[1]);
                apply_cascade(&mut docs, &declared);
                assert_eq!(format!("{:?}", docs[1]), before, "`{key}` is not in CASCADE_KEYS but changes the page");
            }
        }
        // A declared field name is carried as well.
        let mut parent = make_doc("Events", "events/index.html");
        parent.cascade = Some(HashMap::from([("kind".to_string(), serde_yaml::Value::String("Building".to_string()))]));
        let mut docs = vec![parent, make_event_doc("Concert", "events/concert/index.html")];
        apply_cascade(&mut docs, &declared);
        assert!(docs[1].declared.contains_key("kind"), "a declared field cascades to the page beneath it");
    }

    #[test]
    fn a_folder_names_channels_for_its_posts_and_a_post_can_say_otherwise() {
        let mut parent = make_doc("Notes", "notes/index.html");
        parent.cascade = Some(HashMap::from([("syndicate".to_string(), serde_yaml::to_value(vec!["bluesky", "mastodon"]).unwrap())]));
        let inherits = make_doc("A", "notes/a/index.html");
        let mut opts_out = make_doc("B", "notes/b/index.html");
        opts_out.raw_frontmatter.insert("syndicate".into(), serde_json::json!(false));
        opts_out.syndicate = Some(vec![]);
        let mut invalid = make_doc("C", "notes/c/index.html");
        invalid.raw_frontmatter.insert("syndicate".into(), serde_json::json!(3));
        let elsewhere = make_doc("D", "other/d/index.html");

        let mut docs = vec![parent, inherits, opts_out, invalid, elsewhere];
        apply_cascade(&mut docs, &[]);

        assert_eq!(docs[1].syndicate.as_deref(), Some(&["bluesky".to_string(), "mastodon".to_string()][..]));
        assert_eq!(docs[2].syndicate.as_deref(), Some(&[][..]), "`syndicate: false` stops the folder's list");
        assert_eq!(docs[3].syndicate, None, "a page's own value, even an invalid one, is not replaced by the folder's");
        assert_eq!(docs[4].syndicate, None);
    }

    /// Empty cascade map is harmless.
    #[test]
    fn test_empty_cascade() {
        let mut parent = make_doc("Docs", "docs/index.html");
        parent.cascade = Some(HashMap::new());

        let child = make_doc("Page", "docs/page/index.html");

        let mut docs = vec![parent, child];
        apply_cascade(&mut docs, &[]);

        // No fields should be modified
        assert_eq!(docs[1].nav, None);
        assert_eq!(docs[1].breadcrumb, None);
    }

    /// Documents with no cascade at all — function should be a no-op.
    #[test]
    fn test_no_cascade_is_noop() {
        let doc_a = make_doc("Page A", "a/index.html");
        let doc_b = make_doc("Page B", "b/index.html");

        let mut docs = vec![doc_a, doc_b];
        let nav_before = (docs[0].nav, docs[1].nav);
        apply_cascade(&mut docs, &[]);

        assert_eq!((docs[0].nav, docs[1].nav), nav_before);
    }

    /// Cascade propagates content_width to descendants.
    #[test]
    fn test_cascade_content_width() {
        let mut parent = make_doc("Docs", "docs/index.html");
        let mut cascade = HashMap::new();
        cascade.insert(
            "content_width".to_string(),
            serde_yaml::to_value("wide").unwrap(),
        );
        parent.cascade = Some(cascade);

        let child = make_doc("Page", "docs/page/index.html");

        let mut docs = vec![parent, child];
        apply_cascade(&mut docs, &[]);

        assert_eq!(
            docs[1].content_width.as_deref(),
            Some("wide"),
            "Child should inherit content_width: wide from parent cascade"
        );
    }

    /// Explicit content_width on child is not overridden by cascade.
    #[test]
    fn test_cascade_content_width_explicit_not_overridden() {
        let mut parent = make_doc("Docs", "docs/index.html");
        let mut cascade = HashMap::new();
        cascade.insert(
            "content_width".to_string(),
            serde_yaml::to_value("full").unwrap(),
        );
        parent.cascade = Some(cascade);

        let mut child = make_doc("Page", "docs/page/index.html");
        child.content_width = Some("wide".to_string());

        let mut docs = vec![parent, child];
        apply_cascade(&mut docs, &[]);

        assert_eq!(
            docs[1].content_width.as_deref(),
            Some("wide"),
            "Explicit content_width should not be overridden by cascade"
        );
    }

    /// Cascade propagates sort field to descendants.
    #[test]
    fn test_cascade_sort() {
        use moss_core::sort::SortField;
        let mut parent = make_doc("Posts", "posts/index.html");
        let mut cascade = HashMap::new();
        cascade.insert(
            "sort".to_string(),
            serde_yaml::to_value("title").unwrap(),
        );
        parent.cascade = Some(cascade);

        let child = make_doc("Post", "posts/post/index.html");

        let mut docs = vec![parent, child];
        apply_cascade(&mut docs, &[]);

        assert!(
            docs[1].sort.is_some(),
            "Child should inherit sort from parent cascade"
        );
    }

    /// Explicit sort on child is not overridden by cascade.
    #[test]
    fn test_cascade_sort_explicit_not_overridden() {
        use moss_core::sort::{SortAxis, SortField};
        let mut parent = make_doc("Posts", "posts/index.html");
        let mut cascade = HashMap::new();
        cascade.insert(
            "sort".to_string(),
            serde_yaml::to_value("title").unwrap(),
        );
        parent.cascade = Some(cascade);

        let mut child = make_doc("Archive", "posts/archive/index.html");
        child.sort = Some(SortField::Axis(SortAxis::Date));

        let mut docs = vec![parent, child];
        apply_cascade(&mut docs, &[]);

        // Child should keep its explicit sort, not inherit from parent
        assert!(
            docs[1].sort.is_some(),
            "Child should still have sort set"
        );
    }

    /// Invalid content_width values in cascade are ignored.
    #[test]
    fn test_cascade_content_width_invalid_ignored() {
        let mut parent = make_doc("Docs", "docs/index.html");
        let mut cascade = HashMap::new();
        cascade.insert(
            "content_width".to_string(),
            serde_yaml::to_value("banana").unwrap(),
        );
        parent.cascade = Some(cascade);

        let child = make_doc("Page", "docs/page/index.html");

        let mut docs = vec![parent, child];
        apply_cascade(&mut docs, &[]);

        assert_eq!(
            docs[1].content_width,
            None,
            "Invalid content_width value should be ignored"
        );
    }

    /// An event page: `start:` parsed into `event`, no zone of its own.
    fn make_event_doc(title: &str, url_path: &str) -> ParsedDocument {
        let mut doc = make_doc(title, url_path);
        doc.event = crate::build::types::EventFields::from_frontmatter(
            &moss_core::frontmatter_typed::FrontMatter {
                start: Some("2026-11-01 19:00".to_string()),
                ..Default::default()
            },
        );
        doc
    }

    fn folder_with_timezone(zone: &str) -> ParsedDocument {
        let mut parent = make_doc("Events", "events/index.html");
        let mut cascade = HashMap::new();
        cascade.insert("timezone".to_string(), serde_yaml::to_value(zone).unwrap());
        parent.cascade = Some(cascade);
        parent
    }

    /// A folder's `timezone:` cascade fills in the zone of each event beneath it.
    #[test]
    fn test_cascade_timezone_sets_event_zone() {
        let mut docs = vec![
            folder_with_timezone("Europe/Oslo"),
            make_event_doc("Concert", "events/concert/index.html"),
        ];
        apply_cascade(&mut docs, &[]);

        assert_eq!(
            docs[1].event.as_ref().and_then(|e| e.timezone.as_deref()),
            Some("Europe/Oslo"),
        );
    }

    /// A zone the event sets itself is kept over the folder's.
    #[test]
    fn test_cascade_timezone_does_not_override_own_zone() {
        let mut own = make_event_doc("Tour", "events/tour/index.html");
        own.event.as_mut().unwrap().timezone = Some("Asia/Tokyo".to_string());

        let mut docs = vec![folder_with_timezone("Europe/Oslo"), own];
        apply_cascade(&mut docs, &[]);

        assert_eq!(
            docs[1].event.as_ref().and_then(|e| e.timezone.as_deref()),
            Some("Asia/Tokyo"),
        );
    }

    /// A descendant that is not an event stays an event-less page.
    #[test]
    fn test_cascade_timezone_leaves_non_event_pages_alone() {
        let mut docs = vec![
            folder_with_timezone("Europe/Oslo"),
            make_doc("About", "events/about/index.html"),
        ];
        apply_cascade(&mut docs, &[]);

        assert!(docs[1].event.is_none());
    }

    /// A blank cascaded zone counts as absent: not applied, and not reported.
    #[test]
    fn test_cascade_timezone_blank_counts_as_absent() {
        crate::build::cache::tests::logged_count(log::Level::Warn, "");
        let mut docs = vec![
            folder_with_timezone("   "),
            make_event_doc("Concert", "events/concert/index.html"),
        ];
        apply_cascade(&mut docs, &[]);

        assert_eq!(docs[1].event.as_ref().and_then(|e| e.timezone.as_deref()), None);
        assert_eq!(crate::build::cache::tests::logged_count(log::Level::Warn, "`   ` (cascaded from"), 0);
    }

    /// A cascaded zone that is not a zone name is not applied, and is reported with its page.
    #[test]
    fn test_cascade_timezone_malformed_is_not_applied_and_reported() {
        crate::build::cache::tests::logged_count(log::Level::Warn, "");
        let mut docs = vec![
            folder_with_timezone("not a zone"),
            make_event_doc("Concert", "events/concert/index.html"),
        ];
        apply_cascade(&mut docs, &[]);

        assert_eq!(docs[1].event.as_ref().and_then(|e| e.timezone.as_deref()), None);
        assert_eq!(
            crate::build::cache::tests::logged_count(log::Level::Warn, "`not a zone` (cascaded from events/index.html)"),
            1
        );
    }
}
