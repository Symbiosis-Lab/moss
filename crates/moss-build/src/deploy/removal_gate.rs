//! The publish gate's fourth rule, and the words every surface uses for it.
//!
//! An address a published site has served is a promise: links and
//! subscriptions to it live outside the site. A publish that would stop
//! serving one for a reason other than the author deleting its source is
//! refused until the author either keeps the address working or accepts losing
//! it. The list comes from the build's own comparison with the last publish
//! (`manifest::change_set::removed_addresses`); this module owns what the gate
//! does with it and how it reads.

use crate::build::manifest::change_set::{PendingRemoval, RemovalCause, RemovalReason, RemovedAddress, WithdrawnBy};
use crate::build::feeds::redirect_table::moves_a_folder;
use crate::build::served_path::served_address;

/// How many addresses a message names before counting the rest.
pub(crate) const LIST_CAP: usize = 20;

/// One address and what became of it, in plain words.
pub(crate) fn cause_line(r: &RemovedAddress) -> String {
    let at = served_address(&r.path);
    if r.reason == RemovalReason::AuthorRemoved {
        if r.source.is_none() && crate::build::feeds::calendar::is_calendar_file(&r.path) {
            return format!("{at} is gone because no event page says `start:` there any more.");
        }
        match r.withdrawn_by {
            Some(WithdrawnBy::AiAnswers) => {
                return format!("{at} is gone because the site asks AI assistants not to use its pages.");
            }
            Some(WithdrawnBy::AiSwitches) => {
                return format!("{at} is gone because the site no longer sets its AI switches.");
            }
            Some(WithdrawnBy::SelectionTools) => {
                return format!(
                    "{at} is gone because the site no longer offers Share on selected text \
                     (`selection_tools` under [site]), whose pictures carry the page's code."
                );
            }
            None => {}
        }
        return match &r.source {
            Some(src) => format!("{at} is gone because you deleted its source ({src})."),
            None => format!("{at} is gone because you deleted its file."),
        };
    }
    if r.reason == RemovalReason::Withheld {
        return match &r.source {
            Some(src) => format!("{at} is no longer published because {src} says `draft: true`."),
            None => format!("{at} is no longer published because its page says `draft: true`."),
        };
    }
    pending_line(&r.pending())
}

/// One `[redirects]` line as it goes in `.moss/config.toml`, each address
/// quoted the way TOML reads it back.
pub(crate) fn redirect_line(old: &str, new: &str) -> String {
    let quoted = |s: &str| toml::Value::String(s.to_string()).to_string();
    format!("{} = {}", quoted(old), quoted(new))
}

/// The words for a pending removal, built from its structured cause.
pub(crate) fn pending_line(p: &PendingRemoval) -> String {
    let at = &p.address;
    match &p.cause {
        RemovalCause::Moved { to } => format!(
            "{at} now lives at {to}. To keep the old link working, add {} under [redirects] in .moss/config.toml.",
            redirect_line(at, to)
        ),
        RemovalCause::StillInFolder { source } => {
            format!("{at} is still in your folder ({source}) but is no longer published.")
        }
        RemovalCause::Generated => format!("{at} is no longer produced by your site."),
    }
}

/// One entry of the refusal: a folder that two or more moved addresses share
/// (`old`, `new`, how many), or one address on its own.
enum Group<'a> {
    Folder { old: String, new: String, count: usize },
    One(&'a PendingRemoval),
}

/// The pending removals grouped for the refusal. A moved file's folder pair is
/// its two parent folders when its name is unchanged, a moved page's is its two
/// addresses, and a pair is lifted to its parents while the folder names match
/// and that parent pair is moved too, so a folder moved with its subfolders is
/// one group.
fn groups(pending: &[PendingRemoval]) -> Vec<Group<'_>> {
    fn parent(a: &str) -> Option<&str> {
        let t = a.strip_suffix('/').unwrap_or(a);
        t.rfind('/').map(|i| &t[..=i])
    }
    fn last(a: &str) -> &str {
        a.trim_end_matches('/').rsplit('/').next().unwrap_or("")
    }
    let folder_pair = |p: &PendingRemoval| -> Option<(String, String)> {
        let RemovalCause::Moved { to } = &p.cause else { return None };
        if p.address.ends_with('/') {
            return to.ends_with('/').then(|| (p.address.clone(), to.clone()));
        }
        let (old_dir, new_dir) = (parent(&p.address)?, parent(to)?);
        (last(&p.address) == last(to) && old_dir != new_dir).then(|| (old_dir.to_string(), new_dir.to_string()))
    };
    // Only a line that would move a folder is suggested as one.
    let pair = |p: &PendingRemoval| folder_pair(p).filter(|(old, new)| moves_a_folder(old, new));
    let pairs: Vec<Option<(String, String)>> = pending.iter().map(pair).collect();
    let known: std::collections::BTreeSet<(String, String)> = pairs.iter().flatten().cloned().collect();
    let lift = |mut p: (String, String)| loop {
        match (parent(&p.0), parent(&p.1)) {
            (Some(a), Some(b)) if last(&p.0) == last(&p.1) && known.contains(&(a.to_string(), b.to_string())) => {
                p = (a.to_string(), b.to_string());
            }
            _ => return p,
        }
    };
    let folders: Vec<Option<(String, String)>> = pairs.into_iter().map(|p| p.map(lift)).collect();
    let mut sizes: std::collections::BTreeMap<&(String, String), usize> = std::collections::BTreeMap::new();
    for f in folders.iter().flatten() {
        *sizes.entry(f).or_default() += 1;
    }
    let mut printed = std::collections::BTreeSet::new();
    let mut groups = Vec::new();
    for (p, folder) in pending.iter().zip(&folders) {
        match folder.as_ref().filter(|f| sizes[f] > 1) {
            Some(f) if printed.insert(f) => {
                groups.push(Group::Folder { old: f.0.clone(), new: f.1.clone(), count: sizes[f] })
            }
            Some(_) => {}
            None => groups.push(Group::One(p)),
        }
    }
    groups
}

/// The refusal's lines: one `[redirects]` line for each folder that two or
/// more moved addresses share, and [`pending_line`] for every other address.
fn grouped_lines(pending: &[PendingRemoval]) -> Vec<String> {
    groups(pending)
        .into_iter()
        .map(|g| match g {
            Group::Folder { old, new, count } => format!(
                "{count} addresses under {old} now live under {new}. To keep them working, add {} under [redirects] \
                 in .moss/config.toml.",
                redirect_line(&old, &new)
            ),
            Group::One(p) => pending_line(p),
        })
        .collect()
}

/// The `[redirects]` lines that keep every moved address working, as they go
/// in `.moss/config.toml`: one per folder moved together, one per other move.
pub(crate) fn redirect_lines(pending: &[PendingRemoval]) -> Vec<String> {
    groups(pending)
        .into_iter()
        .filter_map(|g| match g {
            Group::Folder { old, new, .. } => Some(redirect_line(&old, &new)),
            Group::One(PendingRemoval { address, cause: RemovalCause::Moved { to }, .. }) => Some(redirect_line(address, to)),
            Group::One(_) => None,
        })
        .collect()
}

/// Where a capped list is given in full.
pub(crate) const JSON_LISTS_ALL: &str = "`moss deploy --dry-run --json` lists every one";

/// The cut line of a list `moss deploy --dry-run --json` gives in full.
pub(crate) fn more_in_json<T>(rest: &[T]) -> String {
    format!("and {} more; {JSON_LISTS_ALL}", rest.len())
}

/// `items` indented two spaces, one per line, the first `LIST_CAP`, then a cut
/// line saying how many more.
pub(crate) fn capped_lines<T>(items: &[T], line: impl Fn(&T) -> String) -> String {
    capped_lines_cut(items, line, |rest| format!("and {} more", rest.len()))
}

/// [`capped_lines`] with the cut line worded by the caller, who can say what
/// was cut (`rest` is everything past the cap).
pub(crate) fn capped_lines_cut<T>(
    items: &[T],
    line: impl Fn(&T) -> String,
    cut: impl Fn(&[T]) -> String,
) -> String {
    let mut out: String = items.iter().take(LIST_CAP).map(|i| format!("  {}\n", line(i))).collect();
    if items.len() > LIST_CAP {
        out.push_str(&format!("  {}\n", cut(&items[LIST_CAP..])));
    }
    out
}

/// The refusal, which has to stand on its own: a caller with no accept button
/// shows only this text.
pub(crate) fn refusal_text(pending: &[PendingRemoval]) -> String {
    let (head, them) = if pending.len() == 1 {
        ("1 address your site has served would stop working, and you did not remove it".to_string(), "it")
    } else {
        (format!("{} addresses your site has served would stop working, and you did not remove them", pending.len()), "them")
    };
    format!(
        "Nothing published — {head}:\n{}Either keep {} working (for a moved page or file, add the redirect shown), \
         or accept losing {them} and publish with `moss deploy --accept-removals`.",
        capped_lines_cut(&grouped_lines(pending), String::clone, more_in_json),
        if pending.len() == 1 { "the address" } else { "the addresses" },
    )
}

/// The refusal for an upload that has no sources: every missing address is
/// unexplained, because nothing here could show the author deleted it.
pub(crate) fn prebuilt_refusal(paths: &[String]) -> String {
    let pending: Vec<PendingRemoval> = paths
        .iter()
        .map(|path| PendingRemoval {
            path: path.clone(),
            address: served_address(path),
            cause: RemovalCause::Generated,
        })
        .collect();
    format!(
        "{} A prebuilt folder has no sources to show you deleted a page on purpose, so every missing address counts.",
        refusal_text(&pending)
    )
}

/// The refusal for `folder_path`, or `None` when the last build found nothing
/// the author has not accepted.
pub(crate) fn refusal_for(folder_path: &str) -> Option<String> {
    let pending = crate::system::build_records::records().pending_removals(folder_path);
    (!pending.is_empty()).then(|| refusal_text(&pending))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vanished_calendar_file_is_worded_as_the_authors_edit() {
        let r = RemovedAddress {
            path: "events/talk/event.ics".into(),
            reason: crate::build::manifest::change_set::RemovalReason::AuthorRemoved,
            moved_to: None,
            source: None,
            withdrawn_by: None,
        };
        assert_eq!(cause_line(&r), "/events/talk/event.ics is gone because no event page says `start:` there any more.");
    }

    #[test]
    fn a_file_a_setting_stopped_names_the_setting() {
        let gone = |path: &str, by| RemovedAddress {
            path: path.into(),
            reason: RemovalReason::AuthorRemoved,
            moved_to: None,
            source: None,
            withdrawn_by: Some(by),
        };
        assert_eq!(
            cause_line(&gone("qr/notes/spring.svg", WithdrawnBy::SelectionTools)),
            "/qr/notes/spring.svg is gone because the site no longer offers Share on selected text \
             (`selection_tools` under [site]), whose pictures carry the page's code."
        );
        assert_eq!(
            cause_line(&gone("llms.txt", WithdrawnBy::AiAnswers)),
            "/llms.txt is gone because the site asks AI assistants not to use its pages."
        );
        assert_eq!(
            cause_line(&gone(".well-known/tdmrep.json", WithdrawnBy::AiSwitches)),
            "/.well-known/tdmrep.json is gone because the site no longer sets its AI switches."
        );
    }
}
