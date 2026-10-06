//! Crosses the REAL rename_entry FS-boundary (no mocks) to lock the bug class
//! that survived mocked unit tests. The existing TS tests
//! mock `renameEntry` and assert on the relative-path argument, baking in the
//! buggy contract; these tests exercise the real path-validation + `fs::rename`
//! core directly.
//!
//! A relative path must be rejected with the project-directory error; an
//! absolute in-project path must succeed and move the file on disk.
//!
//! Target: `moss_build::vault::fs::rename_entry_inner` — the core the app's
//! `rename_entry` command delegates to after resolving the project root. The real error string is
//! `"Paths must be within the project directory"`.
//!
//! `validate_entry_path` itself is unit-tested with the code it belongs to, in
//! `src/vault/fs_tests.rs` — five duplicate copies of those cases lived here
//! until 2026-08-06. What this file still guards is the command's USE of it:
//! that `rename_entry_inner` actually consults the boundary before touching the
//! filesystem.

/// A relative old-path is the original bug input: it does not start with the
/// absolute project root, so the boundary check must reject it rather than
/// letting `fs::rename` operate relative to the process cwd (escape).
#[test]
fn rename_entry_rejects_relative_path_with_project_dir_error() {
    let tmp = std::env::temp_dir().join(format!("moss_rename_bnd_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("post.md"), "# Hi\n").unwrap();

    // Relative old-path (the bug input) -> must error, not escape the project.
    let err = moss_build::vault::fs::rename_entry_inner(tmp.as_path(), "post.md", "renamed.md")
        .unwrap_err();
    assert!(
        err.contains("within the project directory"),
        "got: {err}"
    );

    // The file must NOT have been renamed.
    assert!(tmp.join("post.md").exists());
    std::fs::remove_dir_all(&tmp).ok();
}

/// An absolute in-project path is the fixed contract: it starts with the
/// project root, passes the boundary check, and the file moves on disk.
#[test]
fn rename_entry_accepts_absolute_in_project_path_and_moves_file() {
    let tmp = std::env::temp_dir().join(format!("moss_rename_bnd_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("post.md"), "# Hi\n").unwrap();
    let old_abs = tmp.join("post.md").to_string_lossy().to_string();
    let new_abs = tmp.join("renamed.md").to_string_lossy().to_string();

    moss_build::vault::fs::rename_entry_inner(tmp.as_path(), &old_abs, &new_abs).unwrap();
    assert!(!tmp.join("post.md").exists());
    assert!(tmp.join("renamed.md").exists());
    std::fs::remove_dir_all(&tmp).ok();
}

// ── rename-with-refs: shortcode + frontmatter asset paths ─────────────────
//
// The other REAL FS boundary this file guards: `rename_entry_with_refs_core`
// is the shared core behind both the `rename_entry_with_refs` command and
// `moss rename`. The fixture mirrors the riverbend/河灣 corpus shape — a
// gallery of bare CJK directory-relative paths in a subfolder — because that
// is where the reported data loss happened.

/// A temp project under the repo-local `target/test-tmp` base.
fn ref_tmp_project(tag: &str) -> std::path::PathBuf {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let base = std::fs::canonicalize(&base).unwrap();
    let dir = base.join(format!("rename_refs_{tag}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn w(root: &std::path::Path, rel: &str, content: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

const RIVERBEND_DOC: &str = "---\n\
title: 河灣\n\
cover: 關於/頭像-顧海棠.png     # keep this comment\n\
---\n\
\n\
:::hero {image=\"關於/頭像-顧海棠.png\"}\n\
Overlay copy\n\
:::\n\
\n\
:::gallery 8 {.profiles}\n\
關於/頭像-顧海棠.png\n\
關於/頭像-顧海棠.png|cover top\n\
![](關於/頭像-顧海棠.png)\n\
![[關於/頭像-顧海棠.png]]\n\
關於/頭像-顧海清.png\n\
關於/頭像-顧海棠.jpg\n\
:::\n\
\n\
```\n\
關於/頭像-顧海棠.png\n\
```\n";

fn riverbend_fixture(tag: &str) -> std::path::PathBuf {
    let root = ref_tmp_project(tag);
    w(&root, "articles/河灣.md", RIVERBEND_DOC);
    for name in ["頭像-顧海棠.png", "頭像-顧海清.png", "頭像-顧海棠.jpg"] {
        w(&root, &format!("articles/關於/{name}"), "fake-bytes");
    }
    root
}

#[test]
fn rename_rewrites_every_shortcode_and_frontmatter_asset_path() {
    let root = riverbend_fixture("rename");
    let old = root.join("articles/關於/頭像-顧海棠.png");
    let new = root.join("articles/關於/avatar-gu.png");

    moss_build::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &old.to_string_lossy(),
        &new.to_string_lossy(),
    )
    .expect("rename with refs");

    let out = std::fs::read_to_string(root.join("articles/河灣.md")).unwrap();

    // 1. All six live references, each in its own syntax, now point at the
    //    new name — document-relative, exactly as authored.
    assert_eq!(
        out.matches("關於/avatar-gu.png").count(),
        6,
        "six live refs should be rewritten, got:\n{out}"
    );
    for expected in [
        "cover: 關於/avatar-gu.png",
        "{image=\"關於/avatar-gu.png\"}",
        "\n關於/avatar-gu.png\n",
        "關於/avatar-gu.png|cover top",
        "![](關於/avatar-gu.png)",
        "![[關於/avatar-gu.png]]",
    ] {
        assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
    }

    // 2. A different file, and a same-stem DIFFERENT-extension file, and the
    //    fenced line, are all byte-identical.
    assert!(out.contains("關於/頭像-顧海清.png"), "unrelated file untouched");
    assert!(
        out.contains("關於/頭像-顧海棠.jpg"),
        "same stem, different extension is a DIFFERENT file:\n{out}"
    );
    assert!(
        out.contains("```\n關於/頭像-顧海棠.png\n```"),
        "a path inside a code fence is not a reference:\n{out}"
    );

    // 3. The frontmatter is otherwise byte-identical — key order, spacing,
    //    and the YAML comment survive (nothing was re-serialized).
    assert!(
        out.contains("cover: 關於/avatar-gu.png     # keep this comment"),
        "frontmatter round-trip:\n{out}"
    );
    assert!(out.starts_with("---\ntitle: 河灣\n"));

    // 4. Round-trip fidelity: the file's byte length changed by exactly
    //    6 × (len(new) − len(old)). Fails loudly if anything re-serialized.
    let delta = "avatar-gu.png".len() as isize - "頭像-顧海棠.png".len() as isize;
    assert_eq!(
        out.len() as isize - RIVERBEND_DOC.len() as isize,
        6 * delta,
        "only the six reference spans changed"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn rename_with_refs_follows_the_copy_a_bare_asset_name_resolves_to() {
    // Two files share the name `頭像-顧海棠.png`, both equally far from
    // gallery.md, so the build links the alphabetically first, under
    // `articles/`. Renaming that copy would silently re-point the bare gallery
    // line at the other one, so the line follows the rename; renaming the
    // other copy leaves it alone.
    for (renamed, expect) in [
        ("articles/關於/頭像-顧海棠.png", ":::gallery\navatar-gu.png\n:::\n"),
        ("elsewhere/頭像-顧海棠.png", ":::gallery\n頭像-顧海棠.png\n:::\n"),
    ] {
        let root = riverbend_fixture("ambiguous");
        w(&root, "elsewhere/頭像-顧海棠.png", "fake-bytes");
        w(&root, "gallery.md", ":::gallery\n頭像-顧海棠.png\n:::\n");

        let old = root.join(renamed);
        let new = old.with_file_name("avatar-gu.png");
        moss_build::editor::ref_scan::rename_entry_with_refs_core(
            root.clone(),
            &old.to_string_lossy(),
            &new.to_string_lossy(),
        )
        .expect("rename with refs");

        let out = std::fs::read_to_string(root.join("gallery.md")).unwrap();
        assert_eq!(out, expect, "renaming {renamed}");

        std::fs::remove_dir_all(&root).ok();
    }
}

#[test]
fn clean_references_removes_shortcode_and_frontmatter_asset_paths() {
    let root = riverbend_fixture("clean");
    let target = root.join("articles/關於/頭像-顧海棠.png");

    moss_build::editor::ref_scan::clean_references_to_paths(&root, &[target.to_string_lossy().to_string()])
        .expect("clean refs");

    let out = std::fs::read_to_string(root.join("articles/河灣.md")).unwrap();
    // The only surviving occurrence is the one inside the code fence, which
    // was never a reference.
    assert_eq!(
        out.matches("頭像-顧海棠.png").count(),
        1,
        "every LIVE reference to the target is gone:\n{out}"
    );
    assert!(
        out.contains("```\n關於/頭像-顧海棠.png\n```"),
        "a path inside a code fence is not a reference:\n{out}"
    );
    // The gallery lines vanish whole — no blank residue, no orphan `|attrs`.
    assert!(!out.contains("|cover top"), "orphan attrs left behind:\n{out}");
    assert!(!out.contains("cover:"), "the whole cover: line is removed:\n{out}");
    assert!(
        out.contains("關於/頭像-顧海清.png") && out.contains("關於/頭像-顧海棠.jpg"),
        "untouched entries survive:\n{out}"
    );

    std::fs::remove_dir_all(&root).ok();
}

// ── a link written as a published address, built after the rename ─────────

/// The same headless build `moss build` runs; returns the staged output dir.
fn build_staged(root: &std::path::Path) -> std::path::PathBuf {
    use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(root),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(&root.to_string_lossy()),
        trigger: BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))
    .expect("build");
    root.join(".moss/build.nosync/staging")
}

/// `[a](/docs/)` is a published address, which the build keeps verbatim. When
/// the folder's home page is renamed to a name that is no longer a home page,
/// the page is served at `/docs/intro/`, and that is the address the link must
/// carry in the built page.
#[test]
fn a_published_address_still_reaches_the_renamed_page_in_the_built_site() {
    let root = ref_tmp_project("address");
    w(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    w(&root, "index.md", "# Home\n\n[a](/docs/)\n");
    w(&root, "docs/index.md", "# Docs Home Marker\n");
    w(&root, "docs/other.md", "# Other\n");

    moss_build::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &root.join("docs/index.md").to_string_lossy(),
        &root.join("docs/intro.md").to_string_lossy(),
    )
    .expect("rename with refs");

    let staged = build_staged(&root);
    let home = std::fs::read_to_string(staged.join("index.html")).unwrap();
    let href = home
        .split("href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .find(|h| h.starts_with("/docs"))
        .unwrap_or_else(|| panic!("no /docs link in:\n{home}"));
    let served = std::fs::read_to_string(staged.join(href.trim_start_matches('/')).join("index.html"))
        .unwrap_or_else(|e| panic!("nothing served at {href}: {e}"));
    assert!(served.contains("Docs Home Marker"), "{href} is not the renamed page:\n{served}");
    assert_eq!(href, "/docs/intro/");

    std::fs::remove_dir_all(&root).ok();
}

/// The `/…` hrefs in a built page, in order.
fn root_hrefs(html: &str) -> Vec<&str> {
    html.split("href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .filter(|h| h.starts_with('/') && !h.starts_with("//"))
        .collect()
}

/// A page whose file name gains a space is served at the slugged address, not
/// at its file name; the rewritten link has to carry the address the build
/// serves, because the build keeps a destination starting with `/` verbatim.
#[test]
fn a_published_address_follows_the_slugged_name_in_the_built_site() {
    let root = ref_tmp_project("address-slug");
    w(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    w(&root, "index.md", "# Home\n");
    w(&root, "notes/beta.md", "# Beta Marker\n");
    w(&root, "blog/post.md", "# Post\n\n[b](/notes/beta/)\n");

    moss_build::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &root.join("notes/beta.md").to_string_lossy(),
        &root.join("notes/New Name.md").to_string_lossy(),
    )
    .expect("rename with refs");

    let staged = build_staged(&root);
    let post = std::fs::read_to_string(staged.join("blog/post/index.html")).unwrap();
    let href = root_hrefs(&post)
        .into_iter()
        .find(|h| h.starts_with("/notes"))
        .unwrap_or_else(|| panic!("no /notes link in:\n{post}"));
    let served = std::fs::read_to_string(staged.join(href.trim_start_matches('/')).join("index.html"))
        .unwrap_or_else(|e| panic!("nothing served at {href}: {e}"));
    assert!(served.contains("Beta Marker"), "{href} is not the renamed page:\n{served}");
    assert_eq!(href, "/notes/new-name/");

    std::fs::remove_dir_all(&root).ok();
}

/// A page whose address is set in its frontmatter is linked at that address
/// after a rename, whatever its new file name.
#[test]
fn a_published_address_follows_the_frontmatter_address_in_the_built_site() {
    let root = ref_tmp_project("address-pinned");
    w(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    w(&root, "index.md", "# Home\n");
    w(&root, "notes/beta.md", "---\nurl: pinned-page\n---\n# Beta Marker\n");
    w(&root, "blog/post.md", "# Post\n\n[b](/notes/beta/)\n");

    moss_build::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &root.join("notes/beta.md").to_string_lossy(),
        &root.join("notes/Gamma Two.md").to_string_lossy(),
    )
    .expect("rename with refs");

    let staged = build_staged(&root);
    let post = std::fs::read_to_string(staged.join("blog/post/index.html")).unwrap();
    let href = root_hrefs(&post)
        .into_iter()
        .find(|h| h.starts_with("/notes"))
        .unwrap_or_else(|| panic!("no /notes link in:\n{post}"));
    let served = std::fs::read_to_string(staged.join(href.trim_start_matches('/')).join("index.html"))
        .unwrap_or_else(|e| panic!("nothing served at {href}: {e}"));
    assert!(served.contains("Beta Marker"), "{href} is not the renamed page:\n{served}");
    assert_eq!(href, "/notes/pinned-page/");

    std::fs::remove_dir_all(&root).ok();
}

/// A link is recognised as a published address by what the site serves at it,
/// not by its spelling, so the address one rename writes (`/notes/new-name/`
/// for `notes/New Name.md`) is followed by the next rename.
#[test]
fn an_address_written_by_one_rename_is_followed_by_the_next() {
    let root = ref_tmp_project("address-twice");
    w(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    w(&root, "index.md", "# Home\n");
    w(&root, "notes/beta.md", "# Beta Marker\n");
    w(&root, "blog/post.md", "# Post\n\n[b](/notes/beta/)\n");

    for (old, new) in [("notes/beta.md", "notes/New Name.md"), ("notes/New Name.md", "notes/Final.md")] {
        moss_build::editor::ref_scan::rename_entry_with_refs_core(
            root.clone(),
            &root.join(old).to_string_lossy(),
            &root.join(new).to_string_lossy(),
        )
        .expect("rename with refs");
    }

    let post = std::fs::read_to_string(root.join("blog/post.md")).unwrap();
    assert!(post.contains("[b](/notes/final/)"), "{post}");
    let staged = build_staged(&root);
    let (href, served) = served_by_link(&staged, "blog/post/index.html", "/notes");
    assert_eq!(href, "/notes/final/");
    assert!(served.contains("Beta Marker"), "{href} is not the renamed page:\n{served}");

    std::fs::remove_dir_all(&root).ok();
}

/// The address a page's frontmatter `url:` gives it is followed when the page
/// moves to another folder.
#[test]
fn a_frontmatter_address_is_followed_when_its_page_moves_folder() {
    let root = ref_tmp_project("address-pinned-move");
    w(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    w(&root, "index.md", "# Home\n");
    w(&root, "notes/beta.md", "---\nurl: pinned-page\n---\n# Beta Marker\n");
    w(&root, "archive/old.md", "# Old\n");
    w(&root, "blog/post.md", "# Post\n\n[b](/notes/pinned-page/)\n");

    moss_build::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &root.join("notes/beta.md").to_string_lossy(),
        &root.join("archive/beta.md").to_string_lossy(),
    )
    .expect("rename with refs");

    let staged = build_staged(&root);
    let (href, served) = served_by_link(&staged, "blog/post/index.html", "/archive");
    assert_eq!(href, "/archive/pinned-page/");
    assert!(served.contains("Beta Marker"), "{href} is not the moved page:\n{served}");

    std::fs::remove_dir_all(&root).ok();
}

/// When two pages would claim the same address and which keeps it depends on
/// the order the folder lists them, there is no single address to write, so
/// the rename stops naming the page and changes nothing.
#[test]
fn a_published_address_with_no_single_answer_refuses_the_rename() {
    let root = ref_tmp_project("address-claimed");
    w(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    w(&root, "index.md", "# Home\n");
    w(&root, "notes/beta.md", "# Beta\n");
    w(&root, "notes/new-name.md", "# Taken\n");
    w(&root, "blog/post.md", "# Post\n\n[b](/notes/beta/)\n");

    let err = moss_build::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &root.join("notes/beta.md").to_string_lossy(),
        &root.join("notes/New Name.md").to_string_lossy(),
    )
    .unwrap_err();

    assert!(err.contains("blog/post.md"), "{err}");
    assert!(err.contains("same address as another page"), "names why: {err}");
    assert!(root.join("notes/beta.md").exists());
    assert!(!root.join("notes/New Name.md").exists());
    let post = std::fs::read_to_string(root.join("blog/post.md")).unwrap();
    assert!(post.contains("[b](/notes/beta/)"), "{post}");

    std::fs::remove_dir_all(&root).ok();
}

/// The page a built page's `/…` link starting with `prefix` lands on.
fn served_by_link(staged: &std::path::Path, page: &str, prefix: &str) -> (String, String) {
    let html = std::fs::read_to_string(staged.join(page)).unwrap();
    let href = root_hrefs(&html)
        .into_iter()
        .find(|h| h.starts_with(prefix))
        .unwrap_or_else(|| panic!("no {prefix} link in:\n{html}"))
        .to_string();
    let served = std::fs::read_to_string(staged.join(href.trim_start_matches('/')).join("index.html"))
        .unwrap_or_else(|e| panic!("nothing served at {href}: {e}"));
    (href, served)
}

/// A folder home and its translation share the folder's address in the page
/// map; the build gives it to the site's language and prefixes the other. A
/// link to the folder's address follows the folder when it is renamed.
#[test]
fn a_published_address_follows_a_folder_with_a_translated_home() {
    let root = ref_tmp_project("address-bilingual");
    w(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    w(&root, "index.md", "# Home\n\n[d](/docs/)\n");
    w(&root, "docs/index.md", "# Docs English Marker\n");
    w(&root, "docs/index.zh-hans.md", "# Docs Chinese Marker\n");

    moss_build::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &root.join("docs").to_string_lossy(),
        &root.join("guide").to_string_lossy(),
    )
    .expect("rename with refs");

    let staged = build_staged(&root);
    let (href, served) = served_by_link(&staged, "index.html", "/guide");
    assert_eq!(href, "/guide/");
    assert!(served.contains("Docs English Marker"), "{href} is not the English home:\n{served}");

    std::fs::remove_dir_all(&root).ok();
}

/// A page renamed onto a name whose address a folder already holds is served
/// at the numbered address the build gives it, and the link carries that.
#[test]
fn a_published_address_follows_a_page_numbered_beside_a_folder_of_its_name() {
    let root = ref_tmp_project("address-numbered");
    w(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    w(&root, "index.md", "# Home\n\n[a](/about/)\n");
    w(&root, "about.md", "# About Marker\n");
    w(&root, "docs/index.md", "# Docs\n");

    moss_build::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &root.join("about.md").to_string_lossy(),
        &root.join("docs.md").to_string_lossy(),
    )
    .expect("rename with refs");

    let staged = build_staged(&root);
    let (href, served) = served_by_link(&staged, "index.html", "/docs");
    assert_eq!(href, "/docs-2/");
    assert!(served.contains("About Marker"), "{href} is not the renamed page:\n{served}");

    std::fs::remove_dir_all(&root).ok();
}

/// A folder whose home declares its language: `blog/post.md` takes the
/// folder's language and its English translation keeps the plain address.
fn folder_with_a_declared_language(tag: &str) -> std::path::PathBuf {
    let root = ref_tmp_project(tag);
    w(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    w(&root, "index.md", "# Home\n\n[en](/blog/post/) [zh](/zh-hans/blog/post/) [o](/blog/old/)\n");
    w(&root, "blog/index.md", "---\nlang: zh-hans\n---\n# Blog\n");
    w(&root, "blog/post.md", "# Folder Language Marker\n");
    w(&root, "blog/post.en.md", "# English Marker\n");
    w(&root, "blog/old.md", "# Old Marker\n");
    root
}

#[test]
fn a_page_in_a_folder_with_a_declared_language_is_addressed_in_that_language() {
    let root = folder_with_a_declared_language("address-folder-lang");

    moss_build::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &root.join("blog/post.md").to_string_lossy(),
        &root.join("blog/article.md").to_string_lossy(),
    )
    .expect("rename with refs");

    let home = std::fs::read_to_string(root.join("index.md")).unwrap();
    assert!(home.contains("[en](/blog/post/) [zh](/blog/article/)"), "{home}");
    let staged = build_staged(&root);
    let (_, served) = served_by_link(&staged, "index.html", "/blog/post");
    assert!(served.contains("English Marker"), "{served}");
    let (_, served) = served_by_link(&staged, "index.html", "/blog/article");
    assert!(served.contains("Folder Language Marker"), "{served}");

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_page_renamed_onto_its_folders_language_beside_a_page_of_that_language_is_refused() {
    let root = folder_with_a_declared_language("address-folder-lang-shared");

    let err = moss_build::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &root.join("blog/old.md").to_string_lossy(),
        &root.join("blog/post.zh-hans.md").to_string_lossy(),
    )
    .unwrap_err();

    assert!(err.contains("same address as another page"), "{err}");
    assert!(root.join("blog/old.md").exists());

    std::fs::remove_dir_all(&root).ok();
}

/// With no `[site] lang`, the home page's `lang:` is the site's language, so
/// renaming the home page away changes which of two translations keeps the
/// plain address; the links follow what the site serves after the rename.
#[test]
fn renaming_the_home_page_of_a_site_without_a_declared_language_moves_translations() {
    let root = ref_tmp_project("address-site-lang");
    let english = "This is a long English article about gardening, the weather and the seasons, written so that the language of the page can be told from its text without any doubt at all.";
    w(&root, "index.md", "---\nlang: zh-hans\n---\n# Home\n");
    w(&root, "links.md", "# Links\n\n[z](/notes/a/) [e](/en/notes/a/)\n");
    w(&root, "notes/a.md", &format!("# English Marker\n\n{english}\n"));
    w(&root, "notes/a.zh-hans.md", "# Chinese Marker\n");
    w(&root, "notes/b.md", &format!("# B\n\n{english}\n"));
    let staged = build_staged(&root);
    let (_, served) = served_by_link(&staged, "links/index.html", "/notes/a");
    assert!(served.contains("Chinese Marker"), "fixture: the site starts out Chinese:\n{served}");

    moss_build::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &root.join("index.md").to_string_lossy(),
        &root.join("start.md").to_string_lossy(),
    )
    .expect("rename with refs");

    let staged = build_staged(&root);
    let (href, served) = served_by_link(&staged, "links/index.html", "/zh-hans/notes/a");
    assert!(served.contains("Chinese Marker"), "{href}:\n{served}");
    let (href, served) = served_by_link(&staged, "links/index.html", "/notes/a");
    assert!(served.contains("English Marker"), "{href}:\n{served}");

    std::fs::remove_dir_all(&root).ok();
}
