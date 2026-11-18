//! `moss deploy` hands its deploy plugin a copy of exactly what it publishes:
//! a real build and publish through a fixture plugin that looks at
//! `context.site_dir` with a granted `sh`, writes `context.index_file`, and
//! tries a working directory outside the site. Unix only: the plugin runs `sh`.
#![cfg(unix)]

use std::path::Path;
use std::process::Command;

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

const PLUGIN: &str = r#"
window.LocaldirPlugin = { deploy: async function (ctx) {
  var invoke = window.__TAURI__.core.invoke;
  var seen = await invoke("execute_binary", {
    binaryPath: "sh",
    args: ["-c", "printf x > \"$GIT_INDEX_FILE\" && find . -type f | sort && ls -l index.html"],
    workingDir: ctx.site_dir,
    env: { GIT_INDEX_FILE: ctx.index_file },
    timeoutMs: 10000
  });
  var outside = "";
  try {
    await invoke("execute_binary", { binaryPath: "sh", args: ["-c", "true"], workingDir: "/", timeoutMs: 10000 });
  } catch (e) { outside = String(e); }
  var report = { site_dir: ctx.site_dir, index_file: ctx.index_file, listed: ctx.site_files,
                 seen: seen.stdout, stderr: seen.stderr, outside: outside };
  return { success: true, message: "REPORT " + JSON.stringify(report),
           deployment: { method: "local", url: "http://127.0.0.1:9", deployed_at: new Date().toISOString(), metadata: {} } };
} };
"#;

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[test]
fn a_deploy_hook_reads_a_copy_of_exactly_the_published_files_that_is_gone_afterwards() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path().join("site");
    std::fs::create_dir_all(&home).unwrap();
    write(&root, "index.md", "---\ntitle: Home\n---\nWelcome.\n");
    write(&root, "about.md", "---\ntitle: About\n---\nAbout us.\n");
    write(&root, "wip.md", "---\ntitle: Secret Plan\ndraft: true\n---\nNot yet.\n");
    write(&root, ".moss/config.toml", "[hooks]\ndeploy = \"localdir\"\n");
    write(
        &root,
        ".moss/plugins/localdir/manifest.json",
        r#"{"name":"localdir","version":"0.0.1","display_name":"Local","description":"","author":"","entry":"main.bundle.js","global_name":"LocaldirPlugin","requires":["execute_binary:sh"],"contributes":{"deploy_target":{}}}"#,
    );
    write(&root, ".moss/plugins/localdir/main.bundle.js", PLUGIN);

    let out = Command::new(MOSS)
        .args(["deploy", root.to_str().unwrap(), "--allow-plugins"])
        .env("MOSS_HOME", &home)
        .env("HOME", &home)
        .env_remove("MOSS_ENV")
        .env_remove("MOSS_SETA_URL")
        .output()
        .expect("spawn moss-cli");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "deploy failed: {stderr}");
    let line = stderr.lines().find_map(|l| l.strip_prefix("REPORT ")).unwrap_or_else(|| panic!("{stderr}"));
    let report: serde_json::Value = serde_json::from_str(line).unwrap();
    let text = |k: &str| report[k].as_str().unwrap().to_string();

    let site_dir = Path::new(&text("site_dir")).to_path_buf();
    assert!(site_dir.is_absolute(), "{report}");
    let root = root.canonicalize().unwrap();
    assert!(site_dir.starts_with(root.join(".moss/build.nosync/deploy")), "{report}");

    // The copy holds the listed files and nothing else: the draft is withheld.
    let mut listed: Vec<String> =
        report["listed"].as_array().unwrap().iter().map(|v| format!("./{}", v.as_str().unwrap())).collect();
    listed.sort();
    let seen = text("seen");
    let found: Vec<&str> = seen.lines().filter(|l| l.starts_with("./")).collect();
    assert_eq!(found, listed, "{report}");
    assert!(found.contains(&"./index.html") && !found.iter().any(|f| f.contains("wip")), "{report}");
    let generation: Vec<String> = walk(&root.join(".moss/build.nosync/current"));
    assert!(generation.iter().any(|f| f.contains("wip")), "the build keeps the draft for preview: {generation:?}");
    assert!(seen.lines().any(|l| l.starts_with("-r--r--r--")), "the copy is read-only: {report}");

    assert!(text("stderr").is_empty(), "GIT_INDEX_FILE = context.index_file is accepted: {report}");
    assert!(text("outside").contains("working directory"), "a working directory outside the site refuses: {report}");

    // Gone once the hook returned: the copy and the index beside it.
    assert!(!site_dir.exists(), "{report}");
    assert!(!Path::new(&text("index_file")).exists(), "{report}");
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path.to_string_lossy().into_owned());
        }
    }
    out
}
