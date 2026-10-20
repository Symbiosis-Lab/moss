use super::*;
use std::fs;

const NOW: &str = "2026-10-19";

/// A site with a config that names English as its language and an empty
/// template store.
fn site() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::create_dir_all(p.join(".moss/templates")).unwrap();
    fs::write(p.join(".moss/config.toml"), "[site]\nlang = \"en\"\n").unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

const PRODUCTION: &str = "---\n_template_name: Production\n_template_kind: page\n_template_folder: work/\n---\nPoster\n";

#[test]
fn new_from_template_lands_in_its_folder_from_the_root() {
    let site = site();
    let root = site.path();
    write(&root.join(".moss/templates/production.md"), PRODUCTION);
    fs::create_dir_all(root.join("work")).unwrap();

    let out = new_in(&args(&[".", "--template", "production", "--title", "Example Show"]), root, None, NOW).unwrap();

    assert_eq!(out, "work/Example Show.md");
    let page = fs::read_to_string(root.join("work/Example Show.md")).unwrap();
    assert!(page.contains("Poster"), "{page}");
    assert!(page.contains("Example Show"), "{page}");
    assert!(!root.join("Example Show.md").exists());
}

#[test]
fn new_names_the_file_from_the_title() {
    let site = site();
    let root = site.path();

    let a = new_in(&args(&[".", "--title", "A/B: C?"]), root, None, NOW).unwrap();
    let cjk = new_in(&args(&[".", "--title", "新作品"]), root, None, NOW).unwrap();
    let cjk_again = new_in(&args(&[".", "--title", "新作品"]), root, None, NOW).unwrap();
    let untitled = new_in(&args(&["."]), root, None, NOW).unwrap();

    assert_eq!(a, "A-B- C-.md");
    assert_eq!(cjk, "新作品.md");
    assert_eq!(cjk_again, "新作品 2.md");
    assert_eq!(untitled, "untitled.md");
    for name in [&a, &cjk, &cjk_again, &untitled] {
        assert!(root.join(name).is_file(), "{name} was not written");
    }
    let page = fs::read_to_string(root.join(&a)).unwrap();
    assert!(page.contains("A/B: C?"), "the title is kept in the frontmatter: {page}");
}

#[test]
fn new_refuses_when_the_folder_is_gone() {
    let site = site();
    let root = site.path();
    write(&root.join(".moss/templates/production.md"), PRODUCTION);

    let err = new_in(&args(&[".", "--template", "production", "--title", "Example Show"]), root, None, NOW).unwrap_err();

    assert!(err.contains("work/"), "{err}");
    assert!(!root.join("work").exists(), "a missing folder must not be created");
    assert!(!root.join("Example Show.md").exists());
}

#[test]
fn new_translation_writes_both_files() {
    let site = site();
    let root = site.path();
    let original = "---\ntitle: Paper Bridges\ndate: 2026-05-01\n---\nBody\n";
    write(&root.join("work/paper-bridges.md"), original);
    write(&root.join("zh-hans/index.md"), "---\ntitle: 首页\n---\n");

    let out = new_in(&args(&["--translation", "work/paper-bridges.md", "--lang", "zh-hans"]), root, None, NOW).unwrap();

    assert_eq!(out, "zh-hans/work/paper-bridges.md");
    let translated = fs::read_to_string(root.join("zh-hans/work/paper-bridges.md")).unwrap();
    assert!(translated.contains("translationKey: paper-bridges"), "{translated}");
    assert!(translated.contains("lang: zh-hans"), "{translated}");
    let after = fs::read_to_string(root.join("work/paper-bridges.md")).unwrap();
    let without_key: Vec<&str> = after.lines().filter(|l| !l.starts_with("translationKey:")).collect();
    assert_eq!(without_key, original.lines().collect::<Vec<_>>());
    assert!(after.contains("translationKey: paper-bridges"), "{after}");
}

#[test]
fn new_list_json_reports_templates_trees_and_problems() {
    let site = site();
    let root = site.path();
    write(&root.join(".moss/templates/production.md"), PRODUCTION);
    write(
        &root.join(".moss/templates/production.zh-hans.md"),
        "---\n_template_name: 制作\n_template_kind: page\n---\n海报\n",
    );
    write(&root.join(".moss/templates/broken.md"), "---\n_template_name: Broken\n---\n");
    write(&root.join("zh-hans/index.md"), "---\ntitle: 首页\n---\n");

    let out = new_in(&args(&["--list", "--json"]), root, None, NOW).unwrap();
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();

    assert_eq!(json["default_lang"], "en");
    assert_eq!(json["trees"], serde_json::json!(["zh-hans"]));
    let templates = json["templates"].as_array().unwrap();
    assert_eq!(templates.len(), 1, "{json}");
    assert_eq!(templates[0]["id"], "production");
    assert_eq!(templates[0]["folder"], "work/");
    assert_eq!(templates[0]["langs"], serde_json::json!(["en", "zh-hans"]));
    let problems = json["problems"].as_array().unwrap();
    assert!(problems.iter().any(|p| p.as_str().unwrap().contains("broken.md")), "{json}");
}

#[test]
fn describe_lists_new() {
    assert!(crate::cli::commands::cli_commands().iter().any(|c| c.name == "new"));
}

#[test]
fn new_translation_leaves_the_original_alone_when_the_create_fails() {
    let site = site();
    let root = site.path();
    let original = "---\ntitle: Paper Bridges\ndate: 2026-05-01\n---\nBody\n";
    write(&root.join("work/paper-bridges.md"), original);
    write(&root.join("zh-hans/index.md"), "---\ntitle: 首页\n---\n");
    // Not valid UTF-8, so the site scan skips it and the plan goes ahead.
    fs::create_dir_all(root.join("zh-hans/work")).unwrap();
    fs::write(root.join("zh-hans/work/paper-bridges.md"), [0xff, 0xfe, 0x00]).unwrap();

    let err = new_in(&args(&["--translation", "work/paper-bridges.md", "--lang", "zh-hans"]), root, None, NOW);

    assert!(err.is_err());
    assert_eq!(fs::read_to_string(root.join("work/paper-bridges.md")).unwrap(), original);
}

#[test]
fn new_caps_a_long_cjk_title_in_bytes() {
    let site = site();
    let root = site.path();
    let title = "作".repeat(120);

    let out = new_in(&args(&[".", "--title", &title]), root, None, NOW).unwrap();

    assert!(out.len() <= 200 + ".md".len(), "{} bytes", out.len());
    assert!(root.join(&out).is_file());
}

#[test]
fn new_keeps_the_templates_line_order_and_blank_fields_blank() {
    let site = site();
    let root = site.path();
    write(
        &root.join(".moss/templates/show.md"),
        "---\n_template_name: Show\n_template_kind: page\ntitle: Old\nvenue: Example Hall\nstart:\ndate: 2020-01-01\nend:\nlayout: event\nuid: abc\n---\nBody\n",
    );

    let out = new_in(&args(&[".", "--template", "show", "--title", "Example Show"]), root, None, NOW).unwrap();

    let page = fs::read_to_string(root.join(out)).unwrap();
    assert_eq!(
        page,
        "---\ntitle: Example Show\nvenue: Example Hall\nstart:\ndate: 2026-10-19\nend:\nlayout: event\n---\nBody\n"
    );
}
