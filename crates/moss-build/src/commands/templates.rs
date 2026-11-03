//! Page templates: save a page as one, list, rename and delete them, and make
//! a page or a translation from one. The store, the placement rules and
//! `moss new` are `crate::templates`, `moss_core::template` and
//! `crate::cli::new`; these commands write through the vault primitives the
//! editor's other writes use, so every host and the CLI place a page alike.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use moss_core::template::{
    folder_in_default_tree, normalise_folder, plan_page_from_template, PlanError, SiteLanguages, Template, TemplateKind,
};

use super::{resolve, lossy, site_root, CommandHost, NoArgs, VaultPath};
use crate::templates as store;
use crate::vault::fs::{create_file_with_content_inner, create_folder_inner, recheck_canonical};

/// A saved template, as listed for the "New from Template" menu.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct TemplateMeta {
    pub id: String,
    pub name: String,
    pub kind: TemplateKind,
    pub created_at: String,
    /// The folder its pages belong in (`work/`), when the template names one.
    pub folder: Option<String>,
    /// The languages it has a file for besides the default, e.g. `zh-hans`.
    pub langs: Vec<String>,
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn meta_from_template(t: &Template) -> TemplateMeta {
    let created_at = t
        .files
        .iter()
        .find(|f| f.lang.is_none())
        .and_then(|f| f.frontmatter.get("_template_created_at"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    TemplateMeta {
        id: t.id.clone(),
        name: t.name.clone(),
        kind: t.kind,
        created_at,
        folder: t.folder.clone(),
        langs: t.files.iter().filter_map(|f| f.lang.clone()).collect(),
    }
}

fn plan_message(err: PlanError) -> String {
    match err {
        PlanError::FolderMissing(dir) => {
            format!("The {dir} folder this template belongs to is gone. Choose a folder or delete the template.")
        }
        PlanError::LanguageNotInSite(lang) => format!("This site has no {lang} language folder."),
        PlanError::NoTemplateFile(id) => format!("Template '{id}' has no file to start from."),
        other => format!("{other:?}"),
    }
}

/// `abs` (canonical, inside `root_canonical`) as a site-relative path with `/`
/// separators, `""` for the root.
fn site_rel(root_canonical: &Path, abs: &Path) -> Result<String, String> {
    let rel = abs
        .strip_prefix(root_canonical)
        .map_err(|_| format!("'{}' is not inside the project", abs.display()))?;
    let parts = rel
        .components()
        .map(|c| match c {
            Component::Normal(s) => Ok(s.to_string_lossy().into_owned()),
            _ => Err(format!("'{}' is not a plain path inside the project", abs.display())),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(parts.join("/"))
}

/// The folder a page made from `rel` belongs in, in the default-language tree.
/// A folder's home file (`news/news.md`) belongs in the folder above its own.
fn folder_of_source(rel: &str, is_home: bool, langs: &SiteLanguages) -> Option<String> {
    let mut parts: Vec<&str> = rel.split('/').collect();
    parts.pop();
    if is_home {
        parts.pop();
    }
    let dir = if parts.is_empty() { String::new() } else { format!("{}/", parts.join("/")) };
    let (in_default, _) = folder_in_default_tree(&dir, langs);
    Some(normalise_folder(in_default)).filter(|f| !f.is_empty())
}

/// Guarded core of [`create_template`]. `is_home` is the tree's own verdict
/// on the source (`TreeNode.is_home`): the tree runs the marked election plus
/// a root-only fallback gate, so re-deriving it here from the directory
/// listing would disagree with what the user is looking at. Kind only
/// chooses which vault shape instantiation creates, so the flag is not a
/// trust boundary. `translations` are `(language, absolute page path)` pairs
/// saved beside it as that language's file.
fn create_template_in(
    project_root: &Path,
    source_path: &str,
    name: &str,
    is_home: bool,
    translations: &[(String, String)],
) -> Result<String, String> {
    let src = Path::new(source_path);
    if !src.exists() {
        return Err(format!("Path does not exist: {}", src.display()));
    }
    let (root, src) = recheck_canonical(project_root, src)?;
    let langs = store::site_languages(project_root)?;
    let rel = site_rel(&root, &src)?;
    let kind = if is_home { TemplateKind::Folder } else { TemplateKind::Page };
    let folder = folder_of_source(&rel, is_home, &langs);

    let mut language_pages = Vec::with_capacity(translations.len());
    for (lang, path) in translations {
        let (_, page) = recheck_canonical(project_root, Path::new(path))?;
        language_pages.push((lang.clone(), site_rel(&root, &page)?));
    }
    store::save_template(project_root, &rel, name, kind, folder.as_deref(), &language_pages)
}

fn list_templates_in(project_root: &Path) -> Result<Vec<TemplateMeta>, String> {
    Ok(store::list_templates(project_root)?.templates.iter().map(meta_from_template).collect())
}

fn rename_template_in(project_root: &Path, id: &str, new_name: &str) -> Result<(), String> {
    store::rename_template(project_root, id, new_name)
}

fn delete_template_in(project_root: &Path, id: &str) -> Result<(), String> {
    store::delete_template(project_root, id)
}


/// The `<field>_page` claim keys of the site's declared name-list fields. A
/// template captured from a page that claims a person must not make every new
/// page claim them, and moss-core strips only the keys it is handed. An
/// unreadable config is an error, not an empty list: the keys cannot be known.
fn declared_claim_keys(project_root: &Path) -> Result<Vec<String>, String> {
    let cfg = crate::build::site_config::read_project_config(&project_root.to_string_lossy())?;
    Ok(crate::build::terms::declared_name_list_fields(&cfg).iter().map(|field| crate::build::terms::claim_field_key(field)).collect())
}

/// Guarded core of [`instantiate_template`]: instantiates into a new page
/// (`Page` kind) or a new folder + seeded home file (`Folder` kind,
/// self-named `<name>.md` inside the new folder — the same convention
/// the editor's folder-home takeover uses). `target_dir` is an ABSOLUTE
/// directory path, the same contract `create_files`/`create_folder` take; it
/// is where the page goes only when the template names no folder of its own.
/// A target inside a language tree picks that language's template file.
/// `name` is the destination file/folder name: computed frontend-side by the
/// same untitled-first naming + collision-retry loop `create_files`'s callers
/// already use, so this core carries no naming policy of its own.
fn instantiate_template_in(
    project_root: &Path,
    id: &str,
    target_dir: &str,
    name: &str,
) -> Result<String, String> {
    let template = store::read_template_by_id(project_root, id)?;
    let langs = store::site_languages(project_root)?;
    let claim_keys = declared_claim_keys(project_root)?;

    let (root, target) = recheck_canonical(project_root, Path::new(target_dir))?;
    let anchor = normalise_folder(&site_rel(&root, &target)?);
    let lang = folder_in_default_tree(&anchor, &langs).1.map(str::to_string);

    let mut existing_dirs = BTreeSet::new();
    if let Some(folder) = &template.folder {
        let dir = match &lang {
            Some(l) => format!("{l}/{folder}"),
            None => folder.clone(),
        };
        if project_root.join(&dir).is_dir() {
            existing_dirs.insert(dir);
        }
    }
    let planned = plan_page_from_template(&template, lang.as_deref(), &anchor, &langs, &existing_dirs, None, &today(), &claim_keys)
        .map_err(plan_message)?;
    let content = planned.content.clone();
    let dir: PathBuf = project_root.join(&planned.dir);
    let dir = dir.to_string_lossy().trim_end_matches('/').to_string();

    // The seeded page is written ONCE, with its content. Creating it empty and
    // filling it in a second write cost two structural rebuilds plus a third
    // from the drift sweep (see `create_file_with_content_inner`).
    match template.kind {
        TemplateKind::Page => create_file_with_content_inner(project_root, &dir, name, &content),
        TemplateKind::Folder => {
            let folder_path = create_folder_inner(project_root, &dir, name)?;
            create_file_with_content_inner(project_root, &folder_path, &format!("{name}.md"), &content)
        }
    }
}

/// Guarded core of [`create_translation`]: the same code path as
/// `moss new --translation`, so the app and the CLI place and link the page
/// identically.
fn create_translation_in(project_root: &Path, source_abs: &str, lang: &str) -> Result<String, String> {
    recheck_canonical(project_root, Path::new(source_abs))?;
    let args: Vec<String> = ["--translation", source_abs, "--lang", lang, "--json"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    let out = crate::cli::new::new_in(&args, project_root, None, &today())?;
    let made: serde_json::Value =
        serde_json::from_str(&out).map_err(|e| format!("moss new returned unreadable output: {e}"))?;
    let rel = made["path"].as_str().ok_or("moss new returned no path")?;
    Ok(project_root.join(rel).to_string_lossy().into_owned())
}


#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateTemplateArgs {
    pub source_path: VaultPath,
    pub name: String,
    /// The tree's own verdict that the source is a folder's home file.
    pub is_home: bool,
    /// `(language, page path)` pairs saved beside it as that language's file.
    pub translations: Option<Vec<(String, VaultPath)>>,
}

/// Save a page (or a folder's home file) as a template, recording the folder
/// it sits in and each translation's page. Answers the template's id.
pub async fn create_template(host: &dyn CommandHost, a: CreateTemplateArgs) -> Result<String, String> {
    let root = site_root(host)?;
    let mut translations = Vec::new();
    for (lang, path) in a.translations.unwrap_or_default() {
        translations.push((lang, lossy(&resolve(host, &path)?)));
    }
    create_template_in(root, &lossy(&resolve(host, &a.source_path)?), &a.name, a.is_home, &translations)
}

pub async fn list_templates(host: &dyn CommandHost, _a: NoArgs) -> Result<Vec<TemplateMeta>, String> {
    list_templates_in(site_root(host)?)
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RenameTemplateArgs {
    pub id: String,
    pub new_name: String,
}

pub async fn rename_template(host: &dyn CommandHost, a: RenameTemplateArgs) -> Result<(), String> {
    rename_template_in(site_root(host)?, &a.id, &a.new_name)
}

#[derive(serde::Deserialize, specta::Type)]
pub struct TemplateIdArgs {
    pub id: String,
}

/// Delete a saved template and its language files.
pub async fn delete_template(host: &dyn CommandHost, a: TemplateIdArgs) -> Result<(), String> {
    delete_template_in(site_root(host)?, &a.id)
}

#[derive(serde::Deserialize, specta::Type)]
pub struct SetTemplateFolderArgs {
    pub id: String,
    /// `None` clears it.
    pub folder: Option<String>,
}

/// Set, change or clear the folder a template places its pages in.
pub async fn set_template_folder(host: &dyn CommandHost, a: SetTemplateFolderArgs) -> Result<(), String> {
    store::set_template_folder(site_root(host)?, &a.id, a.folder.as_deref())
}

/// The site's default language and its other language trees.
pub async fn site_languages(host: &dyn CommandHost, _a: NoArgs) -> Result<SiteLanguages, String> {
    store::site_languages(site_root(host)?)
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InstantiateTemplateArgs {
    pub id: String,
    pub target_dir: VaultPath,
    pub name: String,
}

/// Make a new page or folder from a saved template. Answers the new page's path.
pub async fn instantiate_template(host: &dyn CommandHost, a: InstantiateTemplateArgs) -> Result<String, String> {
    instantiate_template_in(site_root(host)?, &a.id, &lossy(&resolve(host, &a.target_dir)?), &a.name)
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateTranslationArgs {
    pub source_abs: VaultPath,
    pub lang: String,
}

/// Make the page's version in `lang` at the mirrored path, linked to the
/// original with `translationKey`. Answers the new page's absolute path.
pub async fn create_translation(host: &dyn CommandHost, a: CreateTranslationArgs) -> Result<String, String> {
    create_translation_in(site_root(host)?, &lossy(&resolve(host, &a.source_abs)?), &a.lang)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn vault() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".moss")).unwrap();
        dir
    }

    fn write(root: &Path, rel: &str, text: &str) -> PathBuf {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    fn save(root: &Path, rel: &str, name: &str) -> String {
        let src = root.join(rel);
        create_template_in(root, src.to_str().unwrap(), name, false, &[]).unwrap()
    }

    #[test]
    fn create_list_rename_delete_round_trip() {
        let vault = vault();
        let root = vault.path();
        write(root, "about.md", "---\ntitle: About\nlayout: page\n---\nHello\n");

        let id = save(root, "about.md", "About template");

        let listed = list_templates_in(root).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
        assert_eq!(listed[0].name, "About template");
        assert_eq!(listed[0].kind, TemplateKind::Page);

        rename_template_in(root, &id, "Renamed").unwrap();
        assert_eq!(list_templates_in(root).unwrap()[0].name, "Renamed");

        delete_template_in(root, &id).unwrap();
        assert!(list_templates_in(root).unwrap().is_empty());
    }

    #[test]
    fn create_template_rejects_a_source_outside_the_project() {
        let vault = vault();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), "hello").unwrap();

        let err = create_template_in(vault.path(), outside.path().to_str().unwrap(), "x", false, &[])
            .unwrap_err();
        assert!(err.contains("must be within the project directory"), "{err}");
    }

    #[test]
    fn create_template_records_the_source_folder() {
        let vault = vault();
        let root = vault.path();
        write(root, "work/farewell.md", "---\ntitle: Farewell\n---\nBody\n");
        write(root, "about.md", "---\ntitle: About\n---\nBody\n");

        save(root, "work/farewell.md", "Production");
        save(root, "about.md", "About");

        let listed = list_templates_in(root).unwrap();
        let folder_of = |name: &str| listed.iter().find(|t| t.name == name).unwrap().folder.clone();
        assert_eq!(folder_of("Production"), Some("work/".to_string()));
        assert_eq!(folder_of("About"), None);
    }

    #[test]
    fn instantiate_lands_in_the_template_folder_whatever_the_dir() {
        let vault = vault();
        let root = vault.path();
        write(root, "work/farewell.md", "---\ntitle: Farewell\ndate: 2020-01-01\nlayout: page\n---\nBody\n");
        let id = save(root, "work/farewell.md", "Production");

        let dest = instantiate_template_in(root, &id, root.to_str().unwrap(), "untitled").unwrap();

        assert!(Path::new(&dest).ends_with(Path::new("work").join("untitled.md")), "{dest}");
        let doc = moss_core::frontmatter::parse(&fs::read_to_string(&dest).unwrap());
        assert_eq!(doc.frontmatter.get("layout").unwrap().as_str(), Some("page"));
        assert_eq!(doc.frontmatter.get("date").unwrap().as_str().unwrap().len(), 10);
        assert!(!doc.frontmatter.keys().any(|k| k.starts_with("_template_")));
    }

    #[test]
    fn instantiate_refuses_when_the_template_folder_is_gone() {
        let vault = vault();
        let root = vault.path();
        write(root, "work/farewell.md", "---\ntitle: Farewell\n---\nBody\n");
        let id = save(root, "work/farewell.md", "Production");
        fs::remove_dir_all(root.join("work")).unwrap();

        let err = instantiate_template_in(root, &id, root.to_str().unwrap(), "untitled").unwrap_err();

        assert!(err.contains("work/") && err.contains("gone"), "{err}");
        assert!(!root.join("work").exists() && !root.join("untitled.md").exists());
    }

    #[test]
    fn instantiate_folder_kind_creates_a_folder_with_a_self_named_home_file() {
        let vault = vault();
        let root = vault.path();
        let home = write(root, "news/news.md", "---\ntitle: News\nnav: true\n---\nWelcome\n");
        let id = create_template_in(root, home.to_str().unwrap(), "Section template", true, &[]).unwrap();
        assert_eq!(list_templates_in(root).unwrap()[0].kind, TemplateKind::Folder);

        let dest = instantiate_template_in(root, &id, root.to_str().unwrap(), "updates").unwrap();
        assert!(Path::new(&dest).ends_with(Path::new("updates").join("updates.md")), "{dest}");
        let doc = moss_core::frontmatter::parse(&fs::read_to_string(&dest).unwrap());
        assert_eq!(doc.frontmatter.get("title"), None);
        assert_eq!(doc.frontmatter.get("nav").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn instantiate_strips_a_template_claim_on_a_declared_name_list_field() {
        let vault = vault();
        let root = vault.path();
        write(root, ".moss/config.toml", "[terms.performers]\nfields = [\"conductor\"]\n");
        write(root, "maestro.md", "---\ntitle: Maestro\nconductor_page: true\n---\nBio\n");
        let id = save(root, "maestro.md", "Conductor");

        let dest = instantiate_template_in(root, &id, root.to_str().unwrap(), "untitled").unwrap();
        let doc = moss_core::frontmatter::parse(&fs::read_to_string(&dest).unwrap());
        assert!(!doc.frontmatter.contains_key("conductor_page"), "{:?}", doc.frontmatter);
    }

    #[test]
    fn instantiate_refuses_when_the_config_exists_but_cannot_be_read() {
        let vault = vault();
        let root = vault.path();
        write(root, "about.md", "---\ntitle: About\n---\nHello\n");
        let id = save(root, "about.md", "About");
        write(root, ".moss/config.toml", "[terms.performers\n");

        let err = instantiate_template_in(root, &id, root.to_str().unwrap(), "untitled").unwrap_err();
        assert!(err.contains("config.toml"), "the error must name the file: {err}");
    }

    #[test]
    fn create_translation_writes_the_mirror_and_links_the_original() {
        let vault = vault();
        let root = vault.path();
        write(root, ".moss/config.toml", "[site]\nlang = \"en\"\n");
        let original = write(root, "work/x.md", "---\ntitle: X\nstart: 2026-05-01\n---\nBody\n");
        write(root, "zh-hans/about.md", "---\ntitle: About\n---\nHi\n");

        let dest = create_translation_in(root, original.to_str().unwrap(), "zh-hans").unwrap();

        assert!(Path::new(&dest).ends_with(Path::new("zh-hans").join("work").join("x.md")), "{dest}");
        let made = moss_core::frontmatter::parse(&fs::read_to_string(&dest).unwrap());
        let orig = moss_core::frontmatter::parse(&fs::read_to_string(&original).unwrap());
        assert_eq!(made.frontmatter.get("translationKey"), orig.frontmatter.get("translationKey"));
        assert_eq!(orig.frontmatter.get("translationKey").unwrap().as_str(), Some("x"));
        assert_eq!(made.frontmatter.get("lang").unwrap().as_str(), Some("zh-hans"));
    }

    #[test]
    fn site_languages_reports_the_default_and_the_trees() {
        let vault = vault();
        let root = vault.path();
        write(root, ".moss/config.toml", "[site]\nlang = \"en\"\n");
        write(root, "zh-hans/about.md", "---\ntitle: About\n---\nHi\n");

        let langs = store::site_languages(root).unwrap();
        assert_eq!((langs.default_lang.as_str(), langs.trees.as_slice()), ("en", &["zh-hans".to_string()][..]));
    }
}
