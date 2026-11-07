//! Sending the copies that are due, after a publish has returned. A failure
//! here never touches the publish: it is already live, and a copy left due is
//! sent by the next run.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::Serialize;

use super::job::{CopyJob, Op, PostRef, Previous};
use super::record::{Cause, CopyOutcome, CopyRecord, Records, State};
use super::shape::{idempotency_key, page_url, shape};
use crate::build::scan::article_map::{ArticleInfo, ArticleMap};
use crate::plugins::manager::copies::{CopyCall, CopyChannel};
use crate::plugins::manager::PluginManager;

/// At most this many new copies per channel per run; the rest stay due. A
/// burst of old posts on someone's timeline cannot be taken back.
pub const PER_RUN: usize = 10;

const INTERRUPTED: &str = "the last send stopped before the network answered";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// About to call the network.
    Sending,
    Posted { url: String },
    Updated { url: String },
    /// The page changed but its copy would not.
    Unchanged,
    /// The page changed and the network cannot edit a copy once posted.
    CannotEdit,
    HandedOff { text: String, open_url: Option<String> },
    /// Nothing was posted; tried again next run.
    Refused { cause: Cause, reason: String },
    /// Whether a copy exists is unknown; not retried until the author says so.
    Unconfirmed { reason: String },
    /// Left due: over this run's limit, or its page is not in this publish.
    Held { reason: String },
    Deleted,
    /// The network cannot remove a copy; it stays where it is.
    CannotDelete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct Sent {
    pub source_path: String,
    pub channel: String,
    #[serde(flatten)]
    pub outcome: Outcome,
    /// The address is in moss's record but could not be written into the page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_back: Option<String>,
}

pub struct SendOptions<'a> {
    /// The site's public address, for each copy's link to its original.
    pub site_url: &'a str,
    /// Channels whose unconfirmed copies the author asked to try again.
    pub retry: &'a [String],
    /// Told about each copy as it starts and as it ends, in order.
    pub emit: &'a (dyn Fn(&Sent) + Sync),
    /// Only this post's copies (by note id), and only on this channel.
    pub only_post: Option<&'a str>,
    pub only_channel: Option<&'a str>,
    /// New copies per channel this run; [`PER_RUN`] unless the author asked
    /// for the held rest now.
    pub cap: usize,
    /// Fetch each page before posting its copy, so no copy links to a page
    /// that does not answer yet. A host that has its own proof the site is
    /// live says false.
    pub check_pages: bool,
}

/// Send the folder's due copies once a publish to `site_url` has landed.
pub async fn after_publish(
    folder: &Path,
    site_url: &str,
    host_ports: &(dyn Fn(&str) -> crate::build::HostPorts + Send + Sync),
    retry: &[String],
    emit: &(dyn Fn(&Sent) + Sync),
) -> Vec<Sent> {
    let opts = SendOptions { site_url, retry, emit, only_post: None, only_channel: None, cap: PER_RUN, check_pages: true };
    let folder_str = folder.to_string_lossy();
    after_live(folder, &opts, || host_ports(&folder_str).plugins.get_or_create(&folder_str)).await
}

/// Send the folder's due copies, for a host whose publish is live: the
/// channels load only when a copy is due.
pub async fn after_live(
    folder: &Path,
    opts: &SendOptions<'_>,
    manager: impl FnOnce() -> Result<std::sync::Arc<PluginManager>, String>,
) -> Vec<Sent> {
    let mp = crate::moss_paths::MossPaths::new(folder);
    let records = Records::in_vault(&mp);
    if records.all().is_empty() {
        return Vec::new();
    }
    let Some(_turn) = records.turn() else { return Vec::new() };
    let articles: ArticleMap = match std::fs::read(mp.article_map()).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
        Ok(map) => map,
        Err(e) => {
            log::warn!(target: "syndication", "copies not sent: the published pages could not be read ({e})");
            return Vec::new();
        }
    };
    let manager = match manager() {
        Ok(m) => m,
        Err(e) => {
            log::warn!(target: "syndication", "copies not sent: the channels could not be loaded ({e})");
            return Vec::new();
        }
    };
    send_in_turn(folder, &manager, &articles, opts).await
}

/// Send every due copy in the folder's records. `articles` is what is live.
pub async fn send_due(folder: &Path, manager: &PluginManager, articles: &ArticleMap, opts: &SendOptions<'_>) -> Vec<Sent> {
    let Some(_turn) = Records::in_vault(&crate::moss_paths::MossPaths::new(folder)).turn() else { return Vec::new() };
    send_in_turn(folder, manager, articles, opts).await
}

/// [`send_due`] for a caller already holding the folder's turn: a second
/// lock on it from this process would be refused, and nothing sent.
async fn send_in_turn(folder: &Path, manager: &PluginManager, articles: &ArticleMap, opts: &SendOptions<'_>) -> Vec<Sent> {
    let records = Records::in_vault(&crate::moss_paths::MossPaths::new(folder));
    let channels = manager.copy_channels();
    let by_uid: HashMap<String, &ArticleInfo> = articles
        .articles
        .values()
        .filter_map(|a| Some((a.uid.clone()?, a)))
        .collect();

    // Post by post, each post's channels in the order its `syndicate:` names them.
    let mut due = records.all();
    due.sort_by_cached_key(|r| {
        let rank = by_uid.get(&r.uid).and_then(|p| p.syndicate.as_ref()).and_then(|c| c.iter().position(|c| *c == r.channel));
        (r.url_path.clone(), rank.unwrap_or(usize::MAX), r.channel.clone())
    });
    let site_lang = crate::build::site_config::get_site_lang(&folder.to_string_lossy()).ok().flatten();
    let mut sent_per_channel: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = Vec::new();
    let mut push = |s: Sent| {
        (opts.emit)(&s);
        out.push(s);
    };
    for mut record in due {
        if opts.only_post.is_some_and(|u| u != record.uid) || opts.only_channel.is_some_and(|c| c != record.channel) {
            continue;
        }
        let Some(channel) = channels.get(&record.channel) else { continue };
        let Some(op) = record.next_op(channel.spec.idempotent) else { continue };
        let Some(post) = by_uid.get(&record.uid) else {
            if record.state == State::Sending {
                mark_unconfirmed(&records, &mut record, INTERRUPTED);
                push(sent(&record, Outcome::Unconfirmed { reason: record.reason.clone().unwrap_or_default() }));
                continue;
            }
            if op == Op::Publish {
                push(sent(&record, Outcome::Held { reason: "its page is not in this publish".into() }));
            }
            continue;
        };
        let lang = post_lang(post, articles, site_lang.as_deref());
        // An uncertain copy is looked up before anything is sent again.
        let op = if op == Op::Find {
            match reconcile(manager, &record, channel, post, lang.clone(), opts).await {
                Found::Copy(outcome) => {
                    push(adopt(folder, &records, record, outcome));
                    continue;
                }
                // A draft still waiting for the author is reported once, when
                // it first becomes one; a draft the author deleted is absent.
                Found::Waiting(handoff) => {
                    if record.handoff.as_ref() != Some(&handoff) {
                        record.state = State::HandedOff;
                        record.handoff = Some(handoff.clone());
                        record.reason = None;
                        push(settle(&records, record, Outcome::HandedOff { text: handoff.text, open_url: handoff.open_url }));
                    }
                    continue;
                }
                Found::Absent => Op::Publish,
                Found::Unknown(_) if opts.retry.contains(&record.channel) => Op::Publish,
                Found::Unknown(why) => {
                    if record.state == State::Sending {
                        mark_unconfirmed(&records, &mut record, INTERRUPTED);
                    }
                    let reason = [record.reason.clone().unwrap_or_default(), why].into_iter().filter(|r| !r.is_empty()).collect::<Vec<_>>().join("; ");
                    push(sent(&record, Outcome::Unconfirmed { reason }));
                    continue;
                }
            }
        } else {
            op
        };
        if op == Op::Publish {
            let n = sent_per_channel.entry(record.channel.clone()).or_default();
            if *n >= opts.cap {
                push(sent(&record, Outcome::Held { reason: format!("over this run's limit of {} new copies on {}", opts.cap, record.channel) }));
                continue;
            }
            *n += 1;
            if opts.check_pages {
                let page = page_url(record.site(opts.site_url), &post.url_path);
                if !answers(&page).await {
                    *n -= 1;
                    push(sent(&record, Outcome::Held { reason: format!("its page does not answer yet at {page}") }));
                    continue;
                }
            }
        }
        if let Some(s) = send_one(folder, manager, &records, record, channel, post, lang, op, opts).await {
            push(s);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
async fn send_one(
    folder: &Path,
    manager: &PluginManager,
    records: &Records,
    mut record: CopyRecord,
    channel: &CopyChannel,
    post: &ArticleInfo,
    lang: Option<String>,
    op: Op,
    opts: &SendOptions<'_>,
) -> Option<Sent> {
    record.source_path.clone_from(&post.source_path);
    record.url_path.clone_from(&post.url_path);

    let (job, hash) = match build_job(&record, channel, post, lang, op, record.site(opts.site_url)) {
        Ok(j) => j,
        Err(reason) => return Some(settle(records, record, Outcome::Refused { cause: Cause::Content, reason })),
    };
    // The source changed but the copy would not (writing `syndicated:` back
    // is such an edit): nothing to send, nothing to say.
    if op == Op::Update && record.hash.as_deref() == Some(hash.as_str()) {
        record.update_due = false;
        record.touch();
        let _ = records.save(&record);
        return Some(sent(&record, Outcome::Unchanged));
    }

    (opts.emit)(&sent(&record, Outcome::Sending));
    if op == Op::Publish {
        record.state = State::Sending;
        record.touch();
        if let Err(e) = records.save(&record) {
            return Some(sent(&record, Outcome::Held { reason: format!("its record could not be written, so nothing was sent ({e})") }));
        }
    }

    let unknown = |mut record: CopyRecord, reason: String| {
        if op == Op::Update {
            return settle(records, record, Outcome::Refused { cause: Cause::Unknown, reason });
        }
        record.state = if channel.spec.idempotent { State::Due } else { State::Unconfirmed };
        record.reason = Some(reason.clone());
        record.touch();
        let _ = records.save(&record);
        if channel.spec.idempotent {
            sent(&record, Outcome::Held { reason: format!("{reason}; sent again next run with the same key") })
        } else {
            sent(&record, Outcome::Unconfirmed { reason })
        }
    };

    Some(match manager.send_copy(&job).await {
        CopyCall::Answered(result) if !result.success => {
            let refusal = result.copy.and_then(|c| c.refused);
            let cause = refusal.as_ref().map(|r| r.cause).unwrap_or_default();
            let reason = refusal
                .and_then(|r| r.message)
                .or(result.message)
                .unwrap_or_else(|| "the network refused the copy".into());
            if op != Op::Update {
                record.state = State::Refused;
            }
            settle(records, record, Outcome::Refused { cause, reason })
        }
        CopyCall::Answered(result) => match result.copy {
            Some(CopyOutcome { handoff: Some(handoff), .. }) => {
                record.state = State::HandedOff;
                record.hash = Some(hash);
                record.handoff = Some(handoff.clone());
                record.reason = None;
                settle(records, record, Outcome::HandedOff { text: handoff.text, open_url: handoff.open_url })
            }
            Some(CopyOutcome { id: Some(id), url: Some(url), parts, .. }) => {
                record.state = State::Posted;
                record.remote = Some(super::record::RemoteRef { id, url: url.clone() });
                record.parts = parts;
                record.hash = Some(hash);
                record.update_due = false;
                record.reason = None;
                record.handoff = None;
                let write_back = write_back(folder, &record.source_path, &url).err();
                let outcome = if op == Op::Update { Outcome::Updated { url } } else { Outcome::Posted { url } };
                let mut s = settle(records, record, outcome);
                s.write_back = write_back;
                s
            }
            _ => unknown(record, format!("{} answered without the copy's id and address", op.hook())),
        },
        CopyCall::NoExport if op == Op::Update => {
            record.update_due = false;
            settle(records, record, Outcome::CannotEdit)
        }
        CopyCall::NoExport => {
            record.state = State::Refused;
            settle(records, record, Outcome::Refused { cause: Cause::Unknown, reason: "the channel exports no publish_copy".into() })
        }
        CopyCall::Unknown(reason) => unknown(record, reason),
    })
}

/// Remove every copy of one post from the networks that can remove one. For a
/// post the author deleted; nothing calls this on its own.
pub async fn delete_copies(folder: &Path, manager: &PluginManager, uid: &str, site_url: &str, emit: &(dyn Fn(&Sent) + Sync)) -> Vec<Sent> {
    let records = Records::in_vault(&crate::moss_paths::MossPaths::new(folder));
    let Some(_turn) = records.turn() else { return Vec::new() };
    let mut out = Vec::new();
    for record in records.all().into_iter().filter(|r| r.uid == uid) {
        let Some(previous) = record.remote.clone().filter(|_| record.state == State::Posted) else { continue };
        let job = CopyJob::delete(&record, previous, site_url);
        emit(&sent(&record, Outcome::Sending));
        let outcome = match manager.send_copy(&job).await {
            CopyCall::Answered(r) if r.success => match records.remove(&record.uid, &record.channel) {
                Ok(()) => Outcome::Deleted,
                Err(e) => Outcome::Refused { cause: Cause::Unknown, reason: format!("deleted, but its record remains ({e})") },
            },
            CopyCall::Answered(r) => Outcome::Refused {
                cause: r.copy.as_ref().and_then(|c| c.refused.as_ref()).map(|f| f.cause).unwrap_or_default(),
                reason: r.message.unwrap_or_else(|| "the network refused".into()),
            },
            CopyCall::NoExport => Outcome::CannotDelete,
            CopyCall::Unknown(reason) => Outcome::Refused { cause: Cause::Unknown, reason },
        };
        let s = sent(&record, outcome);
        emit(&s);
        out.push(s);
    }
    out
}

/// Whether a reader elsewhere following `url` gets the page: a few tries,
/// since a publish can land a little before every edge serves it. An onion
/// address is taken on trust, as nothing here can reach one.
async fn answers(url: &str) -> bool {
    if url.split('/').nth(2).is_some_and(|host| host.ends_with(".onion")) {
        return true;
    }
    let Ok(client) = crate::system::proxy_reqwest::build_client(|builder| {
        builder.timeout(std::time::Duration::from_secs(10))
    }) else {
        return true;
    };
    for wait in [0, 2, 5] {
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
        if client.get(url).send().await.is_ok_and(|r| r.status().is_success()) {
            return true;
        }
    }
    false
}

enum Found {
    Copy(CopyOutcome),
    /// A draft on the network, still for the author to publish.
    Waiting(super::record::Handoff),
    /// The network says there is no such copy, so sending is safe.
    Absent,
    Unknown(String),
}

/// Ask the channel's `find_copy` whether the copy of an uncertain send exists.
async fn reconcile(manager: &PluginManager, record: &CopyRecord, channel: &CopyChannel, post: &ArticleInfo, lang: Option<String>, opts: &SendOptions<'_>) -> Found {
    let (job, _) = match build_job(record, channel, post, lang, Op::Find, record.site(opts.site_url)) {
        Ok(j) => j,
        Err(e) => return Found::Unknown(e),
    };
    match manager.send_copy(&job).await {
        CopyCall::Answered(r) if r.success => match r.copy {
            Some(c @ CopyOutcome { id: Some(_), url: Some(_), .. }) => Found::Copy(c),
            Some(CopyOutcome { handoff: Some(h), .. }) => Found::Waiting(h),
            _ => Found::Absent,
        },
        CopyCall::Answered(r) => Found::Unknown(r.message.unwrap_or_else(|| "the lookup failed".into())),
        CopyCall::NoExport => Found::Unknown("this channel cannot look a copy up".into()),
        CopyCall::Unknown(e) => Found::Unknown(e),
    }
}

/// Record a copy the network turned out to hold.
fn adopt(folder: &Path, records: &Records, mut record: CopyRecord, outcome: CopyOutcome) -> Sent {
    let url = outcome.url.unwrap_or_default();
    record.state = State::Posted;
    record.remote = Some(super::record::RemoteRef { id: outcome.id.unwrap_or_default(), url: url.clone() });
    record.parts = outcome.parts;
    record.reason = None;
    record.handoff = None;
    let write_back = write_back(folder, &record.source_path, &url).err();
    let mut s = settle(records, record, Outcome::Posted { url });
    s.write_back = write_back;
    s
}

fn mark_unconfirmed(records: &Records, record: &mut CopyRecord, reason: &str) {
    record.state = State::Unconfirmed;
    record.reason = Some(reason.to_string());
    record.touch();
    let _ = records.save(record);
}

/// The post's language: its own `lang:`, else its language edition's, else
/// the site's.
fn post_lang(post: &ArticleInfo, articles: &ArticleMap, site_lang: Option<&str>) -> Option<String> {
    let own = post.frontmatter.get("lang").and_then(|l| l.as_str()).map(str::to_string);
    let edition = Some(articles.editions.of(&post.url_path)).filter(|e| !e.is_empty());
    own.or(edition).or_else(|| site_lang.map(str::to_string))
}

/// The job for one copy, and the hash of the copy it carries.
fn build_job(record: &CopyRecord, channel: &CopyChannel, post: &ArticleInfo, lang: Option<String>, op: Op, site_url: &str) -> Result<(CopyJob, String), String> {
    let copy = shape(post, &channel.spec, site_url)?;
    let hash = copy.hash();
    let job = CopyJob {
        op,
        channel: record.channel.clone(),
        idempotency_key: idempotency_key(&record.uid, &record.channel),
        post: PostRef {
            url: page_url(site_url, &post.url_path),
            source_path: post.source_path.clone(),
            title: post.title.clone(),
            lang,
            date: post.date.clone(),
            tags: post.tags.clone(),
        },
        copy: Some(copy),
        previous: record.remote.clone().or_else(|| record.draft()).map(|copy| Previous { copy, parts: record.parts.clone() }),
        config: HashMap::new(),
    };
    Ok((job, hash))
}

fn settle(records: &Records, mut record: CopyRecord, outcome: Outcome) -> Sent {
    if let Outcome::Refused { reason, .. } = &outcome {
        record.reason = Some(reason.clone());
    }
    record.touch();
    let s = sent(&record, outcome);
    if let Err(e) = records.save(&record) {
        log::warn!(target: "syndication", "copy record for {} on {} not saved: {e}", record.source_path, record.channel);
    }
    s
}

fn sent(record: &CopyRecord, outcome: Outcome) -> Sent {
    Sent { source_path: record.source_path.clone(), channel: record.channel.clone(), outcome, write_back: None }
}

/// Add `url` to the page's `syndicated:` list, changing only that field.
fn write_back(folder: &Path, source_path: &str, url: &str) -> Result<(), String> {
    let path = folder.join(source_path);
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut list: Vec<serde_yaml::Value> = match moss_core::frontmatter::parse(&content).frontmatter.get("syndicated") {
        Some(serde_yaml::Value::Sequence(items)) => items.clone(),
        Some(serde_yaml::Value::String(one)) => vec![serde_yaml::Value::String(one.clone())],
        _ => Vec::new(),
    };
    if list.iter().any(|v| v.as_str() == Some(url)) {
        return Ok(());
    }
    list.push(serde_yaml::Value::String(url.to_string()));
    let edited = moss_core::frontmatter::edit_fields(
        &content,
        &[moss_core::frontmatter::FieldEdit::Set { key: "syndicated".into(), value: serde_yaml::Value::Sequence(list) }],
    )?;
    crate::infra::atomic_write::write_atomic(&path, &edited)
}

/// One line per copy for a terminal.
pub fn render(sent: &[Sent]) -> String {
    let mut out = String::new();
    for s in sent {
        let line = match &s.outcome {
            Outcome::Sending | Outcome::Unchanged => continue,
            Outcome::CannotEdit => format!("{} cannot edit its copy; the page changed after it was posted", s.channel),
            Outcome::Posted { url } => format!("posted to {}: {url}", s.channel),
            Outcome::Updated { url } => format!("updated on {}: {url}", s.channel),
            Outcome::HandedOff { text, open_url } if text.is_empty() => {
                format!("waiting for you to publish it on {}: {}", s.channel, open_url.as_deref().unwrap_or_default())
            }
            Outcome::HandedOff { text, open_url } => format!(
                "ready to paste into {}{}:\n{}\n",
                s.channel,
                open_url.as_deref().map(|u| format!(" ({u})")).unwrap_or_default(),
                text
            ),
            Outcome::Refused { cause, reason } => format!("not posted to {} ({}): {reason}", s.channel, serde_json::to_value(cause).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()),
            Outcome::Unconfirmed { reason } => format!(
                "unconfirmed on {}: {reason}. If the copy is there, add its address to syndicated:; otherwise run moss deploy --retry-copies={}",
                s.channel, s.channel
            ),
            Outcome::Held { reason } => format!("held for {}: {reason}", s.channel),
            Outcome::Deleted => format!("deleted from {}", s.channel),
            Outcome::CannotDelete => format!("{} cannot remove a copy; it stays", s.channel),
        };
        out.push_str(&format!("{} — {line}\n", s.source_path));
        if let Some(e) = &s.write_back {
            out.push_str(&format!("  the address is in moss's record but not in the page: {e}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_address_is_added_to_syndicated_and_nothing_else_changes() {
        let dir = tempfile::tempdir().unwrap();
        let page = "---\ntitle: A  # kept\nsyndicated:\n  - https://matters.town/a/1\n---\nBody.\n";
        std::fs::write(dir.path().join("a.md"), page).unwrap();
        write_back(dir.path(), "a.md", "https://bsky.app/x/1").unwrap();
        write_back(dir.path(), "a.md", "https://bsky.app/x/1").unwrap();
        let after = std::fs::read_to_string(dir.path().join("a.md")).unwrap();
        assert!(after.starts_with("---\ntitle: A  # kept\n"), "{after}");
        assert_eq!(after.matches("https://bsky.app/x/1").count(), 1);
        assert!(after.contains("https://matters.town/a/1") && after.ends_with("---\nBody.\n"));
    }

    /// A deleted post's copies go where the network can remove them; where it
    /// cannot, the copy stays and says so.
    #[tokio::test]
    async fn delete_copies_removes_what_each_network_can_and_says_what_it_cannot() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::test_channel(root, "removes", "{ publish_copy: async function () { return { success: true }; }, delete_copy: async function (job) { return { success: job.previous.id === 'r-1' }; } }");
        super::super::test_channel(root, "keeps", "{ publish_copy: async function () { return { success: true }; } }");
        let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
        for (channel, id) in [("removes", "r-1"), ("keeps", "k-1")] {
            let mut r = CopyRecord::due("ab12cd34", channel, "notes/a.md", "notes/a/");
            r.state = State::Posted;
            r.remote = Some(super::super::record::RemoteRef { id: id.into(), url: format!("https://example.social/{id}") });
            records.save(&r).unwrap();
        }

        let cache = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned());
        let manager = cache.get_or_create(&root.to_string_lossy()).unwrap();
        let events = std::sync::Mutex::new(Vec::new());
        let emit = |s: &Sent| events.lock().unwrap().push(s.outcome.clone());
        let done = delete_copies(root, &manager, "ab12cd34", "https://notes.example.org", &emit).await;

        let by_channel: BTreeMap<_, _> = done.iter().map(|s| (s.channel.as_str(), s.outcome.clone())).collect();
        assert_eq!(by_channel["removes"], Outcome::Deleted);
        assert_eq!(by_channel["keeps"], Outcome::CannotDelete);
        assert!(!records.exists("ab12cd34", "removes"));
        assert!(records.exists("ab12cd34", "keeps"), "a copy that stays keeps its record");
        assert_eq!(events.lock().unwrap().iter().filter(|o| **o == Outcome::Sending).count(), 2);
    }

    /// While another run holds the folder's turn, neither a send nor a delete
    /// loads a channel or touches a copy.
    #[tokio::test]
    async fn a_run_that_finds_the_folder_busy_loads_no_channel_and_sends_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::test_channel(root, "removes", "{ publish_copy: async function () { return { success: true }; }, delete_copy: async function () { return { success: true }; } }");
        let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
        let mut r = CopyRecord::due("ab12cd34", "removes", "notes/a.md", "notes/a/");
        r.state = State::Posted;
        r.remote = Some(super::super::record::RemoteRef { id: "r-1".into(), url: "https://example.social/r-1".into() });
        records.save(&r).unwrap();
        let mp = crate::moss_paths::MossPaths::new(root);
        std::fs::create_dir_all(mp.article_map().parent().unwrap()).unwrap();
        std::fs::write(mp.article_map(), serde_json::to_vec(&ArticleMap::default()).unwrap()).unwrap();
        let held = records.turn().expect("the turn is free");

        let emit = |_: &Sent| {};
        let opts = SendOptions { site_url: "https://example.org", retry: &[], emit: &emit, only_post: None, only_channel: None, cap: PER_RUN, check_pages: false };
        assert!(after_live(root, &opts, || panic!("no channel may load")).await.is_empty());
        let manager = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned()).get_or_create(&root.to_string_lossy()).unwrap();
        assert!(delete_copies(root, &manager, "ab12cd34", "https://example.org", &emit).await.is_empty());
        assert!(records.exists("ab12cd34", "removes"), "the copy and its record stay");

        drop(held);
        assert_eq!(delete_copies(root, &manager, "ab12cd34", "https://example.org", &emit).await[0].outcome, Outcome::Deleted);
    }

    /// What a host asks for: one post's copy on one channel now, or the held
    /// rest under its own limit.
    #[tokio::test]
    async fn a_host_sends_one_posts_copy_on_one_channel_or_the_rest_under_its_limit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let posts = "{ publish_copy: async function (job) { return { success: true, copy: { id: job.post.url, url: 'https://example.social/' + job.post.source_path } }; } }";
        super::super::test_channel(root, "one", posts);
        super::super::test_channel(root, "two", posts);
        let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
        let mut articles = ArticleMap::default();
        for (uid, name) in [("aaaa0001", "a"), ("aaaa0002", "b"), ("aaaa0003", "c")] {
            let (source, url) = (format!("notes/{name}.md"), format!("notes/{name}/"));
            std::fs::create_dir_all(root.join("notes")).unwrap();
            std::fs::write(root.join(&source), "Hello.\n").unwrap();
            for channel in ["one", "two"] {
                let mut r = CopyRecord::due(uid, channel, &source, &url);
                // The address the planner recorded wins over the one the publish reported.
                r.site_url = (name == "b").then(|| "https://notes.example.org".into());
                records.save(&r).unwrap();
            }
            let post: ArticleInfo = serde_json::from_value(serde_json::json!({
                "source_path": source, "title": "", "content": "Hello.\n", "url_path": url, "date": null,
                "uid": uid, "syndicate": ["one", "two"]
            }))
            .unwrap();
            articles.articles.insert(post.url_path.clone(), post);
        }
        let cache = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned());
        let manager = cache.get_or_create(&root.to_string_lossy()).unwrap();
        let emit = |_: &Sent| {};
        let opts = |only_post, only_channel, cap| SendOptions {
            site_url: "https://publish.example.net",
            retry: &[],
            emit: &emit,
            only_post,
            only_channel,
            cap,
            check_pages: false,
        };
        let outcomes = |sent: &[Sent]| sent.iter().map(|s| (s.source_path.clone(), s.channel.clone(), s.outcome.clone())).collect::<Vec<_>>();
        let posted = |path: &str| Outcome::Posted { url: format!("https://example.social/{path}") };

        let one = send_due(root, &manager, &articles, &opts(Some("aaaa0002"), Some("two"), PER_RUN)).await;
        assert_eq!(outcomes(&one), [("notes/b.md".into(), "two".into(), posted("notes/b.md"))]);
        assert_eq!(records.get("aaaa0002", "two").unwrap().remote.unwrap().id, "https://notes.example.org/notes/b/");

        let rest = send_due(root, &manager, &articles, &opts(None, None, 1)).await;
        let got = outcomes(&rest);
        let held = |s: &&(String, String, Outcome)| matches!(&s.2, Outcome::Held { reason } if reason.contains("limit of 1"));
        assert_eq!(got.iter().filter(|s| matches!(s.2, Outcome::Posted { .. })).count(), 2, "one new copy per channel: {got:?}");
        assert_eq!(got.iter().filter(held).count(), 3, "{got:?}");
        assert_eq!(records.get("aaaa0001", "two").unwrap().remote.unwrap().id, "https://publish.example.net/notes/a/");
    }

    /// Two runs in one folder, the second started once the first has read its
    /// records and is about to post: the second sends nothing, so the copy
    /// goes out once.
    #[tokio::test]
    async fn a_second_run_in_the_folder_sends_nothing_while_the_first_is_sending() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::test_channel(root, "once", "{ publish_copy: async function () { return { success: true, copy: { id: 'c-1', url: 'https://example.social/c-1' } }; } }");
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(root.join("notes/a.md"), "Hello.\n").unwrap();
        let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
        records.save(&CopyRecord::due("aaaa0001", "once", "notes/a.md", "notes/a/")).unwrap();
        let post: ArticleInfo = serde_json::from_value(serde_json::json!({
            "source_path": "notes/a.md", "title": "", "content": "Hello.\n", "url_path": "notes/a/", "date": null,
            "uid": "aaaa0001", "syndicate": ["once"]
        }))
        .unwrap();
        let mut articles = ArticleMap::default();
        articles.articles.insert(post.url_path.clone(), post);
        async fn run(root: &Path, articles: &ArticleMap, emit: &(dyn Fn(&Sent) + Sync)) -> Vec<Sent> {
            let manager = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned()).get_or_create(&root.to_string_lossy()).unwrap();
            let opts = SendOptions { site_url: "https://example.org", retry: &[], emit, only_post: None, only_channel: None, cap: PER_RUN, check_pages: false };
            send_due(root, &manager, articles, &opts).await
        }
        let posted = |sent: &[Sent]| sent.iter().filter(|s| matches!(s.outcome, Outcome::Posted { .. })).count();

        let second: std::sync::Mutex<Option<Vec<Sent>>> = std::sync::Mutex::new(None);
        let start_second = |s: &Sent| {
            if s.outcome != Outcome::Sending || second.lock().unwrap().is_some() {
                return;
            }
            let quiet = |_: &Sent| {};
            let sent = std::thread::scope(|scope| {
                scope
                    .spawn(|| tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(run(root, &articles, &quiet)))
                    .join()
                    .unwrap()
            });
            *second.lock().unwrap() = Some(sent);
        };
        let first = run(root, &articles, &start_second).await;

        let second = second.into_inner().unwrap().expect("the second run started");
        assert_eq!(posted(&first), 1, "{first:?}");
        assert_eq!(posted(&second), 0, "the second run posted the copy again: {second:?}");
        assert_eq!(records.get("aaaa0001", "once").unwrap().state, State::Posted);
    }

    /// A copy left as a draft for the author to finish on the network waits
    /// quietly, is recorded as posted once the network says it was published,
    /// and is sent again if the author deleted the draft.
    #[tokio::test]
    async fn a_draft_left_for_the_author_is_adopted_once_published_and_resent_once_gone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let finds = |state: &str| {
            format!(
                "{{ publish_copy: async function () {{ return {{ success: false, message: 'sent again' }}; }}, \
                 find_copy: async function (job) {{ if (job.previous.id !== 'd-1') return {{ success: false }}; \
                   return '{state}' === 'published' ? {{ success: true, copy: {{ id: 'a-1', url: 'https://articles.example.net/a-1' }} }} \
                     : '{state}' === 'waiting' ? {{ success: true, copy: {{ handoff: {{ text: '', open_url: 'https://articles.example.net/draft/d-1', id: 'd-1' }} }} }} \
                     : {{ success: true, copy: null }}; }} }}"
            )
        };
        super::super::test_channel(root, "drafts", &finds("waiting"));
        std::fs::create_dir_all(root.join("essays")).unwrap();
        std::fs::write(root.join("essays/a.md"), "---\ntitle: A\n---\nBody.\n").unwrap();
        let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
        let handoff = super::super::record::Handoff { text: String::new(), open_url: Some("https://articles.example.net/draft/d-1".into()), id: Some("d-1".into()) };
        let mut r = CopyRecord::due("aaaa0001", "drafts", "essays/a.md", "essays/a/");
        r.state = State::HandedOff;
        r.handoff = Some(handoff);
        records.save(&r).unwrap();
        let post: ArticleInfo = serde_json::from_value(serde_json::json!({
            "source_path": "essays/a.md", "title": "A", "content": "Body.\n", "url_path": "essays/a/", "date": null,
            "uid": "aaaa0001", "syndicate": ["drafts"]
        }))
        .unwrap();
        let mut articles = ArticleMap::default();
        articles.articles.insert(post.url_path.clone(), post);
        let emit = |_: &Sent| {};
        let opts = SendOptions { site_url: "https://example.org", retry: &[], emit: &emit, only_post: None, only_channel: None, cap: PER_RUN, check_pages: false };
        let opts = &opts;
        let send = |state: &str| {
            std::fs::write(root.join(".moss/plugins/drafts/main.bundle.js"), format!("window.DRAFTSPlugin = {};", finds(state))).unwrap();
            let manager = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned()).get_or_create(&root.to_string_lossy()).unwrap();
            let articles = articles.clone();
            async move { send_due(root, &manager, &articles, opts).await.into_iter().map(|s| s.outcome).collect::<Vec<_>>() }
        };

        assert!(send("waiting").await.is_empty(), "an unpublished draft waits quietly");
        assert_eq!(records.get("aaaa0001", "drafts").unwrap().state, State::HandedOff);

        assert_eq!(send("published").await, [Outcome::Posted { url: "https://articles.example.net/a-1".into() }]);
        assert!(std::fs::read_to_string(root.join("essays/a.md")).unwrap().contains("https://articles.example.net/a-1"));

        r.state = State::HandedOff;
        records.save(&r).unwrap();
        assert_eq!(send("gone").await, [Outcome::Refused { cause: Cause::Unknown, reason: "sent again".into() }], "a deleted draft is sent again");
    }

    #[test]
    fn a_copy_is_in_the_posts_own_language_else_its_editions_else_the_sites() {
        let post = |path: &str, lang: Option<&str>| -> ArticleInfo {
            let mut fm = serde_json::json!({});
            if let Some(l) = lang {
                fm["lang"] = l.into();
            }
            serde_json::from_value(serde_json::json!({
                "source_path": format!("{path}.md"), "title": "", "content": "", "url_path": format!("{path}/"), "frontmatter": fm
            }))
            .unwrap()
        };
        let articles = ArticleMap { editions: moss_core::home::Editions::new("zh", ["de/a.md"]), ..Default::default() };
        assert_eq!(post_lang(&post("notes/a", Some("en")), &articles, Some("zh")).as_deref(), Some("en"));
        assert_eq!(post_lang(&post("de/a", None), &articles, Some("zh")).as_deref(), Some("de"));
        assert_eq!(post_lang(&post("notes/a", None), &articles, Some("zh")).as_deref(), Some("zh"));
    }

    #[test]
    fn a_handoff_prints_its_text_and_where_to_paste_it() {
        let s = Sent {
            source_path: "essays/a.md".into(),
            channel: "substack".into(),
            outcome: Outcome::HandedOff { text: "Autumn\n\nhttps://example.org/a/".into(), open_url: Some("https://example.substack.com/publish".into()) },
            write_back: None,
        };
        let text = render(&[s]);
        assert!(text.contains("ready to paste into substack (https://example.substack.com/publish)"));
        assert!(text.contains("Autumn\n\nhttps://example.org/a/"));
    }

    #[test]
    fn a_draft_on_the_network_says_where_to_publish_it_and_nothing_to_paste() {
        let s = Sent {
            source_path: "essays/a.md".into(),
            channel: "matters".into(),
            outcome: Outcome::HandedOff { text: String::new(), open_url: Some("https://articles.example.net/draft/d-1".into()) },
            write_back: None,
        };
        assert_eq!(render(&[s]), "essays/a.md — waiting for you to publish it on matters: https://articles.example.net/draft/d-1\n");
    }
}
