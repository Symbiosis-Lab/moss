//! `moss new` makes a page from a template, or a page's version in another
//! language, from the command line. The planning is pure and lives in
//! `moss_core::template`; the template store is `crate::templates`. This
//! module reads the site, calls the planner and writes the result, so an
//! agent can make a page and check where it landed.

use std::collections::{BTreeSet, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use moss_core::frontmatter;
use moss_core::template::{
    folder_in_default_tree, normalise_folder, plan_page_from_template, plan_translation, PlanError, SiteLanguages,
    Template, TemplateFile, TemplateKind,
};
use serde_json::{json, Value as Json};
use serde_yaml::Value;

use crate::build::site_config::read_project_config;
use crate::build::terms::{claim_field_key, declared_name_list_fields};
use crate::cli::site_guard::guard_cli_open_in;
use crate::templates::{list_templates, site_languages};
use crate::vault_root::{resolve_input_in, VaultRoot};

/// Entry point for `moss new`. Returns an exit code (0 = success).
pub fn run(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", usage());
        return 0;
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let now = chrono::Local::now().format("%Y-%m-%d").to_string();
    match new_in(args, &cwd, dirs::home_dir().as_deref(), &now) {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

fn usage() -> &'static str {
    "Usage: moss new <folder> [--template <id>] [--lang <code>] [--title \"<title>\"] [--json]\n\
     \x20      moss new --translation <page.md> --lang <code> [--template <id>] [--json]\n\
     \x20      moss new --list [--json]\n\n\
     Makes a page in <folder>, a folder inside the site (relative to the current\n\
     directory). With --template the page starts from that template and lands in\n\
     the template's folder when it names one. Without it the page is blank. The\n\
     file is named from --title (untitled.md without one).\n\n\
     --translation makes the page's version in another language at the mirrored\n\
     path (work/x.md -> zh-hans/work/x.md) and links the two with translationKey.\n\n\
     --list prints the site's templates, language folders and default language."
}

#[derive(Default)]
struct Options {
    folder: Option<String>,
    template: Option<String>,
    lang: Option<String>,
    title: Option<String>,
    translation: Option<String>,
    list: bool,
    json: bool,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        match flag {
            "--list" | "--json" => {
                if inline.is_some() {
                    return Err(format!("{flag} takes no value"));
                }
                if flag == "--list" {
                    o.list = true;
                } else {
                    o.json = true;
                }
            }
            "--template" => o.template = Some(value(inline, &mut it, flag)?),
            "--lang" => o.lang = Some(value(inline, &mut it, flag)?),
            "--title" => o.title = Some(value(inline, &mut it, flag)?),
            "--translation" => o.translation = Some(value(inline, &mut it, flag)?),
            f if f.starts_with('-') => return Err(format!("unknown option {f}\n\n{}", usage())),
            _ => {
                if o.folder.is_some() {
                    return Err(format!("unexpected argument {arg}\n\n{}", usage()));
                }
                o.folder = Some(arg.clone());
            }
        }
    }
    Ok(o)
}

fn value(inline: Option<String>, it: &mut std::slice::Iter<'_, String>, flag: &str) -> Result<String, String> {
    match inline {
        Some(v) => Ok(v),
        None => it.next().cloned().ok_or_else(|| format!("{flag} needs a value")),
    }
}

/// Testable core of [`run`]: `args` are the command's arguments, `cwd` is what
/// relative paths resolve against, `home` is the user's home folder and `now`
/// the `YYYY-MM-DD` date a template's `date` is reset to. Returns the line to
/// print: the page's path, or the JSON object with `--json`.
pub fn new_in(args: &[String], cwd: &Path, home: Option<&Path>, now: &str) -> Result<String, String> {
    let o = parse(args)?;
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    if o.list {
        return list(&o, &cwd, home);
    }
    if let Some(page) = o.translation.clone() {
        return translate(&o, &page, &cwd, home);
    }
    create(&o, &cwd, home, now)
}

/// The site that owns `from`. Without a `.moss/` above it, the current
/// directory stands in when the same guard that `moss rename` uses accepts it.
fn find_site(from: &Path, cwd: &Path, home: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(root) = VaultRoot::find_containing_below(from, cwd, home) {
        return Ok(root.path().to_path_buf());
    }
    if !from.starts_with(cwd) {
        return Err(format!("no site found above {}", from.display()));
    }
    guard_cli_open_in(&cwd.to_string_lossy(), "new", home)?;
    Ok(cwd.to_path_buf())
}

/// `abs` as a site-relative path with `/` separators, `""` for the root.
fn site_rel(root: &Path, abs: &Path) -> Result<String, String> {
    let rel = abs
        .strip_prefix(root)
        .map_err(|_| format!("{} is not inside the site at {}", abs.display(), root.display()))?;
    let parts = rel
        .components()
        .map(|c| match c {
            Component::Normal(s) => Ok(s.to_string_lossy().into_owned()),
            _ => Err(format!("{} is not a plain path inside the site", abs.display())),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(parts.join("/"))
}

fn plan_message(err: PlanError) -> String {
    match err {
        PlanError::FolderMissing(dir) => {
            format!("The {dir} folder this template belongs to is gone. Choose a folder or delete the template.")
        }
        PlanError::LanguageNotInSite(lang) => format!("This site has no {lang} language folder."),
        PlanError::AlreadyTranslated(path) => format!("{path} already exists, so the page is already translated."),
        PlanError::OriginalNotEditable(msg) => format!("The original page cannot be linked to its translation: {msg}"),
        PlanError::NoTemplateFile(id) => format!("Template '{id}' has no file to start from."),
        PlanError::Serialize(msg) => msg,
    }
}

fn create(o: &Options, cwd: &Path, home: Option<&Path>, now: &str) -> Result<String, String> {
    let folder = o
        .folder
        .as_deref()
        .ok_or_else(|| format!("moss new needs a folder\n\n{}", usage()))?;
    let abs = resolve_input_in(Path::new(folder), cwd);
    let root = find_site(&abs, cwd, home)?;
    let langs = site_languages(&root)?;
    let anchor = normalise_folder(&site_rel(&root, &abs)?);

    // A folder inside a language tree names its language, unless --lang says otherwise.
    let (anchor_rel, inferred) = folder_in_default_tree(&anchor, &langs);
    let lang = o.lang.clone().or_else(|| inferred.map(str::to_string)).filter(|l| *l != langs.default_lang);
    let anchor_rel = anchor_rel.to_string();
    let anchor_full = match &lang {
        Some(l) => format!("{l}/{anchor_rel}"),
        None => anchor_rel,
    };

    let claims = claim_keys(&root)?;

    let (template, page) = match &o.template {
        Some(id) => {
            let t = list_templates(&root)?
                .templates
                .into_iter()
                .find(|t| t.id == *id)
                .ok_or_else(|| format!("Template '{id}' does not exist"))?;
            if t.kind == TemplateKind::Folder {
                return Err(format!("Template '{id}' makes a folder, not a page: moss new makes pages only"));
            }
            let mut existing = BTreeSet::new();
            if let Some(f) = &t.folder {
                let dir = match &lang {
                    Some(l) => format!("{l}/{f}"),
                    None => f.clone(),
                };
                if root.join(&dir).is_dir() {
                    existing.insert(dir);
                }
            }
            let planned = plan_page_from_template(&t, lang.as_deref(), &anchor_full, &langs, &existing, o.title.as_deref(), now, &claims)
                .map_err(plan_message)?;
            (Some(t), (planned.dir, planned.content))
        }
        None => {
            if let Some(l) = &lang {
                if !langs.trees.contains(l) {
                    return Err(plan_message(PlanError::LanguageNotInSite(l.clone())));
                }
            }
            let mut frontmatter = HashMap::new();
            if let Some(title) = &o.title {
                frontmatter.insert("title".to_string(), Value::String(title.clone()));
            }
            (None, (anchor_full.clone(), frontmatter::serialize(&frontmatter, "")?))
        }
    };
    let (dir, content) = page;
    let stem = o.title.as_deref().map(file_stem_for_title).unwrap_or_else(|| "untitled".to_string());
    let rel = write_new_page(&root, &dir, &stem, &content)?;

    Ok(if o.json {
        pretty(json!({
            "path": rel,
            "template": template.map(|t| t.id),
            "lang": lang.unwrap_or_else(|| langs.default_lang.clone()),
        }))
    } else {
        rel
    })
}

/// Longest page file name stem; the page also needs room for ` 2.md`.
const STEM_MAX_BYTES: usize = 200;

/// A file name for a page titled `title`: characters that cannot appear in a
/// file name become `-`, the ends are trimmed, and a title with nothing left
/// is `untitled`.
fn file_stem_for_title(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '-' } else { c })
        .collect();
    let mut trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.len() > STEM_MAX_BYTES {
        let mut end = STEM_MAX_BYTES;
        while !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        trimmed = trimmed[..end].trim_end();
    }
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed.to_string()
    }
}

/// The frontmatter keys that hold claimed names, from the site's config.
fn claim_keys(root: &Path) -> Result<Vec<String>, String> {
    let project = root.to_str().ok_or_else(|| format!("'{}' is not a UTF-8 path", root.display()))?;
    let config = read_project_config(project)?;
    Ok(declared_name_list_fields(&config).iter().map(|f| claim_field_key(f)).collect())
}

/// Writes `content` to a file that must not exist yet, creating its folders.
fn write_exclusive(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create '{}': {e}", parent.display()))?;
    }
    // allow:raw_write a new page the author asked for is user content in the vault, not build output
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Failed to create '{}': {e}", path.display()))?;
    file.write_all(content.as_bytes())
        .map_err(|e| format!("Failed to write '{}': {e}", path.display()))
}

/// Writes a new page at `dir` + `stem`.md, adding ` 2`, ` 3`… to the name when
/// a page already has it, and returns the site-relative path it wrote.
fn write_new_page(
    root: &Path,
    dir: &str,
    stem: &str,
    content: &str,
) -> Result<String, String> {
    let rel = (1u32..)
        .map(|n| match n {
            1 => format!("{dir}{stem}.md"),
            n => format!("{dir}{stem} {n}.md"),
        })
        .find(|rel| !root.join(rel).exists())
        .expect("an unused file name exists");
    write_exclusive(&root.join(&rel), content)?;
    Ok(rel)
}

/// Every page in the site with its `translationKey`, if it has one. Hidden
/// folders such as `.moss/` are skipped.
fn pages(root: &Path) -> Result<Vec<(String, Option<String>)>, String> {
    let mut out = Vec::new();
    let walk = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
    for entry in walk {
        let entry = entry.map_err(|e| format!("Failed to scan the site: {e}"))?;
        if !entry.file_type().is_file() || entry.path().extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path()) else { continue };
        let key = frontmatter::parse(&text)
            .frontmatter
            .get("translationKey")
            .and_then(frontmatter::value_as_string)
            .filter(|k| !k.is_empty());
        out.push((site_rel(root, entry.path())?, key));
    }
    Ok(out)
}

fn translate(o: &Options, page: &str, cwd: &Path, home: Option<&Path>) -> Result<String, String> {
    let target = o
        .lang
        .as_deref()
        .ok_or_else(|| format!("--translation needs --lang <code>\n\n{}", usage()))?;
    let abs = resolve_input_in(Path::new(page), cwd);
    if !abs.is_file() {
        return Err(format!("{page} is not a page"));
    }
    let abs = abs.canonicalize().map_err(|e| format!("Failed to read {page}: {e}"))?;
    let root = find_site(&abs, cwd, home)?;
    let langs = site_languages(&root)?;
    let rel = site_rel(&root, &abs)?;
    let source_text = fs::read_to_string(&abs).map_err(|e| format!("Failed to read {page}: {e}"))?;

    let claims = claim_keys(&root)?;

    let target_tree = (target != langs.default_lang).then_some(target);
    let pages = pages(&root)?;
    let existing_paths: BTreeSet<String> = pages.iter().map(|(p, _)| p.clone()).collect();
    let all_keys: BTreeSet<String> = pages.iter().filter_map(|(_, k)| k.clone()).collect();
    let target_keys: BTreeSet<String> = pages
        .iter()
        .filter(|(p, _)| folder_in_default_tree(p, &langs).1 == target_tree)
        .filter_map(|(_, k)| k.clone())
        .collect();

    let template_file = template_for_translation(o, &root, &rel, &langs, target_tree)?;

    let plan = plan_translation(
        &rel,
        &source_text,
        target,
        &langs,
        &existing_paths,
        &target_keys,
        &all_keys,
        template_file.as_ref(),
        &claims,
    )
    .map_err(plan_message)?;

    let edited = if plan.original_edits.is_empty() {
        None
    } else {
        Some(frontmatter::edit_fields(&source_text, &plan.original_edits)?)
    };
    write_exclusive(&root.join(&plan.path), &plan.content)?;
    if let Some(edited) = edited {
        // allow:raw_write the original page is user content in the vault, not build output
        fs::write(&abs, edited).map_err(|e| format!("Failed to update {rel}: {e}"))?;
    }

    Ok(if o.json {
        pretty(json!({ "path": plan.path, "source": rel, "lang": target }))
    } else {
        plan.path
    })
}

/// The template whose body a translation starts from: the one `--template`
/// names, else the only one whose folder is the source page's folder. Two
/// templates for one folder are refused rather than guessed between.
fn template_for_translation(
    o: &Options,
    root: &Path,
    rel: &str,
    langs: &SiteLanguages,
    target_tree: Option<&str>,
) -> Result<Option<TemplateFile>, String> {
    let templates = list_templates(root)?.templates;
    let chosen: Option<Template> = match &o.template {
        Some(id) => Some(
            templates
                .into_iter()
                .find(|t| t.id == *id)
                .ok_or_else(|| format!("Template '{id}' does not exist"))?,
        ),
        None => {
            let (default_rel, _) = folder_in_default_tree(rel, langs);
            let source_dir = match default_rel.rsplit_once('/') {
                Some((dir, _)) => format!("{dir}/"),
                None => String::new(),
            };
            let mut matching: Vec<Template> = templates
                .into_iter()
                .filter(|t| !source_dir.is_empty() && t.folder.as_deref() == Some(source_dir.as_str()))
                .collect();
            match matching.len() {
                0 => None,
                1 => matching.pop(),
                _ => {
                    let ids: Vec<String> = matching.iter().map(|t| t.id.clone()).collect();
                    return Err(format!(
                        "more than one template is for the {source_dir} folder ({}); name one with --template",
                        ids.join(", ")
                    ));
                }
            }
        }
    };
    Ok(chosen.and_then(|t| t.files.into_iter().find(|f| f.lang.as_deref() == target_tree)))
}

fn list(o: &Options, cwd: &Path, home: Option<&Path>) -> Result<String, String> {
    let root = find_site(cwd, cwd, home)?;
    let langs = site_languages(&root)?;
    let listing = list_templates(&root)?;
    let lang_codes = |t: &Template| -> Vec<String> {
        t.files.iter().map(|f| f.lang.clone().unwrap_or_else(|| langs.default_lang.clone())).collect()
    };
    if o.json {
        let templates: Vec<Json> = listing
            .templates
            .iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "name": t.name,
                    "kind": t.kind,
                    "folder": t.folder,
                    "langs": lang_codes(t),
                })
            })
            .collect();
        return Ok(pretty(json!({
            "default_lang": langs.default_lang,
            "trees": langs.trees,
            "templates": templates,
            "problems": listing.problems,
        })));
    }
    let mut lines = vec![
        format!("Default language: {}", langs.default_lang),
        format!("Language folders: {}", if langs.trees.is_empty() { "none".to_string() } else { langs.trees.join(", ") }),
    ];
    if listing.templates.is_empty() {
        lines.push("No templates.".to_string());
    }
    for t in &listing.templates {
        let folder = t.folder.as_deref().unwrap_or("(no folder)");
        lines.push(format!("{}  {}  in {folder}  [{}]", t.id, t.name, lang_codes(t).join(", ")));
    }
    for problem in &listing.problems {
        lines.push(format!("Not listed: {problem}"));
    }
    Ok(lines.join("\n"))
}

fn pretty(value: Json) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

#[cfg(test)]
#[path = "new_tests.rs"]
mod tests;
