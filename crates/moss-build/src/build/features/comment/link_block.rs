//! Spam's second half: the link hosts a comment carries, the hosts the owner
//! has blocked (signed `block`/`unblock` events in the same moderation log as
//! `hide`/`unhide`), and `_moss/comments.json`, which publishes them as
//! sha256 digests so the comment server can refuse a new comment linking to one
//! without the site publishing a readable list.
//!
//! A host is matched exactly: parsed as a WHATWG URL, lowercased (IDN as
//! punycode), one trailing dot and a leading `www.` removed, port and user
//! dropped. Never a registrable domain (one spammer on a free host would
//! block every site there) and never a substring.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use super::moderation::ModEvent;
use super::NormalizedComment;
use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;

pub const COMMENTS_PATH: &str = "_moss/comments.json";

/// The `source` a block event carries; its `target` is the host.
pub const LINK_SOURCE: &str = "link";

/// Every http(s) URL in a comment's text, whether written bare, as a markdown
/// link or in an `href`: each ends at whitespace, a quote, a bracket or `<`.
static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)https?://[^\s<>"'()\[\]{}]+"#).expect("static regex"));

/// The host a link names, normalized as the module doc says; `None` for
/// anything that is not an http(s) address with a host. A value without a
/// scheme (a website field reading `spam.example`) is read as `http://`; one
/// with another scheme (`mailto:`) is not a link host.
pub fn host_of(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let url = match url::Url::parse(raw) {
        Ok(url) if raw.contains("://") || !url.scheme().contains('.') => url,
        _ => url::Url::parse(&format!("http://{raw}")).ok()?,
    };
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let host = host.strip_suffix('.').unwrap_or(&host);
    let host = host.strip_prefix("www.").unwrap_or(host);
    (!host.is_empty()).then(|| host.to_string())
}

/// The hosts a comment links to: its website field and every URL in its text.
pub fn comment_hosts(c: &NormalizedComment) -> BTreeSet<String> {
    let website = c.author.url.as_deref().and_then(host_of);
    website.into_iter().chain(URL_RE.find_iter(&c.content).filter_map(|m| host_of(m.as_str()))).collect()
}

/// The latest verified event per `(source, target)` among `kinds`, ordered by
/// `(seq, ts)`. Events no key in `owners` signed are ignored.
pub(super) fn latest_by_target<'e>(
    events: &'e [ModEvent],
    owners: &[String],
    kinds: [&str; 2],
) -> HashMap<(&'e str, &'e str), &'e ModEvent> {
    let mut latest: HashMap<(&'e str, &'e str), &'e ModEvent> = HashMap::new();
    for e in events {
        if !kinds.contains(&e.kind.as_str()) || !owners.contains(&e.pubkey) || !e.verify(&e.pubkey) {
            continue;
        }
        let key = (e.source.as_str(), e.target.as_str());
        if latest.get(&key).is_none_or(|prev| (prev.seq, prev.ts) < (e.seq, e.ts)) {
            latest.insert(key, e);
        }
    }
    latest
}

/// Each blocked host with the `seq` of the block in force: a host whose latest
/// verified `block`/`unblock` event is a block.
pub fn active_blocks<'e>(events: &'e [ModEvent], owners: &[String]) -> BTreeMap<&'e str, u64> {
    latest_by_target(events, owners, ["block", "unblock"])
        .into_iter()
        .filter(|((source, _), e)| *source == LINK_SOURCE && e.kind == "block")
        .map(|((_, host), e)| (host, e.seq))
        .collect()
}

/// The hosts this site answers on, which Spam never blocks: its own address
/// (with and without `www.`, as [`host_of`] reads both the same) and its
/// address on moss hosting.
pub fn own_hosts(project_path: &str) -> BTreeSet<String> {
    let mut hosts = BTreeSet::new();
    let paths = crate::moss_paths::MossPaths::new(Path::new(project_path));
    let address = crate::build::site_config::read_managed_toml_no_wait(&paths.config())
        .ok()
        .flatten()
        .and_then(|text| crate::config::ConfigFile::parse(&text).ok())
        .and_then(|c| c.site_address().ok().flatten());
    hosts.extend(address.as_deref().and_then(host_of));
    let config = crate::build::site_config::get_domain_config(project_path).ok();
    if let Some(domain) = config.as_ref().and_then(|c| c.domain.as_deref()) {
        hosts.extend(host_of(domain));
    }
    if let Some(site_id) = config.as_ref().and_then(|c| c.site_id.as_deref()) {
        let env = crate::build::site_config::resolve_environment(project_path);
        hosts.extend(host_of(&env.assumed_site_url(site_id)));
    }
    hosts
}

/// `sha256(host)` as lowercase hex, the form the comment server computes for
/// each host of an incoming comment. Unsalted, so lists from many sites can be
/// joined; a guessable host falls to a dictionary either way.
pub fn digest(host: &str) -> String {
    hex::encode(Sha256::digest(host.as_bytes()))
}

/// What the build knows about a folder's moderation: how much of the log a
/// trusted key verifies, and the hosts it blocks.
#[derive(Debug, Clone, Default)]
pub struct CommentModeration {
    pub events: usize,
    pub verified: usize,
    /// The keys that signed events which do not apply, sorted.
    pub untrusted: Vec<String>,
    pub blocked: BTreeSet<String>,
}

impl CommentModeration {
    pub fn load(project_path: &str) -> Self {
        let events = super::moderation::load_mod_events(project_path);
        if events.is_empty() {
            return Self::default();
        }
        let owners = crate::identity::keypair::Identity::read_trusted_pubkeys(Path::new(project_path));
        let own = own_hosts(project_path);
        Self {
            events: events.len(),
            verified: events.iter().filter(|e| owners.contains(&e.pubkey) && e.verify(&e.pubkey)).count(),
            untrusted: super::moderation::untrusted_signers(&events, &owners),
            blocked: active_blocks(&events, &owners).into_keys().filter(|h| !own.contains(*h)).map(str::to_string).collect(),
        }
    }

    /// A build's reading, `None` with comments off; recorded for the publish
    /// gate on every build, so a fixed folder clears the refusal.
    pub fn for_build(project_path: &str, services: Option<&crate::config::services::ServicesConfig>) -> Option<Self> {
        let moderation = services.and_then(|s| s.comments_on()).map(|_| Self::load(project_path));
        let unverifiable = moderation.as_ref().filter(|m| m.unverifiable()).map_or(0, |m| m.events);
        crate::system::build_records::records().record_unverifiable_moderation(project_path, unverifiable);
        moderation
    }

    /// The log has events and no trusted key verifies any: the build would
    /// show every hidden comment and publish an empty block list.
    pub fn unverifiable(&self) -> bool {
        self.events > 0 && self.verified == 0
    }

    fn published_json(&self) -> String {
        let hosts: BTreeSet<String> = self.blocked.iter().map(|h| digest(h)).collect();
        let mut doc = serde_json::json!({ "version": 2, "blocked_hosts": hosts });
        if self.unverifiable() {
            doc["moderation"] = "unverified".into();
        }
        doc.to_string()
    }
}

/// The text a person reads when a publish is refused because no key verifies
/// the folder's moderation log.
pub fn unverifiable_text(events: usize) -> String {
    format!(
        "Nothing published — this folder's moderation log has {events} event(s) and no key in .moss/identity/public.json verifies any, so hidden comments would show. Publish from the folder that holds the site's own .moss/identity."
    )
}

/// The stage hook: reports moderation that does not apply and writes
/// `_moss/comments.json` for a site with comments on.
pub fn emit(moderation: Option<&CommentModeration>, output_dir: &Path, pending: &mut PendingManifest) -> Result<(), String> {
    let Some(moderation) = moderation else { return Ok(()) };
    if moderation.unverifiable() {
        crate::build::cli_output::log_warn_problem!(
            "No key in .moss/identity/public.json verifies any of this folder's {} moderation event(s), so this build hides nothing and blocks no link; publishing is refused until the site's own .moss/identity is here.",
            moderation.events
        );
    } else if moderation.verified < moderation.events {
        crate::build::cli_output::log_warn_problem!(
            "{} of {} moderation event(s) do not apply: signed by {}, not one of this folder's keys; `moss comments list` says how to keep them.",
            moderation.events - moderation.verified,
            moderation.events,
            moderation.untrusted.join(", ")
        );
    }
    BuildContext::for_render(output_dir, pending)
        .emit_held(&ServedPath::for_comments_json(), moderation.published_json().into_bytes(), HashBucket::Files)
        .map_err(|e| format!("Failed to emit {COMMENTS_PATH}: {e}"))
}

/// A `--prebuilt` deploy refuses a tree another copy built without a key that
/// verified its moderation, and a folder whose own log no key verifies. Judged
/// on what that build recorded, never on a file being absent: a tree another
/// generator built has no `_moss/comments.json` and is not refused.
pub fn refuse_prebuilt(project_folder: &Path, prebuilt_dir: &Path) -> Result<(), String> {
    let own = CommentModeration::load(&project_folder.to_string_lossy());
    if own.unverifiable() {
        return Err(unverifiable_text(own.events));
    }
    let recorded = std::fs::read_to_string(prebuilt_dir.join(COMMENTS_PATH))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    if recorded.is_some_and(|v| v["moderation"] == "unverified") {
        return Err(format!(
            "Nothing published — {} was built where no key verified the site's moderation log (.moss/identity/public.json), so hidden comments would show. Build it in the folder that holds the site's own .moss/identity.",
            prebuilt_dir.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "link_block_tests.rs"]
mod tests;
