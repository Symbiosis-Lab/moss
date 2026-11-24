//! The record of every copy: one small JSON file per (post, channel) under
//! `.moss/deploy/copies/`.
//!
//! One file per copy: a cloud client's conflict on one shared file drops
//! entries, and a dropped *posted* entry is a repost. They live in the folder
//! so another computer publishing it sees what was posted.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::job::Op;
use crate::build::scan::article_map::{ArticleInfo, ArticleMap};
use crate::moss_paths::MossPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Planned; the next send posts it.
    Due,
    /// Written before the network is called, so an interrupted send leaves a
    /// mark instead of an empty slot.
    Sending,
    Posted,
    /// The network refused before anything was posted; tried again next run.
    Refused,
    /// The latest published page cannot be copied; a fitting edit can resume it.
    Unavailable,
    /// The author was given the copy to post themselves; a lookup adopts it
    /// once it is published.
    HandedOff,
    /// A send stopped mid-call on a network that cannot retry safely. Not
    /// retried until the author says so.
    Unconfirmed,
}

/// A copy's id and address on its network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RemoteRef {
    pub id: String,
    pub url: String,
}

/// A copy the author posts by hand: the text to paste and where, or a draft
/// the network holds for the author to finish there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Handoff {
    /// Empty for a draft on the network, which already holds the copy.
    pub text: String,
    #[serde(default)]
    pub open_url: Option<String>,
    /// The draft's id on the network. Each later send asks the channel's
    /// `find_copy` whether the author has published it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// A copy hook's answer, carried in `HookResult.copy`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CopyOutcome {
    /// The copy (a thread's first part).
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// A thread's later parts, in order.
    #[serde(default)]
    pub parts: Vec<RemoteRef>,
    #[serde(default)]
    pub handoff: Option<Handoff>,
    /// `find_copy` or `update_copy`: the network says there is no such copy.
    /// The one answer that lets moss post it again; any other answer without
    /// the copy leaves whether it exists unknown.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub absent: bool,
    /// `copy_account` only: the account the channel is signed in to, as an
    /// id that stays the same while it is that account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// `copy_account` only: the account as a person knows it (`@ada`), for
    /// display; never compared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `copy_account` only: a definite answer that the channel has no usable
    /// session or credential (it was read and holds none, or the service
    /// refused authentication). A refusal with cause `Auth` means the same.
    /// A read that failed or is not available here is a failed call with any
    /// other cause, which leaves the sign-in as it was.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub signed_out: bool,
    /// Why the network refused, with `success: false`.
    #[serde(default)]
    pub refused: Option<CopyRefusal>,
    /// What the site should carry so the network can tie the copy back to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<Verification>,
}

/// A channel's ask of the site: a link in the head of the copied page and a
/// file under `.well-known/`. Both pass the identity allowlists at build time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Verification {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_link: Option<VerifyLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_file: Option<VerifyFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct VerifyLink {
    pub rel: String,
    pub href: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct VerifyFile {
    pub path: String,
    pub body: String,
}

/// What kind of refusal it was, so a person knows what to do about it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    /// The credential is missing, wrong or expired.
    Auth,
    /// The network would not take this text or picture.
    Content,
    /// Too many posts for now.
    Rate,
    Network,
    /// An edit the network will not take for this copy (past its edit window
    /// or revision limit), said only on an explicit refusal.
    Locked,
    /// Also any cause this moss does not know yet, so a newer plugin's answer still parses.
    #[default]
    #[serde(other)]
    Unknown,
}

/// Named for copies: a host exports every type into one TypeScript module,
/// where the plugin registry's `Refusal` already lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CopyRefusal {
    #[serde(default)]
    pub cause: Cause,
    /// The network's own words.
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopyRecord {
    /// The post's note id; with `channel`, the record's identity.
    pub uid: String,
    pub channel: String,
    pub state: State,
    /// The submitted form survives edits and reconciliation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<super::channel::Form>,
    /// Attempted thread length, retained only until remote ids establish it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_count: Option<usize>,
    /// Where the post was when this record last changed. Informational: the
    /// uid is what follows a renamed post.
    pub source_path: String,
    pub url_path: String,
    /// The site's canonical address when the copy was planned, which the copy
    /// links to; absent, the address the publish reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_url: Option<String>,
    /// The post as the publish that made this copy due (or due for an
    /// update) published it, which is what the copy sends: a later build, a
    /// landing on another target or a send from another computer cannot change
    /// its text. Kept while the copy has work left. A record from before these
    /// were kept has none and sends the latest build's post.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post: Option<Box<PublishedPost>>,
    /// The copy on the network (a thread's first part).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteRef>,
    /// A thread's later parts, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<RemoteRef>,
    /// The hash of the shaped copy last sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// The post's source changed in a landed publish after this copy was sent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub update_due: bool,
    /// Why the posted copy stays as it is, from the network's own answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stays: Option<Stays>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff: Option<Handoff>,
    /// The account the copy was sent from, for a channel that says which
    /// (`copy_account`). Signed in to another, the channel leaves it alone.
    /// A record from before accounts were kept takes the channel's account
    /// the next time it is worked on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// The run has said the copy waits for the account it was sent from.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held_for_account: bool,
    /// What the channel asked the site to carry; read on every build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<Verification>,
    pub updated_at: String,
}

/// Why a posted copy is not rewritten when its page changes; only an
/// explicit press posts it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stays {
    /// The network refused the edit (`Cause::Locked`).
    Locked,
    /// The network says the copy is gone.
    Deleted,
}

/// Whether a copy follows its page: see [`CopyRecord::edit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Current,
    /// The page changed and the copy can be rewritten.
    Due,
    /// The copy stays as posted. A channel without `update_copy` reads as
    /// `Stays::Locked`.
    Stays(Stays),
}

impl CopyRecord {
    /// The one reader of a copy's standing. `can_update` is whether the
    /// channel exports `update_copy` now; it is read each run and never
    /// stored, so a channel that gains it updates copies it could not edit.
    pub fn edit(&self, can_update: bool) -> Edit {
        match (self.state, self.stays) {
            (State::Posted, Some(why)) => Edit::Stays(why),
            (State::Posted, None) if !can_update => Edit::Stays(Stays::Locked),
            (State::Posted, None) if self.update_due => Edit::Due,
            _ => Edit::Current,
        }
    }

    /// What a send does with this copy now, if anything.
    pub fn next_op(&self, idempotent: bool, can_update: bool) -> Option<Op> {
        match self.state {
            State::Unconfirmed => Some(Op::Find),
            // An interrupted send goes again where the key makes that safe,
            // and is looked up first everywhere else.
            State::Sending if !idempotent => Some(Op::Find),
            State::Due | State::Refused | State::Sending => Some(Op::Publish),
            State::Posted => (self.edit(can_update) == Edit::Due).then_some(Op::Update),
            State::HandedOff => Some(Op::Find),
            State::Unavailable => None,
        }
    }

    /// Forget the copy on the network so the next send posts a new one.
    pub(super) fn start_over(&mut self) {
        self.state = State::Due;
        self.remote = None;
        self.handoff = None;
        self.update_due = false;
        self.stays = None;
        self.reason = None;
        self.discard_attempt();
    }

    /// No network copy or uncertain attempt exists, so a newer publish may replace this work.
    pub(super) fn definite_first(&self) -> bool {
        matches!(self.state, State::Due | State::Refused | State::Unavailable)
            && self.remote.is_none() && self.handoff.is_none() && self.form.is_none()
            && self.part_count.is_none() && self.hash.is_none() && self.parts.is_empty()
    }

    /// Cancel only definite work. Remote identity and uncertain side effects survive.
    pub(super) fn make_unavailable(&mut self, reason: &str) -> bool {
        if self.state == State::Unavailable && self.reason.as_deref() == Some(reason) {
            return false;
        }
        if self.definite_first() {
            self.state = State::Unavailable;
        } else if self.state == State::Posted && self.update_due {
            self.update_due = false;
        } else { return false; }
        self.post = None;
        self.reason = Some(reason.to_string());
        self.touch();
        true
    }

    /// A definitive refusal or absent lookup leaves no submitted copy to
    /// preserve, unless the network holds one by id (a copy or a draft).
    pub(super) fn discard_attempt(&mut self) {
        if self.remote.is_none() && self.draft().is_none() {
            self.form = None;
            self.part_count = None;
            self.hash = None;
            self.parts.clear();
            self.handoff = None;
        }
    }

    /// The draft a handed-off copy waits in on its network, which a lookup names.
    pub fn draft(&self) -> Option<RemoteRef> {
        let h = self.handoff.as_ref()?;
        Some(RemoteRef { id: h.id.clone()?, url: h.open_url.clone().unwrap_or_default() })
    }

    /// The site address the copy links to: the one planned, else `reported`.
    pub fn site<'a>(&'a self, reported: &'a str) -> &'a str {
        self.site_url.as_deref().unwrap_or(reported)
    }

    pub fn due(uid: &str, channel: &str, source_path: &str, url_path: &str) -> Self {
        Self {
            uid: uid.to_string(),
            channel: channel.to_string(),
            state: State::Due,
            form: None,
            part_count: None,
            source_path: source_path.to_string(),
            url_path: url_path.to_string(),
            site_url: None,
            post: None,
            remote: None,
            parts: Vec::new(),
            hash: None,
            update_due: false,
            stays: None,
            reason: None,
            handoff: None,
            account: None,
            held_for_account: false,
            verify: None,
            updated_at: now(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = now();
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// A post as one publish published it, with the language it was published in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishedPost {
    pub article: ArticleInfo,
    /// Its rendered language, with authored/edition fallbacks for older maps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

impl PublishedPost {
    /// `article` as `map`, the build it is in, publishes it.
    pub fn new(article: &ArticleInfo, map: &ArticleMap) -> Self {
        Self { article: article.clone(), lang: effective_language(article, Some(map), None).map(|l| l.into_owned()) }
    }
}

/// Authored language, rendered language, edition, then the site's fallback.
pub(super) fn effective_language<'a>(article: &'a ArticleInfo, map: Option<&'a ArticleMap>, fallback: Option<&'a str>) -> Option<std::borrow::Cow<'a, str>> {
    article.frontmatter.get("lang").and_then(|l| l.as_str()).map(std::borrow::Cow::Borrowed)
        .or_else(|| map.and_then(|m| m.page_languages.get(&article.url_path)).map(|l| std::borrow::Cow::Borrowed(l.as_str())))
        .or_else(|| map.map(|m| m.editions.of(&article.url_path)).filter(|l| !l.is_empty()).map(std::borrow::Cow::Owned))
        .or_else(|| fallback.map(std::borrow::Cow::Borrowed))
}

/// Held while one run sends a site's copies; see [`Records::turn`].
pub struct SendTurn(std::fs::File);

/// Unlocked explicitly rather than by closing the file: the kernel keeps the
/// lock while any copy of the descriptor is open, and a child this process
/// forks meanwhile holds one until it execs.
impl Drop for SendTurn {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// The records of one site.
pub struct Records {
    dir: PathBuf,
}

impl Records {
    pub fn in_vault(mp: &MossPaths) -> Self {
        Self { dir: mp.deploy_dir().join("copies") }
    }

    #[cfg(test)]
    pub(crate) fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn path(&self, uid: &str, channel: &str) -> Result<PathBuf, String> {
        if !plain_id(uid) || !plain_id(channel) {
            return Err(format!("a copy record needs a plain note id and channel id, not {uid:?} / {channel:?}"));
        }
        Ok(self.dir.join(format!("{uid}.{channel}.json")))
    }

    pub fn get(&self, uid: &str, channel: &str) -> Option<CopyRecord> {
        let path = self.path(uid, channel).ok()?;
        read(&path)
    }

    /// [`Self::get`] without waiting on a download: a record still in the
    /// cloud or unreadable is an error; only an absent one is `Ok(None)`.
    pub fn get_no_wait(&self, uid: &str, channel: &str) -> Result<Option<CopyRecord>, String> {
        crate::build::cloud_readiness::read_json_no_wait(&self.path(uid, channel)?)
    }

    /// Every readable record. A file that does not parse is skipped with a
    /// warning rather than treated as absent-and-due, which would repost.
    pub fn all(&self) -> Vec<CopyRecord> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut out: Vec<CopyRecord> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| read(&p))
            .collect();
        out.sort_by(|a, b| (a.channel.as_str(), a.url_path.as_str()).cmp(&(b.channel.as_str(), b.url_path.as_str())));
        out
    }

    /// Whether a record file exists, readable or not. The planner asks this
    /// rather than [`Self::get`], so an unreadable record blocks a repost.
    pub fn exists(&self, uid: &str, channel: &str) -> bool {
        self.path(uid, channel).is_ok_and(|p| p.exists())
    }

    /// This run's turn to send the site's copies, or `None` while another
    /// run in the folder holds it: two runs working from the same due records
    /// would both post. The turn is an OS lock, so it ends with the process
    /// that took it, however that process ends.
    pub fn turn(&self) -> Option<SendTurn> {
        let path = self.dir.with_extension("lock");
        match lock(&path) {
            Ok(turn) => Some(turn),
            Err(std::fs::TryLockError::WouldBlock) => {
                log::info!(target: "syndication", "another run is sending this site's copies; the rest stay due for the next run");
                None
            }
            Err(std::fs::TryLockError::Error(e)) => {
                log::warn!(target: "syndication", "copies not sent: {} could not be locked ({e})", path.display());
                None
            }
        }
    }

    /// This run's turn on one channel's copies, waited for while another send
    /// holds it, so two sends never post the same record: whoever holds it
    /// re-reads the channel's records under it. Lock order: a channel turn is
    /// taken before the action panel's turn, never while holding it.
    pub async fn channel_turn(&self, channel: &str) -> Result<SendTurn, String> {
        if !plain_id(channel) {
            return Err(format!("a channel needs a plain id, not {channel:?}"));
        }
        let path = self.dir.with_extension(format!("{channel}.lock"));
        loop {
            match lock(&path) {
                Ok(turn) => return Ok(turn),
                Err(std::fs::TryLockError::WouldBlock) => tokio::time::sleep(CHANNEL_TURN_POLL).await,
                Err(std::fs::TryLockError::Error(e)) => return Err(format!("{} could not be locked ({e})", path.display())),
            }
        }
    }

    /// Settled copies need neither a publication snapshot nor an attempted count.
    pub fn save(&self, record: &CopyRecord) -> Result<(), String> {
        let mut saved = record.clone();
        if saved.next_op(false, true).is_none() { saved.post = None; }
        if saved.remote.is_some() { saved.part_count = None; }
        let record = &saved;
        let path = self.path(&record.uid, &record.channel)?;
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("create {}: {e}", self.dir.display()))?;
        let json = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
        crate::infra::atomic_write::write_atomic(&path, &json)
    }
}

fn plain_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

const CHANNEL_TURN_POLL: std::time::Duration = std::time::Duration::from_millis(200);

/// An OS lock on `path`, which ends with the process however it ends.
fn lock(path: &Path) -> Result<SendTurn, std::fs::TryLockError> {
    path.parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        // allow:raw_write the lock file under .moss/deploy, opened without truncating; its bytes are never read
        .and_then(|()| std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path))
        .map_err(std::fs::TryLockError::Error)
        .and_then(|file| file.try_lock().map(|()| SendTurn(file)))
}

fn read(path: &Path) -> Option<CopyRecord> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(r) => Some(r),
        Err(e) => {
            log::warn!(target: "syndication", "copy record {} is unreadable ({e}); its post is not sent again until it is fixed or removed", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cause_this_moss_does_not_know_reads_as_unknown() {
        let cause: Cause = serde_json::from_str("\"some_future_cause\"").unwrap();
        assert_eq!(cause, Cause::Unknown);
        assert_eq!(serde_json::from_str::<Cause>("\"locked\"").unwrap(), Cause::Locked);
    }

    /// A child forked while the turn is held (an encoder, a plugin host)
    /// carries a copy of its descriptor until it execs, and the kernel keeps
    /// the lock while any copy is open.
    #[test]
    fn a_released_turn_is_free_while_a_copy_of_its_descriptor_is_still_open() {
        let dir = tempfile::tempdir().unwrap();
        let records = Records::at(dir.path().join("copies"));
        let turn = records.turn().expect("the turn is free");
        let inherited = turn.0.try_clone().unwrap();
        drop(turn);
        assert!(records.turn().is_some(), "the run that let go of the turn still holds it through the copy");
        drop(inherited);
    }

    #[test]
    fn each_state_says_what_a_send_does_next() {
        let at = |state, update_due, idempotent| {
            let mut r = CopyRecord::due("ab12cd34", "c", "a.md", "a/");
            r.state = state;
            r.update_due = update_due;
            r.next_op(idempotent, true)
        };
        assert_eq!(at(State::Sending, false, true), Some(Op::Publish), "the same key makes a second copy impossible");
        assert_eq!(at(State::Sending, false, false), Some(Op::Find));
        assert_eq!(at(State::Unconfirmed, false, true), Some(Op::Find));
        assert_eq!(at(State::Refused, false, false), Some(Op::Publish));
        assert_eq!(at(State::Posted, true, false), Some(Op::Update));
        assert_eq!(at(State::Posted, false, false), None);
        assert_eq!(at(State::HandedOff, false, false), Some(Op::Find), "a handed-off copy is looked up, draft id or not");
    }

    /// Planning the same unavailable copy again leaves its record, and its
    /// file, as they were.
    #[test]
    fn an_unavailable_copy_is_not_rewritten_for_the_same_reason() {
        let mut r = CopyRecord::due("ab12cd34", "c", "a.md", "a/");
        assert!(r.make_unavailable("too long"));
        r.updated_at = "then".into();
        assert!(!r.make_unavailable("too long"));
        assert_eq!(r.updated_at, "then");
        assert!(r.make_unavailable("no pictures"), "a new reason is saved");
    }

    #[test]
    fn a_record_round_trips_through_its_own_file() {
        let dir = tempfile::tempdir().unwrap();
        let records = Records::at(dir.path().join("copies"));
        let mut r = CopyRecord::due("ab12cd34", "bluesky", "notes/a.md", "notes/a/");
        records.save(&r).unwrap();
        r.state = State::Posted;
        r.remote = Some(RemoteRef { id: "at://x/1".into(), url: "https://bsky.app/x".into() });
        records.save(&r).unwrap();
        assert_eq!(records.get("ab12cd34", "bluesky"), Some(r.clone()));
        assert!(dir.path().join("copies/ab12cd34.bluesky.json").exists());
        assert_eq!(records.all(), vec![r]);
    }

    #[test]
    fn an_unreadable_record_still_counts_as_existing() {
        let dir = tempfile::tempdir().unwrap();
        let records = Records::at(dir.path().to_path_buf());
        std::fs::write(dir.path().join("ab12cd34.mastodon.json"), "{ not json").unwrap();
        assert!(records.get("ab12cd34", "mastodon").is_none());
        assert!(records.exists("ab12cd34", "mastodon"));
    }

    #[test]
    fn ids_that_would_escape_the_folder_are_refused() {
        let records = Records::at(PathBuf::from("/nowhere"));
        assert!(records.save(&CopyRecord::due("../x", "bluesky", "", "")).is_err());
        assert!(records.save(&CopyRecord::due("ab12", "blue/sky", "", "")).is_err());
    }

    fn posted(update_due: bool, stays: Option<Stays>) -> CopyRecord {
        let mut r = CopyRecord::due("ab12cd34", "c", "a.md", "a/");
        r.state = State::Posted;
        r.remote = Some(RemoteRef { id: "1".into(), url: "https://x.example/1".into() });
        r.update_due = update_due;
        r.stays = stays;
        r
    }

    #[test]
    fn a_posted_copy_is_edited_only_when_due_capable_and_not_locked() {
        assert_eq!(posted(true, None).next_op(false, true), Some(Op::Update));
        assert_eq!(posted(true, None).edit(true), Edit::Due);
        assert_eq!(posted(true, None).next_op(false, false), None, "no update_copy export: nothing to send");
        assert_eq!(posted(true, None).edit(false), Edit::Stays(Stays::Locked));
        assert_eq!(posted(true, Some(Stays::Locked)).next_op(false, true), None, "refused before: left as posted");
        assert_eq!(posted(true, Some(Stays::Locked)).edit(true), Edit::Stays(Stays::Locked));
        assert_eq!(posted(false, None).edit(true), Edit::Current);
    }

    #[test]
    fn a_channel_that_gains_update_copy_updates_a_copy_it_could_not_edit() {
        let r = posted(true, None);
        assert_eq!(r.next_op(false, false), None);
        assert_eq!(r.next_op(false, true), Some(Op::Update), "the record kept the change; only the capability moved");
    }

    #[test]
    fn an_attempt_holding_an_id_is_never_discarded() {
        let mut r = CopyRecord::due("ab12cd34", "c", "a.md", "a/");
        r.state = State::HandedOff;
        r.handoff = Some(Handoff { text: "t".into(), open_url: None, id: Some("d-1".into()) });
        r.discard_attempt();
        assert!(r.draft().is_some());
        r.handoff.as_mut().unwrap().id = None;
        r.discard_attempt();
        assert!(r.handoff.is_none());
    }
}
