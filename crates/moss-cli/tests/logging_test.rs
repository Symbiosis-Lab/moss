//! Proves `moss-cli::main`'s `install_headless_logger()` call actually
//! reaches every crossed verb, not just `build`/`deploy`/`history` (the
//! three that already called it from inside moss-build) — and that
//! installing a stderr logger for every verb never leaks a line into a
//! `--json` command's stdout.
//!
//! Both tests spawn the real compiled binary (`CARGO_BIN_EXE_moss-cli`),
//! the same pattern `folder_build_lock_test.rs` and `served_child.rs` use,
//! so a regression here means the shipped binary is silent again, not just
//! an in-process `log::Record`.

use std::fs;
use std::process::Command;

/// A local Strikingly-shaped blog post: one recognized `RichText` section
/// plus one component type the dialect has no match arm for
/// (`FancyNewWidget`) — the same shape
/// `strikingly::unknown_component_types_are_skipped_and_counted_without_panic`
/// uses to reach `log_skipped`'s `log::warn!`. Routed here through
/// `import_local_file` (no network: `moss import <path.html> <dir>` parses
/// it offline), which is exactly the code path the gap report named.
const STRIKINGLY_BLOG_HTML: &str = r#"<html><head><title>Test</title></head><body>
<script>//<![CDATA[
window.$S={};$S.blogPostData={"blogPostMeta":{"publishedAt":"2024-10-13T20:08:41.387-07:00","socialMediaConfig":{"url":"https://example.test/blog/post","title":"Test Post"}},"content":{"type":"Blog.Post","sections":[{"type":"Blog.Section","id":"s1","component":{"type":"RichText","id":"c1","value":"<p>Known paragraph content.</p>"}},{"type":"Blog.Section","id":"s2","component":{"type":"FancyNewWidget","id":"w1","data":{"nested":[1,2,3]}}}]}};$S.stores={"blogData":{}};
//]]></script>
<img src="https://custom-images.strikinglycdn.com/res/hrscywv4p/image/upload/c_limit/123/x.jpeg">
</body></html>"#;

#[test]
fn import_warning_reaches_stderr_through_the_installed_logger() {
    let input_dir = tempfile::tempdir().expect("tempdir for input");
    let output_dir = tempfile::tempdir().expect("tempdir for output");
    let html_path = input_dir.path().join("post.html");
    fs::write(&html_path, STRIKINGLY_BLOG_HTML).expect("write fixture html");

    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("import")
        .arg(&html_path)
        .arg(output_dir.path())
        .output()
        .expect("spawn moss-cli import");

    assert!(
        out.status.success(),
        "import of a well-formed local fixture should succeed; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("strikingly import: skipped unknown component types") && stderr.contains("FancyNewWidget"),
        "the strikingly adapter's log::warn! never reached stderr — `import` has no logging \
         backend installed; stderr was:\n{stderr}"
    );
}

#[test]
fn json_output_commands_keep_stdout_free_of_log_lines() {
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("describe")
        .arg("--json")
        .output()
        .expect("spawn moss-cli describe --json");

    assert!(
        out.status.success(),
        "describe --json should succeed; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "describe --json stdout was not valid JSON — a log line likely leaked into it: {e}\n\
             stdout was:\n{}",
            String::from_utf8_lossy(&out.stdout)
        )
    });
    assert!(parsed.is_object(), "describe --json should emit a JSON object");
}
