//! The site's one redirect table.
//!
//! An address that stops existing keeps working only through a redirect, and
//! three different things can retire an address: a page renamed under the same
//! uid (or a term kind that moved namespace), an address the author redirects
//! by hand in `[redirects]`, and a path the generator itself used to write
//! (the feed's former `feed.xml`). Each is a source of entries here; this
//! module merges them, decides what static form each entry can take, and
//! renders the one file (`_moss/redirects.json`) a host that can answer a real
//! HTTP 301 reads. Writing the stubs and copies is `redirects`' job.
//!
//! The static form depends on the old address:
//!
//! - a page address (`/old/`, `/old`) or a `.html` file gets the HTML stub;
//! - any other file gets a byte copy of its target, when the target is a file
//!   this build serves;
//! - a file whose target is a page or an external URL has no static form. It
//!   is still listed, and the build says it works only on a host that reads
//!   the table.
//!
//! A hand-declared entry is checked and reported on; the other two sources are
//! the build's own record and are trusted, so they skip quietly when they
//! would sit on a real output.

use crate::build::cli_output::log_warn_problem;
use crate::build::manifest::PendingManifest;
use crate::build::served_path::ServedPath;
use serde::Serialize;
use std::collections::BTreeMap;
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
/// outside the site already say and a host reading the table matches that
/// string. Only the static form's path on disk is derived from it
/// (percent-decoded once, then named the way every output is named).
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

    /// What the table and a stub's link carry.
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
}

impl Probe<'_> {
    /// This build writes it, or the site folder provides it (the folder's own
    /// files are copied in after this phase, so the folder is asked directly).
    /// The stage is not asked: it keeps what the previous build left there,
    /// including this table's own copies.
    fn serves(&self, a: &Addr) -> bool {
        self.pending.is_registered(a.key())
            || [a.key(), a.raw.as_str()].iter().any(|rel| self.source_root.join(rel).is_file())
    }

    /// Where the bytes of a file target are: the stage if this build wrote
    /// them, else the site folder's own file.
    pub(crate) fn copy_source(&self, a: &Addr, output_dir: &Path) -> Option<CopySource> {
        let staged = output_dir.join(a.key());
        if self.pending.is_registered(a.key()) && staged.is_file() {
            return Some(CopySource::Stage(staged));
        }
        [a.key(), a.raw.as_str()]
            .iter()
            .map(|rel| self.source_root.join(rel))
            .find(|p| p.is_file())
            .map(CopySource::Folder)
    }
}

pub(crate) enum CopySource {
    Stage(std::path::PathBuf),
    Folder(std::path::PathBuf),
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
    /// Listed in the table only.
    TableOnly,
}

#[derive(Debug, Clone)]
pub(crate) struct Planned {
    pub from: Addr,
    pub to: Target,
    pub form: Form,
}

/// Merge the candidates (later sources override earlier ones for the same old
/// address) into the entries this build serves, sorted by old address.
pub(crate) fn plan(candidates: Vec<Candidate>, probe: &Probe) -> Vec<Planned> {
    let mut table: BTreeMap<String, Planned> = BTreeMap::new();
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
        if probe.serves(&from) {
            if c.declared {
                log_warn_problem!(
                    "redirect '{}' skipped: the site already serves that address, and what it serves wins",
                    from.served()
                );
            }
            continue;
        }
        if c.declared {
            let refusal = match &to {
                Target::Internal(t, _) if t.key() == from.key() => Some("it points at itself".to_string()),
                Target::Internal(t, _) if !probe.serves(t) => {
                    Some(format!("'{}' is not an address this site serves", t.served()))
                }
                _ => None,
            };
            if let Some(why) = refusal {
                log_warn_problem!("redirect '{}' skipped: {why}", from.served());
                continue;
            }
        }
        let form = match (&from.kind, &to) {
            // A stub at the rewritten path would leave the real old address dead
            // while looking as if the redirect worked.
            _ if !from.carried => {
                log_warn_problem!(
                    "redirect '{}' to '{}' works only on a host that reads _moss/redirects.json: the address \
                     has characters (capitals or spaces in a folder name) that a published file path cannot carry",
                    from.served(),
                    to.href()
                );
                Form::TableOnly
            }
            (Kind::File, Target::Internal(t, _)) if t.kind == Kind::File => Form::Copy,
            (Kind::File, _) => {
                log_warn_problem!(
                    "redirect '{}' to '{}' has no file to copy, so it works only on a host that reads \
                     _moss/redirects.json",
                    from.served(),
                    to.href()
                );
                Form::TableOnly
            }
            _ => Form::Stub,
        };
        table.insert(from.served(), Planned { from, to, form });
    }
    table.into_values().collect()
}

#[derive(Serialize)]
struct TableFile {
    version: u32,
    redirects: Vec<Row>,
}

#[derive(Serialize)]
struct Row {
    from: String,
    to: String,
    status: u16,
}

/// The shipped `_moss/redirects.json`: every entry, sorted by old address.
pub(crate) fn to_json(planned: &[Planned]) -> Vec<u8> {
    let redirects = planned
        .iter()
        .map(|p| Row { from: p.from.served(), to: p.to.href(), status: 301 })
        .collect();
    serde_json::to_vec(&TableFile { version: 1, redirects }).expect("a table of strings serializes")
}

#[cfg(test)]
#[path = "redirect_table_tests.rs"]
mod tests;
