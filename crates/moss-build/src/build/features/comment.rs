//! Comment feature: renders threaded comment sections for articles.
//!
//! Reads comment data from `.moss/data/social/comment.json` (keyed by article uid)
//! and generates HTML comment sections matching the former comment plugin output.

pub mod event_log;
pub mod moderation;
pub mod reduce;
pub mod render;
pub mod sanitize;
pub mod sync_remote;

// Re-exports consumed by features.rs and features/sync.rs
pub use render::render_comment_section;
// process_comments is pub(in crate::build::features) — only sync.rs (same features
// module) may call it via super::comment::process_comments(…) unchanged.
pub(in crate::build::features) use sync_remote::process_comments;

use crate::i18n::{t, Language};
use crate::moss_paths::MossPaths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::HashMap;

/// Comment data for the entire site, loaded from `.moss/data/social/comment.json`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommentData {
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    // BTreeMap: serde_json serializes in iteration (key-sorted) order, giving
    // deterministic output across processes. HashMap order is random per-process,
    // which would defeat process_comments' write-only-on-change guard (spurious
    // rewrite + rebuild every session) and make comment.json diffs noisy in
    // git-tracked vaults.
    pub articles: BTreeMap<String, ArticleComments>,
}

/// Comments for a single article.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArticleComments {
    pub comments: Vec<NormalizedComment>,
}

pub(super) fn default_source() -> String {
    "artalk".to_string()
}

/// A single normalized comment, shared across all social sources (Artalk fetch, Matters, Douban, …).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NormalizedComment {
    pub id: String,
    /// Which sync wrote this comment: "artalk" | "matters" | "douban" | <file stem>.
    /// Drives reply affordance (in-page form vs syndicated link-out) and
    /// thread-tree namespacing. Defaults to "artalk" for legacy comment.json.
    #[serde(default = "default_source")]
    pub source: String,
    pub content: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub author: CommentAuthor,
    #[serde(rename = "replyToId", skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
    /// Legacy tombstone field: absent or "active" = visible; anything else =
    /// tombstoned. Filtered at render-load time by `normalize_social_comment`
    /// as a defense-in-depth safeguard during the migration window. New
    /// entries written by `normalize_artalk_comment` always set this to None.
    /// After the one-time tombstone→signed-event migration runs in
    /// `sync_remote::migrate_tombstones_to_signed_events`, pre-existing
    /// tombstoned records are replaced by server-wins reconcile and this field
    /// disappears from the JSON. DO NOT use as the moderation signal — signed
    /// hide events in `moderation.jsonl` are the sole bake-time authority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// Comment author info.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommentAuthor {
    #[serde(rename = "displayName", skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl CommentAuthor {
    pub fn label(&self, lang: Language) -> &str {
        self.display_name
            .as_deref()
            .or(self.name.as_deref())
            .unwrap_or(t(lang, "anonymous"))
    }
}

/// Localized strings for comment UI.
pub struct CommentI18n {
    pub comment_count_zero: &'static str,
    pub comment_count_one: &'static str,
    pub comment_count_many: &'static str,
    pub placeholder: &'static str,
    pub name_label: &'static str,
    pub email_label: &'static str,
    pub website_label: &'static str,
    pub reply_btn: &'static str,
}

/// Get i18n strings for the given language, sourced from the central
/// registry (`crate::i18n::strings`) so all three languages — including
/// Traditional Chinese — stay in one place.
pub fn get_i18n(lang: Language) -> CommentI18n {
    CommentI18n {
        comment_count_zero: t(lang, "comment_count_zero"),
        comment_count_one: t(lang, "comment_count_one"),
        comment_count_many: t(lang, "comment_count_many"),
        placeholder: t(lang, "comment_placeholder"),
        name_label: t(lang, "comment_name"),
        email_label: t(lang, "comment_email"),
        website_label: t(lang, "comment_website"),
        reply_btn: t(lang, "comment_reply"),
    }
}

/// Normalize a comment from any social source into NormalizedComment.
///
/// Handles varying formats: Artalk (id/content/date/nick), Matters
/// (id/content/createdAt/author.displayName), Douban, etc.
/// Filters out inactive comments (state must be absent or "active").
///
/// `fallback_source` is the file stem of the `.moss/data/social/*.json` file
/// being loaded (e.g. `"comment"` for `comment.json`, `"matters"` for
/// `matters.json`). The stored `"source"` field in the comment wins; legacy
/// entries that predate schema 1.1.0 fall back to `fallback_source`, with
/// `"comment"` (comment.json's stem) mapped to its real provider, `"artalk"`.
///
/// RENDER PATH ONLY: non-active comments are filtered out here so tombstones
/// never display. The sync/reconcile path loads CommentData directly
/// (load_comment_data) and DOES see tombstoned records — do not reuse this
/// normalizer there.
fn normalize_social_comment(raw: &serde_json::Value, fallback_source: &str, matters_domain: &str) -> Option<NormalizedComment> {
    // ID: string or integer
    let id = raw.get("id").and_then(|i| {
        i.as_str().map(|s| s.to_string())
            .or_else(|| i.as_i64().map(|n| n.to_string()))
    })?;

    // Defense-in-depth: sanitize at render-load too. Newly-synced entries are
    // already sanitized at ingest (sync_remote), but legacy comment.json entries
    // and syndicated mirrors may hold unsanitized HTML. Idempotent.
    let content = sanitize::sanitize_comment_html(raw.get("content").and_then(|c| c.as_str())?);

    // Filter inactive
    if let Some(state) = raw.get("state").and_then(|s| s.as_str()) {
        if state != "active" { return None; }
    }

    let created_at = raw.get("createdAt")
        .or_else(|| raw.get("date"))
        .and_then(|d| d.as_str())
        .unwrap_or("")
        .to_string();

    // Source resolution: the stored field wins (sync writes it since schema
    // 1.1.0); legacy entries fall back to the originating file stem, with
    // comment.json's stem ("comment") mapping to its real source, artalk.
    let source = raw
        .get("source")
        .and_then(|s| s.as_str())
        .map(String::from)
        .unwrap_or_else(|| {
            if fallback_source == "comment" { "artalk".to_string() } else { fallback_source.to_string() }
        });

    let author_obj = raw.get("author");
    let display_name = author_obj
        .and_then(|a| {
            a.get("displayName").or(a.get("name")).or(a.get("userName"))
        })
        .and_then(|n| n.as_str())
        .unwrap_or("Anonymous")
        .to_string();

    let author_url = author_obj
        .and_then(|a| a.get("url"))
        .and_then(|u| u.as_str())
        .and_then(|s| if s.is_empty() { None } else { Some(s.to_string()) })
        .or_else(|| {
            if source == "matters" {
                author_obj
                    .and_then(|a| a.get("userName"))
                    .and_then(|u| u.as_str())
                    .map(|name| format!("https://{}/@{}", matters_domain, name))
            } else {
                None
            }
        });

    let reply_to_id = raw.get("replyToId")
        .and_then(|r| r.as_str())
        .and_then(|s| if s.is_empty() { None } else { Some(s.to_string()) });

    Some(NormalizedComment {
        id,
        source,
        content,
        created_at,
        author: CommentAuthor {
            display_name: Some(display_name),
            name: None,
            url: author_url,
        },
        reply_to_id,
        state: raw.get("state").and_then(|s| s.as_str()).map(String::from),
    })
}

/// The domain used when nobody resolved one — the same string
/// `resolve_matters_domain` falls back to.
pub const MATTERS_DOMAIN_FALLBACK: &str = "matters.town";

/// Decide the Matters domain override for an environment. An explicit process-env
/// value wins; otherwise non-production uses the test instance and production uses
/// the default (None ⇒ plugin falls back to its built-in `matters.town`,
/// [`MATTERS_DOMAIN_FALLBACK`]).
///
/// Lived in `plugins::runtime` until 2026-08-27; it is a pure function of the
/// environment, and the build's slot resolver needs it, so it lives with the
/// fallback it completes (the compiler never names the runtime).
pub fn resolve_matters_domain(
    env: &crate::config::environment::HostingEnvironment,
    explicit: Option<String>,
) -> Option<String> {
    if let Some(v) = explicit {
        if !v.is_empty() {
            return Some(v);
        }
    }
    match env {
        crate::config::environment::HostingEnvironment::Production => None,
        _ => Some("matters.icu".to_string()),
    }
}

/// Load comments from ALL `.moss/data/social/*.json` files (except review.json).
///
/// Returns comments grouped by article key (uid). All social sources
/// (comment.json, matters.json, douban.json, etc.) are merged.
/// Comments are sorted newest-first within each article.
pub fn load_all_social_comments(
    project_path: &str,
    // The Matters domain to attribute matters.json comments to. Handed in
    // rather than resolved here: resolving it means calling
    // `plugins::runtime::resolve_matters_domain`, which keeps the
    // compiler clear of the plugin runtime. Callers that have no opinion pass
    // `MATTERS_DOMAIN_FALLBACK`.
    matters_domain: &str,
) -> HashMap<String, Vec<NormalizedComment>> {
    let social_dir = MossPaths::new(std::path::Path::new(project_path)).social_dir();
    if !social_dir.exists() {
        return HashMap::new();
    }

    let mut by_key: HashMap<String, Vec<NormalizedComment>> = HashMap::new();

    let entries = match std::fs::read_dir(&social_dir) {
        Ok(e) => e,
        Err(_) => return HashMap::new(),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // Skip review.json (not comments)
        if path.file_stem().and_then(|s| s.to_str()) == Some("review") {
            continue;
        }

        let source = path.file_stem().unwrap().to_string_lossy().to_string();
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let data: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let articles = match data.get("articles").and_then(|a| a.as_object()) {
            Some(a) => a,
            None => continue,
        };

        for (key, article_data) in articles {
            let comments_arr = match article_data.get("comments").and_then(|c| c.as_array()) {
                Some(c) => c,
                None => continue,
            };
            let normalized: Vec<NormalizedComment> = comments_arr
                .iter()
                .filter_map(|c| normalize_social_comment(c, &source, matters_domain))
                .collect();
            if !normalized.is_empty() {
                by_key.entry(key.clone()).or_default().extend(normalized);
            }
        }
    }

    // Sort each article's comments into a deterministic total order.
    for comments in by_key.values_mut() {
        sort_comments_deterministic(comments);
    }

    by_key
}

/// Sort an article's comments into a deterministic, total order: newest first by
/// `created_at`, then by `(source, id)` to break ties. Without the tiebreak, two
/// comments sharing a `created_at` keep `read_dir` accumulation order, which is not
/// stable across platforms — defeating deploy-manifest determinism.
fn sort_comments_deterministic(comments: &mut Vec<NormalizedComment>) {
    comments.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// Load comment data from `.moss/data/social/comment.json`.
pub fn load_comment_data(project_path: &str) -> Option<CommentData> {
    let path = MossPaths::new(std::path::Path::new(project_path))
        .social_dir()
        .join("comment.json");

    if !path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Data loading ---

    #[test]
    fn test_load_comment_data_missing_file() {
        assert!(load_comment_data("/nonexistent/path").is_none());
    }

    #[test]
    fn test_load_comment_data_round_trip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let social_dir = tmp.path().join(".moss").join("data").join("social");
        std::fs::create_dir_all(&social_dir).unwrap();
        // Write existing data
        std::fs::write(
            social_dir.join("comment.json"),
            r#"{"schemaVersion":"1.0.0","articles":{"uid1":{"comments":[{"id":"1","content":"old","createdAt":"2026-01-01","author":{"displayName":"A","name":"A","url":null},"replyToId":null}]}}}"#,
        ).unwrap();
        // Verify we can load it back
        let data = load_comment_data(tmp.path().to_str().unwrap());
        assert!(data.is_some());
        let data = data.unwrap();
        assert_eq!(data.articles.get("uid1").unwrap().comments.len(), 1);
    }

    // --- Normalize functions ---

    #[test]
    fn test_normalize_social_comment_matters() {
        let raw = serde_json::json!({
            "id": "abc123",
            "content": "<p>Great post!</p>",
            "createdAt": "2026-03-01T10:00:00Z",
            "author": { "displayName": "Alice", "userName": "alice" }
        });
        let c = normalize_social_comment(&raw, "matters", "matters.town").unwrap();
        assert_eq!(c.id, "abc123");
        assert_eq!(c.author.label(Language::En), "Alice");
        assert_eq!(c.author.url, Some("https://matters.town/@alice".to_string()));
    }

    #[test]
    fn test_normalize_social_comment_filters_inactive() {
        let raw = serde_json::json!({
            "id": "x1",
            "content": "deleted",
            "state": "collapsed",
            "author": { "name": "Bob" }
        });
        assert!(normalize_social_comment(&raw, "matters", "matters.town").is_none());
    }

    #[test]
    fn test_normalize_social_comment_active_state() {
        let raw = serde_json::json!({
            "id": "x2",
            "content": "<p>visible</p>",
            "state": "active",
            "author": { "name": "Carol" }
        });
        assert!(normalize_social_comment(&raw, "matters", "matters.town").is_some());
    }

    #[test]
    fn test_normalize_social_comment_sanitizes_content_at_render_load() {
        // Defense-in-depth: a legacy/syndicated entry whose stored content carries
        // a script must be neutralized when loaded for render.
        let raw = serde_json::json!({
            "id": "z1",
            "content": "<p>ok</p><script>alert(1)</script><img src=x onerror=alert(1)>",
            "author": { "name": "Eve" }
        });
        let c = normalize_social_comment(&raw, "comment", "matters.town").unwrap();
        assert!(c.content.contains("<p>ok</p>"), "allowed content survives: {}", c.content);
        assert!(!c.content.contains("<script"), "script stripped: {}", c.content);
        assert!(!c.content.contains("onerror"), "handler stripped: {}", c.content);
    }

    #[test]
    fn test_load_all_social_comments_reads_multiple_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let social_dir = tmp.path().join(".moss").join("data").join("social");
        std::fs::create_dir_all(&social_dir).unwrap();

        // comment.json — keyed by uid
        std::fs::write(social_dir.join("comment.json"), r#"{
            "schemaVersion": "1.0.0",
            "articles": {
                "uid1": { "comments": [{"id": "c1", "content": "hello", "createdAt": "2026-01-01", "author": {"name": "A"}}] }
            }
        }"#).unwrap();

        // matters.json — also keyed by uid
        std::fs::write(social_dir.join("matters.json"), r#"{
            "schemaVersion": "1.0.0",
            "articles": {
                "uid1": { "comments": [{"id": "m1", "content": "from matters", "createdAt": "2026-01-02", "author": {"displayName": "B", "userName": "bob"}}] },
                "uid2": { "comments": [{"id": "m2", "content": "other article", "createdAt": "2026-01-03", "author": {"name": "C"}}] }
            }
        }"#).unwrap();

        // review.json should be skipped
        std::fs::write(social_dir.join("review.json"), r#"{"schemaVersion":"1.0.0","articles":{}}"#).unwrap();

        let result = load_all_social_comments(tmp.path().to_str().unwrap(), MATTERS_DOMAIN_FALLBACK);

        // uid1 should have 2 comments (merged from comment.json + matters.json)
        assert_eq!(result.get("uid1").map(|c| c.len()), Some(2));
        // uid2 should have 1 comment (from matters.json only)
        assert_eq!(result.get("uid2").map(|c| c.len()), Some(1));
        // Matters author URL should be resolved
        let uid2_comments = result.get("uid2").unwrap();
        assert!(uid2_comments[0].author.url.is_none()); // "C" has no userName
    }

    #[test]
    fn test_load_all_social_comments_empty_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = load_all_social_comments(tmp.path().to_str().unwrap(), MATTERS_DOMAIN_FALLBACK);
        assert!(result.is_empty());
    }

    /// Writer and reader must agree on one path. The Matters plugin never
    /// touches the filesystem directly — it calls `writeFile`, which is
    /// `write_project_file_impl` — so this writes through that same command
    /// body, exactly like the plugin does, and confirms `load_all_social_comments`
    /// (the build-side reader) sees the result. A regression in the `.moss/`
    /// sandbox that blocks this path would leave the write silently refused
    /// while this test still compiles against a mock-free real reader — the
    /// gap real social-data writers actually hit.
    #[tokio::test]
    async fn social_data_written_through_write_project_file_is_visible_to_the_reader() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_path = tmp.path().to_str().unwrap();

        let payload = serde_json::json!({
            "schemaVersion": "1.1.0",
            "articles": {
                "uid-from-plugin": {
                    "comments": [{
                        "id": "m1",
                        "source": "matters",
                        "content": "<p>via write_project_file</p>",
                        "createdAt": "2026-09-01T00:00:00Z",
                        "author": { "displayName": "Reader", "userName": "reader" }
                    }]
                }
            }
        });

        crate::plugins::project_files::write_project_file_impl(
            project_path,
            Some("matters"),
            ".moss/data/social/matters.json",
            &payload.to_string(),
        )
        .await
        .expect("the plugin's write_project_file call must be allowed for its own shared social-data file");

        let result = load_all_social_comments(project_path, MATTERS_DOMAIN_FALLBACK);
        let comments = result.get("uid-from-plugin").expect("reader must find the article the plugin wrote");
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].content, "<p>via write_project_file</p>");
    }

    /// Regression: `load_all_social_comments` previously passed the file stem
    /// "comment" as the `source` field, stamping Artalk comments with
    /// `source: "comment"` instead of `"artalk"`. This broke the reply-button
    /// branch (`if comment.source == "artalk"`) and produced wrong DOM ids
    /// (`comment-comment-8` instead of `comment-artalk-8`).
    ///
    /// Verified fix: stored `"source"` field wins; legacy entries (no stored
    /// field) get `"comment"` stem mapped to `"artalk"`.
    #[test]
    fn test_load_all_social_comments_source_never_comment_stem() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf();
        let tmp_base = repo_root.join("target").join("test-tmp");
        std::fs::create_dir_all(&tmp_base).unwrap();
        let tmp = tempfile::TempDir::new_in(&tmp_base).unwrap();

        let social_dir = tmp.path().join(".moss").join("data").join("social");
        std::fs::create_dir_all(&social_dir).unwrap();

        // Legacy-style entry: NO "source" field, id "8"
        // New-style entry:    "source": "artalk", id "10"
        std::fs::write(social_dir.join("comment.json"), r#"{
            "schemaVersion": "1.0.0",
            "articles": {
                "uid1": { "comments": [
                    {"id": "8",  "content": "legacy",  "createdAt": "2026-01-01", "author": {"name": "A"}},
                    {"id": "10", "content": "new-style", "createdAt": "2026-01-02", "source": "artalk", "author": {"name": "B"}}
                ]}
            }
        }"#).unwrap();

        let result = load_all_social_comments(tmp.path().to_str().unwrap(), MATTERS_DOMAIN_FALLBACK);
        let comments = result.get("uid1").expect("uid1 must be present");
        assert_eq!(comments.len(), 2, "both comments should load");
        for c in comments {
            assert_eq!(c.source, "artalk",
                "comment id={} should have source='artalk', got '{}'", c.id, c.source);
        }

        // Render and verify affordances
        let html = render_comment_section(
            comments, "https://comments.example.com", "example.com",
            "uid1", "Test", Language::En, &[], 0,
        );
        assert!(html.contains("data-reply-id=\"8\""),
            "legacy comment must get reply button; got:\n{html}");
        assert!(html.contains("id=\"comment-artalk-8\""),
            "DOM id must be comment-artalk-8; got:\n{html}");
        assert!(html.contains("data-reply-id=\"10\""),
            "new-style comment must get reply button; got:\n{html}");
        assert!(html.contains("id=\"comment-artalk-10\""),
            "DOM id must be comment-artalk-10; got:\n{html}");
        // No "comment-comment-" ids anywhere
        assert!(!html.contains("id=\"comment-comment-"),
            "source mis-stamp detected (comment-comment-*) in:\n{html}");
    }

    #[test]
    fn test_normalize_social_comment_matters_staging_domain() {
        // In a staging/local project `load_all_social_comments` resolves
        // matters.icu as the domain and passes it down. Verify that
        // `normalize_social_comment` with "matters.icu" produces the right
        // author URL.
        let raw = serde_json::json!({
            "id": "abc123",
            "content": "<p>Great post!</p>",
            "createdAt": "2026-03-01T10:00:00Z",
            "author": { "displayName": "Alice", "userName": "alice" }
        });
        let c = normalize_social_comment(&raw, "matters", "matters.icu").unwrap();
        assert_eq!(c.author.url, Some("https://matters.icu/@alice".to_string()));
    }

    #[test]
    fn test_normalize_social_comment_stamps_source() {
        let raw = serde_json::json!({
            "id": "m1", "content": "<p>hi</p>", "createdAt": "2026-01-01",
            "author": { "displayName": "A", "userName": "a" }
        });
        let c = normalize_social_comment(&raw, "matters", "matters.town").unwrap();
        assert_eq!(c.source, "matters");
    }

    #[test]
    fn test_comment_json_defaults_source_to_artalk() {
        // Legacy comment.json entries have no `source`; serde default applies.
        let c: NormalizedComment = serde_json::from_str(
            r#"{"id":"8","content":"x","createdAt":"2026-01-01","author":{"displayName":"M","name":null,"url":null},"replyToId":null}"#,
        )
        .unwrap();
        assert_eq!(c.source, "artalk");
    }

    fn make_comment(id: &str, source: &str, created_at: &str) -> NormalizedComment {
        NormalizedComment {
            id: id.to_string(),
            source: source.to_string(),
            content: "<p>test</p>".to_string(),
            created_at: created_at.to_string(),
            author: CommentAuthor {
                display_name: Some("Tester".to_string()),
                name: None,
                url: None,
            },
            reply_to_id: None,
            state: None,
        }
    }

    /// `sort_comments_deterministic` must produce a byte-identical order regardless
    /// of input (read_dir accumulation) order. Two comments sharing a `created_at`
    /// must be stable-sorted by `(source, id)` so comment HTML is identical
    /// across macOS dev and Linux CI — the core of deploy-manifest determinism.
    #[test]
    fn sort_comments_deterministic_is_total_order_and_stable_across_input_orders() {
        // Three comments: c1 and c2 share the same created_at but differ in
        // (source, id). c3 is older.  Two vecs in different input orders simulate
        // read_dir returning files in different sequences on macOS vs Linux.
        let c1 = make_comment("100", "artalk", "2026-03-15T12:00:00Z");
        let c2 = make_comment("m-xyz", "matters", "2026-03-15T12:00:00Z"); // same timestamp
        let c3 = make_comment("99", "artalk", "2026-01-01T00:00:00Z");    // older

        let mut order_a = vec![c1.clone(), c2.clone(), c3.clone()];
        let mut order_b = vec![c2.clone(), c3.clone(), c1.clone()]; // different accumulation order

        sort_comments_deterministic(&mut order_a);
        sort_comments_deterministic(&mut order_b);

        // Both must produce the same (created_at, source, id) sequence.
        let key = |c: &NormalizedComment| (c.created_at.clone(), c.source.clone(), c.id.clone());
        let keys_a: Vec<_> = order_a.iter().map(key).collect();
        let keys_b: Vec<_> = order_b.iter().map(key).collect();
        assert_eq!(keys_a, keys_b,
            "different input orders must produce identical sorted order");

        // Newest comment comes first (2026-03-15 before 2026-01-01).
        assert_eq!(order_a[0].created_at, "2026-03-15T12:00:00Z");
        assert_eq!(order_a[2].created_at, "2026-01-01T00:00:00Z");

        // Among same-timestamp comments, artalk < matters (lexicographic source).
        assert_eq!(order_a[0].source, "artalk");
        assert_eq!(order_a[0].id, "100");
        assert_eq!(order_a[1].source, "matters");
        assert_eq!(order_a[1].id, "m-xyz");
    }
}
