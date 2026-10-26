//! The placeholder page written beside each file a plugin can turn into pages.
//!
//! A stub is an ordinary markdown file made of embeds of its own source file,
//! laid out by the plugin's manifest (`contribution.stub`), so core never
//! knows what the format looks like. It is written once, when the author shows
//! a folder as pages or when a new file arrives in a shown folder, and never
//! from a build scan: a scan would bring back a stub the author deleted.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use moss_core::resolve::md_extract::{extract_md_references, RefSyntax};

use crate::build::cloud_readiness::{read_to_string_with_materialize_wait, INTERACTIVE_DEADLINE};
use super::derive_folders::{decisions, folder_key, Decision};
use crate::moss_paths::MossPaths;
use crate::system::derive::registry::{DerivationEntry, DerivationRegistry};

/// A file name that cannot be written inside `![[…]]` without changing what it
/// means.
fn embeddable(name: &str) -> bool {
    !name.contains(['[', ']', '|', '#', '^', '/', '\\'])
}

fn extension_accepted(entry: &DerivationEntry, path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| entry.contribution.accepts.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

/// The files directly inside `folder` this derivation accepts, by name.
pub fn accepted_files(folder: &Path, entry: &DerivationEntry) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut files: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && extension_accepted(entry, p))
        .collect();
    files.sort();
    files
}

/// The file names the markdown pages directly inside `folder` embed, read once
/// for the whole folder.
fn embedded_names(folder: &Path) -> HashSet<String> {
    let Ok(read) = std::fs::read_dir(folder) else { return HashSet::new() };
    let mut names = HashSet::new();
    for page in read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")))
    {
        // An evicted page is waited for; one that cannot be read is not counted.
        let Ok(text) = read_to_string_with_materialize_wait(&page, INTERACTIVE_DEADLINE) else { continue };
        for r in extract_md_references(&text) {
            let is_embed = matches!(
                r.syntax,
                RefSyntax::WikilinkStemEmbed | RefSyntax::WikilinkPathEmbed | RefSyntax::WikilinkAliasedEmbed { .. }
            );
            if is_embed {
                let target = moss_core::resolve::embeds::split_embed_target(&r.text).0;
                names.insert(target.rsplit('/').next().unwrap_or(target).to_string());
            }
        }
    }
    names
}

/// The stub a file would get, as `(stub path, file name)`, for every accepted
/// file that has neither a page of that name nor a page embedding it.
fn missing_stubs(
    folder: &Path,
    entry: &DerivationEntry,
    only: Option<&Path>,
    embedded: &HashSet<String>,
) -> Vec<(PathBuf, String)> {
    accepted_files(folder, entry)
        .into_iter()
        .filter(|f| only.is_none_or(|o| o == f))
        .filter_map(|file| {
            let name = file.file_name()?.to_str()?.to_string();
            let stub = file.with_extension("md");
            let wanted = embeddable(&name) && !stub.exists() && !embedded.contains(&name);
            wanted.then_some((stub, name))
        })
        .collect()
}

/// How many stubs [`write_stubs`] would write now.
pub fn stubs_missing(folder: &Path, entry: &DerivationEntry) -> usize {
    missing_stubs(folder, entry, None, &embedded_names(folder)).len()
}

/// The stub's text: the plugin's layout, one embed per line of it.
fn stub_text(entry: &DerivationEntry, file_name: &str) -> String {
    let embeds: Vec<String> = entry
        .contribution
        .stub
        .iter()
        .map(|line| format!("![[{file_name}{line}]]"))
        .collect();
    format!("---\ninferred: [title, description, date]\n---\n{}\n", embeds.join("\n\n"))
}

/// Create `stub` with `text`, unless something is already there. Exclusive on
/// purpose: the existence check in [`missing_stubs`] and the write are two
/// steps, and a page the author made in between must survive.
fn create_stub(stub: &Path, text: &str) -> std::io::Result<bool> {
    use std::io::Write;
    // allow:raw_write a new page in the author's own folder, created exclusively -- never .moss/build.nosync/
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(stub);
    match file {
        Ok(mut file) => file.write_all(text.as_bytes()).map(|()| true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
}

fn write_all(missing: Vec<(PathBuf, String)>, entry: &DerivationEntry) -> Vec<PathBuf> {
    missing
        .into_iter()
        .filter_map(|(stub, name)| match create_stub(&stub, &stub_text(entry, &name)) {
            Ok(true) => Some(stub),
            Ok(false) => None,
            Err(e) => {
                log::warn!(target: "derive", "could not write {}: {e}", stub.display());
                None
            }
        })
        .collect()
}

/// Write the stubs `folder` is missing. Idempotent; an existing file is never
/// touched. Returns the files written.
pub fn write_stubs(folder: &Path, entry: &DerivationEntry) -> Vec<PathBuf> {
    write_all(missing_stubs(folder, entry, None, &embedded_names(folder)), entry)
}

/// The watcher saw these files appear. For each one inside a folder the author
/// chose to show, write its stub (and only its). Returns the files written.
pub fn stub_arrivals(site: &MossPaths, registry: &DerivationRegistry, created: &[PathBuf]) -> Vec<PathBuf> {
    let decided = decisions(site);
    let mut written = Vec::new();
    // Each folder's pages are read once, however many files arrived in it.
    let mut embedded: HashMap<&Path, HashSet<String>> = HashMap::new();
    for file in created {
        let (Some(folder), Some(ext)) = (file.parent(), file.extension().and_then(|e| e.to_str())) else {
            continue;
        };
        let Some(entry) = registry.for_extension(ext) else { continue };
        let shown = folder_key(site, folder)
            .and_then(|key| decided.get(&key))
            .is_some_and(|d| matches!(d, Decision::Show(ids) if ids.contains(&entry.derivation_id)));
        if shown && file.is_file() {
            let names = embedded.entry(folder).or_insert_with(|| embedded_names(folder));
            written.extend(write_all(missing_stubs(folder, entry, Some(file), names), entry));
        }
    }
    written
}

#[cfg(test)]
#[path = "derive_stubs_tests.rs"]
mod derive_stubs_tests;
