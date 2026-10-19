//! Pure logic for the page-templates feature: what kind a captured page is,
//! and how a template's frontmatter is reset when it's instantiated into a
//! new page.
//!
//! Template storage and file I/O live in the callers (`moss_build` and the
//! desktop app's `.moss/templates/` commands) — this module never touches the
//! filesystem. Frontmatter here is the same untyped
//! `HashMap<String, serde_yaml::Value>` `crate::frontmatter` reads and writes,
//! not the vault's typed `FrontMatter`/`BUILTIN_FIELDS` schema: a template's
//! fields are copied verbatim, including ones the schema doesn't know about,
//! so round-tripping through the typed struct (which has no catch-all field)
//! would silently drop them.

use std::collections::{BTreeSet, HashMap};
use serde_yaml::Value;

use crate::frontmatter::{self, FieldEdit};
use crate::schema_fields;

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
/// comments/redirects join key), `url` (the pinned address), every term-page
/// claim (`schema_fields::term_claim_fields()` — `author_page` / `tag_page` /
/// `editor_page` / `jury_page` / `place_page` today; two claimants resolve to
/// the first `url_path`, so a copy could steal the original's term page),
/// `translationKey` (a copy
/// makes the pair "one page's translations" and links them), `syndicated`
/// (where the captured page was published, written by the matters plugin),
/// and `origin` (where the captured page was imported from — provenance of
/// that one specific page, not of a new page made from its template). Every
/// other field — layout, tags, cascade, and anything else the template
/// carries — is copied verbatim, since that's the point of templating them.
///
/// A template without a `date` produces an instance without one (2026-09-05,
/// user report): the captured page's author chose not to date it, and a
/// dateless page is how moss renders an undated section or note; inventing
/// the field would put a date line on every page made from that template.
///
/// `declared_claim_keys` are the site's own `<field>_page:` keys, one per
/// declared name-list field (`claim_field_key` over `declared_name_list_fields`).
/// Callers must pass them; the built-in claims are always dropped.
///
/// `now` is a caller-supplied `YYYY-MM-DD` string (moss-core has no `chrono`
/// dependency by convention — see `date.rs`) so this stays pure and
/// deterministic; the Tauri I/O boundary reads the clock, not this function.
pub fn instantiate_template_frontmatter(
    mut frontmatter: HashMap<String, Value>,
    now: &str,
    declared_claim_keys: &[String],
) -> HashMap<String, Value> {
    frontmatter.remove("title");
    frontmatter.remove("uid");
    // `url:` pins the captured page's slug. Copied, the instance publishes to
    // the SAME address, the build's slug dedup moves it to `<slug>-2/`, and
    // the preview waits on the path-derived URL that never arrives (seen in
    // the 2026-09-05 log: `測試獎.md` sent to /awards/writing-2/).
    frontmatter.remove("url");
    for claim in schema_fields::term_claim_fields().chain(["translationKey", "syndicated", "origin"]) {
        frontmatter.remove(claim);
    }
    for claim in declared_claim_keys {
        frontmatter.remove(claim);
    }
    if frontmatter.contains_key("date") {
        frontmatter.insert("date".to_string(), Value::String(now.to_string()));
    }
    frontmatter
}

/// The site's languages: the default language, and the language trees (folders
/// named by a language code) beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteLanguages {
    pub default_lang: String,
    /// Folder names of the non-default language trees, e.g. `["zh-hans"]`.
    pub trees: Vec<String>,
}

/// One language's file of a template. `lang` is `None` for the default language.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateFile {
    pub lang: Option<String>,
    pub frontmatter: HashMap<String, Value>,
    pub body: String,
}

/// A saved template with all its language files.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub kind: TemplateKind,
    /// The folder its pages belong in, as written in the template.
    pub folder: Option<String>,
    pub files: Vec<TemplateFile>,
}

/// A page to write for a template.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedPage {
    /// Site-relative directory ending in `/`, or `""` for the site root.
    pub dir: String,
    pub frontmatter: HashMap<String, Value>,
    pub body: String,
}

/// The other language's version of a page.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedTranslation {
    pub path: String,
    pub content: String,
    /// Edits to apply to the original page. Empty when it already has a key.
    pub original_edits: Vec<FieldEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The folder a template names does not exist in the target tree.
    FolderMissing(String),
    LanguageNotInSite(String),
    /// The page already has a version in that language.
    AlreadyTranslated(String),
    /// The original's frontmatter cannot take the translation key.
    OriginalNotEditable(String),
    NoTemplateFile(String),
    Serialize(String),
}

/// A folder as written in `_template_folder` (`work`, `work/`, `/work/`,
/// `./work`), as `work/`. The site root is `""`.
pub fn normalise_folder(raw: &str) -> String {
    let mut s = raw.trim();
    while let Some(rest) = s.strip_prefix("./").or_else(|| s.strip_prefix('/')) {
        s = rest;
    }
    let s = s.trim_end_matches('/');
    if s.is_empty() || s == "." {
        String::new()
    } else {
        format!("{s}/")
    }
}

/// Splits a site-relative path into its path inside the default-language tree
/// and the language tree it sits in.
pub fn folder_in_default_tree<'a>(path: &'a str, langs: &SiteLanguages) -> (&'a str, Option<&'a str>) {
    if let Some((first, rest)) = path.split_once('/') {
        if langs.trees.iter().any(|tree| tree == first) {
            return (rest, Some(first));
        }
    }
    (path, None)
}

/// The page a template makes, in its folder when it names one, else in
/// `anchor_dir`. `lang` picks the language file and the tree the folder is
/// made in.
pub fn plan_page_from_template(
    template: &Template,
    lang: Option<&str>,
    anchor_dir: &str,
    langs: &SiteLanguages,
    existing_dirs: &BTreeSet<String>,
    now: &str,
    declared_claim_keys: &[String],
) -> Result<PlannedPage, PlanError> {
    let lang = lang.filter(|l| *l != langs.default_lang);
    if let Some(l) = lang {
        if !langs.trees.iter().any(|tree| tree == l) {
            return Err(PlanError::LanguageNotInSite(l.to_string()));
        }
    }
    let file = template
        .files
        .iter()
        .find(|f| f.lang.as_deref() == lang)
        .or_else(|| template.files.iter().find(|f| f.lang.is_none()))
        .ok_or_else(|| PlanError::NoTemplateFile(template.id.clone()))?;
    let dir = match template.folder.as_deref().map(normalise_folder).filter(|f| !f.is_empty()) {
        Some(folder) => {
            let dir = match lang {
                Some(l) => format!("{l}/{folder}"),
                None => folder,
            };
            if !existing_dirs.contains(&dir) {
                return Err(PlanError::FolderMissing(dir));
            }
            dir
        }
        None => normalise_folder(anchor_dir),
    };
    let mut frontmatter = file.frontmatter.clone();
    frontmatter.retain(|key, _| !key.starts_with("_template_"));
    Ok(PlannedPage {
        dir,
        frontmatter: instantiate_template_frontmatter(frontmatter, now, declared_claim_keys),
        body: file.body.clone(),
    })
}

/// Fields that describe one language's text or name this page alone; a
/// translation does not copy them.
const LANGUAGE_FIELDS: [&str; 8] = ["title", "description", "lang", "uid", "url", "origin", "syndicated", "translationKey"];

/// The other language's version of `source_path`, at the mirrored path in
/// `target_lang`. Its key links it to the original, and the original gains
/// that key when it had none.
#[allow(clippy::too_many_arguments)]
pub fn plan_translation(
    source_path: &str,
    source_text: &str,
    target_lang: &str,
    langs: &SiteLanguages,
    existing_paths: &BTreeSet<String>,
    target_keys: &BTreeSet<String>,
    all_keys: &BTreeSet<String>,
    template_file: Option<&TemplateFile>,
    declared_claim_keys: &[String],
) -> Result<PlannedTranslation, PlanError> {
    let target = (target_lang != langs.default_lang).then_some(target_lang);
    if let Some(tree) = target {
        if !langs.trees.iter().any(|t| t == tree) {
            return Err(PlanError::LanguageNotInSite(tree.to_string()));
        }
    }
    let (rel, source_tree) = folder_in_default_tree(source_path, langs);
    let path = match target {
        Some(tree) => format!("{tree}/{rel}"),
        None => rel.to_string(),
    };
    if source_tree == target || existing_paths.contains(&path) {
        return Err(PlanError::AlreadyTranslated(path));
    }

    let source = frontmatter::parse(source_text);
    let mut original_edits = Vec::new();
    let key = match source.frontmatter.get("translationKey").and_then(frontmatter::value_as_string).filter(|k| !k.is_empty()) {
        Some(key) => key,
        None => {
            let key = unique_key(file_stem(rel), all_keys);
            original_edits.push(FieldEdit::Set { key: "translationKey".to_string(), value: Value::String(key.clone()) });
            key
        }
    };
    if target_keys.contains(&key) {
        return Err(PlanError::AlreadyTranslated(path));
    }
    if !original_edits.is_empty() {
        frontmatter::edit_fields(source_text, &original_edits).map_err(PlanError::OriginalNotEditable)?;
    }

    let mut frontmatter = template_file.map(|f| f.frontmatter.clone()).unwrap_or_default();
    frontmatter.retain(|k, _| !k.starts_with("_template_"));
    for (k, v) in source.frontmatter {
        if !is_language_bound(&k, declared_claim_keys) {
            frontmatter.insert(k, v);
        }
    }
    frontmatter.insert("lang".to_string(), Value::String(target_lang.to_string()));
    frontmatter.insert("translationKey".to_string(), Value::String(key));
    let body = template_file.map(|f| f.body.clone()).unwrap_or_default();
    let content = frontmatter::serialize(&frontmatter, &body).map_err(PlanError::Serialize)?;
    Ok(PlannedTranslation { path, content, original_edits })
}

fn is_language_bound(key: &str, declared_claim_keys: &[String]) -> bool {
    LANGUAGE_FIELDS.contains(&key)
        || schema_fields::term_claim_fields().any(|claim| claim == key)
        || declared_claim_keys.iter().any(|claim| claim == key)
}

fn file_stem(rel: &str) -> &str {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

fn unique_key(base: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
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
            ("editor_page", "ada"),
            ("jury_page", "kane"),
            ("place_page", "kyoto"),
            ("translationKey", "about"),
            ("origin", "https://old-site.example/about"),
            ("date", "2020-01-01"),
        ] {
            fm.insert(k.to_string(), Value::String(v.to_string()));
        }
        fm.insert("syndicated".to_string(), Value::Sequence(vec![Value::String("https://m/x".to_string())]));

        let out = instantiate_template_frontmatter(fm, "2026-09-03", &[]);

        for identity in ["title", "uid", "url", "author_page", "tag_page", "editor_page", "jury_page", "place_page", "translationKey", "syndicated", "origin"] {
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

        let out = instantiate_template_frontmatter(fm, "2026-09-03", &[]);

        assert_eq!(out.get("layout"), Some(&Value::String("article".to_string())));
        assert_eq!(
            out.get("tags"),
            Some(&Value::Sequence(vec![Value::String("a".to_string())]))
        );
        assert_eq!(out.get("cascade"), Some(&Value::Mapping(cascade)));
    }

    #[test]
    fn instantiate_drops_a_site_declared_claim_too() {
        let mut fm = HashMap::new();
        fm.insert("conductor_page".to_string(), Value::String("Ada Example".to_string()));
        fm.insert("layout".to_string(), Value::String("article".to_string()));

        let out = instantiate_template_frontmatter(fm, "2026-09-03", &["conductor_page".to_string()]);

        assert_eq!(out.get("conductor_page"), None, "a copy would otherwise claim the captured page's person");
        assert_eq!(out.get("layout"), Some(&Value::String("article".to_string())));
    }

    #[test]
    fn instantiate_adds_no_date_when_the_template_has_none() {
        let mut fm = HashMap::new();
        fm.insert("layout".to_string(), Value::String("page".to_string()));
        let out = instantiate_template_frontmatter(fm, "2026-09-03", &[]);
        assert_eq!(out.get("date"), None);
    }

    fn fm(pairs: &[(&str, &str)]) -> HashMap<String, Value> {
        pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn site() -> SiteLanguages {
        SiteLanguages { default_lang: "en".to_string(), trees: vec!["zh-hans".to_string()] }
    }

    fn zh_file() -> TemplateFile {
        TemplateFile {
            lang: Some("zh-hans".to_string()),
            frontmatter: fm(&[("_template_name", "新作品"), ("start", "")]),
            body: "## 關於作品\n".to_string(),
        }
    }

    fn production(folder: Option<&str>) -> Template {
        Template {
            id: "production".to_string(),
            name: "Production".to_string(),
            kind: TemplateKind::Page,
            folder: folder.map(str::to_string),
            files: vec![
                TemplateFile {
                    lang: None,
                    frontmatter: fm(&[("_template_name", "Production"), ("_template_kind", "page"), ("start", "")]),
                    body: "## About the work\n".to_string(),
                },
                zh_file(),
            ],
        }
    }

    const PAPER: &str = "---\ntitle: Paper Bridges\ndescription: An English note\nuid: abc123\ndate: 2026-05-01\nstart: \"2026-06-01 19:00\"\ntickets: https://example.test/tickets\ncover: cover.jpg\n---\n# Paper Bridges\n\nEnglish body.\n";

    const KEYED: &str = "---\ntranslationKey: paper-bridges\ntitle: Paper Bridges\n---\n# Paper Bridges\n";

    #[allow(clippy::too_many_arguments)]
    fn translate(
        path: &str,
        text: &str,
        target: &str,
        existing: &[&str],
        keys: &[&str],
        all: &[&str],
        template: Option<&TemplateFile>,
    ) -> Result<PlannedTranslation, PlanError> {
        plan_translation(path, text, target, &site(), &set(existing), &set(keys), &set(all), template, &[])
    }

    #[test]
    fn folder_is_normalised() {
        for raw in ["work", "work/", "/work/", "./work"] {
            assert_eq!(normalise_folder(raw), "work/", "spelling `{raw}`");
        }
        assert_eq!(normalise_folder("./"), "");
    }

    #[test]
    fn template_folder_wins_over_anchor() {
        let page = plan_page_from_template(&production(Some("work")), None, "about/", &site(), &set(&["work/", "about/"]), "2026-10-19", &[]).unwrap();
        assert_eq!(page.dir, "work/");
        assert_eq!(page.body, "## About the work\n");
    }

    #[test]
    fn language_file_goes_to_the_tree() {
        let page = plan_page_from_template(&production(Some("work")), Some("zh-hans"), "", &site(), &set(&["zh-hans/work/"]), "2026-10-19", &[]).unwrap();
        assert_eq!(page.dir, "zh-hans/work/");
        assert_eq!(page.body, "## 關於作品\n");
    }

    #[test]
    fn missing_folder_is_an_error() {
        let err = plan_page_from_template(&production(Some("work")), None, "", &site(), &set(&[]), "2026-10-19", &[]).unwrap_err();
        assert_eq!(err, PlanError::FolderMissing("work/".to_string()));
    }

    #[test]
    fn no_folder_uses_anchor() {
        let page = plan_page_from_template(&production(None), None, "about", &site(), &set(&[]), "2026-10-19", &[]).unwrap();
        assert_eq!(page.dir, "about/");
    }

    #[test]
    fn template_keys_do_not_reach_the_page() {
        let page = plan_page_from_template(&production(Some("work")), None, "", &site(), &set(&["work/"]), "2026-10-19", &[]).unwrap();
        assert!(page.frontmatter.keys().all(|k| !k.starts_with("_template_")));
        assert!(page.frontmatter.contains_key("start"));
    }

    #[test]
    fn translation_paths_mirror_in_both_directions() {
        let zh = |path| translate(path, "# Page\n", "zh-hans", &[], &[], &[], None).unwrap().path;
        assert_eq!(zh("work/work.md"), "zh-hans/work/work.md");
        assert_eq!(zh("about.md"), "zh-hans/about.md");
        assert_eq!(translate("zh-hans/work/x.md", "# Page\n", "en", &[], &[], &[], None).unwrap().path, "work/x.md");
        assert_eq!(folder_in_default_tree("zh-hans/work/", &site()), ("work/", Some("zh-hans")));
    }

    #[test]
    fn translation_copies_facts_not_words() {
        let zh = zh_file();
        let out = translate("work/paper-bridges.md", PAPER, "zh-hans", &[], &[], &[], Some(&zh)).unwrap();
        let read = frontmatter::parse(&out.content).frontmatter;
        for kept in ["date", "start", "tickets", "cover"] {
            assert!(read.contains_key(kept), "`{kept}` is the same fact in both languages");
        }
        for dropped in ["title", "description", "uid"] {
            assert!(!read.contains_key(dropped), "`{dropped}` is written in a language");
        }
        assert_eq!(read.get("lang"), Some(&Value::String("zh-hans".to_string())));
    }

    #[test]
    fn translation_keeps_the_original_date() {
        let mut zh = zh_file();
        zh.frontmatter.insert("date".to_string(), Value::String("2030-01-01".to_string()));
        let out = translate("work/paper-bridges.md", PAPER, "zh-hans", &[], &[], &[], Some(&zh)).unwrap();
        let original = frontmatter::parse(PAPER).frontmatter.get("date").cloned();
        assert!(original.is_some());
        assert_eq!(frontmatter::parse(&out.content).frontmatter.get("date").cloned(), original);
    }

    #[test]
    fn translation_body_comes_from_the_language_template() {
        let zh = zh_file();
        let out = translate("work/paper-bridges.md", PAPER, "zh-hans", &[], &[], &[], Some(&zh)).unwrap();
        assert_eq!(frontmatter::parse(&out.content).body, "## 關於作品\n");
    }

    #[test]
    fn missing_key_is_minted_from_the_stem_and_written_to_both() {
        let out = translate("work/paper-bridges.md", PAPER, "zh-hans", &[], &[], &[], None).unwrap();
        assert_eq!(
            out.original_edits,
            vec![frontmatter::FieldEdit::Set { key: "translationKey".to_string(), value: Value::String("paper-bridges".to_string()) }]
        );
        let edited = frontmatter::edit_fields(PAPER, &out.original_edits).unwrap();
        assert_eq!(edited.replacen("translationKey: paper-bridges\n", "", 1), PAPER, "only the key line changes");
        assert_eq!(frontmatter::parse(&out.content).frontmatter.get("translationKey"), Some(&Value::String("paper-bridges".to_string())));
    }

    #[test]
    fn minted_key_is_suffixed_on_collision() {
        let out = translate("work/paper-bridges.md", PAPER, "zh-hans", &[], &[], &["paper-bridges"], None).unwrap();
        assert_eq!(frontmatter::parse(&out.content).frontmatter.get("translationKey"), Some(&Value::String("paper-bridges-2".to_string())));
    }

    #[test]
    fn existing_key_is_reused_and_original_untouched() {
        let out = translate("work/paper-bridges.md", KEYED, "zh-hans", &[], &[], &["paper-bridges"], None).unwrap();
        assert!(out.original_edits.is_empty());
        assert_eq!(frontmatter::parse(&out.content).frontmatter.get("translationKey"), Some(&Value::String("paper-bridges".to_string())));
    }

    #[test]
    fn already_translated_is_an_error() {
        let by_path = translate("work/paper-bridges.md", KEYED, "zh-hans", &["zh-hans/work/paper-bridges.md"], &[], &["paper-bridges"], None);
        assert_eq!(by_path.unwrap_err(), PlanError::AlreadyTranslated("zh-hans/work/paper-bridges.md".to_string()));
        let by_key = translate("work/paper-bridges.md", KEYED, "zh-hans", &[], &["paper-bridges"], &["paper-bridges"], None);
        assert_eq!(by_key.unwrap_err(), PlanError::AlreadyTranslated("zh-hans/work/paper-bridges.md".to_string()));
    }
}
