//! Pure reducer: fold owner-signed moderation events over the comment set to
//! produce the visible view at RENDER time (ADR-025 §8). Signed `hide`/`unhide`
//! events in `moderation.jsonl` are the SOLE bake-time moderation path —
//! the legacy replica-tombstone path has been retired. `sync_remote` runs a
//! one-time migration that converts any pre-existing tombstones to signed events
//! before the first server-wins reconcile.

use std::collections::{HashMap, HashSet};

use super::moderation::ModEvent;
use super::NormalizedComment;

/// Return the visible comments in input order (the renderer builds the thread).
///
/// A comment is hidden if the latest *verified* visibility event for its
/// `(source, id)` is `hide`, OR any ancestor (via `reply_to_id`, same source) is
/// hidden. "Latest" = max `(seq, ts)`. Unhiding a parent restores only the
/// descendants that are not independently hidden. Events not signed by
/// `owner_pubkey`, and `pin`/`unpin` (v1), are ignored.
pub fn resolve<'a>(
    comments: &'a [NormalizedComment],
    events: &[ModEvent],
    owner_pubkey: &str,
) -> Vec<&'a NormalizedComment> {
    // Latest verified visibility event per (source, target), ordered by (seq, ts).
    let mut latest: HashMap<(&str, &str), &ModEvent> = HashMap::new();
    for e in events {
        if (e.kind != "hide" && e.kind != "unhide") || !e.verify(owner_pubkey) {
            continue;
        }
        let key = (e.source.as_str(), e.target.as_str());
        match latest.get(&key) {
            Some(prev) if (prev.seq, prev.ts) >= (e.seq, e.ts) => {}
            _ => {
                latest.insert(key, e);
            }
        }
    }
    let directly_hidden: HashSet<(&str, &str)> = latest
        .iter()
        .filter(|(_, e)| e.kind == "hide")
        .map(|(k, _)| *k)
        .collect();

    let by_id: HashMap<(&str, &str), &NormalizedComment> = comments
        .iter()
        .map(|c| ((c.source.as_str(), c.id.as_str()), c))
        .collect();

    let max_depth = comments.len() + 1; // cycle guard
    let mut visible = Vec::new();
    'each: for c in comments {
        let mut cur = c;
        for _ in 0..max_depth {
            if directly_hidden.contains(&(cur.source.as_str(), cur.id.as_str())) {
                continue 'each;
            }
            match cur.reply_to_id.as_deref() {
                Some(pid) => match by_id.get(&(cur.source.as_str(), pid)) {
                    Some(parent) => cur = parent,
                    None => break, // dangling parent → treat as top-level
                },
                None => break,
            }
        }
        visible.push(c);
    }
    visible
}

/// Drop hidden comments from an owned vec in place — the build-path convenience
/// over [`resolve`]. No events → no work (and the caller need not even load the
/// identity). Lean: mutates the already-owned comment map, no clone of the kept set.
pub fn retain_visible(comments: &mut Vec<NormalizedComment>, events: &[ModEvent], owner_pubkey: &str) {
    if events.is_empty() {
        return;
    }
    let visible: HashSet<(String, String)> = resolve(comments, events, owner_pubkey)
        .iter()
        .map(|c| (c.source.clone(), c.id.clone()))
        .collect();
    comments.retain(|c| visible.contains(&(c.source.clone(), c.id.clone())));
}

#[cfg(test)]
mod tests {
    use super::super::moderation::sign_mod_event;
    use super::super::CommentAuthor;
    use super::*;
    use k256::schnorr::SigningKey;

    /// A throwaway owner keypair. k256's own generator rather than
    /// `identity::keypair::Identity`, so this module's tests do not
    /// reintroduce the app-keyring dependency the production code sheds.
    struct Owner {
        sk: SigningKey,
        pubkey: String,
    }

    fn owner() -> Owner {
        let sk = SigningKey::random(&mut rand::rngs::OsRng);
        let pubkey = hex::encode(sk.verifying_key().to_bytes());
        Owner { sk, pubkey }
    }

    fn mk(id: &str, reply_to: Option<&str>) -> NormalizedComment {
        NormalizedComment {
            id: id.into(),
            source: "artalk".into(),
            content: "x".into(),
            created_at: "2026-01-01".into(),
            author: CommentAuthor {
                display_name: Some("A".into()),
                name: None,
                url: None,
            },
            reply_to_id: reply_to.map(String::from),
            state: None,
        }
    }

    fn ev(id: &Owner, kind: &str, target: &str, seq: u64, ts: u64) -> ModEvent {
        sign_mod_event(&id.sk, kind, "artalk", target, "guo", "p", seq, ts)
    }

    fn ids(v: &[&NormalizedComment]) -> Vec<String> {
        v.iter().map(|c| c.id.clone()).collect()
    }

    #[test]
    fn hide_removes_comment() {
        let id = owner();
        let comments = vec![mk("1", None)];
        assert_eq!(resolve(&comments, &[ev(&id, "hide", "1", 1, 100)], &id.pubkey).len(), 0);
    }

    #[test]
    fn later_seq_unhide_wins_even_with_lower_ts() {
        let id = owner();
        let comments = vec![mk("1", None)];
        let events = vec![ev(&id, "hide", "1", 1, 100), ev(&id, "unhide", "1", 2, 90)];
        assert_eq!(resolve(&comments, &events, &id.pubkey).len(), 1);
    }

    #[test]
    fn forged_event_is_ignored() {
        let id = owner();
        let other = owner();
        let comments = vec![mk("1", None)];
        // hide signed by `other`, resolved against `id` → ignored, comment visible.
        assert_eq!(resolve(&comments, &[ev(&other, "hide", "1", 1, 100)], &id.pubkey).len(), 1);
    }

    #[test]
    fn hidden_parent_hides_subtree() {
        let id = owner();
        let comments = vec![mk("1", None), mk("2", Some("1")), mk("3", Some("2"))];
        assert_eq!(resolve(&comments, &[ev(&id, "hide", "1", 1, 100)], &id.pubkey).len(), 0);
    }

    #[test]
    fn independently_hidden_reply_stays_hidden_on_parent_unhide() {
        let id = owner();
        let comments = vec![mk("1", None), mk("2", Some("1"))];
        let events = vec![
            ev(&id, "hide", "1", 1, 10),
            ev(&id, "hide", "2", 2, 10),
            ev(&id, "unhide", "1", 3, 10),
        ];
        assert_eq!(ids(&resolve(&comments, &events, &id.pubkey)), vec!["1"]);
    }

    #[test]
    fn pin_events_are_noop_in_v1() {
        let id = owner();
        let comments = vec![mk("1", None)];
        // a pin event must not hide and must not error
        assert_eq!(resolve(&comments, &[ev(&id, "pin", "1", 1, 100)], &id.pubkey).len(), 1);
    }
}
