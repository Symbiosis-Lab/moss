//! The site's one redirect table.
//!
//! An address that stops existing keeps working only through a redirect, and
//! three different things can retire an address: a page renamed under the same
//! uid (or a term kind that moved namespace), an address the author redirects
//! by hand in `[redirects]`, and a path the generator itself used to write
//! (the feed's former `feed.xml`). Each is a source of entries here; this
//! module merges them and decides what static form each entry can take.
//! Writing the stubs and copies is `redirects`' job. No table of the entries is published: no host reads
//! one, and it would list every old address next to its new one, including an
//! unlisted page's secret address.
//!
//! The static form depends on the old address:
//!
//! - a page address (`/old/`, `/old`) or a `.html` file gets the HTML stub;
//! - any other file gets a copy of its target, when the target is a file this
//!   build serves: the bytes it serves there, so a folder picture's copy is
//!   written by the seal (`kept_addresses`), after the image pipeline;
//! - a file whose target is a page or an external URL has no static form, so
//!   it is reported and left out.
//!
//! A hand-declared entry is checked and reported on; the other two sources are
//! the build's own record and are trusted, so they skip quietly when they
//! would sit on a real output. A trusted entry is also re-targeted, whichever
//! of the two it comes from: when its target sits in a language folder
//! (`/zh-hans/new/`) and this build no longer serves it, the entry goes to the
//! default language's page at the same place (`/new/`) if that one is served.
//! The recorded history is not changed, so a translation published there again
//! is pointed at again.

use crate::build::cli_output::log_warn_problem;
use crate::build::manifest::PendingManifest;
use crate::build::served_path::ServedPath;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Paths the generator used to write and must keep answering, old address to
/// its replacement. Feed readers store the URL they subscribed to, so a path
/// the generator renames is added here, never just changed. Only the
/// replacement is linked from pages or listed in the sitemap.
pub(crate) const FORMER_PATHS: [(&str, &str); 1] = [("/feed.xml", "/rss.xml")];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `/old/` or `/old`: served from `<dir>/index.html`.
    Page,
    /// A `.html` file, served at exactly that path.
    Html,
    /// Any other file.
    File,
}

/// An address this site serves (or is asked to), parsed from any slash spelling.
///
/// The address is kept exactly as written, because it is whatever links
/// outside the site already say, and an internal stub's link carries it.
/// Only the static form's path on disk is derived from it (percent-decoded
/// once, then named the way every output is named).
#[derive(Debug, Clone)]
pub(crate) struct Addr {
    sp: ServedPath,
    /// As written, with one leading slash, and a trailing one for a page.
    literal: String,
    /// The decoded path without its leading slash, for looking in the site folder.
    raw: String,
    kind: Kind,
    /// Naming it the way every output is named left it as written.
    carried: bool,
}

impl Addr {
    pub(crate) fn parse(input: &str) -> Result<Self, String> {
        let input = input.trim();
        if input.starts_with("//") {
            return Err("it starts with // (a protocol-relative address)".into());
        }
        if input.contains(['?', '#']) {
            return Err("an address names a path, without a ? or # part".into());
        }
        let written = input.trim_start_matches('/');
        let raw = moss_core::resolve::fuzzy_path::percent_decode_path(written);
        if raw.split('/').any(|seg| seg == "..") {
            return Err("it climbs out of the site with ..".into());
        }
        let last = raw.rsplit('/').next().unwrap_or("");
        let kind = if raw.is_empty() || raw.ends_with('/') {
            Kind::Page
        } else if last.ends_with(".html") {
            Kind::Html
        } else if last.contains('.') {
            Kind::File
        } else {
            Kind::Page
        };
        let fs = match kind {
            Kind::Page => super::redirects::pretty_url_to_fs_path(&raw),
            _ => raw.clone(),
        };
        let sp = ServedPath::from_source(&fs).map_err(|e| e.to_string())?;
        let carried = sp.as_str() == fs;
        let slash = if kind == Kind::Page && !written.is_empty() && !written.ends_with('/') { "/" } else { "" };
        let literal = format!("/{written}{slash}"); // allow:served-path-url-construct (redirect table address, kept as the author wrote it)
        Ok(Self { sp, literal, raw, kind, carried })
    }

    /// The manifest key this address is written to, e.g. `old/index.html`.
    pub(crate) fn key(&self) -> &str {
        self.sp.as_str()
    }

    /// The decoded path, as the file is named in the site folder.
    pub(crate) fn raw(&self) -> &str {
        &self.raw
    }

    pub(crate) fn served_path(&self) -> &ServedPath {
        &self.sp
    }

    /// How a browser asks for it: `/old/`, `/scale-compare.html`.
    pub(crate) fn served(&self) -> String {
        self.literal.clone()
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Target {
    /// An address of this site, and the `#fragment` or `?query` after it, if any.
    Internal(Addr, String),
    External(String),
}

impl Target {
    fn parse(input: &str) -> Result<Self, String> {
        let input = input.trim();
        if let Some(rest) = input.strip_prefix("https://") {
            let clean = !rest.is_empty() && !rest.chars().any(|c| c.is_whitespace() || matches!(c, '"' | '<' | '>'));
            return if clean { Ok(Self::External(input.to_string())) } else { Err("not a usable https URL".into()) };
        }
        let cut = input.find(['#', '?']).unwrap_or(input.len());
        let (path, suffix) = input.split_at(cut);
        // `http://x`, `javascript:x`, `mailto:x`: anything that opens with a scheme.
        if path.split('/').next().is_some_and(|first| first.contains(':')) {
            return Err("an external target must be an https:// URL".into());
        }
        Addr::parse(path).map(|a| Self::Internal(a, suffix.to_string()))
    }

    /// What a stub's link carries.
    pub(crate) fn href(&self) -> String {
        match self {
            Self::Internal(a, suffix) => format!("{}{suffix}", a.served()),
            Self::External(url) => url.clone(),
        }
    }
}

/// What the build has to look at to say whether an address is already taken or
/// a target exists.
pub(crate) struct Probe<'a> {
    pub pending: &'a PendingManifest,
    pub source_root: &'a Path,
    /// Pages the background phase writes after this table is planned, at
    /// addresses the scan already fixes (static `.html` pages, notebook viewers).
    pub written_later: &'a BTreeSet<String>,
}

impl Probe<'_> {
    /// The address is taken. Asked after every page this build registers
    /// before its background phase: rendered, generated (folder indexes, media
    /// collections, subscribe pages) and carried forward from the last build
    /// because the source is still in the cloud. Pages the background phase
    /// writes come from `written_later`, and the folder's other files, copied
    /// in as they are, from the site folder itself. The stage is not asked: it
    /// keeps what the previous build left there, including this table's own
    /// copies.
    fn serves(&self, a: &Addr) -> bool {
        self.pending.is_registered(a.key())
            || self.written_later.contains(a.key())
            || [a.key(), a.raw.as_str()].iter().any(|rel| self.source_root.join(rel).is_file())
    }

    /// Where a target stands. A target inside a language folder that this
    /// build does not serve is answered with the default language's address at
    /// the same place, when that one is served: the page the translation stood
    /// in for. Only an address this build serves itself counts, never another
    /// entry's stub.
    fn locate(&self, t: &Addr) -> Found {
        if self.serves(t) {
            return Found::Served;
        }
        // The written form, which keeps a page's trailing slash, so `zh-hans`
        // and `zh-hans/` are both the language's home.
        let written = t.literal.trim_start_matches('/');
        let twin = moss_core::home::lang_tree_prefix(written).and_then(|lang| Addr::parse(&written[lang.len()..]).ok());
        match twin {
            Some(twin) if self.serves(&twin) => Found::DefaultLanguage(twin),
            _ => Found::Missing,
        }
    }
}

enum Found {
    Served,
    /// Not served, but the default language's page at the same place is.
    DefaultLanguage(Addr),
    Missing,
}

/// One entry as a source states it, before any check.
pub(crate) struct Candidate {
    pub from: String,
    pub to: String,
    /// Written by hand in `[redirects]`: checked, and reported on.
    pub declared: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Form {
    Stub,
    Copy,
}

#[derive(Debug, Clone)]
pub(crate) struct Planned {
    pub from: Addr,
    pub to: Target,
    pub form: Form,
}

/// A folder line's output prefixes, e.g. `review/old/` and `review/client/old/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FolderMove {
    pub from: String,
    pub to: String,
    /// The site already serves the old folder's own address, so the line
    /// does something only if it keeps a file under it.
    pub old_served: bool,
}

/// What the build serves for the table: its entries, and the folder moves the
/// seal fills in (`kept_addresses`), since only it knows every file served
/// under a new folder.
pub(crate) struct Plan {
    pub entries: Vec<Planned>,
    pub folder_moves: Vec<FolderMove>,
}

/// The folder move a declared line makes, when its two sides both end in `/`
/// and name two folders of this site, neither of them the root. `Err` is a
/// folder line that names the root: moving every address of the site is not
/// something one line should do by accident.
fn folder_move(c: &Candidate, from: &Addr, to: &Target, old_served: bool) -> Result<Option<FolderMove>, ()> {
    let Target::Internal(t, suffix) = to else { return Ok(None) };
    if !c.declared || !c.from.trim().ends_with('/') || !c.to.trim().ends_with('/') || !suffix.is_empty() || !from.carried || t.key() == from.key() {
        return Ok(None);
    }
    let prefix = |a: &Addr| a.key().strip_suffix("index.html").unwrap_or_default().to_string();
    let (from, to) = (prefix(from), prefix(t));
    if from.is_empty() || to.is_empty() {
        return Err(());
    }
    Ok(Some(FolderMove { from, to, old_served }))
}

/// Merge the candidates into the entries this build serves. A later candidate
/// for the same output replaces an earlier one only when it can take a static
/// form; one that cannot is reported and leaves the earlier entry standing.
///
/// A declared line whose two sides both end in `/` also moves everything under
/// the old folder. Its own page half then forwards the folder's address only
/// where it can: an old folder that still serves a page (its listing, while
/// a file is left in it) keeps it, and a new folder with no front page is left
/// for the seal to report if nothing is served under it either.
pub(crate) fn plan(candidates: Vec<Candidate>, probe: &Probe) -> Plan {
    let mut table: BTreeMap<String, Planned> = BTreeMap::new();
    let mut folder_moves: Vec<FolderMove> = Vec::new();
    for c in candidates {
        let from = match Addr::parse(&c.from) {
            Ok(a) => a,
            Err(e) => {
                log_warn_problem!("redirect '{}' is not a valid address ({e}); it will not redirect", c.from);
                continue;
            }
        };
        let to = match Target::parse(&c.to) {
            Ok(t) => t,
            Err(e) => {
                log_warn_problem!("redirect '{}' to '{}': {e}; it will not redirect", c.from, c.to);
                continue;
            }
        };
        let old_served = probe.serves(&from);
        match folder_move(&c, &from, &to, old_served) {
            Ok(Some(m)) => {
                folder_moves.push(m);
                let Target::Internal(t, _) = &to else { unreachable!("a folder move is internal") };
                if old_served || !matches!(probe.locate(t), Found::Served) {
                    continue;
                }
            }
            Ok(None) => {}
            Err(()) => log_warn_problem!(
                "redirect '{}' to '{}' moves no folder: a folder line cannot name the site root (only the page itself is redirected)",
                from.served(),
                to.href()
            ),
        }
        if old_served {
            if c.declared {
                log_warn_problem!(
                    "redirect '{}' skipped: the site already serves that address, and what it serves wins",
                    from.served()
                );
            }
            continue;
        }
        // A hand-declared entry must name an address this site serves. An
        // entry from the build's own record is kept whatever its target, and
        // goes to the default language's page when its translation is gone.
        let to = match to {
            Target::Internal(t, suffix) => match (c.declared, probe.locate(&t)) {
                (true, _) if t.key() == from.key() => {
                    log_warn_problem!("redirect '{}' skipped: it points at itself", from.served());
                    continue;
                }
                (true, Found::Served) => Target::Internal(t, suffix),
                (true, _) => {
                    log_warn_problem!("redirect '{}' skipped: '{}' is not an address this site serves", from.served(), t.served());
                    continue;
                }
                (false, Found::DefaultLanguage(twin)) => Target::Internal(twin, suffix),
                (false, _) => Target::Internal(t, suffix),
            },
            external => external,
        };
        let form = match (&from.kind, &to) {
            // A stub at the rewritten path would leave the real old address dead
            // while looking as if the redirect worked.
            _ if !from.carried => {
                log_warn_problem!(
                    "redirect '{}' to '{}' skipped: the address has characters (capitals or spaces in a \
                     folder name) that a published file path cannot carry",
                    from.served(),
                    to.href()
                );
                continue;
            }
            (Kind::File, Target::Internal(t, _)) if t.kind == Kind::File => Form::Copy,
            (Kind::File, _) => {
                log_warn_problem!(
                    "redirect '{}' to '{}' skipped: a file can only be redirected to another file of this \
                     site, which it is copied from (an earlier redirect of the address, if any, still stands)",
                    from.served(),
                    to.href()
                );
                continue;
            }
            _ => Form::Stub,
        };
        table.insert(from.key().to_string(), Planned { from, to, form });
    }
    Plan { entries: table.into_values().collect(), folder_moves }
}

/// What the build does with a `[redirects]` line, one sentence each, as
/// `moss describe --json` prints them. Kept here, beside the code they
/// describe, so a change to one is a change to the other.
pub(crate) const RULES: [&str; 7] = [
    "Both sides are site-absolute; the new side is an address this site serves or an https:// URL.",
    "A page address (/old/ or /old) or an .html file gets a forwarding page.",
    "Any other file can only be sent to another non-HTML file of the site, and gets a copy of the file the site serves there (for a picture, the processed one), so both addresses give the same bytes; any other target is reported and does not redirect.",
    "When both sides are folders of this site (ending in /, with no # or ?), the line also moves everything under the old folder: each page and .html file served under the new folder gets a forwarding page at the same path under the old one, and every other file a copy of the served file. A folder line naming the site root on either side moves no folder.",
    "An old address the site still serves keeps what it serves; a line that changes nothing is reported.",
    "An old address with capitals or spaces in a folder name cannot be a file path, so it is reported and does not redirect.",
    "No list of redirects is published: each old address answers through its own forwarding page or copy.",
];

#[cfg(test)]
#[path = "redirect_table_tests.rs"]
mod tests;
