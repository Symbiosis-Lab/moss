//! Publishing by writing the built site into a destination moss writes
//! itself, file by file, rather than handing it to moss hosting or to a
//! deploy plugin.
//!
//! The destination is a folder a web server already serves, often shared
//! with files moss did not put there (`.htaccess`, `cgi-bin`, `.well-known`,
//! another app). Two rules follow, and [`publish`] owns both:
//!
//! - **A visitor never meets a missing file.** Content-addressed assets go
//!   first, other files next, HTML last, and deletions only after every put
//!   has landed. Each put replaces its file in one step. Mid-publish a visitor
//!   can see one page new and another still old, never a page whose
//!   stylesheet is gone.
//! - **moss deletes only what it published.** The destination holds a record
//!   ([`plan::RECORD_NAME`]) naming the site that published there and every
//!   path it put. Only those paths are ever deleted, a destination with no
//!   record gets no deletions, and a destination holding another site's
//!   record is refused. The record also stands in for a remote hash, and it
//!   is what the mass-removal gate judges against.

pub mod destination;
#[cfg(test)]
mod folder;
pub mod plan;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub use destination::{Body, Destination};
use plan::{Plan, Record, RECORD_NAME};

use crate::deploy::progress::{DeploySink, DeployStage, UploadProgressState};

/// What a publish changed on the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Published {
    pub uploaded: u32,
    pub removed: u32,
}

/// Publish `files` (output path → manifest entry), read from `site_dir`, into
/// `dest` as the site `owner`, a stable identity of the site that is the same
/// on every computer publishing it.
///
/// `allow_mass_removal` lifts the gate that refuses a publish deleting most
/// of what the record says is live, the signature of an empty or stale build.
pub async fn publish(
    dest: &dyn Destination,
    owner: &str,
    site_dir: &Path,
    files: &HashMap<String, String>,
    sink: &Arc<dyn DeploySink>,
    allow_mass_removal: bool,
) -> Result<Published, String> {
    if let Some(bad) = files.keys().find(|p| !plan::is_safe_path(p)) {
        return Err(format!("the site has a file moss cannot publish here: {bad:?}"));
    }
    let mut on_dest = match dest.get(RECORD_NAME).await? {
        Some(bytes) => Some(Record::parse(&bytes)?),
        None => None,
    };
    if let Some(other) = on_dest.as_ref().map(Record::owner).filter(|o| *o != owner) {
        return Err(format!(
            "this destination holds files another site published ({other:?}); publish each site to its own folder"
        ));
    }
    let plan = Plan::new(owner, files, on_dest.as_ref());
    if let Some(live) = &on_dest {
        super::mass_remove::check_mass_removal(
            files.len(),
            plan.puts.len(),
            &plan.deletes,
            Some(live.files().len()),
            allow_mass_removal,
        )?;
    }
    let puts = local_files(site_dir, files, &plan.puts)?;

    put_record(dest, &mut on_dest, &plan.before_puts).await?;
    put_files(dest, puts, plan.deletes.len() as u32, sink).await?;
    put_record(dest, &mut on_dest, &plan.after_puts).await?;

    if !plan.deletes.is_empty() {
        sink.stage(DeployStage::Committing, 0, plan.deletes.len() as u32, &format!("Removing {} files", plan.deletes.len()));
    }
    for path in &plan.deletes {
        dest.delete(path).await?;
    }
    put_record(dest, &mut on_dest, &plan.after_deletes).await?;

    Ok(Published { uploaded: plan.puts.len() as u32, removed: plan.deletes.len() as u32 })
}

/// One file to put, read from the site before anything is written.
struct Put<'a> {
    path: &'a str,
    body: Body,
    size: u64,
}

/// Everything the puts will send, checked first: a link out of the site or an
/// unknown entry stops the publish before the destination is touched.
fn local_files<'a>(site_dir: &Path, files: &HashMap<String, String>, paths: &'a [String]) -> Result<Vec<Put<'a>>, String> {
    use crate::types::content::{parse_entry, MODE_FILE, MODE_SYMLINK};
    paths
        .iter()
        .map(|path| {
            let local = site_dir.join(path);
            match parse_entry(&files[path]).0 {
                MODE_FILE => {
                    let size = std::fs::metadata(&local).map_err(|e| format!("read {path}: {e}"))?.len();
                    Ok(Put { path, body: Body::File(local), size })
                }
                MODE_SYMLINK => {
                    let target = std::fs::read_link(&local).map_err(|e| format!("read link {path}: {e}"))?;
                    let target = target.to_string_lossy().into_owned();
                    if !plan::link_stays_inside(path, &target) {
                        return Err(format!("the link {path} points outside the site ({target})"));
                    }
                    Ok(Put { path, body: Body::Link(target), size: 0 })
                }
                mode => Err(format!("unknown manifest mode {mode} for {path}")),
            }
        })
        .collect()
}

async fn put_files(dest: &dyn Destination, puts: Vec<Put<'_>>, removing: u32, sink: &Arc<dyn DeploySink>) -> Result<(), String> {
    let progress = UploadProgressState::new(puts.iter().map(|p| p.size).sum(), puts.len() as u32, removing);
    for put in puts {
        progress.begin_file(put.path, put.size);
        dest.put_atomic(put.path, put.body).await?;
        progress.credit_bytes(put.size);
        progress.finish_file(put.path);
        sink.upload_sample(&progress);
    }
    Ok(())
}

/// Write `record` unless the destination already holds exactly it.
async fn put_record(dest: &dyn Destination, on_dest: &mut Option<Record>, record: &Record) -> Result<(), String> {
    if on_dest.as_ref() == Some(record) {
        return Ok(());
    }
    dest.put_atomic(RECORD_NAME, Body::Bytes(record.to_bytes())).await?;
    *on_dest = Some(record.clone());
    Ok(())
}

#[cfg(test)]
#[path = "direct/publish_tests.rs"]
mod publish_tests;
