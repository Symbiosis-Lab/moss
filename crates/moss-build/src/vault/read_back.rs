//! A hosted site's records come home to its folder.
//!
//! seta and commerce hold records nothing else has: subscribers' consent and
//! form answers, comments with their commenters' emails, bookings with their
//! Stripe ids, analytics events. Each part is a JSON Lines stream under
//! `.moss/data/`, pulled from a signed owner route that pages by an integer
//! cursor (`?since=&limit=` → `{records, cursor, more}`). Streams are
//! append-only and strictly increasing by their key, so the key of the last
//! complete line is the cursor: there is no sidecar to fall out of step with
//! the file, and a record already held is never appended again.
//!
//! The `since` a pull sends is the server's proof of what the folder holds,
//! and its retention refuses to delete records past it.
//!
//! ```text
//! .moss/data/
//!   events.jsonl                 analytics, by `id`
//!   email/events.jsonl           newsletter and forms: the subscriber stream, by `seq`
//!   forms/<scope>.csv            a view of the form entries in that stream, rewritten when one arrives
//!   comments/comment.jsonl       comments with their commenter's email, by `id`
//!   comments/approved.jsonl      what became of a comment held as pending
//!   booking/bookings.jsonl       every change to a booking, by `changeSeq`; the newest line per `id` is the booking
//! ```
//!
//! These files hold the site's contacts' details. They travel with the folder,
//! and are never published (`is_moss_internal`, pinned by
//! `moss_own_files_are_told_from_the_ones_an_author_edits`) or pushed
//! (`the_generated_gitignore_keeps_the_redirect_history_and_ignores_data`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::seta::client::{MossSetaClient, SetaError};
use crate::seta::sites::RecordsPage;

pub mod cli;

/// One part of a site's records, named as the verb, the routes and the folders name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    Newsletter,
    Forms,
    Comments,
    Bookings,
    Analytics,
}

impl Part {
    pub const ALL: [Part; 5] = [Part::Newsletter, Part::Forms, Part::Comments, Part::Bookings, Part::Analytics];

    pub fn name(self) -> &'static str {
        match self {
            Part::Newsletter => "newsletter",
            Part::Forms => "forms",
            Part::Comments => "comments",
            Part::Bookings => "bookings",
            Part::Analytics => "analytics",
        }
    }

    pub fn parse(name: &str) -> Option<Part> {
        Part::ALL.into_iter().find(|p| p.name() == name)
    }
}

/// A stream file and the field each of its records is keyed by.
struct Stream {
    file: &'static str,
    key: &'static str,
}

const ANALYTICS: Stream = Stream { file: ".moss/data/events.jsonl", key: "id" };
const SUBSCRIBER: Stream = Stream { file: ".moss/data/email/events.jsonl", key: "seq" };
// Not under `data/social/`: that folder is watched and read by comment plugins,
// and these lines carry commenters' emails.
const COMMENTS: Stream = Stream { file: ".moss/data/comments/comment.jsonl", key: "id" };
const BOOKINGS: Stream = Stream { file: ".moss/data/booking/bookings.jsonl", key: "changeSeq" };
const COMMENT_ANSWERS: &str = ".moss/data/comments/approved.jsonl";
const FORMS_DIR: &str = ".moss/data/forms";
/// Where analytics kept its cursor before the stream's tail became it.
const OLD_ANALYTICS_CURSOR: &str = ".moss/data/events-cursor.json";

const PAGE_LIMIT: u32 = 10_000;
/// seta answers at most this many pending ids a request.
const PENDING_PER_ASK: usize = 200;

/// Where a site's records are read from: seta always, commerce where it runs.
pub struct Sources<'a> {
    pub seta: &'a MossSetaClient,
    pub commerce: Option<MossSetaClient>,
    pub site_id: &'a str,
}

impl<'a> Sources<'a> {
    /// seta, and commerce where `env` runs it, both signed by `seta`'s identity.
    pub fn new(seta: &'a MossSetaClient, env: &crate::config::environment::HostingEnvironment, site_id: &'a str) -> Self {
        Sources { seta, commerce: env.service_url("commerce").map(|url| seta.at(url)), site_id }
    }
}

/// What one part's pull did: the records it added, or why it could not.
pub type Outcome = (Part, Result<usize, SetaError>);

/// What a pull of the subscriber stream added: every record, and the form entries among them.
#[derive(Clone, Copy)]
struct SubscriberPull {
    added: usize,
    entries: usize,
}

/// Pull each named part into `root`. One part's failure never stops another's.
/// Newsletter and forms share one stream, read once.
pub async fn pull(sources: &Sources<'_>, root: &Path, parts: &[Part]) -> Vec<Outcome> {
    let lock_root = root.to_path_buf();
    let _held = match blocking(move || {
        crate::infra::folder_lock::acquire_named(&lock_root, "read-back")
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    })
    .await
    {
        Ok(held) => held,
        Err(e) => return parts.iter().map(|&p| (p, Err(SetaError::Io(e.to_string())))).collect(),
    };

    let mut out = Vec::new();
    let mut subscriber: Option<Result<SubscriberPull, String>> = None;
    for &part in parts {
        let pulled = match part {
            Part::Newsletter | Part::Forms => {
                let read = match subscriber.clone() {
                    Some(read) => read,
                    None => {
                        let read = pull_subscriber(sources, root).await.map_err(|e| e.to_string());
                        subscriber = Some(read.clone());
                        read
                    }
                };
                read.map(|r| if part == Part::Forms { r.entries } else { r.added }).map_err(SetaError::Io)
            }
            Part::Comments => pull_comments(sources, root).await,
            Part::Bookings => match &sources.commerce {
                Some(commerce) => {
                    let pulled = pull_stream(root, &BOOKINGS, 1, |since, _| commerce.pull_bookings(sources.site_id, since, PAGE_LIMIT), |_| {}).await;
                    match pulled {
                        // Commerce knows only the sites that take bookings.
                        Err(SetaError::Api { status: 404, message }) if message.contains("unknown_site") => Ok(0),
                        other => other.map(|(n, _)| n),
                    }
                }
                None => Ok(0),
            },
            Part::Analytics => {
                let old = root.join(OLD_ANALYTICS_CURSOR);
                let _ = blocking(move || fs::remove_file(&old)).await;
                pull_stream(root, &ANALYTICS, 1, |since, _| sources.seta.pull_events(sources.site_id, since, PAGE_LIMIT), |_| {})
                    .await
                    .map(|(n, _)| n)
            }
        };
        out.push((part, pulled));
    }
    out
}

/// The subscriber stream, then the forms view when form entries arrived,
/// whichever part asked for the stream.
async fn pull_subscriber(sources: &Sources<'_>, root: &Path) -> Result<SubscriberPull, SetaError> {
    let mut entries = 0;
    let fetch = |since: u64, _: usize| sources.seta.pull_subscriber_events(sources.site_id, since, PAGE_LIMIT);
    let (added, _) = pull_stream(root, &SUBSCRIBER, 1, fetch, |fresh| entries += fresh.iter().filter(|e| is_form_entry(e)).count()).await?;
    if entries > 0 {
        let r = root.to_path_buf();
        blocking(move || write_forms_view(&r)).await?;
    }
    Ok(SubscriberPull { added, entries })
}

fn is_form_entry(event: &Value) -> bool {
    event["fact"]["type"] == "response_submitted"
}

/// Comments, then what became of the ones held as pending. Answers are asked
/// by id rather than re-sent under the cursor, so the stream stays strictly
/// increasing; an id is asked about until it has an answer. seta reads every
/// comment for each request, so each page carries the next batch of ids and
/// a request is made only for a page or a batch still owed.
async fn pull_comments(sources: &Sources<'_>, root: &Path) -> Result<usize, SetaError> {
    let r = root.to_path_buf();
    let pending = blocking(move || unanswered_pending(&r)).await?;
    let asks: Vec<&[u64]> = pending.chunks(PENDING_PER_ASK).collect();
    let fetch = |since: u64, n: usize| sources.seta.pull_comment_records(sources.site_id, since, PAGE_LIMIT, asks.get(n).copied().unwrap_or(&[]));
    let (added, answers) = pull_stream(root, &COMMENTS, asks.len(), fetch, |_| {}).await?;
    let r = root.to_path_buf();
    blocking(move || append_answers(&r, answers)).await?;
    Ok(added)
}

/// Page one stream from its tail to the server's end, appending each page,
/// and make at least `pages` requests. `fetch` is given the cursor and the
/// request's index; `seen` each page's new records. Returns the records added
/// and every page's `answers`.
async fn pull_stream<F, Fut>(
    root: &Path,
    stream: &'static Stream,
    pages: usize,
    mut fetch: F,
    mut seen: impl FnMut(&[Value]),
) -> Result<(usize, Vec<Value>), SetaError>
where
    F: FnMut(u64, usize) -> Fut,
    Fut: Future<Output = Result<RecordsPage, SetaError>>,
{
    let path = root.join(stream.file);
    let p = path.clone();
    let mut since = blocking(move || tail_key(&p, stream.key)).await?;
    let mut added = 0;
    let mut answers = Vec::new();
    for n in 0.. {
        crate::infra::liveness::bump();
        let page = fetch(since, n).await?;
        answers.extend(page.answers);
        let fresh: Vec<Value> = page.records.into_iter().filter(|r| key_of(r, stream.key).is_some_and(|k| k > since)).collect();
        let last = fresh.last().and_then(|r| key_of(r, stream.key));
        if let Some(last) = last {
            seen(&fresh);
            added += fresh.len();
            let p = path.clone();
            blocking(move || append_lines(&p, &fresh)).await?;
            crate::infra::liveness::bump();
            since = last;
        }
        // A page that adds nothing ends the stream even if it claims more.
        if n + 1 >= pages && (!page.more || last.is_none()) {
            break;
        }
    }
    Ok((added, answers))
}

fn key_of(record: &Value, key: &str) -> Option<u64> {
    record.get(key).and_then(Value::as_u64)
}

/// A stream opened for reading, `None` when it does not exist. An evicted
/// file is materialized rather than read as empty, which would re-fetch everything.
fn open_stream(path: &Path) -> std::io::Result<Option<File>> {
    use crate::build::{cloud_readiness, icloud};
    // allow:raw_read the stream was just materialized, and is read only from its tail
    let opened = cloud_readiness::materialize_input(path, cloud_readiness::INTERACTIVE_DEADLINE).and_then(|()| File::open(path));
    match opened {
        Ok(file) => Ok(Some(file)),
        Err(e) if icloud::is_definitely_absent(path, &e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The key of the stream's last complete line, 0 for a stream not yet begun.
/// A line cut short by a crash has no newline and is not complete. Only the
/// tail is read: a window from the end, doubled until it holds a keyed line.
fn tail_key(path: &Path, key: &str) -> std::io::Result<u64> {
    let Some(mut file) = open_stream(path)? else { return Ok(0) };
    let len = file.metadata()?.len();
    let mut window: u64 = 4096;
    loop {
        let start = len.saturating_sub(window);
        file.seek(SeekFrom::Start(start))?;
        let mut buf = Vec::new();
        (&mut file).take(len - start).read_to_end(&mut buf)?;
        let complete = buf.iter().rposition(|&b| b == b'\n').map_or(&buf[..0], |end| &buf[..end]);
        // Unless the window reaches the start, its first line may be cut by the window.
        let whole = match start {
            0 => complete,
            _ => complete.iter().position(|&b| b == b'\n').map_or(&complete[..0], |i| &complete[i + 1..]),
        };
        let found = whole.rsplit(|&b| b == b'\n').find_map(|line| serde_json::from_slice::<Value>(line).ok().and_then(|v| key_of(&v, key)));
        if found.is_some() || start == 0 {
            return Ok(found.unwrap_or(0));
        }
        window *= 2;
    }
}

/// Append records as lines, first closing a line a crash cut short so it
/// stays one malformed line that readers skip.
pub(super) fn append_lines(path: &Path, records: &[Value]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // allow:raw_write appends to the author's own record stream; O_APPEND truncates nothing
    let mut file = OpenOptions::new().create(true).read(true).append(true).open(path)?;
    let mut last = [b'\n'];
    if file.metadata()?.len() > 0 {
        file.seek(SeekFrom::End(-1))?;
        file.read_exact(&mut last)?;
    }
    let open_tail = last[0] != b'\n';
    let mut buf = Vec::new();
    if open_tail {
        buf.push(b'\n');
    }
    for record in records {
        serde_json::to_writer(&mut buf, record)?;
        buf.push(b'\n');
    }
    file.write_all(&buf)?;
    file.sync_data()
}

fn read_lines(path: &Path) -> std::io::Result<Vec<Value>> {
    let text = crate::build::cloud_readiness::read_input_if_present(path)?.unwrap_or_default();
    Ok(text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
}

fn answered(root: &Path) -> std::io::Result<BTreeSet<u64>> {
    Ok(read_lines(&root.join(COMMENT_ANSWERS))?.iter().filter_map(|a| key_of(a, "id")).collect())
}

/// Comments the folder holds as pending whose fate it has not yet heard.
fn unanswered_pending(root: &Path) -> std::io::Result<Vec<u64>> {
    let done = answered(root)?;
    Ok(read_lines(&root.join(COMMENTS.file))?
        .iter()
        .filter(|c| c.get("pending").and_then(Value::as_bool) == Some(true))
        .filter_map(|c| key_of(c, "id"))
        .filter(|id| !done.contains(id))
        .collect())
}

fn append_answers(root: &Path, answers: Vec<Value>) -> std::io::Result<()> {
    let mut seen = answered(root)?;
    let fresh: Vec<Value> = answers.into_iter().filter(|a| key_of(a, "id").is_some_and(|id| seen.insert(id))).collect();
    if fresh.is_empty() {
        return Ok(());
    }
    append_lines(&root.join(COMMENT_ANSWERS), &fresh)
}

/// Regenerate `forms/<scope>.csv` from the form entries in the subscriber
/// stream: one row per entry, one column per question asked in that scope.
/// A scope's file is written only when its bytes change, since each write is
/// a change the folder syncs. It runs only when form entries arrive, so a view
/// the person deletes comes back with the next entry.
fn write_forms_view(root: &Path) -> Result<(), SetaError> {
    let io = |e: std::io::Error| SetaError::Io(e.to_string());
    let mut scopes: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    let events = read_lines(&root.join(SUBSCRIBER.file)).map_err(io)?;
    for event in events.iter().filter(|e| is_form_entry(e)) {
        let scope = event["fact"]["scope"].as_str().filter(|s| !s.is_empty()).unwrap_or("-");
        scopes.entry(scope.to_string()).or_default().push(event);
    }
    for (scope, entries) in &scopes {
        let questions: BTreeSet<&str> = entries
            .iter()
            .filter_map(|e| e["fact"]["answers"].as_object())
            .flat_map(|a| a.keys().map(String::as_str))
            .collect();
        let mut csv = String::new();
        let header = ["recordedAt", "email"].into_iter().chain(questions.iter().copied());
        push_row(&mut csv, header.map(str::to_string));
        for entry in entries {
            let cell = |v: &Value| match v {
                Value::Null => String::new(),
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let answers = &entry["fact"]["answers"];
            let row = [cell(&entry["recordedAt"]), cell(&entry["email"])]
                .into_iter()
                .chain(questions.iter().map(|q| cell(&answers[*q])));
            push_row(&mut csv, row);
        }
        let path = forms_view_path(root, scope);
        // allow:raw_read moss's own regenerated view; a copy that cannot be read is rewritten
        if fs::read(&path).ok().as_deref() != Some(csv.as_bytes()) {
            crate::infra::atomic_write::write_atomic(&path, csv).map_err(SetaError::Io)?;
        }
    }
    Ok(())
}

/// A scope names a file only by its safe characters.
fn forms_view_path(root: &Path, scope: &str) -> PathBuf {
    let name: String = scope.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    root.join(FORMS_DIR).join(format!("{name}.csv"))
}

fn push_row(csv: &mut String, cells: impl Iterator<Item = String>) {
    let quoted: Vec<String> = cells
        .map(|c| if c.contains([',', '"', '\n', '\r']) { format!("\"{}\"", c.replace('"', "\"\"")) } else { c })
        .collect();
    csv.push_str(&quoted.join(","));
    csv.push('\n');
}

/// Folder I/O runs on the blocking pool, so a stalled cloud read cannot wedge
/// a runtime worker and stays cancellable with the future.
async fn blocking<T: Send + 'static, E: std::fmt::Display>(
    f: impl FnOnce() -> Result<T, E> + Send + 'static,
) -> Result<T, SetaError>
where
    E: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| SetaError::Io(e.to_string()))?
        .map_err(|e| SetaError::Io(e.to_string()))
}

#[cfg(test)]
#[path = "read_back_tests.rs"]
mod tests;
