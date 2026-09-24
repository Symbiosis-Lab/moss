//! Source test: what every publish path owes once it has landed — the record
//! written AFTER the landing and never before.
//!
//! Open-half twin of three checks in the desktop app's
//! `publish_record_ordering_test.rs`:
//!
//! 1. `last_published_record_is_saved_after_the_commit` — reads
//!    `src/deploy/push.rs`, wholly this crate's own source. Moved verbatim.
//! 2. `the_plugin_path_records_after_the_plugin_reported_success` — reads
//!    `src/deploy/plugin_push.rs`, wholly this crate's own source. Moved
//!    verbatim.
//! 3. The open half of `the_shared_helper_is_what_writes_the_record` — the
//!    desktop original also asserts `deploy::app_seam::recompute_after_landing`
//!    (a desktop-only file) drops the pre-publish change set. That half stays
//!    on the desktop side; this twin keeps only the two assertions about
//!    `src/deploy/landed.rs` itself.
//!
//! Not carried here: `the_app_publishes_through_the_shared_plugin_driver`,
//! `deploy_site_message_matches_the_frontend`,
//! `the_plugin_path_publishes_under_the_publish_guard`, and
//! `every_app_publish_path_stamps_the_folder` — each reads a desktop-only file
//! (the app's preview commands, its deploy driver, its headless-startup
//! module) or the frontend tree, none of which this row classifies as
//! splittable.

use std::path::Path;

/// Extract the body of `fn <name>` from Rust source by brace matching.
fn fn_body<'a>(src: &'a str, name: &str) -> &'a str {
    let sig = format!("fn {name}(");
    let start = src
        .find(&sig)
        .unwrap_or_else(|| panic!("`fn {name}` not found — was it renamed? Update this invariant."));
    let open =
        src[start..].find('{').unwrap_or_else(|| panic!("no body brace for `fn {name}`")) + start;
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    for i in open..bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &src[open..=i];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces in `fn {name}`");
}

#[test]
fn last_published_record_is_saved_after_the_commit() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/deploy/push.rs");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let body = fn_body(&src, "push_site_inner_impl");

    let commit = body.find("commit_sync(").expect(
        "`push_site_inner_impl` must call `commit_sync` — if the commit moved to a helper, this \
         invariant needs re-deriving, not deleting: the record must still follow the commit.",
    );
    let save = body.find("record_landed").expect(
        "`push_site_inner_impl` must record what went live via `landed::record_landed`. Without \
         it every publish is unclassified forever and the publish button can only ever show a \
         flat file count.",
    );

    assert!(
        save > commit,
        "`published_record::save` must come AFTER `commit_sync` in `push_site_inner_impl` \
         (found save at byte {save}, commit at byte {commit}). A record written before the \
         commit — or on a path where the commit failed — claims pages are live that never \
         shipped, so the NEXT publish reports nothing to publish while real changes sit \
         unshipped. Everything before the commit `?`-propagates, which is what makes \
         after-the-commit also mean only-on-success."
    );
}

/// The plugin path's twin. Its "the publish landed" is the plugin returning
/// `success` with a deployment — for OnionPress, after the receiver's
/// `/commit` returned — so the record must follow `execute_deploy`.
#[test]
fn the_plugin_path_records_after_the_plugin_reported_success() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/deploy/plugin_push.rs");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let body = fn_body(&src, "run_plugin_deploy_inner");

    let executed = body.find("execute_deploy(").expect(
        "`run_plugin_deploy_inner` must call `execute_deploy` — if it moved to a helper, this \
         invariant needs re-deriving, not deleting: the record must still follow the deploy.",
    );
    let save = body.find("record_landing(").expect(
        "`run_plugin_deploy_inner` must record what went live. Without it every plugin publish \
         is unclassified forever, and the publish button tells an OnionPress author \"no machine \
         record\" after every single publish.",
    );
    assert!(
        save > executed,
        "the record must be written AFTER `execute_deploy` in `run_plugin_deploy_inner` (found \
         record at byte {save}, deploy at byte {executed}). A record written first claims pages \
         are live that never shipped, so the next publish reports nothing to publish while real \
         changes sit unshipped."
    );
    assert!(
        fn_body(&src, "record_landing").contains("record_landed("),
        "`record_landing` must delegate to `landed::record_landed`, or the ordering asserted \
         above is about a call that writes nothing."
    );
}

/// Open half of the desktop original's `the_shared_helper_is_what_writes_the_record`:
/// both publish paths delegate the recording to `landed::record_landed`, so
/// the two ordering assertions above are only worth anything if that helper
/// actually writes the record and also keeps the live-article mapping
/// current. The desktop half separately checks that its own
/// `deploy::app_seam::recompute_after_landing` clears the pre-publish change
/// set — that assertion stays on the desktop side.
#[test]
fn the_shared_helper_is_what_writes_the_record() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/deploy/landed.rs");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    assert!(
        src.contains("published_record::save"),
        "`deploy::landed` must call `published_record::save` — it is the only thing standing \
         between a landed publish and a change set that can name verbs. If the write moved \
         again, re-point the two ordering tests at wherever it went."
    );
    assert!(
        src.contains("read_live_article_mapping"),
        "`deploy::landed` must also record which note ID is live at which page — and where \
         that page is served, so the mapping cannot drift out of step with the note IDs the \
         way a past bug let it — for the same reason the two ordering tests exist: it is \
         what the NEXT build diffs to find renamed pages. This lived in `push_site_inner_impl` \
         alone until 2026-08-17, so a site published through a deploy plugin never got a \
         redirect stub when a page moved; it was a second file of its own until a fix folded \
         it into this one."
    );
}
