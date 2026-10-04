//! The publish gate's fourth rule, and the words every surface uses for it.
//!
//! An address a published site has served is a promise: links and
//! subscriptions to it live outside the site. A publish that would stop
//! serving one for a reason other than the author deleting its source is
//! refused until the author either keeps the address working or accepts losing
//! it. The list comes from the build's own comparison with the last publish
//! (`manifest::change_set::removed_addresses`); this module owns what the gate
//! does with it and how it reads.

use std::collections::BTreeSet;

use crate::build::manifest::change_set::{RemovalReason, RemovedAddress};
use crate::build::served_path::served_address;

/// How many addresses a message names before counting the rest.
pub(crate) const LIST_CAP: usize = 20;

/// The unexplained removals the author has not accepted. A set accepted once
/// covers exactly the addresses in it: a new address asks again.
pub(crate) fn unaccepted_unexplained(
    removed: &[RemovedAddress],
    accepted: Option<&BTreeSet<String>>,
) -> Vec<RemovedAddress> {
    removed
        .iter()
        .filter(|r| r.reason == RemovalReason::Unexplained)
        .filter(|r| !accepted.is_some_and(|set| set.contains(&r.path)))
        .cloned()
        .collect()
}

/// One address and what became of it, in plain words.
pub(crate) fn cause_line(r: &RemovedAddress) -> String {
    let at = served_address(&r.path);
    if r.reason == RemovalReason::AuthorRemoved {
        return match &r.source {
            Some(src) => format!("{at} is gone because you deleted its source ({src})."),
            None => format!("{at} is gone because you deleted its file."),
        };
    }
    match (&r.moved_to, &r.source) {
        (Some(to), _) => {
            let to = served_address(to);
            format!(
                "{at} now lives at {to}. To keep the old link working, add \"{at}\" = \"{to}\" \
                 under [redirects] in .moss/config.toml."
            )
        }
        (None, Some(src)) => format!("{at} is still in your folder ({src}) but is no longer published."),
        (None, None) => format!("{at} is no longer produced by your site."),
    }
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
pub(crate) fn refusal_text(pending: &[RemovedAddress]) -> String {
    let (head, them) = if pending.len() == 1 {
        ("1 address your site has served would stop working, and you did not remove it".to_string(), "it")
    } else {
        (format!("{} addresses your site has served would stop working, and you did not remove them", pending.len()), "them")
    };
    format!(
        "Nothing published — {head}:\n{}Either keep {} working (for a moved page, add the redirect shown), \
         or accept losing {them} and publish with `moss deploy --accept-removals`.",
        capped_lines(pending, cause_line),
        if pending.len() == 1 { "the address" } else { "the addresses" },
    )
}

/// The refusal for `folder_path`, or `None` when the last build found nothing
/// the author has not accepted.
pub(crate) fn refusal_for(folder_path: &str) -> Option<String> {
    let records = crate::system::build_records::records();
    let pending = unaccepted_unexplained(
        &records.removed_addresses(folder_path)?,
        records.accepted_removals(folder_path).as_ref(),
    );
    (!pending.is_empty()).then(|| refusal_text(&pending))
}
