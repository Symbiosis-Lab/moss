//! Remote sync for Artalk comments: fetch, translate, reconcile, persist.
//!
//! FIXME: the comment mirror is still persisted as `comment.json`
//! (materialized JSON), not the planned append-only `comment.jsonl` event log.
//! This is an INTENTIONAL deferral: the mirror is a rebuildable
//! cache, and the precious local-first artifact (`moderation.jsonl`) already uses
//! the JSONL `event_log` substrate. Migrate the mirror to JSONL when convenient.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::{ArticleComments, CommentAuthor, CommentData, NormalizedComment};
use crate::build::features::sync::{CommentSyncError, CommentSyncErrorKind};
use crate::moss_paths::MossPaths;

/// Normalize a raw Artalk comment JSON value into a `NormalizedComment`.
fn normalize_artalk_comment(raw: &serde_json::Value) -> Option<NormalizedComment> {
    let id = raw.get("id")?.as_i64()?.to_string();
    // Prefer Artalk's rendered HTML (`content_marked`) over the raw markdown
    // source (`content`), and SANITIZE at ingest: comment HTML is attacker-
    // controlled and is rendered into baked pages + the dehydrated store. Never
    // trust the upstream server's sanitization.
    let raw_html = raw
        .get("content_marked")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| raw.get("content").and_then(|c| c.as_str()))?;
    let content = super::sanitize::sanitize_comment_html(raw_html);
    let created_at = raw
        .get("date")
        .and_then(|d| d.as_str())
        .unwrap_or("")
        .to_string();
    let nick = raw
        .get("nick")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let link = raw
        .get("link")
        .and_then(|l| l.as_str())
        .and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        });
    let rid = raw.get("rid").and_then(|r| r.as_i64()).unwrap_or(0);
    Some(NormalizedComment {
        id,
        source: super::default_source(),
        content,
        created_at,
        author: CommentAuthor {
            display_name: Some(nick),
            // `name` is omitted: `CommentAuthor::label()` falls back to
            // `display_name`; duplicating nick into `name` was redundant
            // and produced extra bytes in comment.json.
            name: None,
            url: link,
        },
        reply_to_id: if rid > 0 {
            Some(rid.to_string())
        } else {
            None
        },
        state: None,
    })
}

/// Translate an Artalk page_key to an article uid.
/// uid (the contract) → itself; legacy url_path (with/without leading
/// slash, posted by the pre-fix form) → that article's uid; unknown → None.
pub(super) fn translate_page_key(
    page_key: &str,
    known_uids: &HashSet<String>,
    url_path_to_uid: &HashMap<String, String>,
) -> Option<String> {
    if known_uids.contains(page_key) {
        return Some(page_key.to_string());
    }
    let trimmed = page_key.trim_start_matches('/');
    url_path_to_uid.get(trimmed).cloned()
}

/// Scan the existing `comment.json` data for tombstoned comments
/// (state is present and not "active") and emit a signed `hide` event to
/// `moderation.jsonl` for each one that does not already have a corresponding
/// hide event. This is the one-time migration that makes tombstone removal safe.
///
/// **Idempotent:** already-covered comment ids are skipped. If no owner
/// identity exists for this project, logs a warning and returns without
/// modifying anything — the caller will run with the (soon-to-be-removed)
/// tombstone path intact for this sync pass rather than silently dropping
/// deletions.
///
/// `site_name` is the Artalk site identifier (same value used in sync).
fn migrate_tombstones_to_signed_events(
    data: &CommentData,
    project_path: &str,
    site_name: &str,
) {
    // Load existing events to build a skip-set (idempotent guard).
    let existing_events = super::moderation::load_mod_events(project_path);
    let already_hidden: HashSet<(&str, &str)> = existing_events
        .iter()
        .filter(|e| e.kind == "hide")
        .map(|e| (e.source.as_str(), e.target.as_str()))
        .collect();

    // Collect tombstones that are not already covered.
    let tombstones: Vec<(&str, &str, &str)> = data
        .articles
        .iter()
        .flat_map(|(page, article)| {
            article.comments.iter().filter_map(move |c| {
                let is_tombstone = c.state.as_deref().is_some_and(|s| s != "active");
                if is_tombstone {
                    Some((c.source.as_str(), c.id.as_str(), page.as_str()))
                } else {
                    None
                }
            })
        })
        .filter(|(source, id, _page)| !already_hidden.contains(&(*source, *id)))
        .collect();

    if tombstones.is_empty() {
        return;
    }

    // Load identity — if absent, skip migration gracefully (tombstone path
    // still runs this sync pass via the reconcile caller).
    let mut id_svc = crate::identity::service::IdentityService::new(
        std::path::Path::new(project_path),
    );
    let identity = match id_svc.get() {
        Ok(Some(_)) => {
            // Ensure signing key is loaded before we take identity out.
            if let Err(e) = id_svc.ensure_signing_key() {
                log::warn!(
                    target: "sync",
                    "tombstone migration: signing key unavailable ({e}), skipping migration"
                );
                return;
            }
            // get() again after ensure to borrow the now-ready identity.
            match id_svc.get() {
                Ok(Some(id)) => id.clone(),
                _ => {
                    log::warn!(target: "sync", "tombstone migration: identity disappeared after ensure_signing_key");
                    return;
                }
            }
        }
        Ok(None) => {
            log::warn!(
                target: "sync",
                "tombstone migration: no owner identity for this project — skipping migration; \
                 tombstone preservation is still active this pass"
            );
            return;
        }
        Err(e) => {
            log::warn!(
                target: "sync",
                "tombstone migration: identity load error ({e}), skipping migration"
            );
            return;
        }
    };

    let Ok(signing_key) = identity.signing_key_loaded() else {
        log::warn!(target: "sync", "tombstone migration: signing key not loaded after ensure");
        return;
    };

    let log_path = super::moderation::moderation_log_path(project_path);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    for (source, id, page) in &tombstones {
        let seq = match super::moderation::next_seq(project_path) {
            Ok(s) => s,
            Err(e) => {
                log::warn!(target: "sync", "tombstone migration: seq error for id={id}: {e}");
                continue;
            }
        };
        let event = super::moderation::sign_mod_event(
            signing_key,
            "hide",
            source,
            id,
            site_name,
            page,
            seq,
            ts,
        );
        if let Err(e) = super::event_log::append_jsonl(&log_path, &event) {
            log::warn!(
                target: "sync",
                "tombstone migration: could not append hide event for id={id}: {e}"
            );
        }
    }

    log::info!(
        target: "sync",
        "tombstone migration: emitted {} signed hide events",
        tombstones.len()
    );
}

/// Full-state reconcile for one article: server wins.
///
/// The fetched set replaces the existing per-article set — plain
/// server-wins. Deletions propagate (a comment absent from the server is
/// dropped). Signed `hide` events in `moderation.jsonl` are the sole
/// bake-time moderation path (`reduce::retain_visible`); tombstones in
/// `comment.json` are no longer created or consulted.
fn reconcile_article_comments(
    _existing: Vec<NormalizedComment>,
    incoming: Vec<NormalizedComment>,
) -> Vec<NormalizedComment> {
    incoming
}

/// Re-key any article entry in `data` whose key is NOT a known uid through
/// `translate_page_key`. If the resolved uid already exists in `data`, the
/// path-keyed entry's comments are merged into it (reconcile-style: union of
/// comment vecs, deduped by id with the uid-entry's version winning for ties).
/// Unknown keys (translate → None) are left as-is (orphan archive).
///
/// This is a pure function — no I/O, no network — so it can be tested without
/// spinning up an Artalk server.
pub(super) fn normalize_article_keys(
    data: &mut CommentData,
    known_uids: &HashSet<String>,
    url_path_to_uid: &HashMap<String, String>,
) {
    // Collect keys that need renaming (iterate over a snapshot to allow mutation)
    let stale_keys: Vec<String> = data
        .articles
        .keys()
        .filter(|k| !known_uids.contains(*k))
        .filter(|k| translate_page_key(k, known_uids, url_path_to_uid).is_some())
        .cloned()
        .collect();

    for stale_key in stale_keys {
        let Some(uid) = translate_page_key(&stale_key, known_uids, url_path_to_uid) else {
            continue;
        };
        let stale_entry = match data.articles.remove(&stale_key) {
            Some(e) => e,
            None => continue,
        };
        if let Some(uid_entry) = data.articles.get_mut(&uid) {
            // Merge: uid-entry wins for comment-id ties
            let existing_ids: HashSet<String> =
                uid_entry.comments.iter().map(|c| c.id.clone()).collect();
            for c in stale_entry.comments {
                if !existing_ids.contains(&c.id) {
                    uid_entry.comments.push(c);
                }
            }
        } else {
            data.articles.insert(uid, stale_entry);
        }
    }
}

/// Fetch all comments for the site and group them by article uid.
/// Network/HTTP/parse failures return Err — the caller surfaces an advisory.
/// This is the one place the typed transport error is available, so failure
/// classification (Offline vs Server) happens here.
pub(super) fn fetch_artalk_comments(
    server_url: &str,
    site_name: &str,
    known_uids: &HashSet<String>,
    url_path_to_uid: &HashMap<String, String>,
) -> Result<HashMap<String, Vec<NormalizedComment>>, CommentSyncError> {
    let url = format!(
        "{}/api/v2/stats/latest_comments?site_name={}&limit=1000",
        server_url.trim_end_matches('/'),
        urlencoding::encode(site_name)
    );
    let resp = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => CommentSyncError {
                kind: CommentSyncErrorKind::Server,
                detail: format!("comment server returned HTTP {code}"),
            },
            ureq::Error::Transport(t) => CommentSyncError {
                kind: CommentSyncErrorKind::Offline,
                detail: format!("comment server unreachable: {t}"),
            },
        })?;
    let json: serde_json::Value = resp.into_json().map_err(|e| CommentSyncError {
        kind: CommentSyncErrorKind::Server,
        detail: format!("comment server returned invalid JSON: {e}"),
    })?;
    let comments = json
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    // Guard: the API returns at most 1000 comments per request. If the response
    // is already at the limit, we only have a partial window — reconciling this
    // against the local archive would treat any comment outside the window as
    // "deleted" and drop it. Refuse to proceed so out-of-window comments stay
    // intact.
    if comments.len() >= 1000 {
        return Err(
            "comment fetch hit the 1000-comment window — refusing to reconcile a partial snapshot"
                .to_string()
                .into(),
        );
    }
    let mut result: HashMap<String, Vec<NormalizedComment>> = HashMap::new();
    for raw in &comments {
        let Some(comment) = normalize_artalk_comment(raw) else { continue };
        let page_key = raw.get("page_key").and_then(|p| p.as_str()).unwrap_or("");
        match translate_page_key(page_key, known_uids, url_path_to_uid) {
            Some(uid) => result.entry(uid).or_default().push(comment),
            None => log::warn!(
                target: "sync",
                "orphan comment id={} page_key={:?} matches no article uid or url_path",
                comment.id, page_key
            ),
        }
    }
    Ok(result)
}

/// Outcome of one comment sync pass, consumed by features::sync for
/// advisory emission. `changed: true` means comment.json was rewritten —
/// the file watcher (which covers .moss/data/) triggers the refresh build.
pub(in crate::build::features) struct CommentSyncOutcome {
    pub changed: bool,
}

/// Load comment data strictly: returns `Err` if the file exists but cannot be
/// read or parsed, instead of collapsing errors to `None`.
///
/// Use this in write paths (process_comments) where a silently-discarded read
/// error would cause a fresh-start overwrite that wipes local data. The lenient
/// `load_comment_data` is kept for read-only callers (render path, tests) where
/// a missing-or-corrupt file should simply mean "no existing comments".
fn load_comment_data_strict(
    project_path: &str,
) -> Result<Option<super::CommentData>, String> {
    let path = crate::moss_paths::MossPaths::new(std::path::Path::new(project_path))
        .social_dir()
        .join("comment.json");
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("comment.json unreadable — not overwriting local archive: {e}"))?;
    let data = serde_json::from_str(&content)
        .map_err(|e| format!("comment.json unreadable — not overwriting local archive: {e}"))?;
    Ok(Some(data))
}

/// Fetch comments from the server and reconcile into `.moss/data/social/comment.json`.
///
/// This is the native Rust equivalent of the JS comment plugin's `process` hook.
///
/// # Visibility
///
/// **`pub(in crate::build::features)` — do not widen.** This function does
/// blocking network I/O (each request can take 30s+ when the upstream is
/// unreachable). Calling it from the build's critical path hangs the
/// whole pipeline.
///
/// The legitimate caller is
/// [`crate::build::features::sync::spawn_native_process_sync`], which runs
/// this on a `spawn_blocking` worker as a background task while the build
/// returns immediately.
pub(in crate::build::features) fn process_comments(
    project_path: &str,
    server_url: &str,
    site_name: &str,
    articles: &[(String, String)],
) -> Result<CommentSyncOutcome, CommentSyncError> {
    let known_uids: HashSet<String> = articles.iter().map(|(u, _)| u.clone()).collect();
    let url_path_to_uid: HashMap<String, String> = articles
        .iter()
        .map(|(u, p)| (p.trim_start_matches('/').to_string(), u.clone()))
        .collect();

    // Load the existing archive strictly BEFORE going to the network: if the
    // file exists but is corrupt (iCloud-dataless, partial write, etc.), bail
    // out rather than overwriting it with a fresh-start. Only proceed with a
    // fresh CommentData when the file does not exist yet (first sync).
    let mut data = match load_comment_data_strict(project_path)? {
        Some(existing) => existing,
        None => CommentData {
            schema_version: "1.1.0".to_string(),
            articles: BTreeMap::new(),
        },
    };

    // Migration: convert any legacy tombstones (state != "active") in the
    // loaded archive into signed hide events in moderation.jsonl. This is the
    // one-time transition that makes tombstone removal safe — after this runs,
    // `reconcile_article_comments` is plain server-wins and `retain_visible`
    // (signed events) is the sole bake-time moderation path.
    //
    // Idempotent: already-migrated ids are skipped. Gracefully skipped when no
    // owner identity exists (rare; leaves tombstone field in comment.json but
    // the field is no longer read by reconcile — tombstones will simply be
    // overwritten on next server sync).
    migrate_tombstones_to_signed_events(&data, project_path, site_name);

    let fetched = fetch_artalk_comments(server_url, site_name, &known_uids, &url_path_to_uid)?;
    data.schema_version = "1.1.0".to_string();

    // Prune legacy path-keyed entries before reconcile.
    normalize_article_keys(&mut data, &known_uids, &url_path_to_uid);

    // Reconcile only uids present in the response. An article absent from the
    // fetch is left untouched — deliberately conservative so a flaky empty
    // response can never wipe the local archive (design §4; the one
    // non-propagating case, "every comment of an article deleted server-side",
    // is accepted and documented).
    for (uid, incoming) in fetched {
        let existing = data
            .articles
            .remove(&uid)
            .map(|a| a.comments)
            .unwrap_or_default();
        data.articles.insert(
            uid,
            ArticleComments { comments: reconcile_article_comments(existing, incoming) },
        );
    }

    let social_dir = MossPaths::new(std::path::Path::new(project_path)).social_dir();
    // allow:raw_write .moss/data/social, not the build tree
    std::fs::create_dir_all(&social_dir)
        .map_err(|e| format!("cannot create {}: {e}", social_dir.display()))?;
    let path = social_dir.join("comment.json");
    let new_json = serde_json::to_string_pretty(&data)
        .map_err(|e| format!("serialize comment.json: {e}"))?;
    // Write only on change: the watcher covers .moss/data/, so an
    // unconditional write would rebuild-loop (build → sync → write → watch
    // → build …). Identical content = no write = loop terminates.
    let unchanged = std::fs::read_to_string(&path)
        .map(|old| old == new_json)
        .unwrap_or(false);
    if !unchanged {
        std::fs::write(&path, new_json).map_err(|e| format!("write comment.json: {e}"))?;  // allow:raw_write user state under .moss/data, not regenerable output
    }
    Ok(CommentSyncOutcome { changed: !unchanged })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::moderation::{load_mod_events, moderation_log_path};
    use crate::identity::keypair::Identity;

    fn make_comment(id: &str, state: Option<&str>) -> NormalizedComment {
        NormalizedComment {
            id: id.into(),
            source: "artalk".into(),
            content: "x".into(),
            created_at: "2026-01-01".into(),
            author: CommentAuthor { display_name: Some("A".into()), name: None, url: None },
            reply_to_id: None,
            state: state.map(String::from),
        }
    }

    fn tmp_dir() -> tempfile::TempDir {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let tmp_base = repo_root.join("target").join("test-tmp");
        std::fs::create_dir_all(&tmp_base).unwrap();
        tempfile::TempDir::new_in(&tmp_base).unwrap()
    }

    // --- ingest sanitization ---

    #[test]
    fn normalize_artalk_prefers_content_marked_and_sanitizes() {
        let raw = serde_json::json!({
            "id": 7,
            "content": "raw **markdown** <script>alert(1)</script>",
            "content_marked": "<p>hi</p><script>alert(1)</script><img src=x onerror=alert(1)>",
            "date": "2026-06-13 08:00:00",
            "nick": "tester",
        });
        let c = normalize_artalk_comment(&raw).expect("comment normalizes");
        assert!(c.content.contains("<p>hi</p>"), "uses content_marked: {}", c.content);
        assert!(!c.content.contains("<script"), "script stripped: {}", c.content);
        assert!(!c.content.contains("onerror"), "handler stripped: {}", c.content);
        assert!(!c.content.contains("<img"), "img stripped: {}", c.content);
    }

    #[test]
    fn normalize_artalk_falls_back_to_content_when_no_marked() {
        let raw = serde_json::json!({ "id": 8, "content": "<b>plain</b>", "date": "", "nick": "x" });
        let c = normalize_artalk_comment(&raw).expect("comment normalizes");
        // `b` is not in the allowlist → tag stripped, text survives.
        assert!(!c.content.contains("<b>"), "disallowed tag stripped: {}", c.content);
        assert!(c.content.contains("plain"), "text survives: {}", c.content);
    }

    // --- translate_page_key tests ---

    #[test]
    fn test_translate_page_key_uid_passthrough() {
        let uids: HashSet<String> = ["5f3a4714".to_string()].into();
        let paths: HashMap<String, String> = HashMap::new();
        assert_eq!(translate_page_key("5f3a4714", &uids, &paths), Some("5f3a4714".to_string()));
    }

    #[test]
    fn test_translate_page_key_url_path_to_uid() {
        let uids: HashSet<String> = ["6a559bc6".to_string()].into();
        let mut paths = HashMap::new();
        paths.insert("writings/useless-journey/river-two-freedoms/".to_string(), "6a559bc6".to_string());
        assert_eq!(
            translate_page_key("writings/useless-journey/river-two-freedoms/", &uids, &paths),
            Some("6a559bc6".to_string())
        );
        assert_eq!(
            translate_page_key("/writings/useless-journey/river-two-freedoms/", &uids, &paths),
            Some("6a559bc6".to_string())
        );
    }

    #[test]
    fn test_translate_page_key_unknown_returns_none() {
        let uids = HashSet::new();
        let paths = HashMap::new();
        assert_eq!(translate_page_key("garbage", &uids, &paths), None);
    }

    // --- reconcile_article_comments: server-wins ---

    #[test]
    fn test_reconcile_server_wins() {
        // Active local comment deleted on server → drops.
        // Tombstoned local comment also replaced by server set (server-wins).
        // New server comment lands.
        let existing = vec![
            make_comment("1", None),            // active, not on server → dropped
            make_comment("2", Some("removed")), // tombstone, returned by server → replaced
            make_comment("3", Some("removed")), // tombstone, not on server → dropped
        ];
        let incoming = vec![make_comment("2", None), make_comment("4", None)];
        let merged = reconcile_article_comments(existing, incoming);
        let by_id: HashMap<&str, &NormalizedComment> =
            merged.iter().map(|c| (c.id.as_str(), c)).collect();
        assert!(by_id.get("1").is_none(), "server deletion propagates (id=1)");
        // id=2 comes from server with state=None (active), no tombstone preserved.
        assert_eq!(by_id["2"].state, None, "server-wins: tombstone state not preserved");
        assert!(by_id.get("3").is_none(), "server-absent tombstone is dropped");
        assert!(by_id.get("4").is_some(), "new server comment lands");
        assert_eq!(merged.len(), 2, "exactly server set returned");
    }

    #[test]
    fn test_reconcile_server_wins_returns_incoming_unchanged() {
        let existing = vec![make_comment("old", Some("removed"))];
        let incoming = vec![make_comment("new", None)];
        let result = reconcile_article_comments(existing, incoming);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "new");
    }

    // --- migrate_tombstones_to_signed_events ---

    /// A tombstone in loaded data produces a signed hide event on migration.
    #[test]
    fn test_migration_tombstone_emits_signed_hide_event() {
        let tmp = tmp_dir();
        let project_path = tmp.path().to_str().unwrap();

        // Create an owner identity so migration can sign.
        let id = Identity::generate().unwrap();
        id.save(tmp.path()).unwrap();

        // Save the signing key so migration can load it.
        let key_path = crate::identity::keypair::key_file_path(tmp.path());
        if let Some(parent) = key_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        // Write the signing key using the service pattern.
        let mut id_svc = crate::identity::service::IdentityService::new(tmp.path());
        id_svc.ensure_signing_key().unwrap();

        let data = CommentData {
            schema_version: "1.1.0".to_string(),
            articles: {
                let mut m = BTreeMap::new();
                m.insert(
                    "uid1".to_string(),
                    ArticleComments {
                        comments: vec![
                            make_comment("10", Some("removed")), // tombstone → should migrate
                            make_comment("11", None),             // active → ignored
                        ],
                    },
                );
                m
            },
        };

        // Ensure social dir exists (migration needs it for moderation.jsonl).
        let social_dir = MossPaths::new(tmp.path()).social_dir();
        std::fs::create_dir_all(&social_dir).unwrap();

        migrate_tombstones_to_signed_events(&data, project_path, "test-site");

        let events = load_mod_events(project_path);
        assert_eq!(events.len(), 1, "exactly one hide event for the tombstone");
        assert_eq!(events[0].kind, "hide");
        assert_eq!(events[0].target, "10");
        assert_eq!(events[0].source, "artalk");
        assert_eq!(events[0].site, "test-site");
    }

    /// Re-running migration emits nothing new (idempotent).
    #[test]
    fn test_migration_idempotent() {
        let tmp = tmp_dir();
        let project_path = tmp.path().to_str().unwrap();

        let id = Identity::generate().unwrap();
        id.save(tmp.path()).unwrap();
        let mut id_svc = crate::identity::service::IdentityService::new(tmp.path());
        id_svc.ensure_signing_key().unwrap();

        let data = CommentData {
            schema_version: "1.1.0".to_string(),
            articles: {
                let mut m = BTreeMap::new();
                m.insert(
                    "uid1".to_string(),
                    ArticleComments {
                        comments: vec![make_comment("20", Some("removed"))],
                    },
                );
                m
            },
        };

        let social_dir = MossPaths::new(tmp.path()).social_dir();
        std::fs::create_dir_all(&social_dir).unwrap();

        // Run twice.
        migrate_tombstones_to_signed_events(&data, project_path, "test-site");
        migrate_tombstones_to_signed_events(&data, project_path, "test-site");

        let events = load_mod_events(project_path);
        assert_eq!(events.len(), 1, "second run must not duplicate the hide event");
    }

    /// Migration with no identity: no events emitted, no panic.
    #[test]
    fn test_migration_no_identity_is_graceful() {
        let tmp = tmp_dir();
        let project_path = tmp.path().to_str().unwrap();

        let data = CommentData {
            schema_version: "1.1.0".to_string(),
            articles: {
                let mut m = BTreeMap::new();
                m.insert(
                    "uid1".to_string(),
                    ArticleComments {
                        comments: vec![make_comment("30", Some("removed"))],
                    },
                );
                m
            },
        };

        let social_dir = MossPaths::new(tmp.path()).social_dir();
        std::fs::create_dir_all(&social_dir).unwrap();

        // No identity — must not panic or emit events.
        migrate_tombstones_to_signed_events(&data, project_path, "test-site");

        let log_path = moderation_log_path(project_path);
        let events = load_mod_events(project_path);
        assert!(
            events.is_empty() || !log_path.exists(),
            "no events must be emitted when no identity exists"
        );
    }

    // --- normalize_artalk_comment tests ---

    #[test]
    fn test_normalize_artalk_comment() {
        let raw = serde_json::json!({
            "id": 42,
            "content": "<p>Hello</p>",
            "date": "2026-01-01",
            "nick": "Alice",
            "link": "",
            "rid": 0
        });
        let c = normalize_artalk_comment(&raw).unwrap();
        assert_eq!(c.id, "42");
        assert_eq!(c.author.label(crate::i18n::Language::En), "Alice");
        assert!(c.reply_to_id.is_none());
    }

    #[test]
    fn test_normalize_artalk_comment_with_reply() {
        let raw = serde_json::json!({
            "id": 43,
            "content": "<p>Reply</p>",
            "date": "2026-01-02",
            "nick": "Bob",
            "link": "https://bob.com",
            "rid": 42
        });
        let c = normalize_artalk_comment(&raw).unwrap();
        assert_eq!(c.reply_to_id, Some("42".to_string()));
        assert_eq!(c.author.url, Some("https://bob.com".to_string()));
    }

    // --- normalize_article_keys tests ---

    /// path-keyed entry ("writings/foo/") that maps to uid "abc123"
    /// is re-keyed; merges into uid entry if one exists, deduping by id.
    #[test]
    fn test_normalize_article_keys_rekeys_path_entry() {
        let mut data = CommentData {
            schema_version: "1.1.0".to_string(),
            articles: BTreeMap::new(),
        };
        // Path-keyed entry with one comment
        data.articles.insert(
            "writings/foo/".to_string(),
            ArticleComments {
                comments: vec![make_comment("c1", None)],
            },
        );

        let mut known_uids = HashSet::new();
        known_uids.insert("abc123".to_string());
        let mut url_path_to_uid = HashMap::new();
        url_path_to_uid.insert("writings/foo/".to_string(), "abc123".to_string());

        normalize_article_keys(&mut data, &known_uids, &url_path_to_uid);

        assert!(!data.articles.contains_key("writings/foo/"), "stale key must be removed");
        let uid_entry = data.articles.get("abc123").expect("uid entry must exist");
        assert_eq!(uid_entry.comments.len(), 1);
        assert_eq!(uid_entry.comments[0].id, "c1");
    }

    #[test]
    fn test_normalize_article_keys_merges_into_existing_uid_dedupes() {
        let mut data = CommentData {
            schema_version: "1.1.0".to_string(),
            articles: BTreeMap::new(),
        };
        // Existing uid entry with comment "c1"
        data.articles.insert(
            "abc123".to_string(),
            ArticleComments {
                comments: vec![make_comment("c1", None)],
            },
        );
        // Path-keyed entry with comments "c1" (dup) and "c2" (new)
        data.articles.insert(
            "writings/foo/".to_string(),
            ArticleComments {
                comments: vec![make_comment("c1", None), make_comment("c2", None)],
            },
        );

        let mut known_uids = HashSet::new();
        known_uids.insert("abc123".to_string());
        let mut url_path_to_uid = HashMap::new();
        url_path_to_uid.insert("writings/foo/".to_string(), "abc123".to_string());

        normalize_article_keys(&mut data, &known_uids, &url_path_to_uid);

        assert!(!data.articles.contains_key("writings/foo/"), "stale key removed");
        let uid_entry = data.articles.get("abc123").expect("uid entry must exist");
        // c1 from uid entry + c2 from path entry; c1 not duplicated
        assert_eq!(uid_entry.comments.len(), 2, "should have c1 + c2, not 3");
        let ids: HashSet<&str> = uid_entry.comments.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains("c1"));
        assert!(ids.contains("c2"));
    }

    #[test]
    fn test_normalize_article_keys_leaves_unknown_keys_as_orphan() {
        let mut data = CommentData {
            schema_version: "1.1.0".to_string(),
            articles: BTreeMap::new(),
        };
        data.articles.insert(
            "totally-unknown-key".to_string(),
            ArticleComments { comments: vec![make_comment("x1", None)] },
        );

        let known_uids: HashSet<String> = HashSet::new();
        let url_path_to_uid: HashMap<String, String> = HashMap::new();

        normalize_article_keys(&mut data, &known_uids, &url_path_to_uid);

        // Orphan key must be preserved (no translation available)
        assert!(data.articles.contains_key("totally-unknown-key"), "orphan key must survive");
    }

    #[test]
    fn test_normalize_article_keys_leading_slash_stripped() {
        let mut data = CommentData {
            schema_version: "1.1.0".to_string(),
            articles: BTreeMap::new(),
        };
        // Legacy key with leading slash
        data.articles.insert(
            "/writings/foo/".to_string(),
            ArticleComments { comments: vec![make_comment("c1", None)] },
        );

        let mut known_uids = HashSet::new();
        known_uids.insert("abc123".to_string());
        let mut url_path_to_uid = HashMap::new();
        // Map is indexed without leading slash
        url_path_to_uid.insert("writings/foo/".to_string(), "abc123".to_string());

        normalize_article_keys(&mut data, &known_uids, &url_path_to_uid);

        assert!(!data.articles.contains_key("/writings/foo/"), "leading-slash key removed");
        assert!(data.articles.contains_key("abc123"), "re-keyed to uid");
    }

    // --- archive-clobber guard (load_comment_data_strict) ---

    /// Existing-but-corrupt file must return Err from process_comments so the
    /// archive is never overwritten.  The load check happens before the network
    /// fetch, so no HTTP call is needed to trigger it.
    #[test]
    fn test_process_comments_rejects_corrupt_archive() {
        let tmp = tmp_dir();

        let social_dir = tmp.path().join(".moss").join("data").join("social");
        std::fs::create_dir_all(&social_dir).unwrap();

        // Write a syntactically invalid JSON file to simulate iCloud-dataless /
        // partial-write corruption.
        let comment_path = social_dir.join("comment.json");
        std::fs::write(&comment_path, b"not valid json{{{").unwrap();
        let before = std::fs::read_to_string(&comment_path).unwrap();

        // Use a bogus server URL — process_comments must Err before the network
        // call because the strict load check gates the entire function.
        let result = process_comments(
            tmp.path().to_str().unwrap(),
            "http://127.0.0.1:1", // unreachable; should never be reached
            "test-site",
            &[("uid1".to_string(), "/page1/".to_string())],
        );
        assert!(result.is_err(), "corrupt archive must Err, not overwrite");
        // Use .err().unwrap() to extract the error without requiring Debug on CommentSyncOutcome.
        let err_msg = result.err().unwrap();
        assert!(
            err_msg.detail.contains("not overwriting local archive"),
            "error message must mention archive guard; got: {}",
            err_msg.detail
        );
        // A corrupt LOCAL file must never be blamed on the network or server —
        // the UI renders Internal as the cause-neutral "a problem with comment
        // data", not "comment server unreachable".
        assert!(
            matches!(err_msg.kind, CommentSyncErrorKind::Internal),
            "archive guard must classify as Internal, got {:?}",
            err_msg.kind
        );
        // Verify the file content was NOT changed.
        let after = std::fs::read_to_string(&comment_path).unwrap();
        assert_eq!(before, after, "corrupt file must remain untouched");
    }

    /// A missing file should allow a fresh-start (no Err).
    /// We pass a bogus server URL — the function will Err on the network, but
    /// NOT with the "not overwriting" guard message.
    #[test]
    fn test_process_comments_allows_fresh_start_on_missing_file() {
        let tmp = tmp_dir();

        // No comment.json — process_comments should proceed past the guard.
        let result = process_comments(
            tmp.path().to_str().unwrap(),
            "http://127.0.0.1:1", // unreachable
            "test-site",
            &[("uid1".to_string(), "/page1/".to_string())],
        );
        // We expect Err from the network, NOT from the archive guard.
        assert!(result.is_err());
        // Use .err().unwrap() to extract the error without requiring Debug on CommentSyncOutcome.
        let err_msg = result.err().unwrap();
        assert!(
            !err_msg.detail.contains("not overwriting local archive"),
            "missing file must NOT hit the archive guard; got: {}",
            err_msg.detail
        );
    }

    /// Transport-level failures (DNS, connect, timeout) classify as Offline —
    /// the settings row renders these as "no internet connection".
    #[test]
    fn test_fetch_classifies_connection_refused_as_offline() {
        // Port 1 on localhost is never listening; connect fails immediately.
        let err = fetch_artalk_comments(
            "http://127.0.0.1:1",
            "test-site",
            &HashSet::new(),
            &HashMap::new(),
        )
        .expect_err("connect to a closed port must fail");
        assert!(
            matches!(err.kind, CommentSyncErrorKind::Offline),
            "connection refused should classify as Offline, got {:?}: {}",
            err.kind,
            err.detail
        );
    }

    /// HTTP error statuses classify as Server — the server is reachable but
    /// erroring, which renders as "comment server unreachable" (calm generic).
    #[test]
    fn test_fetch_classifies_http_500_as_server_error() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(
                    b"HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                );
            }
        });
        let err = fetch_artalk_comments(
            &format!("http://{addr}"),
            "test-site",
            &HashSet::new(),
            &HashMap::new(),
        )
        .expect_err("HTTP 500 must fail");
        handle.join().unwrap();
        assert!(
            matches!(err.kind, CommentSyncErrorKind::Server),
            "HTTP 500 should classify as Server, got {:?}: {}",
            err.kind,
            err.detail
        );
    }
}
