//! Sending the copies that are due, after a publish has returned. A failure
//! here never touches the publish: it is already live, and a copy left due is
//! sent by the next run.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::Serialize;

use super::job::{CopyJob, Op, PostRef, Previous};
use super::record::{Cause, CopyOutcome, CopyRecord, PublishedPost, Records, State};
use super::shape::{idempotency_key, page_url, shape_record, ShapeOptions};
use crate::build::scan::article_map::{ArticleInfo, ArticleMap};
use crate::build::scan::path_collision::{logical_path, on_disk};
use crate::plugins::manager::copies::{Account, CopyCall, CopyChannel};
use crate::plugins::manager::PluginManager;

/// At most this many new copies per channel per run; the rest stay due. A
/// burst of old posts on someone's timeline cannot be taken back.
pub const PER_RUN: usize = 10;

const INTERRUPTED: &str = "the last send stopped before the network answered";

/// How long after it was handed off a copy is looked up on its own; later, it
/// is adopted when the author lists its address.
const HANDOFF_LOOKUP_DAYS: i64 = 30;

const OTHER_ACCOUNT: &str = "posted from another account. Sign in to that account.";

/// The account each channel is signed in to, asked once per run and only of
/// a channel with a copy to work on.
#[derive(Default)]
struct Accounts(HashMap<String, Account>);

impl Accounts {
    /// `record` tied to its channel's account, or why it waits; see [`bind`].
    async fn check(&mut self, manager: &PluginManager, record: &mut CopyRecord) -> Result<(), Outcome> {
        if !self.0.contains_key(&record.channel) {
            let account = manager.copy_account(&record.channel).await;
            self.0.insert(record.channel.clone(), account);
        }
        bind(record, &self.0[&record.channel])
    }
}

/// Tie `record` to the account its channel is signed in to, or say why it
/// waits: sent from another account, nothing may update, delete, look up or
/// resend it, since the copy is not this account's. A copy never sent, or
/// refused outright, has nothing on the network and goes with the new one.
fn bind(record: &mut CopyRecord, account: &Account) -> Result<(), Outcome> {
    match (account, &record.account) {
        (Account::Unsaid, _) => Ok(()),
        (Account::Unknown(why), _) => Err(Outcome::Held { why: HeldWhy::Failed, reason: format!("which {} account is signed in is unknown ({why})", record.channel) }),
        (Account::Known(now), Some(sent_from)) if sent_from != now && !record.definite_first() => Err(Outcome::Held { why: HeldWhy::OtherAccount, reason: OTHER_ACCOUNT.into() }),
        (Account::Known(now), _) => {
            record.account = Some(now.clone());
            Ok(())
        }
    }
}

/// The handed-off copies this run looks up: per channel, the `cap` newest
/// handed off within [`HANDOFF_LOOKUP_DAYS`]. A lookup that changes nothing
/// does not rewrite the record, so `updated_at` is when it was handed off, and
/// older ones age out rather than crowd the newest.
fn handoffs_to_look_up(records: &[CopyRecord], cap: usize) -> HashSet<(String, String)> {
    let since = chrono::Utc::now() - chrono::Duration::days(HANDOFF_LOOKUP_DAYS);
    let mut recent: Vec<(&CopyRecord, chrono::DateTime<chrono::FixedOffset>)> = records
        .iter()
        .filter(|r| r.state == State::HandedOff)
        .filter_map(|r| Some((r, chrono::DateTime::parse_from_rfc3339(&r.updated_at).ok().filter(|at| *at > since)?)))
        .collect();
    recent.sort_by(|a, b| b.1.cmp(&a.1));
    let mut per_channel: BTreeMap<&str, usize> = BTreeMap::new();
    recent
        .into_iter()
        .filter(|(r, _)| {
            let n = per_channel.entry(r.channel.as_str()).or_default();
            *n += 1;
            *n <= cap
        })
        .map(|(r, _)| (r.uid.clone(), r.channel.clone()))
        .collect()
}

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
    /// Left due and sent by a later run. `why` is what a person is told;
    /// `reason` is the detail, for the log and the command line.
    Held { why: HeldWhy, reason: String },
    Deleted,
    /// The network cannot remove a copy; it stays where it is.
    CannotDelete,
}

/// Why a copy was left for a later run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum HeldWhy {
    /// Its page is not in this publish.
    NotPublished,
    /// Over this run's limit of new copies on the channel.
    OverLimit,
    PageNotAnswering,
    /// Its record could not be written, so nothing was sent.
    NotRecorded,
    /// The channel's plugin failed before answering; the same key makes a
    /// second try safe.
    Failed,
    /// The copy was sent from an account other than the one the channel is
    /// signed in to now.
    OtherAccount,
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
    /// Only this post's copies (by its source file), and only on this channel.
    pub only_source: Option<&'a str>,
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
    only_source: Option<&str>,
    emit: &(dyn Fn(&Sent) + Sync),
) -> Vec<Sent> {
    let opts = SendOptions { site_url, retry, emit, only_source, only_channel: None, cap: PER_RUN, check_pages: true };
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
    // The latest build's pages: which post a host asks for, and which channels
    // each post still names. A record that keeps its post answers without them.
    let articles: ArticleMap = match std::fs::read(mp.article_map()).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
        Ok(map) => map,
        Err(e) => {
            log::warn!(target: "syndication", "the latest build's pages could not be read ({e}); only copies that keep their post are sent");
            ArticleMap::default()
        }
    };
    let latest = latest_by_uid(&articles);
    if !records.all().iter().any(|r| asked_for(r, post_now(r, &latest), opts)) {
        return Vec::new();
    }
    let manager = match manager() {
        Ok(m) => m,
        Err(e) => {
            log::warn!(target: "syndication", "copies not sent: the channels could not be loaded ({e})");
            return Vec::new();
        }
    };
    send_in_turn(folder, &manager, &articles, opts).await
}

/// Send every due copy in the folder's records, each with the post its record
/// keeps; `articles`, the latest build's, answers for a record that keeps none.
pub async fn send_due(folder: &Path, manager: &PluginManager, articles: &ArticleMap, opts: &SendOptions<'_>) -> Vec<Sent> {
    let Some(_turn) = Records::in_vault(&crate::moss_paths::MossPaths::new(folder)).turn() else { return Vec::new() };
    send_in_turn(folder, manager, articles, opts).await
}

/// [`send_due`] for a caller already holding the folder's turn: a second
/// lock on it from this process would be refused, and nothing sent.
async fn send_in_turn(folder: &Path, manager: &PluginManager, articles: &ArticleMap, opts: &SendOptions<'_>) -> Vec<Sent> {
    let mp = crate::moss_paths::MossPaths::new(folder);
    let records = Records::in_vault(&mp);
    let channels = manager.copy_channels();
    let by_uid = latest_by_uid(articles);

    // Post by post, each post's channels in the order its `syndicate:` names them.
    let mut due = records.all();
    due.sort_by_cached_key(|r| {
        let post = r.post.as_ref().map(|p| &p.article).or_else(|| by_uid.get(&r.uid).copied());
        let rank = post.and_then(|p| p.syndicate.as_ref()).and_then(|c| c.iter().position(|c| *c == r.channel));
        (r.url_path.clone(), rank.unwrap_or(usize::MAX), r.channel.clone())
    });
    let config = match crate::build::site_config::read_project_config(&folder.to_string_lossy()) {
        Ok(config) => config,
        Err(e) => { log::warn!(target: "syndication", "copies not sent: {e}"); return Vec::new(); }
    };
    let mut sent_per_channel: BTreeMap<String, usize> = BTreeMap::new();
    let handoffs = handoffs_to_look_up(&due, opts.cap);
    let mut accounts = Accounts::default();
    let mut out = Vec::new();
    let mut push = |s: Sent| {
        (opts.emit)(&s);
        out.push(s);
    };
    for mut record in due {
        let now = post_now(&record, &by_uid);
        if !asked_for(&record, now, opts) {
            continue;
        }
        // The latest build is the author's latest word: a channel the post no
        // longer names gets nothing more, and its record keeps its history.
        if now.is_some_and(|p| !p.syndicate.as_ref().is_some_and(|c| c.contains(&record.channel))) {
            continue;
        }
        let Some(channel) = channels.get(&record.channel) else { continue };
        let Some(op) = record.next_op(channel.spec.idempotent) else { continue };
        // A handed-off copy rests there: it is handed off again only when the
        // author retries its channel for this one post.
        let retry = opts.retry.contains(&record.channel) && (record.state != State::HandedOff || opts.only_source.is_some());
        let rests = record.state == State::HandedOff && !retry;
        if rests && !handoffs.contains(&(record.uid.clone(), record.channel.clone())) {
            continue;
        }
        match accounts.check(manager, &mut record).await {
            // Said once for each time the copy is held.
            Err(held @ Outcome::Held { why: HeldWhy::OtherAccount, .. }) => {
                if !record.held_for_account {
                    record.held_for_account = true;
                    let _ = records.save(&record);
                    push(sent(&record, held));
                }
                continue;
            }
            // A handed-off copy has nothing to send, so it waits without a word.
            Err(_) if rests => continue,
            Err(held) => {
                push(sent(&record, held));
                continue;
            }
            Ok(()) if record.held_for_account => {
                record.held_for_account = false;
                let _ = records.save(&record);
            }
            Ok(()) => {}
        }
        // A record from before copies kept their post reads the latest build.
        let published = record.post.as_deref().cloned().or_else(|| by_uid.get(&record.uid).map(|a| PublishedPost::new(a, articles)));
        let Some(published) = published else {
            if record.state == State::Sending {
                mark_unconfirmed(&records, &mut record, INTERRUPTED);
                push(sent(&record, Outcome::Unconfirmed { reason: record.reason.clone().unwrap_or_default() }));
                continue;
            }
            if op == Op::Publish {
                push(sent(&record, Outcome::Held { why: HeldWhy::NotPublished, reason: "its page is not in this publish".into() }));
            }
            continue;
        };
        let post = &published.article;
        let shape = ShapeOptions::new(&config, folder, articles, post, &record.channel, published.lang.as_deref());
        // An uncertain copy is looked up before anything is sent again.
        let op = if op == Op::Find {
            match reconcile(manager, &record, channel, post, &shape, opts).await {
                Found::Copy(outcome) => {
                    push(adopt(folder, &records, record, outcome));
                    continue;
                }
                // A draft still waiting for the author is reported when it
                // first becomes one, and whenever the author asks about its post.
                Found::Waiting(handoff) => {
                    if record.handoff.as_ref() != Some(&handoff) || opts.only_source.is_some() {
                        record.state = State::HandedOff;
                        record.handoff = Some(handoff.clone());
                        record.reason = None;
                        push(settle(&records, record, Outcome::HandedOff { text: handoff.text, open_url: handoff.open_url }));
                    }
                    continue;
                }
                // A handoff waits for the author whatever the network says,
                // unless they asked for it to be handed off again.
                Found::Absent | Found::Unknown(_) if rests => continue,
                Found::Absent => {
                    record.discard_attempt();
                    record.state = State::Due;
                    Op::Publish
                }
                Found::Unknown(_) if retry => {
                    if record.state == State::HandedOff {
                        record.discard_attempt();
                        record.state = State::Due;
                    }
                    Op::Publish
                }
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
                push(sent(&record, Outcome::Held { why: HeldWhy::OverLimit, reason: format!("over this run's limit of {} new copies on {}", opts.cap, record.channel) }));
                continue;
            }
            *n += 1;
            if opts.check_pages {
                let page = page_url(record.site(opts.site_url), &post.url_path);
                if !answers(&page).await {
                    *n -= 1;
                    push(sent(&record, Outcome::Held { why: HeldWhy::PageNotAnswering, reason: format!("its page does not answer yet at {page}") }));
                    continue;
                }
            }
        }
        if let Some(s) = send_one(folder, manager, &records, record, channel, post, &shape, op, opts).await {
            push(s);
        }
    }
    out
}

fn latest_by_uid(articles: &ArticleMap) -> HashMap<String, &ArticleInfo> {
    articles.articles.values().filter_map(|a| Some((a.uid.clone()?, a))).collect()
}

/// A record's post as the author last left it: the latest build's page, else
/// the post the record keeps.
fn post_now<'a>(record: &'a CopyRecord, latest: &HashMap<String, &'a ArticleInfo>) -> Option<&'a ArticleInfo> {
    latest.get(&record.uid).copied().or(record.post.as_ref().map(|p| &p.article))
}

/// Whether `source` is a page of the latest build, in any spelling of it.
pub fn is_a_page(folder: &Path, source: &str) -> bool {
    let map = std::fs::read(crate::moss_paths::MossPaths::new(folder).article_map()).ok().and_then(|b| serde_json::from_slice::<ArticleMap>(&b).ok());
    map.is_some_and(|m| m.articles.values().any(|a| a.source_path == logical_path(source)))
}

/// A host may hand over the disk's spelling of the source; pages are keyed by
/// its logical path.
fn asked_for(record: &CopyRecord, now: Option<&ArticleInfo>, opts: &SendOptions<'_>) -> bool {
    opts.only_source.is_none_or(|s| now.is_some_and(|p| p.source_path == logical_path(s))) && opts.only_channel.is_none_or(|c| c == record.channel)
}

#[allow(clippy::too_many_arguments)]
async fn send_one(
    folder: &Path,
    manager: &PluginManager,
    records: &Records,
    mut record: CopyRecord,
    channel: &CopyChannel,
    post: &ArticleInfo,
    shape: &ShapeOptions,
    op: Op,
    opts: &SendOptions<'_>,
) -> Option<Sent> {
    record.source_path.clone_from(&post.source_path);
    record.url_path.clone_from(&post.url_path);
    let (job, hash) = match build_job(&record, channel, post, shape, op, record.site(opts.site_url)) {
        Ok(j) => j,
        // A copy already on the network keeps its saved record as it is.
        Err(reason) if record.remote.is_some() => return Some(sent(&record, Outcome::Refused { cause: Cause::Content, reason })),
        Err(reason) => return Some(settle(records, record, Outcome::Refused { cause: Cause::Content, reason })),
    };
    let had_uncertain_attempt = record.handoff.is_some()
        || matches!(record.state, State::Sending | State::Unconfirmed)
        || (record.state == State::Due && record.form.is_some());
    // The source changed but the copy would not (writing `syndicated:` back
    // is such an edit): nothing to send, nothing to say.
    if op == Op::Update && record.hash.as_deref() == Some(hash.as_str()) {
        record.update_due = false;
        record.touch();
        let _ = records.save(&record);
        return Some(sent(&record, Outcome::Unchanged));
    }

    if let Some(copy) = &job.copy {
        record.form = Some(copy.form);
        record.part_count = (record.remote.is_none() && copy.form == super::channel::Form::Thread).then_some(copy.parts.len());
    }
    (opts.emit)(&sent(&record, Outcome::Sending));
    if op == Op::Publish {
        record.state = State::Sending;
        record.touch();
        if let Err(e) = records.save(&record) {
            return Some(sent(&record, Outcome::Held { why: HeldWhy::NotRecorded, reason: format!("its record could not be written, so nothing was sent ({e})") }));
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
            sent(&record, Outcome::Held { why: HeldWhy::Failed, reason: format!("{reason}; sent again next run with the same key") })
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
            if op != Op::Update && had_uncertain_attempt {
                unknown(record, reason)
            } else {
                if op != Op::Update {
                    record.state = State::Refused;
                    record.discard_attempt();
                }
                settle(records, record, Outcome::Refused { cause, reason })
            }
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
                if op != Op::Update || !parts.is_empty() {
                    record.parts = parts;
                }
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
        CopyCall::NoExport if had_uncertain_attempt => unknown(record, "the channel exports no publish_copy".into()),
        CopyCall::NoExport => {
            record.state = State::Refused;
            record.discard_attempt();
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
    let mut accounts = Accounts::default();
    for mut record in records.all().into_iter().filter(|r| r.uid == uid) {
        let Some(previous) = record.remote.clone().filter(|_| record.state == State::Posted) else { continue };
        if let Err(held) = accounts.check(manager, &mut record).await {
            let s = sent(&record, held);
            emit(&s);
            out.push(s);
            continue;
        }
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
    /// Anything else: a failed or unrecognised answer, or none.
    Unknown(String),
}

/// Ask the channel's `find_copy` whether the copy of an uncertain send exists.
async fn reconcile(manager: &PluginManager, record: &CopyRecord, channel: &CopyChannel, post: &ArticleInfo, shape: &ShapeOptions, opts: &SendOptions<'_>) -> Found {
    let (job, _) = match build_job(record, channel, post, shape, Op::Find, record.site(opts.site_url)) {
        Ok(j) => j,
        Err(e) => return Found::Unknown(e),
    };
    match manager.send_copy(&job).await {
        CopyCall::Answered(r) if r.success => match r.copy {
            Some(c @ CopyOutcome { id: Some(_), url: Some(_), .. }) => Found::Copy(c),
            Some(CopyOutcome { handoff: Some(h), .. }) => Found::Waiting(h),
            Some(CopyOutcome { absent: true, .. }) => Found::Absent,
            _ => Found::Unknown("the lookup did not say whether the copy exists".into()),
        },
        CopyCall::Answered(r) => Found::Unknown(r.message.unwrap_or_else(|| "the lookup failed".into())),
        CopyCall::NoExport => Found::Unknown("this channel cannot look a copy up".into()),
        CopyCall::Unknown(e) => Found::Unknown(e),
    }
}

/// Record a copy the network holds. The caller holds the folder's send turn.
pub(super) fn adopt(folder: &Path, records: &Records, mut record: CopyRecord, outcome: CopyOutcome) -> Sent {
    let url = outcome.url.unwrap_or_default();
    record.state = State::Posted;
    record.remote = Some(super::record::RemoteRef { id: outcome.id.unwrap_or_default(), url: url.clone() });
    if !outcome.parts.is_empty() {
        record.parts = outcome.parts;
    }
    record.reason = None;
    record.handoff = None;
    let mut s = settle(records, record, Outcome::Posted { url: url.clone() });
    s.write_back = write_back(folder, &s.source_path, &url).err();
    s
}

fn mark_unconfirmed(records: &Records, record: &mut CopyRecord, reason: &str) {
    record.state = State::Unconfirmed;
    record.reason = Some(reason.to_string());
    record.touch();
    let _ = records.save(record);
}


/// The job for one copy, and the hash of the copy it carries.
fn build_job(record: &CopyRecord, channel: &CopyChannel, post: &ArticleInfo, options: &ShapeOptions, op: Op, site_url: &str) -> Result<(CopyJob, String), String> {
    let copy = match shape_record(post, &channel.spec, site_url, options, record) {
        Ok(copy) => Some(copy),
        // A lookup needs the stable identity even when changed limits prevent
        // this payload from being sent again.
        Err(_) if op == Op::Find => None,
        Err(e) => return Err(e),
    };
    let hash = copy.as_ref().map(|c| c.hash()).unwrap_or_default();
    let job = CopyJob {
        op,
        channel: record.channel.clone(),
        idempotency_key: idempotency_key(&record.uid, &record.channel),
        post: PostRef {
            url: page_url(site_url, &post.url_path),
            source_path: post.source_path.clone(),
            title: post.title.clone(),
            lang: options.lang.clone(),
            date: post.date.clone(),
            tags: post.tags.clone(),
        },
        copy,
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
pub(super) fn write_back(folder: &Path, source_path: &str, url: &str) -> Result<(), String> {
    crate::build::cloud_readiness::rewrite_input(&on_disk(folder, source_path), |bytes| {
        let content = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        let mut list: Vec<serde_yaml::Value> = match moss_core::frontmatter::parse(content).frontmatter.get("syndicated") {
            Some(serde_yaml::Value::Sequence(items)) => items.clone(),
            Some(serde_yaml::Value::String(one)) => vec![serde_yaml::Value::String(one.clone())],
            _ => Vec::new(),
        };
        if list.iter().any(|v| v.as_str() == Some(url)) {
            return Ok(None);
        }
        list.push(serde_yaml::Value::String(url.to_string()));
        moss_core::frontmatter::edit_fields(content, &[moss_core::frontmatter::FieldEdit::Set { key: "syndicated".into(), value: serde_yaml::Value::Sequence(list) }]).map(Some)
    })
    .map(drop)
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
                "unconfirmed on {}: {reason}. If the copy is there, add its address to syndicated: (for a thread, the first post's: moss tracks only that one). If it is not, run moss deploy --retry-copies={}; any part of a thread that did get through is posted a second time",
                s.channel, s.channel
            ),
            Outcome::Held { reason, .. } => format!("held for {}: {reason}", s.channel),
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
#[path = "send_account_tests.rs"]
mod account_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn planning_preview_and_send_share_emitted_formula_assets() {
        use super::super::{plan, preview, shape};
        use crate::build::emit::math_png::{email_math_img, emit_math_pngs};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let site = "https://example.org";
        let expected_url = page_url(site, email_math_img("x", false).unwrap().served_path().as_str());
        let js = format!(r#"{{ publish_copy: async function(job) {{
            if (job.pictures.length !== 1 || job.pictures[0].url !== {} ||
                job.pictures[0].alt !== '$x$' || job.included_pictures !== 1 || job.post.lang !== 'zh-Hans')
                throw new Error('formula or saved language differs from preview');
            return {{ success: true, copy: {{ id: 'formula', url: 'https://example.social/formula' }} }};
        }} }}"#, serde_json::to_string(&expected_url).unwrap());
        super::super::test_channel(root, "one", &js);
        let manifest_path = root.join(".moss/plugins/one/manifest.json");
        let mut manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["contributes"]["channel"]["syndication"]["images"] = serde_json::json!({"max": 4, "alt": true});
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let mp = crate::moss_paths::MossPaths::new(root);
        let doc = crate::build::types::ParsedDocument { content: "$x$".into(), ..Default::default() };
        emit_math_pngs(&[doc], true, &mp.initial_serve_dir(), &mut crate::build::manifest::PendingManifest::new(Default::default())).unwrap();
        std::fs::write(root.join("page.md"), "---\nuid: aa11aa11\nsyndicate: one\n---\n$x$\n").unwrap();
        let post: ArticleInfo = serde_json::from_value(serde_json::json!({
            "source_path": "page.md", "title": "", "content": "$x$", "url_path": "page/", "uid": "aa11aa11", "syndicate": ["one"],
            "html_content": crate::build::markdown::math::render_math("x", false).unwrap()
        })).unwrap();
        let mut map = ArticleMap::default();
        map.articles.insert(post.url_path.clone(), post.clone());
        map.page_languages.insert(post.url_path.clone(), "zh-Hans".into());
        map.page_site_names.insert(post.url_path.clone(), "山居".into());
        std::fs::create_dir_all(mp.article_map().parent().unwrap()).unwrap();
        std::fs::write(mp.article_map(), serde_json::to_vec(&map).unwrap()).unwrap();
        let config = crate::build::site_config::read_project_config(&root.to_string_lossy()).unwrap();
        let channels = super::super::landing::channels(root);
        let records = Records::in_vault(&mp);
        let plans = plan::decide(&plan::PlanInputs {
            folder: root, articles: &map, previously_live: None, edited_sources: &Default::default(),
            channels: &channels, config: &config, site_url: Some(site),
        }, &records);
        assert_eq!(plans[0].entries[0].action, plan::Action::Due);
        let options = ShapeOptions::new(&config, root, &map, &post, "one", None);
        assert_eq!(options.site_name.as_deref(), Some("山居"));
        let planned = shape::assess(&post, &channels["one"].spec, Some(site), &options, &CopyRecord::due("aa11aa11", "one", "page.md", "page/")).unwrap();
        assert_eq!(planned.pictures.len(), 1);
        assert_eq!(planned.pictures[0].url, expected_url);
        let previews = preview::for_post(root, "page.md", site);
        assert_eq!(previews[0].pictures, planned.pictures);
        assert_eq!(previews[0].included_pictures, 1);
        plan::apply(plans, &records, Some(site), Some(&map), &Default::default());
        // A subsequent build's language cannot change the saved publication.
        map.page_languages.insert(post.url_path.clone(), "en".into());
        let manager = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned()).get_or_create(&root.to_string_lossy()).unwrap();
        let emit = |_: &Sent| {};
        let opts = SendOptions { site_url: site, retry: &[], emit: &emit, only_source: None, only_channel: None, cap: PER_RUN, check_pages: false };
        let sent = send_due(root, &manager, &map, &opts).await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].outcome, Outcome::Posted { url: "https://example.social/formula".into() });
    }

    #[tokio::test]
    async fn only_definitively_failed_first_publications_discard_their_form() {
        use super::super::channel::{ChannelSpec, Form};
        struct Case { name: &'static str, js: &'static str, op: Op, uncertain: bool, state: State, form: Option<Form> }
        for Case { name, js, op, uncertain, state: expected_state, form: expected_form } in [
            Case { name: "missing export", js: "{}", op: Op::Publish, uncertain: false, state: State::Refused, form: None },
            Case { name: "first refusal", js: "{ publish_copy: async function () { return { success: false }; } }", op: Op::Publish, uncertain: false, state: State::Refused, form: None },
            Case { name: "lost answer", js: "{ publish_copy: async function () { throw new Error('lost answer'); } }", op: Op::Publish, uncertain: false, state: State::Unconfirmed, form: Some(Form::Short) },
            Case { name: "update refusal", js: "{ update_copy: async function () { return { success: false }; } }", op: Op::Update, uncertain: false, state: State::Posted, form: Some(Form::Short) },
            Case { name: "uncertain missing export", js: "{}", op: Op::Publish, uncertain: true, state: State::Unconfirmed, form: Some(Form::Short) },
            Case { name: "uncertain refusal", js: "{ publish_copy: async function () { return { success: false }; } }", op: Op::Publish, uncertain: true, state: State::Unconfirmed, form: Some(Form::Short) },
        ] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            super::super::test_channel(root, "one", js);
            let manager = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned()).get_or_create(&root.to_string_lossy()).unwrap();
            let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
            let mut record = CopyRecord::due("u", "one", "page.md", "page/");
            if uncertain {
                record.state = State::Unconfirmed;
                record.form = Some(Form::Short);
                record.part_count = Some(1);
            }
            if op == Op::Update {
                record.state = State::Posted;
                record.form = Some(Form::Short);
                record.part_count = Some(1);
                record.remote = Some(super::super::record::RemoteRef { id: "1".into(), url: "https://example.social/1".into() });
                record.update_due = true;
            }
            let channel = CopyChannel { spec: ChannelSpec { forms: vec![Form::Short], max_chars: Some(300), ..Default::default() }, domains: Vec::new() };
            let post: ArticleInfo = serde_json::from_value(serde_json::json!({"source_path": "page.md", "title": "", "content": "Hello.", "url_path": "page/"})).unwrap();
            let emit = |_: &Sent| {};
            let opts = SendOptions { site_url: "https://example.org", retry: &[], emit: &emit, only_source: None, only_channel: None, cap: PER_RUN, check_pages: false };
            send_one(root, &manager, &records, record, &channel, &post, &ShapeOptions::default(), op, &opts).await.unwrap();
            let saved = records.get("u", "one").unwrap();
            assert_eq!(saved.state, expected_state, "{name}");
            assert_eq!(saved.form, expected_form, "{name}");
            assert_eq!(saved.part_count, None, "short copies need no attempted thread count: {name}");
        }
    }

    #[tokio::test]
    async fn old_posted_copies_are_frozen_without_mutating_the_record() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::test_channel(root, "one", "{ update_copy: async function () { throw new Error('must not send'); } }");
        let manager = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned()).get_or_create(&root.to_string_lossy()).unwrap();
        let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
        let mut record = CopyRecord::due("u", "one", "page.md", "page/");
        record.state = State::Posted;
        record.remote = Some(super::super::record::RemoteRef { id: "1".into(), url: "https://example.social/1".into() });
        record.update_due = true;
        records.save(&record).unwrap();
        let before = records.get("u", "one").unwrap();
        let post: ArticleInfo = serde_json::from_value(serde_json::json!({"source_path": "edited.md", "title": "New", "content": "Edited", "url_path": "edited/"})).unwrap();
        let channel = CopyChannel { spec: super::super::channel::ChannelSpec { forms: vec![super::super::channel::Form::Long], ..Default::default() }, domains: Vec::new() };
        let emit = |_: &Sent| panic!("a frozen copy cannot send");
        let opts = SendOptions { site_url: "https://example.org", retry: &[], emit: &emit, only_source: None, only_channel: None, cap: PER_RUN, check_pages: false };
        let result = send_one(root, &manager, &records, record, &channel, &post, &ShapeOptions::default(), Op::Update, &opts).await.unwrap();
        assert!(matches!(result.outcome, Outcome::Refused { reason, .. } if reason.contains("retaining the prior version")));
        assert_eq!(records.get("u", "one").unwrap(), before);
    }

    #[test]
    fn lookup_keeps_the_identity_when_the_original_form_no_longer_fits() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = CopyRecord::due("u", "one", "page.md", "page/");
        record.form = Some(super::super::channel::Form::Short);
        record.part_count = Some(1);
        let channel = CopyChannel { spec: super::super::channel::ChannelSpec { forms: vec![super::super::channel::Form::Short], max_chars: Some(1), ..Default::default() }, domains: Vec::new() };
        let post: ArticleInfo = serde_json::from_value(serde_json::json!({ "source_path": "page.md", "title": "", "content": "Longer than allowed", "url_path": "page/", "date": null })).unwrap();
        let (job, _) = build_job(&record, &channel, &post, &ShapeOptions::default(), Op::Find, "https://example.org").unwrap();
        assert_eq!(job.idempotency_key, idempotency_key("u", "one"));
        assert!(job.copy.is_none());
        assert!(build_job(&record, &channel, &post, &ShapeOptions::default(), Op::Publish, "https://example.org").is_err());
    }

    #[test]
    fn an_unconfirmed_copy_names_what_each_recovery_costs_a_thread() {
        let line = render(&[Sent {
            source_path: "a.md".into(),
            channel: "bluesky".into(),
            outcome: Outcome::Unconfirmed { reason: INTERRUPTED.into() },
            write_back: None,
        }]);
        assert!(line.contains("syndicated: (for a thread, the first post's"), "{line}");
        assert!(line.contains("--retry-copies=bluesky; any part of a thread that did get through is posted a second time"), "{line}");
    }

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
        let opts = SendOptions { site_url: "https://example.org", retry: &[], emit: &emit, only_source: None, only_channel: None, cap: PER_RUN, check_pages: false };
        assert!(after_live(root, &opts, || panic!("no channel may load")).await.is_empty());
        let manager = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned()).get_or_create(&root.to_string_lossy()).unwrap();
        assert!(delete_copies(root, &manager, "ab12cd34", "https://example.org", &emit).await.is_empty());
        assert!(records.exists("ab12cd34", "removes"), "the copy and its record stay");

        drop(held);
        assert_eq!(delete_copies(root, &manager, "ab12cd34", "https://example.org", &emit).await[0].outcome, Outcome::Deleted);
    }

    /// A post is asked for by its source file, which the latest build names
    /// (a page renamed since its copy was planned answers to its new name),
    /// and a post no record has loads no channel.
    #[tokio::test]
    async fn a_post_is_asked_for_by_its_source_and_one_without_copies_loads_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
        records.save(&CopyRecord::due("aaaa0001", "one", "notes/old.md", "notes/old/")).unwrap();
        let mut articles = ArticleMap::default();
        let post: ArticleInfo = serde_json::from_value(serde_json::json!({
            "source_path": "notes/new.md", "title": "", "content": "Hello.\n", "url_path": "notes/new/", "date": null,
            "uid": "aaaa0001", "syndicate": ["one"]
        }))
        .unwrap();
        articles.articles.insert(post.url_path.clone(), post);
        let mp = crate::moss_paths::MossPaths::new(root);
        std::fs::create_dir_all(mp.article_map().parent().unwrap()).unwrap();
        std::fs::write(mp.article_map(), serde_json::to_vec(&articles).unwrap()).unwrap();
        let emit = |_: &Sent| {};
        let opts = |only_source| SendOptions { site_url: "https://example.org", retry: &[], emit: &emit, only_source, only_channel: None, cap: PER_RUN, check_pages: false };

        assert!(after_live(root, &opts(Some("notes/other.md")), || panic!("no channel may load")).await.is_empty());
        assert!(after_live(root, &opts(Some("notes/old.md")), || panic!("no channel may load")).await.is_empty());
        let loaded = std::sync::atomic::AtomicBool::new(false);
        after_live(root, &opts(Some("notes/new.md")), || {
            loaded.store(true, std::sync::atomic::Ordering::SeqCst);
            Err("no channels here".into())
        })
        .await;
        assert!(loaded.into_inner(), "the renamed post's copy is asked for by its new name");
    }

    /// A host may hand over the name a sync client wrote decomposed; the page
    /// is keyed by its composed spelling.
    #[tokio::test]
    async fn a_post_asked_for_by_its_decomposed_name_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
        records.save(&CopyRecord::due("aaaa0001", "one", "notes/caf\u{e9}.md", "notes/cafe/")).unwrap();
        let mut articles = ArticleMap::default();
        let post: ArticleInfo = serde_json::from_value(serde_json::json!({
            "source_path": "notes/caf\u{e9}.md", "title": "", "content": "Hello.\n", "url_path": "notes/cafe/", "date": null,
            "uid": "aaaa0001", "syndicate": ["one"]
        }))
        .unwrap();
        articles.articles.insert(post.url_path.clone(), post);
        let mp = crate::moss_paths::MossPaths::new(root);
        std::fs::create_dir_all(mp.article_map().parent().unwrap()).unwrap();
        std::fs::write(mp.article_map(), serde_json::to_vec(&articles).unwrap()).unwrap();
        let emit = |_: &Sent| {};
        let opts = SendOptions { site_url: "https://example.org", retry: &[], emit: &emit, only_source: Some("notes/cafe\u{301}.md"), only_channel: None, cap: PER_RUN, check_pages: false };
        let loaded = std::sync::atomic::AtomicBool::new(false);
        after_live(root, &opts, || {
            loaded.store(true, std::sync::atomic::Ordering::SeqCst);
            Err("no channels here".into())
        })
        .await;
        assert!(loaded.into_inner());
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
        let opts = |only_source, only_channel, cap| SendOptions {
            site_url: "https://publish.example.net",
            retry: &[],
            emit: &emit,
            only_source,
            only_channel,
            cap,
            check_pages: false,
        };
        let outcomes = |sent: &[Sent]| sent.iter().map(|s| (s.source_path.clone(), s.channel.clone(), s.outcome.clone())).collect::<Vec<_>>();
        let posted = |path: &str| Outcome::Posted { url: format!("https://example.social/{path}") };

        let one = send_due(root, &manager, &articles, &opts(Some("notes/b.md"), Some("two"), PER_RUN)).await;
        assert_eq!(outcomes(&one), [("notes/b.md".into(), "two".into(), posted("notes/b.md"))]);
        assert_eq!(records.get("aaaa0002", "two").unwrap().remote.unwrap().id, "https://notes.example.org/notes/b/");

        let rest = send_due(root, &manager, &articles, &opts(None, None, 1)).await;
        let got = outcomes(&rest);
        let held = |s: &&(String, String, Outcome)| matches!(&s.2, Outcome::Held { why: HeldWhy::OverLimit, .. });
        assert_eq!(got.iter().filter(|s| matches!(s.2, Outcome::Posted { .. })).count(), 2, "one new copy per channel: {got:?}");
        assert_eq!(got.iter().filter(held).count(), 3, "{got:?}");
        assert_eq!(records.get("aaaa0001", "two").unwrap().remote.unwrap().id, "https://publish.example.net/notes/a/");
    }

    /// A plugin that throws is held for the next run, which its key makes
    /// safe, and says so as a kind a person can be told, with the throw's own
    /// words as the detail.
    #[tokio::test]
    async fn a_channel_whose_plugin_throws_is_held_as_failed_with_its_words() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::test_channel(root, "keyed", "{ publish_copy: async function () { throw new TypeError('no route'); } }");
        let manifest = root.join(".moss/plugins/keyed/manifest.json");
        let mut m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
        m["contributes"]["channel"]["syndication"]["idempotent"] = true.into();
        std::fs::write(&manifest, m.to_string()).unwrap();
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(root.join("notes/a.md"), "Hello.\n").unwrap();
        Records::in_vault(&crate::moss_paths::MossPaths::new(root)).save(&CopyRecord::due("aaaa0001", "keyed", "notes/a.md", "notes/a/")).unwrap();
        let post: ArticleInfo = serde_json::from_value(serde_json::json!({
            "source_path": "notes/a.md", "title": "", "content": "Hello.\n", "url_path": "notes/a/", "date": null,
            "uid": "aaaa0001", "syndicate": ["keyed"]
        }))
        .unwrap();
        let mut articles = ArticleMap::default();
        articles.articles.insert(post.url_path.clone(), post);
        let manager = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned()).get_or_create(&root.to_string_lossy()).unwrap();
        let emit = |_: &Sent| {};
        let opts = SendOptions { site_url: "https://example.org", retry: &[], emit: &emit, only_source: None, only_channel: None, cap: PER_RUN, check_pages: false };
        let sent = send_due(root, &manager, &articles, &opts).await;
        let Some(Outcome::Held { why, reason }) = sent.last().map(|s| s.outcome.clone()) else { panic!("{sent:?}") };
        assert_eq!(why, HeldWhy::Failed);
        assert!(reason.contains("TypeError: no route") && !reason.contains("Exception"), "{reason}");
    }

    /// The author's latest word decides: a channel the post's `syndicate:` no
    /// longer names is sent nothing, though the post the record keeps named it,
    /// and its record, posted copy included, stays as it was.
    #[tokio::test]
    async fn a_channel_the_post_no_longer_names_is_sent_nothing_and_keeps_its_record() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let posts = "{ publish_copy: async function (job) { return { success: true, copy: { id: job.post.url, url: 'https://example.social/' + job.post.source_path } }; }, update_copy: async function () { return { success: true, copy: { id: 'x', url: 'https://example.social/updated' } }; } }";
        super::super::test_channel(root, "one", posts);
        super::super::test_channel(root, "two", posts);
        std::fs::create_dir_all(root.join("notes")).unwrap();
        let article = |uid: &str, name: &str, syndicate: &[&str]| -> ArticleInfo {
            std::fs::write(root.join(format!("notes/{name}.md")), "Hello.\n").unwrap();
            serde_json::from_value(serde_json::json!({
                "source_path": format!("notes/{name}.md"), "title": "", "content": "Hello.\n", "url_path": format!("notes/{name}/"), "date": null,
                "uid": uid, "syndicate": syndicate
            }))
            .unwrap()
        };
        let mut landed = ArticleMap::default();
        for a in [article("aaaa0001", "a", &["one", "two"]), article("aaaa0002", "b", &["two"])] {
            landed.articles.insert(a.url_path.clone(), a);
        }
        let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
        for channel in ["one", "two"] {
            let mut r = CopyRecord::due("aaaa0001", channel, "notes/a.md", "notes/a/");
            r.post = Some(Box::new(PublishedPost::new(&landed.articles["notes/a/"], &landed)));
            records.save(&r).unwrap();
        }
        let mut posted = CopyRecord::due("aaaa0002", "two", "notes/b.md", "notes/b/");
        posted.state = State::Posted;
        posted.update_due = true;
        posted.remote = Some(super::super::record::RemoteRef { id: "b-1".into(), url: "https://example.social/b-1".into() });
        posted.post = Some(Box::new(PublishedPost::new(&landed.articles["notes/b/"], &landed)));
        records.save(&posted).unwrap();

        let mut latest = ArticleMap::default();
        for a in [article("aaaa0001", "a", &["one"]), article("aaaa0002", "b", &[])] {
            latest.articles.insert(a.url_path.clone(), a);
        }
        let cache = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned());
        let manager = cache.get_or_create(&root.to_string_lossy()).unwrap();
        let emit = |_: &Sent| {};
        let opts = SendOptions { site_url: "https://publish.example.net", retry: &[], emit: &emit, only_source: None, only_channel: None, cap: PER_RUN, check_pages: false };
        let sent = send_due(root, &manager, &latest, &opts).await;

        let got: Vec<_> = sent.iter().map(|s| (s.source_path.as_str(), s.channel.as_str(), &s.outcome)).collect();
        assert_eq!(got, [("notes/a.md", "one", &Outcome::Posted { url: "https://example.social/notes/a.md".into() })]);
        assert_eq!(records.get("aaaa0001", "two").unwrap().state, State::Due);
        let kept = records.get("aaaa0002", "two").unwrap();
        assert!(kept.state == State::Posted && kept.update_due && kept.remote.unwrap().id == "b-1");
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
            let opts = SendOptions { site_url: "https://example.org", retry: &[], emit, only_source: None, only_channel: None, cap: PER_RUN, check_pages: false };
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

    /// A channel whose lookup answers `state` for every copy, and posts every
    /// new copy as `p-<uid>`.
    fn lookup_channel(root: &Path, state: &str) {
        let js = format!(
            "{{ publish_copy: async function (job) {{ return {{ success: true, copy: {{ id: 'p', url: 'https://social.example.net/p-' + job.post.source_path }} }}; }}, \
             find_copy: async function (job) {{ \
               return '{state}' === 'found' ? {{ success: true, copy: {{ id: 'f', url: 'https://social.example.net/f-' + job.post.source_path }} }} \
                 : '{state}' === 'waiting' ? {{ success: true, copy: {{ handoff: {{ text: '', open_url: 'https://social.example.net/draft', id: 'd-1' }} }} }} \
                 : '{state}' === 'absent' ? {{ success: true, copy: {{ absent: true }} }} \
                 : {{ success: true, copy: null }}; }} }}"
        );
        if root.join(".moss/plugins/social").exists() {
            std::fs::write(root.join(".moss/plugins/social/main.bundle.js"), format!("window.SOCIALPlugin = {js};")).unwrap();
        } else {
            super::super::test_channel(root, "social", &js);
        }
    }

    /// Posts `a0.md`, `a1.md`, … each naming the channel, with a record in
    /// `state` handed off (or last touched) `i` days ago.
    fn lookup_site(root: &Path, states: &[State]) -> (Records, ArticleMap) {
        let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
        let mut articles = ArticleMap::default();
        for (i, state) in states.iter().enumerate() {
            let (source, url, uid) = (format!("a{i}.md"), format!("a{i}/"), format!("aaaa000{i}"));
            std::fs::write(root.join(&source), "---\ntitle: A\n---\nBody.\n").unwrap();
            let mut r = CopyRecord::due(&uid, "social", &source, &url);
            r.state = *state;
            r.form = Some(super::super::channel::Form::Short);
            if *state == State::HandedOff {
                r.handoff = Some(super::super::record::Handoff { text: "Body".into(), open_url: Some("https://social.example.net/new".into()), id: None });
            }
            r.updated_at = (chrono::Utc::now() - chrono::Duration::days(i as i64)).to_rfc3339();
            records.save(&r).unwrap();
            let post: ArticleInfo = serde_json::from_value(serde_json::json!({
                "source_path": source, "title": "A", "content": "Body.\n", "url_path": url, "date": null, "uid": uid, "syndicate": ["social"]
            })).unwrap();
            articles.articles.insert(post.url_path.clone(), post);
        }
        (records, articles)
    }

    async fn run_lookups(root: &Path, articles: &ArticleMap, retry: &[String], only_source: Option<&str>, cap: usize) -> Vec<Sent> {
        let manager = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned()).get_or_create(&root.to_string_lossy()).unwrap();
        let emit = |_: &Sent| {};
        let opts = SendOptions { site_url: "https://example.org", retry, emit: &emit, only_source, only_channel: None, cap, check_pages: false };
        send_due(root, &manager, articles, &opts).await.into_iter().filter(|s| s.outcome != Outcome::Sending).collect()
    }

    /// A handed-off copy is looked up whether or not the network keeps a
    /// draft id; only a published copy changes it, and silence, an empty
    /// answer or absence leaves it waiting for the author.
    #[tokio::test]
    async fn a_handed_off_copy_is_adopted_once_found_and_otherwise_waits_quietly() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (records, articles) = lookup_site(root, &[State::HandedOff]);
        for state in ["unknown", "absent", "waiting"] {
            lookup_channel(root, state);
            let before = records.get("aaaa0000", "social").unwrap();
            let out = run_lookups(root, &articles, &[], None, PER_RUN).await;
            let after = records.get("aaaa0000", "social").unwrap();
            assert_eq!(after.state, State::HandedOff, "{state}");
            if state == "waiting" {
                assert!(matches!(&out[..], [Sent { outcome: Outcome::HandedOff { .. }, .. }]), "a new draft is said once: {out:?}");
            } else {
                assert!(out.is_empty(), "{state}: {out:?}");
                assert_eq!(after, before, "{state}");
            }
        }
        assert!(run_lookups(root, &articles, &[], None, PER_RUN).await.is_empty(), "the same draft is not said again");
        let asked = run_lookups(root, &articles, &[], Some("a0.md"), PER_RUN).await;
        assert!(matches!(&asked[..], [Sent { outcome: Outcome::HandedOff { .. }, .. }]), "unless its post is asked about: {asked:?}");
        lookup_channel(root, "found");
        let out = run_lookups(root, &articles, &[], None, PER_RUN).await;
        assert_eq!(out[0].outcome, Outcome::Posted { url: "https://social.example.net/f-a0.md".into() });
        assert!(std::fs::read_to_string(root.join("a0.md")).unwrap().contains("f-a0.md"));
    }

    /// Only an explicit absence lets an interrupted send go again: an empty
    /// answer, which an earlier plugin gave for "not found", leaves it held.
    #[tokio::test]
    async fn only_an_explicit_absence_lets_an_uncertain_copy_be_sent_again() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (records, articles) = lookup_site(root, &[State::Unconfirmed]);
        lookup_channel(root, "unknown");
        let out = run_lookups(root, &articles, &[], None, PER_RUN).await;
        assert!(matches!(&out[..], [Sent { outcome: Outcome::Unconfirmed { .. }, .. }]), "{out:?}");
        assert_eq!(records.get("aaaa0000", "social").unwrap().state, State::Unconfirmed);
        lookup_channel(root, "absent");
        let out = run_lookups(root, &articles, &[], None, PER_RUN).await;
        assert_eq!(out[0].outcome, Outcome::Posted { url: "https://social.example.net/p-a0.md".into() });
    }

    /// One run asks about at most `cap` handed-off copies per channel, the
    /// newest, and none handed off longer ago than the lookup window.
    #[tokio::test]
    async fn a_run_looks_up_a_bounded_number_of_handoffs_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (records, articles) = lookup_site(root, &[State::HandedOff; 6]);
        // Ages run against the order the run walks the records in.
        for i in 0..6 {
            let mut r = records.get(&format!("aaaa000{i}"), "social").unwrap();
            let days = if i == 0 { HANDOFF_LOOKUP_DAYS + 1 } else { 6 - i as i64 };
            r.updated_at = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();
            records.save(&r).unwrap();
        }
        lookup_channel(root, "found");
        let adopted = |out: Vec<Sent>| out.into_iter().map(|s| s.source_path).collect::<Vec<_>>();
        assert_eq!(adopted(run_lookups(root, &articles, &[], None, 3).await), ["a3.md", "a4.md", "a5.md"], "the three newest");
        assert_eq!(adopted(run_lookups(root, &articles, &[], None, 3).await), ["a1.md", "a2.md"], "the rest inside the window");
        assert_eq!(records.get("aaaa0000", "social").unwrap().state, State::HandedOff, "never one handed off before the window");
    }

    /// Retrying a channel sends its uncertain copies again and leaves its
    /// handed-off ones as they are; a retry for one post hands that post's
    /// copy off again.
    #[tokio::test]
    async fn a_retry_hands_off_again_only_the_post_it_names() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (records, articles) = lookup_site(root, &[State::Unconfirmed, State::HandedOff, State::HandedOff]);
        lookup_channel(root, "unknown");
        let handed = [records.get("aaaa0001", "social").unwrap(), records.get("aaaa0002", "social").unwrap()];
        let retry = ["social".to_string()];
        let out = run_lookups(root, &articles, &retry, None, PER_RUN).await;
        assert_eq!(out.iter().map(|s| (s.source_path.as_str(), &s.outcome)).collect::<Vec<_>>(), [("a0.md", &Outcome::Posted { url: "https://social.example.net/p-a0.md".into() })]);
        assert_eq!([records.get("aaaa0001", "social").unwrap(), records.get("aaaa0002", "social").unwrap()], handed);

        let out = run_lookups(root, &articles, &retry, Some("a2.md"), PER_RUN).await;
        assert_eq!(out.iter().map(|s| (s.source_path.as_str(), &s.outcome)).collect::<Vec<_>>(), [("a2.md", &Outcome::Posted { url: "https://social.example.net/p-a2.md".into() })]);
        assert_eq!(records.get("aaaa0001", "social").unwrap(), handed[0]);
    }

    #[test]
    fn a_copy_is_in_the_posts_own_language_else_its_editions() {
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
        let lang = |p: ArticleInfo| PublishedPost::new(&p, &articles).lang;
        assert_eq!(lang(post("notes/a", Some("en"))).as_deref(), Some("en"));
        assert_eq!(lang(post("de/a", None)).as_deref(), Some("de"));
        assert_eq!(lang(post("notes/a", None)), None, "a send falls back to the site's");
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
