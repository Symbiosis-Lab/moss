//! The publish gate's fourth rule, and the words every surface uses for it.
//!
//! An address a published site has served is a promise: links and
//! subscriptions to it live outside the site. A publish that would stop
//! serving one for a reason other than the author deleting its source is
//! refused until the author either keeps the address working or accepts losing
//! it. The list comes from the build's own comparison with the last publish
//! (`manifest::change_set::removed_addresses`); this module owns what the gate
//! does with it and how it reads.

use crate::build::manifest::change_set::{PendingRemoval, RemovalCause, RemovalReason, RemovedAddress};
use crate::build::served_path::served_address;

/// How many addresses a message names before counting the rest.
pub(crate) const LIST_CAP: usize = 20;

/// One address and what became of it, in plain words.
pub(crate) fn cause_line(r: &RemovedAddress) -> String {
    let at = served_address(&r.path);
    if r.reason == RemovalReason::AuthorRemoved {
        return match &r.source {
            Some(src) => format!("{at} is gone because you deleted its source ({src})."),
            None => format!("{at} is gone because you deleted its file."),
        };
    }
    pending_line(&r.pending())
}

/// The words for a pending removal, built from its structured cause.
pub(crate) fn pending_line(p: &PendingRemoval) -> String {
    let at = &p.address;
    match &p.cause {
        RemovalCause::Moved { to } => format!(
            "{at} now lives at {to}. To keep the old link working, add \"{at}\" = \"{to}\" \
             under [redirects] in .moss/config.toml."
        ),
        RemovalCause::StillInFolder { source } => {
            format!("{at} is still in your folder ({source}) but is no longer published.")
        }
        RemovalCause::Generated => format!("{at} is no longer produced by your site."),
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
pub(crate) fn refusal_text(pending: &[PendingRemoval]) -> String {
    let (head, them) = if pending.len() == 1 {
        ("1 address your site has served would stop working, and you did not remove it".to_string(), "it")
    } else {
        (format!("{} addresses your site has served would stop working, and you did not remove them", pending.len()), "them")
    };
    format!(
        "Nothing published — {head}:\n{}Either keep {} working (for a moved page, add the redirect shown), \
         or accept losing {them} and publish with `moss deploy --accept-removals`.",
        capped_lines(pending, pending_line),
        if pending.len() == 1 { "the address" } else { "the addresses" },
    )
}

/// The refusal for `folder_path`, or `None` when the last build found nothing
/// the author has not accepted.
pub(crate) fn refusal_for(folder_path: &str) -> Option<String> {
    let pending = crate::system::build_records::records().pending_removals(folder_path);
    (!pending.is_empty()).then(|| refusal_text(&pending))
}
