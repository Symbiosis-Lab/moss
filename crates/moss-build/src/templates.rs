//! The template store: the `.moss/templates/` files that the create menu and
//! `moss new` read and write. Planning a page from a template is pure and lives
//! in `moss_core::template`; this module does the file I/O around it.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

use moss_core::frontmatter;
use moss_core::home::{is_known_language_code, lang_suffix, strip_lang_suffix};
use moss_core::slug::generate_slug;
use moss_core::template::{normalise_folder, SiteLanguages, Template, TemplateFile, TemplateKind};
use serde_yaml::Value;

pub const FOLDER_KEY: &str = "_template_folder";
const NAME_KEY: &str = "_template_name";
const KIND_KEY: &str = "_template_kind";
const CREATED_AT_KEY: &str = "_template_created_at";
const ID_MAX: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct TemplateListing {
    pub templates: Vec<Template>,
    /// Files in the templates folder that could not be listed, with the reason.
    pub problems: Vec<String>,
}

type Frontmatter = HashMap<String, Value>;

/// Base files by id, and language files by base id with their language.
type Scan = (BTreeMap<String, PathBuf>, BTreeMap<String, Vec<(String, PathBuf)>>);

fn templates_dir(site_root: &Path) -> PathBuf {
    site_root.join(".moss").join("templates")
}

/// A template id is its file stem: lowercase letters, digits and hyphens, so a
/// name typed in any script yields a usable id and no id can leave the folder.
pub fn validate_template_id(id: &str) -> Result<(), String> {
    let usable = !id.is_empty()
        && id.chars().count() <= ID_MAX
        && id.chars().all(|c| c == '-' || (c.is_alphanumeric() && !c.is_uppercase()));
    if usable { Ok(()) } else { Err(format!("'{id}' is not a template id")) }
}

fn template_path(site_root: &Path, id: &str) -> Result<PathBuf, String> {
    validate_template_id(id)?;
    Ok(templates_dir(site_root).join(format!("{id}.md")))
}

fn read_doc(path: &Path) -> Result<(Frontmatter, String), String> {
    read_doc_text(path).map(|(fm, body, _)| (fm, body))
}

/// A template file's frontmatter, body and the text as written.
fn read_doc_text(path: &Path) -> Result<(Frontmatter, String, String), String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read template '{}': {e}", path.display()))?;
    let doc = frontmatter::parse(&raw);
    if let Some(err) = doc.frontmatter_error {
        return Err(format!("'{}' has broken frontmatter: {err}", path.display()));
    }
    Ok((doc.frontmatter, doc.body, raw))
}

fn write_doc(path: &Path, fm: &Frontmatter, body: &str) -> Result<(), String> {
    let content = frontmatter::serialize(fm, body)?;
    // allow:raw_write a template under .moss/templates is user state in the vault, not build output
    std::fs::write(path, content).map_err(|e| format!("Failed to write '{}': {e}", path.display()))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn scan(dir: &Path) -> Result<Scan, String> {
    let mut bases = BTreeMap::new();
    let mut langs: BTreeMap<String, Vec<(String, PathBuf)>> = BTreeMap::new();
    if !dir.exists() {
        return Ok((bases, langs));
    }
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("Failed to read '{}': {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("Failed to read template entry: {e}"))?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") || !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
            continue;
        };
        match (strip_lang_suffix(&stem), lang_suffix(&stem)) {
            (Some(base), Some(lang)) => {
                langs.entry(base.to_string()).or_default().push((lang.to_string(), path));
            }
            _ => {
                bases.insert(stem, path);
            }
        }
    }
    for files in langs.values_mut() {
        files.sort_by(|a, b| a.0.cmp(&b.0));
    }
    Ok((bases, langs))
}

fn build_template(id: &str, path: &Path, lang_files: &[(String, PathBuf)]) -> Result<Template, String> {
    let file = file_name(path);
    validate_template_id(id)
        .map_err(|_| format!("'{file}' is not listed: its file name must be lowercase letters, digits and hyphens"))?;
    let (fm, body, text) = read_doc_text(path)?;
    let name = fm
        .get(NAME_KEY)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("'{file}' has no `{NAME_KEY}`, so it is not listed"))?
        .to_string();
    let kind = fm
        .get(KIND_KEY)
        .and_then(|v| serde_yaml::from_value::<TemplateKind>(v.clone()).ok())
        .ok_or_else(|| format!("'{file}' has no valid `{KIND_KEY}` (page or folder), so it is not listed"))?;
    let folder = fm
        .get(FOLDER_KEY)
        .and_then(Value::as_str)
        .map(normalise_folder)
        .filter(|f| !f.is_empty());
    let mut files = vec![TemplateFile { lang: None, frontmatter: fm, body, text }];
    for (lang, lang_path) in lang_files {
        let (fm, body, text) = read_doc_text(lang_path)?;
        files.push(TemplateFile { lang: Some(lang.clone()), frontmatter: fm, body, text });
    }
    Ok(Template { id: id.to_string(), name, kind, folder, files })
}

pub fn list_templates(site_root: &Path) -> Result<TemplateListing, String> {
    let (bases, langs) = scan(&templates_dir(site_root))?;
    let mut listing = TemplateListing { templates: Vec::new(), problems: Vec::new() };
    for (id, path) in &bases {
        let lang_files = langs.get(id).cloned().unwrap_or_default();
        match build_template(id, path, &lang_files) {
            Ok(template) => listing.templates.push(template),
            Err(problem) => listing.problems.push(problem),
        }
    }
    let listed: BTreeSet<&str> = listing.templates.iter().map(|t| t.id.as_str()).collect();
    for (base, files) in &langs {
        if listed.contains(base.as_str()) {
            continue;
        }
        for (lang, path) in files {
            let reason = if bases.contains_key(base) {
                format!("its base template {base}.md is not listed")
            } else {
                format!("it has no {base}.md")
            };
            listing.problems.push(format!("'{}' is not listed (language {lang}): {reason}", file_name(path)));
        }
    }
    listing.templates.sort_by_key(|t| t.name.to_lowercase());
    Ok(listing)
}

pub fn read_template_by_id(site_root: &Path, id: &str) -> Result<Template, String> {
    validate_template_id(id)?;
    let (bases, langs) = scan(&templates_dir(site_root))?;
    let base = bases.get(id).ok_or_else(|| format!("Template '{id}' does not exist"))?;
    build_template(id, base, langs.get(id).map(Vec::as_slice).unwrap_or_default())
}

/// A page inside the site, named relative to the site root.
fn page_path(site_root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() || rel_path.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(format!("'{rel}' is not a page inside the site"));
    }
    let path = site_root.join(rel_path);
    let real = path.canonicalize().map_err(|e| format!("Failed to read '{rel}': {e}"))?;
    let root = site_root.canonicalize().map_err(|e| format!("Failed to read the site: {e}"))?;
    if !real.starts_with(&root) {
        return Err(format!("'{rel}' must be within the project directory"));
    }
    Ok(path)
}

/// The file stem a new template gets: its name as a slug, with dots removed so
/// the stem cannot read as a language suffix.
fn stem_for(name: &str) -> String {
    let slug: String = generate_slug(name).replace('.', "-").chars().take(ID_MAX).collect();
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() { "template".to_string() } else { slug.to_string() }
}

fn free_id(dir: &Path, base: &str) -> Result<String, String> {
    let (bases, langs) = scan(dir)?;
    let taken = |id: &str| bases.contains_key(id) || langs.contains_key(id);
    if !taken(base) {
        return Ok(base.to_string());
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !taken(candidate))
        .ok_or_else(|| "no free template id".to_string())
}

fn strip_template_keys(fm: &mut Frontmatter) {
    fm.retain(|key, _| !key.starts_with("_template_"));
}

pub fn save_template(
    site_root: &Path,
    source_rel: &str,
    name: &str,
    kind: TemplateKind,
    folder: Option<&str>,
    translations: &[(String, String)],
) -> Result<String, String> {
    let (mut fm, body) = read_doc(&page_path(site_root, source_rel)?)?;
    strip_template_keys(&mut fm);
    fm.insert(NAME_KEY.to_string(), Value::String(name.to_string()));
    fm.insert(KIND_KEY.to_string(), serde_yaml::to_value(kind).map_err(|e| e.to_string())?);
    fm.insert(
        CREATED_AT_KEY.to_string(),
        Value::String(chrono::Local::now().format("%Y-%m-%d").to_string()),
    );
    if let Some(folder) = folder.map(normalise_folder).filter(|f| !f.is_empty()) {
        fm.insert(FOLDER_KEY.to_string(), Value::String(folder));
    }

    // Read and check every translation before anything is written, so a bad
    // one leaves no half-saved template behind.
    let mut language_files = Vec::new();
    for (lang, rel) in translations {
        if !is_known_language_code(lang) {
            return Err(format!("'{lang}' is not a language moss recognizes"));
        }
        let (mut tfm, tbody) = read_doc(&page_path(site_root, rel)?)?;
        strip_template_keys(&mut tfm);
        language_files.push((lang.clone(), tfm, tbody));
    }

    let dir = templates_dir(site_root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create '{}': {e}", dir.display()))?;
    let id = free_id(&dir, &stem_for(name))?;
    write_doc(&dir.join(format!("{id}.md")), &fm, &body)?;
    for (lang, tfm, tbody) in &language_files {
        write_doc(&dir.join(format!("{id}.{lang}.md")), tfm, tbody)?;
    }
    Ok(id)
}

pub fn set_template_folder(site_root: &Path, id: &str, folder: Option<&str>) -> Result<(), String> {
    let path = template_path(site_root, id)?;
    let (mut fm, body) = read_doc(&path)?;
    match folder.map(normalise_folder).filter(|f| !f.is_empty()) {
        Some(folder) => fm.insert(FOLDER_KEY.to_string(), Value::String(folder)),
        None => fm.remove(FOLDER_KEY),
    };
    write_doc(&path, &fm, &body)
}

pub fn rename_template(site_root: &Path, id: &str, new_name: &str) -> Result<(), String> {
    let path = template_path(site_root, id)?;
    let (mut fm, body) = read_doc(&path)?;
    fm.insert(NAME_KEY.to_string(), Value::String(new_name.to_string()));
    write_doc(&path, &fm, &body)
}

pub fn delete_template(site_root: &Path, id: &str) -> Result<(), String> {
    let path = template_path(site_root, id)?;
    let (bases, mut langs) = scan(&templates_dir(site_root))?;
    if !bases.contains_key(id) {
        return Err(format!("Template '{id}' does not exist"));
    }
    let mut doomed = vec![path];
    doomed.extend(langs.remove(id).unwrap_or_default().into_iter().map(|(_, p)| p));
    for file in doomed {
        std::fs::remove_file(&file)
            .map_err(|e| format!("Failed to delete '{}': {e}", file.display()))?;
    }
    Ok(())
}

/// The site's languages: `[site] lang` from the config, and the top-level
/// folders named by a known language code that hold Markdown, other than the
/// default language's own folder.
pub fn site_languages(site_root: &Path) -> Result<SiteLanguages, String> {
    let project = site_root
        .to_str()
        .ok_or_else(|| format!("'{}' is not a UTF-8 path", site_root.display()))?;
    let config = crate::build::site_config::read_project_config(project)?;
    let default_lang = config.site_str("lang").unwrap_or_default().to_string();

    let entries = std::fs::read_dir(site_root)
        .map_err(|e| format!("Failed to read '{}': {e}", site_root.display()))?;
    let mut trees = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read site entry: {e}"))?;
        let name = file_name(&entry.path());
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir || !is_known_language_code(&name) || name.eq_ignore_ascii_case(&default_lang) {
            continue;
        }
        let has_markdown = walkdir::WalkDir::new(entry.path())
            .into_iter()
            .flatten()
            .any(|e| e.file_type().is_file() && e.path().extension().and_then(|x| x.to_str()) == Some("md"));
        if has_markdown {
            trees.push(name);
        }
    }
    trees.sort();
    Ok(SiteLanguages { default_lang, trees })
}

#[cfg(test)]
#[path = "templates_tests.rs"]
mod tests;
