use super::*;
use std::fs;

const PRODUCTION: &str = "---\n_template_name: Production\n_template_kind: page\n---\nPoster\n";

fn site() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".moss").join("templates")).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn tpl(root: &Path, file: &str) -> PathBuf {
    root.join(".moss").join("templates").join(file)
}

#[test]
fn save_names_the_file_from_the_name() {
    let root = site();
    write(&root.path().join("work/farewell.md"), "---\ntitle: Farewell\n---\nBody\n");

    let first = save_template(root.path(), "work/farewell.md", "Production", TemplateKind::Page, Some("work"), &[])
        .unwrap();
    let second = save_template(root.path(), "work/farewell.md", "Production", TemplateKind::Page, None, &[])
        .unwrap();

    assert_eq!(first, "production");
    assert_eq!(second, "production-2");
    assert!(tpl(root.path(), "production-2.md").exists());
    let saved = read_template_by_id(root.path(), "production").unwrap();
    assert_eq!(saved.folder.as_deref(), Some("work/"));
}

#[test]
fn save_rejects_a_page_outside_the_site() {
    let root = site();
    let err = save_template(root.path(), "../outside.md", "X", TemplateKind::Page, None, &[]).unwrap_err();
    assert!(err.contains("not a page inside the site"), "{err}");
}

#[test]
fn save_writes_language_files_beside_the_template() {
    let root = site();
    write(&root.path().join("work/x.md"), "---\ntitle: X\n---\nEnglish\n");
    write(&root.path().join("zh-hans/work/x.md"), "---\ntitle: 甲\n---\n中文\n");

    let id = save_template(
        root.path(),
        "work/x.md",
        "Production",
        TemplateKind::Page,
        Some("work"),
        &[("zh-hans".to_string(), "zh-hans/work/x.md".to_string())],
    )
    .unwrap();

    assert!(tpl(root.path(), &format!("{id}.zh-hans.md")).exists());
    let listing = list_templates(root.path()).unwrap();
    assert_eq!(listing.templates[0].files.len(), 2);
}

#[test]
fn uuid_named_templates_still_list() {
    let root = site();
    let id = "3f2a9c1e-7b4d-4e8a-9c2d-5b6e7f8a9b0c";
    write(&tpl(root.path(), &format!("{id}.md")), PRODUCTION);

    let listing = list_templates(root.path()).unwrap();

    assert!(listing.problems.is_empty(), "{:?}", listing.problems);
    assert_eq!(listing.templates.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec![id]);
}

#[test]
fn language_files_group_under_their_template() {
    let root = site();
    write(&tpl(root.path(), "production.md"), PRODUCTION);
    write(&tpl(root.path(), "production.zh-hans.md"), "---\n_template_name: 制作\n---\n海報\n");

    let listing = list_templates(root.path()).unwrap();

    assert!(listing.problems.is_empty(), "{:?}", listing.problems);
    assert_eq!(listing.templates.len(), 1);
    let langs: Vec<Option<&str>> = listing.templates[0].files.iter().map(|f| f.lang.as_deref()).collect();
    assert_eq!(langs, vec![None, Some("zh-hans")]);
}

#[test]
fn orphan_language_file_is_reported_not_listed() {
    let root = site();
    write(&tpl(root.path(), "production.zh-hans.md"), "---\n_template_name: 制作\n_template_kind: page\n---\n");

    let listing = list_templates(root.path()).unwrap();

    assert!(listing.templates.is_empty());
    assert_eq!(listing.problems.len(), 1, "{:?}", listing.problems);
    assert!(listing.problems[0].contains("production.zh-hans.md"), "{:?}", listing.problems);
}

#[test]
fn template_without_kind_is_reported_not_listed() {
    let root = site();
    write(&tpl(root.path(), "plain.md"), "---\n_template_name: Plain\n---\nBody\n");

    let listing = list_templates(root.path()).unwrap();

    assert!(listing.templates.is_empty());
    assert!(listing.problems.iter().any(|p| p.contains("_template_kind")), "{:?}", listing.problems);
}

#[test]
fn broken_frontmatter_is_reported_not_listed() {
    let root = site();
    write(&tpl(root.path(), "production.md"), "---\n_template_name: [\n---\nBody\n");

    let listing = list_templates(root.path()).unwrap();

    assert!(listing.templates.is_empty());
    assert_eq!(listing.problems.len(), 1, "{:?}", listing.problems);
}

#[test]
fn delete_removes_language_files() {
    let root = site();
    write(&root.path().join("work/x.md"), "---\ntitle: X\n---\nEnglish\n");
    write(&root.path().join("zh-hans/work/x.md"), "---\ntitle: 甲\n---\n中文\n");
    save_template(
        root.path(),
        "work/x.md",
        "Production",
        TemplateKind::Page,
        Some("work"),
        &[("zh-hans".to_string(), "zh-hans/work/x.md".to_string())],
    )
    .unwrap();

    delete_template(root.path(), "production").unwrap();

    assert!(!tpl(root.path(), "production.md").exists());
    assert!(!tpl(root.path(), "production.zh-hans.md").exists());
}

#[test]
fn set_template_folder_round_trips_and_clears() {
    let root = site();
    write(&tpl(root.path(), "production.md"), PRODUCTION);

    set_template_folder(root.path(), "production", Some("/work")).unwrap();
    assert_eq!(read_template_by_id(root.path(), "production").unwrap().folder.as_deref(), Some("work/"));

    set_template_folder(root.path(), "production", None).unwrap();
    let after = read_template_by_id(root.path(), "production").unwrap();
    assert_eq!(after.folder, None);
    assert!(!fs::read_to_string(tpl(root.path(), "production.md")).unwrap().contains(FOLDER_KEY));
}

#[test]
fn site_languages_reads_config_and_trees() {
    let root = site();
    write(&root.path().join(".moss/config.toml"), "[site]\nlang = \"en\"\n");
    write(&root.path().join("zh-hans/work/x.md"), "---\ntitle: 甲\n---\n");
    write(&root.path().join("assets/notes.md"), "notes\n");

    let langs = site_languages(root.path()).unwrap();

    assert_eq!(langs, SiteLanguages { default_lang: "en".to_string(), trees: vec!["zh-hans".to_string()] });
}

#[test]
fn template_ids_are_lowercase_names_that_stay_in_the_folder() {
    for bad in ["../x", "a/b", "a.b", "Upper", ""] {
        assert!(validate_template_id(bad).is_err(), "{bad:?} was accepted");
    }
    let uuid = "123e4567-e89b-12d3-a456-426614174000";
    for good in ["production", "production-2", uuid, "新作品"] {
        assert!(validate_template_id(good).is_ok(), "{good:?} was refused");
    }
}
