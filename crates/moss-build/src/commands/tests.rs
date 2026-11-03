use std::path::{Path, PathBuf};

use futures::future::BoxFuture;
use serde_json::{json, Value};

use super::rename_undo::{RenameUndoStore, RENAME_UNDO_CAPACITY};
use super::*;
use crate::editor::rename_plan::RenameApplyResult;

/// A representative body for each command, in the shape moss's UI sends.
/// Each is parsed with the command's own argument type, so a renamed or
/// retyped argument fails here before it fails a caller.
fn samples() -> Vec<(&'static str, Value)> {
    vec![
        ("scan_shortcodes", json!({ "text": "# hi" })),
        ("parse_frontmatter", json!({ "filePath": "a.md" })),
        ("editor_resolve_asset", json!({ "target": "i.png", "fromFile": "a.md" })),
        (
            "editor_resolve_references",
            json!({ "targets": [], "fromFile": "a.md" }),
        ),
        ("editor_bootstrap", json!({ "sourcePath": null })),
        (
            "list_directory",
            json!({ "path": "posts", "projectPath": "/ignored", "showInternal": false }),
        ),
        ("list_tree", json!({ "path": "/ignored", "showInternal": true })),
        ("get_file_info", json!({ "filePath": "a.md" })),
        (
            "compute_heading_state",
            json!({ "filePath": "a.md", "frontmatter": {}, "body": "" }),
        ),
        ("describe_source_role", json!({ "filePath": "a.md" })),
        ("page_facts", json!({ "filePath": "a.md" })),
        ("resolve_url_for_file", json!({ "filePath": "a.md" })),
        ("validate_content", json!({ "filePath": "a.md" })),
        ("get_publish_preflight", json!({ "folderPath": "/open/folder" })),
        ("list_vault_terms", json!({})),
        ("get_cloud_unavailable_files", json!({ "folderPath": "/open/folder" })),
        (
            "list_versions",
            json!({ "projectPath": "/ignored", "scope": "site", "path": null }),
        ),
        ("read_version", json!({ "id": "v1", "path": "a.md" })),
        ("reveal_history_store", json!({ "projectPath": "/ignored" })),
        ("resolve_attachment_dir", json!({ "pageRelativePath": "posts/a.md" })),
        ("resolve_page_source", json!({ "urlPath": "/posts/a/" })),
        (
            "create_files",
            json!({ "files": [{ "dir": "posts", "name": "a.md", "frontmatter": {} }] }),
        ),
        ("create_folder", json!({ "parentDir": "posts", "name": "new" })),
        ("delete_entry", json!({ "path": "a.md" })),
        ("delete_entries", json!({ "paths": ["a.md"] })),
        ("rename_entry_with_refs", json!({ "oldPath": "a.md", "newPath": "b.md" })),
        ("move_entries", json!({ "paths": ["a.md"], "targetDir": "posts" })),
        ("scan_references_for_delete", json!({ "paths": ["a.md", "posts"] })),
        ("clean_references_and_delete", json!({ "paths": ["a.md"] })),
        ("undo_rename", json!({ "undoId": "id" })),
        (
            "save_editor_content",
            json!({ "filePath": "a.md", "frontmatter": {}, "body": "", "expectedHash": "h", "keepDisplaced": false }),
        ),
        (
            "restore_version",
            json!({ "projectPath": "/ignored", "id": "v1", "path": null, "mode": "page" }),
        ),
        ("save_version", json!({ "projectPath": "/ignored", "label": null })),
    ]
}

#[test]
fn every_command_parses_the_json_its_callers_send() {
    let samples = samples();
    for (cmd, body) in &samples {
        let parsed = public_read::parse(cmd, body.clone())
            .or_else(|| session_read::parse(cmd, body.clone()))
            .or_else(|| write::parse(cmd, body.clone()))
            .unwrap_or_else(|| panic!("{cmd} is in no tier"));
        parsed.unwrap_or_else(|e| panic!("{cmd} refused {body}: {e}"));
    }
    let sampled: Vec<&str> = samples.iter().map(|(cmd, _)| *cmd).collect();
    for cmd in all_commands() {
        assert!(sampled.contains(&cmd), "{cmd} has no wire sample here");
    }
}

fn all_commands() -> impl Iterator<Item = &'static str> {
    public_read::COMMANDS.iter().chain(session_read::COMMANDS).chain(write::COMMANDS).copied()
}

/// A command is in exactly one tier, so a transport that maps tiers to trust
/// classes cannot expose a write as a read.
#[test]
fn the_tiers_are_disjoint() {
    let all: Vec<&str> = all_commands().collect();
    for (i, cmd) in all.iter().enumerate() {
        assert!(!all[i + 1..].contains(cmd), "{cmd} is in two tiers");
    }
}

struct TestHost {
    /// `None` is a host with no folder open, whose root no command may read.
    root: Option<PathBuf>,
    scope: EditorScope,
    undo: RenameUndoStore,
    dates: crate::editor::filesystem::PublishDateMemo,
    announced: std::sync::Mutex<Vec<(String, String)>>,
    withdrawn: std::sync::Mutex<Vec<(String, String)>>,
    saved: std::sync::Mutex<Vec<String>>,
}

impl TestHost {
    fn site(root: &Path) -> Self {
        Self::new(root, EditorScope::Site)
    }
    fn loose(root: &Path) -> Self {
        Self::new(root, EditorScope::Folder)
    }
    fn without_folder() -> Self {
        TestHost { root: None, ..Self::site(Path::new("")) }
    }
    fn new(root: &Path, scope: EditorScope) -> Self {
        TestHost {
            root: Some(root.to_path_buf()),
            scope,
            undo: RenameUndoStore::default(),
            dates: Default::default(),
            announced: Default::default(),
            withdrawn: Default::default(),
            saved: Default::default(),
        }
    }
}

impl CommandHost for TestHost {
    fn root(&self) -> &Path {
        self.root.as_deref().expect("a command read the root of a host with no folder open")
    }
    /// Canonical, like the HTTP carrier's own resolve.
    fn resolve(&self, path: &VaultPath) -> Result<PathBuf, String> {
        let joined = self.root.as_deref().unwrap_or(Path::new("")).join(path.as_written());
        Ok(joined.canonicalize().unwrap_or(joined))
    }
    fn scope(&self) -> EditorScope {
        self.scope
    }
    fn rename_undo(&self) -> &RenameUndoStore {
        &self.undo
    }
    fn publish_dates(&self) -> &crate::editor::filesystem::PublishDateMemo {
        &self.dates
    }
    fn announce_moves(&self, pairs: &[(String, String)]) {
        self.announced.lock().unwrap().extend_from_slice(pairs);
    }
    fn withdraw_moves(&self, pairs: &[(String, String)]) {
        self.withdrawn.lock().unwrap().extend_from_slice(pairs);
    }
    fn served_dir(&self) -> PathBuf {
        self.root().join(".moss")
    }
    fn publish(&self, _: &MossEvent) {}
    fn saving(
        &self,
        file_path: &str,
        save: &mut dyn FnMut() -> Result<crate::editor::content::SaveResult, String>,
    ) -> Result<crate::editor::content::SaveResult, String> {
        self.saved.lock().unwrap().push(file_path.to_string());
        save()
    }
    fn sealed_now(&self) -> BoxFuture<'_, Result<SealedManifest, String>> {
        Box::pin(async { Err("no build in tests".to_string()) })
    }
}

/// A folder of pages: `a.md`, and `b.md` linking to it.
fn pages() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir(root.join(".moss")).unwrap();
    // allow:raw_write test fixture pages in a temp folder
    std::fs::write(root.join("a.md"), "---\ntitle: A\ndate: 2026-01-02\n---\nbody").unwrap();
    // allow:raw_write test fixture pages in a temp folder
    std::fs::write(root.join("b.md"), "see [A](a.md)").unwrap();
    (dir, root)
}

async fn ok<T: serde::de::DeserializeOwned>(out: Option<CommandResult>) -> T {
    match out {
        Some(Ok(v)) => serde_json::from_value(v).unwrap(),
        other => panic!("expected an answer, got {other:?}"),
    }
}

#[tokio::test]
async fn the_commands_that_run_without_a_folder_never_read_its_root() {
    let (_d, root) = pages();
    let host = TestHost::without_folder();
    let file = root.join("a.md").to_string_lossy().into_owned();
    for cmd in RUN_WITHOUT_FOLDER {
        assert!(all_commands().any(|c| c == *cmd), "{cmd} is not a command");
        let (_, mut body) = samples().into_iter().find(|(c, _)| c == cmd).unwrap();
        if body.get("filePath").is_some() {
            body["filePath"] = json!(file);
        }
        match dispatch(&host, cmd, body).await {
            Some(Ok(_)) => {}
            other => panic!("{cmd} with no folder: {other:?}"),
        }
    }
}

#[tokio::test]
async fn one_dispatch_reaches_every_tier_and_a_save_goes_through_the_host() {
    let (_d, root) = pages();
    let host = TestHost::site(&root);
    let parsed: Value = ok(dispatch(&host, "parse_frontmatter", json!({ "filePath": "a.md" })).await).await;
    ok::<Value>(dispatch(&host, "get_file_info", json!({ "filePath": "a.md" })).await).await;
    let saved: Value = ok(dispatch(
        &host,
        "save_editor_content",
        json!({ "filePath": "a.md", "frontmatter": { "title": "B" }, "body": "x", "expectedHash": parsed["hash"] }),
    )
    .await)
    .await;
    assert_eq!(saved["status"], json!("saved"), "{saved}");
    assert_eq!(*host.saved.lock().unwrap(), vec![root.join("a.md").to_string_lossy().into_owned()]);
    assert!(dispatch(&host, "no_such_command", json!({})).await.is_none());
}

#[tokio::test]
async fn a_loose_document_boots_with_its_folder_as_editor_root_and_no_project() {
    let (_d, root) = pages();
    let boot: Value = ok(session_read::dispatch(&TestHost::loose(&root), "editor_bootstrap", json!({ "sourcePath": "a.md" })).await).await;
    assert_eq!(boot["project_path"], Value::Null, "{boot}");
    assert_eq!(boot["editor_root"], json!(root.to_string_lossy()), "{boot}");
    assert_eq!(boot["initial_file"]["frontmatter"]["title"], json!("A"), "{boot}");
}

/// The bounded walk reads no frontmatter, so it finds no publish date.
#[tokio::test]
async fn a_loose_document_folder_is_listed_without_reading_its_pages() {
    let (_d, root) = pages();
    let date_of = |tree: &Value| {
        tree["children"].as_array().unwrap().iter().find(|c| c["name"] == "a.md").unwrap()["publish_date"].clone()
    };
    let site: Value = ok(session_read::dispatch(&TestHost::site(&root), "list_tree", json!({ "showInternal": false })).await).await;
    assert!(date_of(&site).is_string(), "{site}");
    let loose: Value = ok(session_read::dispatch(&TestHost::loose(&root), "list_tree", json!({ "showInternal": false })).await).await;
    assert_eq!(date_of(&loose), Value::Null, "{loose}");
}

#[tokio::test]
async fn site_facts_are_refused_in_a_loose_document() {
    let (_d, root) = pages();
    let host = TestHost::loose(&root);
    for (cmd, args) in [
        ("describe_source_role", json!({ "filePath": "a.md" })),
        ("page_facts", json!({ "filePath": "a.md" })),
        ("resolve_url_for_file", json!({ "filePath": "a.md" })),
        ("resolve_page_source", json!({ "urlPath": "/a/" })),
        ("list_vault_terms", json!({})),
    ] {
        match session_read::dispatch(&host, cmd, args).await {
            Some(Err(CommandError::Failed(msg))) => assert_eq!(msg, crate::vault::paths::NO_PROJECT_OPEN, "{cmd}"),
            other => panic!("{cmd}: expected the no-project refusal, got {other:?}"),
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn site_facts_answer_through_a_symlinked_root() {
    let (d, root) = pages();
    let doc = crate::build::types::ParsedDocument {
        source_path: Some("a.md".into()),
        url_path: "a/index.html".into(),
        ..Default::default()
    };
    crate::build::emit::inventory::write_inventory(&[doc], &root.join(".moss"), false, "en", None).unwrap();
    let link = d.path().join("link");
    std::os::unix::fs::symlink(&root, &link).unwrap();
    let host = TestHost::site(&link);
    let facts: Value = ok(session_read::dispatch(&host, "page_facts", json!({ "filePath": "a.md" })).await).await;
    assert_eq!(facts, json!({ "position": "page", "auto": facts["auto"].clone() }));
    assert_eq!(facts["auto"]["url"], json!("a"), "{facts}");
    let role: Value = ok(session_read::dispatch(&host, "describe_source_role", json!({ "filePath": "a.md" })).await).await;
    assert_eq!(role["can_be_page"], json!(true), "{role}");
}

#[tokio::test]
async fn a_loose_rename_moves_the_file_and_rewrites_no_link() {
    let (_d, root) = pages();
    let host = TestHost::loose(&root);
    let summary: Value =
        ok(write::dispatch(&host, "rename_entry_with_refs", json!({ "oldPath": "a.md", "newPath": "c.md" })).await).await;
    assert!(root.join("c.md").exists() && !root.join("a.md").exists());
    assert_eq!(std::fs::read_to_string(root.join("b.md")).unwrap(), "see [A](a.md)");
    assert_eq!(summary["undo_id"], json!(""), "{summary}");
    assert_eq!(*host.announced.lock().unwrap(), vec![("a.md".to_string(), "c.md".to_string())]);
}

#[tokio::test]
async fn a_site_rename_rewrites_links_and_announces_the_move() {
    let (_d, root) = pages();
    let host = TestHost::site(&root);
    ok::<Value>(write::dispatch(&host, "rename_entry_with_refs", json!({ "oldPath": "a.md", "newPath": "c.md" })).await).await;
    assert!(std::fs::read_to_string(root.join("b.md")).unwrap().contains("c.md"));
    assert_eq!(*host.announced.lock().unwrap(), vec![("a.md".to_string(), "c.md".to_string())]);
}

#[tokio::test]
async fn undo_announces_the_reverse_move() {
    let (_d, root) = pages();
    let host = TestHost::site(&root);
    let summary: Value =
        ok(write::dispatch(&host, "rename_entry_with_refs", json!({ "oldPath": "a.md", "newPath": "c.md" })).await).await;
    ok::<Value>(write::dispatch(&host, "undo_rename", json!({ "undoId": summary["undo_id"] })).await).await;
    assert!(root.join("a.md").exists());
    assert_eq!(host.announced.lock().unwrap().last(), Some(&("c.md".to_string(), "a.md".to_string())));
}

/// `decoy/x.md` makes `x.md` ambiguous, so the links cannot resolve by unique name.
#[tokio::test]
async fn a_batch_move_rewrites_links_to_each_suffixed_page_and_undoes_as_one() {
    let (_d, root) = pages();
    for (rel, text) in [("one/x.md", "# one"), ("two/x.md", "# two"), ("decoy/x.md", "# decoy"), ("c.md", "[1](one/x.md) [2](two/x.md)")] {
        std::fs::create_dir_all(root.join(rel).parent().unwrap()).unwrap();
        // allow:raw_write test fixture pages in a temp folder
        std::fs::write(root.join(rel), text).unwrap();
    }
    std::fs::create_dir(root.join("dest")).unwrap();
    let host = TestHost::site(&root);
    let summary: Value =
        ok(write::dispatch(&host, "move_entries", json!({ "paths": ["one/x.md", "two/x.md"], "targetDir": "dest" })).await).await;
    assert_eq!(std::fs::read_to_string(root.join("dest/x-2.md")).unwrap(), "# two");
    assert_eq!(std::fs::read_to_string(root.join("c.md")).unwrap(), "[1](dest/x.md) [2](dest/x-2.md)");
    assert_eq!(summary["edited_link_count"], json!(2), "{summary}");
    assert_eq!(
        *host.announced.lock().unwrap(),
        vec![("one/x.md".to_string(), "dest/x.md".to_string()), ("two/x.md".to_string(), "dest/x-2.md".to_string())]
    );
    ok::<Value>(write::dispatch(&host, "undo_rename", json!({ "undoId": summary["undo_id"] })).await).await;
    assert!(root.join("one/x.md").exists() && root.join("two/x.md").exists());
    assert_eq!(std::fs::read_to_string(root.join("c.md")).unwrap(), "[1](one/x.md) [2](two/x.md)");
}

/// A loose rename that cannot happen takes back the move it announced.
#[tokio::test]
async fn a_failed_loose_rename_withdraws_its_announcement() {
    let (_d, root) = pages();
    let host = TestHost::loose(&root);
    let out = write::dispatch(&host, "rename_entry_with_refs", json!({ "oldPath": "a.md", "newPath": "no-dir/c.md" })).await;
    assert!(matches!(out, Some(Err(_))), "{out:?}");
    assert!(root.join("a.md").exists());
    assert_eq!(*host.withdrawn.lock().unwrap(), *host.announced.lock().unwrap());
    assert!(!host.withdrawn.lock().unwrap().is_empty());
}

/// The delete is refused at its first path (outside the folder), after the
/// clean has run: a site has lost its links to `a.md`, a loose folder keeps
/// them. Nothing reaches the OS trash.
#[tokio::test]
async fn a_loose_delete_leaves_links_to_the_deleted_page() {
    let (_d, root) = pages();
    let args = json!({ "paths": ["../outside.md", "a.md"] });
    let loose = write::dispatch(&TestHost::loose(&root), "clean_references_and_delete", args.clone()).await;
    assert!(matches!(loose, Some(Err(_))), "{loose:?}");
    assert_eq!(std::fs::read_to_string(root.join("b.md")).unwrap(), "see [A](a.md)");
    let site = write::dispatch(&TestHost::site(&root), "clean_references_and_delete", args).await;
    assert!(matches!(site, Some(Err(_))), "{site:?}");
    assert!(!std::fs::read_to_string(root.join("b.md")).unwrap().contains("a.md"));
    assert!(root.join("a.md").exists());
}

#[tokio::test]
async fn the_delete_scan_counts_each_reference_once_and_none_in_a_loose_document() {
    let (_d, root) = pages();
    let args = json!({ "paths": ["a.md", "a.md", "gone.md"] });
    let site: Vec<Value> = ok(write::dispatch(&TestHost::site(&root), "scan_references_for_delete", args.clone()).await).await;
    assert_eq!(site.len(), 1, "{site:?}");
    let loose: Vec<Value> = ok(write::dispatch(&TestHost::loose(&root), "scan_references_for_delete", args).await).await;
    assert!(loose.is_empty(), "{loose:?}");
}

#[tokio::test]
async fn cloud_file_details_refuse_a_folder_that_is_not_the_open_one() {
    let (_d, root) = pages();
    let other = tempfile::tempdir().unwrap();
    let ask = |folder: &Path| json!({ "folderPath": folder.to_string_lossy() });
    match session_read::dispatch(&TestHost::site(&root), "get_cloud_unavailable_files", ask(other.path())).await {
        Some(Err(CommandError::Failed(msg))) => assert!(msg.contains("different folder"), "{msg}"),
        other => panic!("expected the folder check to refuse, got {other:?}"),
    }
}

#[tokio::test]
async fn the_preflight_answers_only_for_the_open_folder() {
    let (_d, root) = pages();
    let other = tempfile::tempdir().unwrap();
    let ask = |folder: &Path| json!({ "folderPath": folder.to_string_lossy() });
    let host = TestHost::site(&root);
    assert!(matches!(session_read::dispatch(&host, "get_publish_preflight", ask(other.path())).await, Some(Err(_))));
    let none: Option<Value> = ok(session_read::dispatch(&host, "get_publish_preflight", ask(&root)).await).await;
    assert!(none.is_none());
}

/// A folder move announces every page under it, the folder's own home page
/// renamed with it included, and its undo announces the reverse.
#[tokio::test]
async fn a_folder_move_announces_each_page_inside_and_undo_reverses_them() {
    let (_d, root) = pages();
    std::fs::create_dir_all(root.join("posts/deep")).unwrap();
    for page in ["posts/posts.md", "posts/deep/c.md"] {
        // allow:raw_write test fixture pages in a temp folder
        std::fs::write(root.join(page), "# page").unwrap();
    }
    let host = TestHost::site(&root);
    let summary: Value =
        ok(write::dispatch(&host, "rename_entry_with_refs", json!({ "oldPath": "posts", "newPath": "essays" })).await).await;
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    let mut moved = host.announced.lock().unwrap().clone();
    moved.sort();
    assert_eq!(moved, vec![pair("posts/deep/c.md", "essays/deep/c.md"), pair("posts/posts.md", "essays/essays.md")]);
    host.announced.lock().unwrap().clear();
    ok::<Value>(write::dispatch(&host, "undo_rename", json!({ "undoId": summary["undo_id"] })).await).await;
    let mut back = host.announced.lock().unwrap().clone();
    back.sort();
    assert_eq!(back, vec![pair("essays/deep/c.md", "posts/deep/c.md"), pair("essays/essays.md", "posts/posts.md")]);
}

#[test]
fn an_undo_id_works_once_and_an_unknown_one_not_at_all() {
    let store = RenameUndoStore::default();
    let id = store.summarize_and_store(Path::new("/f"), RenameApplyResult::default()).undo_id;
    assert!(store.take(&id).is_some());
    assert!(store.take(&id).is_none());
    assert!(store.take("no-such-id").is_none());
}

async fn boom(_: &dyn CommandHost, _: NoArgs) -> Result<(), String> {
    panic!("the core exploded")
}

#[tokio::test]
async fn a_panicking_command_answers_with_its_name_instead_of_unwinding() {
    let (_d, root) = pages();
    let err = call(&TestHost::site(&root), boom, NoArgs {}).await.unwrap_err();
    assert!(err.contains("`boom`") && err.contains("the core exploded"), "{err}");
}

#[test]
fn the_undo_store_keeps_only_the_newest_renames() {
    let store = RenameUndoStore::default();
    let ids: Vec<String> = (0..=RENAME_UNDO_CAPACITY)
        .map(|_| store.summarize_and_store(Path::new("/f"), RenameApplyResult::default()).undo_id)
        .collect();
    assert!(store.take(&ids[0]).is_none(), "the oldest is dropped past capacity");
    assert!(store.take(&ids[RENAME_UNDO_CAPACITY]).is_some());
}

/// An id minted in one folder does not undo anything once the host is in another.
#[tokio::test]
async fn undo_refuses_a_rename_from_another_folder() {
    let host = TestHost::site(Path::new("/now/open"));
    let id = host.undo.summarize_and_store(Path::new("/was/open"), RenameApplyResult::default()).undo_id;
    match write::dispatch(&host, "undo_rename", json!({ "undoId": id })).await {
        Some(Err(CommandError::Failed(msg))) => assert!(msg.contains("folder has changed"), "{msg}"),
        other => panic!("expected the folder check to refuse, got {other:?}"),
    }
}

const CONTRACT: &str = "../../packages/moss-ui/src/engine.generated.ts";

/// The engine's command contract in TypeScript: every argument and answer
/// type, one entry per command, and each command's tier.
fn render_contract() -> String {
    use specta::datatype::FunctionResultVariant;
    use specta_typescript::{BigIntExportBehavior, Typescript};

    let mut types = specta::TypeMap::default();
    let tiers = [
        public_read::signatures(&mut types),
        session_read::signatures(&mut types),
        write::signatures(&mut types),
    ];
    let ts = Typescript::default().bigint(BigIntExportBehavior::Number).remove_default_header();
    let render = |dt: &specta::DataType| {
        specta_typescript::datatype(&ts, &FunctionResultVariant::Value(dt.clone()), &types).expect("renders")
    };

    let mut commands = String::from("export type EngineCommands = {\n");
    let mut tier_of = String::from("export const ENGINE_TIER = {\n");
    for (tier, signatures) in &tiers {
        for (name, args, out) in signatures {
            commands += &format!("\t{name}: {{ args: {}; out: {} }};\n", render(args), render(out));
            tier_of += &format!("\t{name}: \"{tier}\",\n");
        }
    }
    commands += "};\n";
    tier_of += "} as const;\n";

    let named = specta::Language::export(&ts, types.clone()).expect("exports");
    format!(
        "// @generated from moss-build's command table. Do not edit.\n\
         // Regenerate: UPDATE_ENGINE_CONTRACT=1 cargo test -p moss-build --lib commands::tests::the_contract\n\n\
         {named}{commands}\nexport type EngineCommand = keyof EngineCommands;\n\n{tier_of}"
    )
}

#[test]
fn the_contract_matches_the_command_table() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(CONTRACT);
    let rendered = render_contract();
    if std::env::var_os("UPDATE_ENGINE_CONTRACT").is_some() {
        // allow:raw_write a checked-in source file in this repository, not build output
        std::fs::write(&path, &rendered).expect("write the contract");
        return;
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        on_disk == rendered,
        "{} is stale; regenerate it with UPDATE_ENGINE_CONTRACT=1 cargo test -p moss-build --lib commands::tests::the_contract",
        path.display()
    );
}
