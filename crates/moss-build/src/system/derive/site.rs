//! Derived text in a build: the files `![[file.ext#fragment]]` embeds, read by
//! the plugin that declared the extension.
//!
//! Two ways to meet a file nobody has read yet, chosen by [`DeriveMode`], which
//! follows whether a watcher is registered for the folder (something is there
//! to receive the rebuild), as video conversion does:
//! `Await` reads it before rendering (a build whose output is published, or a
//! process that exits when the build ends); `Adopt` renders a placeholder now,
//! reads in the background, and asks for a rebuild when the text exists. Both
//! resolve through the same content-addressed cache, so the rebuild is a hit.
//!
//! A read that failed is remembered by the cache layer (`derive::remembered`),
//! by the same key the cache uses, only for these background reads. Failures
//! are not cached on disk (the cause is usually the environment's), but without
//! that memory the rebuild would find a miss again, start the same read, and
//! loop. A build that waits for text never consults it: a publish retries.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use moss_core::content_graph::ContentGraph;
use moss_core::resolve::embeds::derived::{extension_of, DerivedFragment, DerivedTable};
use moss_core::resolve::md_extract::{extract_md_references, RefSyntax};

use crate::build::ports::reporter::BuildReporter;
use crate::build::progress::PipelineEvent;
use crate::moss_paths::MossPaths;
use crate::system::derive::priority::Schedule;
use crate::system::derive::registry::{DerivationEntry, DerivationRegistry};
use crate::system::derive::{self as derive, batch::DeriveJob, DeriveFailure, DeriveOutcome};

/// What a build does about derived text it does not have yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeriveMode {
    /// Read it first. Nothing rendered is ever pending.
    Await,
    /// Render a placeholder, read in the background, rebuild when done.
    Adopt,
}

/// One file some derivation must read.
#[derive(Debug, Clone)]
pub struct Demand {
    pub entry: DerivationEntry,
    /// Site-relative, `/`-separated.
    pub path: String,
    /// The pages that embed it, site-relative.
    pub pages: Vec<String>,
}

enum Slot {
    Outcome(DeriveOutcome),
    /// An earlier key's result for these exact bytes, shown while the read
    /// under the current key runs.
    Previous(DeriveOutcome),
    Failed(String),
    Pending,
}

pub struct DerivedSite {
    registry: DerivationRegistry,
    table: DerivedTable,
    paths: MossPaths,
    root: PathBuf,
    folder: String,
    mode: DeriveMode,
    slots: Mutex<HashMap<String, Arc<Slot>>>,
    background: Mutex<Vec<Demand>>,
    /// Files whose block this build rendered as a placeholder or a failure
    /// note, recorded as the reader hands them out.
    unready: Mutex<BTreeMap<String, Unready>>,
}

/// Why a rendered block shows no text.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Unready {
    Pending,
    Failed(String),
}

/// The blocks a build rendered without text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnreadyReport {
    /// Files still being read; a publish waits for zero.
    pub pending: usize,
    /// `(file, message)` of each file whose read failed.
    pub failed: Vec<(String, String)>,
}

impl DerivedSite {
    /// The derivations of the plugins this folder carries, or `None` when
    /// plugins are off or none declares one. Admission is the loader's:
    /// a refused plugin is not in the list.
    pub fn load(plugins_on: bool, folder: &Path) -> Option<Arc<Self>> {
        if !plugins_on {
            return None;
        }
        let folder_str = folder.to_string_lossy().to_string();
        let plugins = crate::plugins::discovery::discover_installed_plugins(&folder_str).ok()?;
        let registry = DerivationRegistry::from_plugins(&plugins);
        if registry.is_empty() {
            return None;
        }
        for note in registry.diagnostics() {
            log::warn!(target: "derive", "{note}");
        }
        // A registered watcher receives the rebuild a background read asks
        // for. Without one (a one-shot build, a deploy) nobody would, so the
        // build waits for the text instead.
        let mode = if crate::ops::watch::worker::get(&folder_str).is_some() { DeriveMode::Adopt } else { DeriveMode::Await };
        let mut table = DerivedTable::new();
        for plugin in &plugins {
            let derivations = plugin.manifest.contributes.as_ref().and_then(|c| c.derivations.as_ref());
            for contribution in derivations.into_iter().flat_map(|d| d.values()) {
                for ext in &contribution.accepts {
                    // The registry decides which plugin owns an extension; only
                    // its winner's fragments are claimable.
                    if let Some(entry) = registry.for_extension(ext) {
                        for fragment in entry.contribution.fragments.keys() {
                            table.declare(ext, fragment);
                        }
                    }
                }
            }
        }
        Some(Arc::new(Self {
            registry,
            table,
            paths: MossPaths::new(folder),
            root: folder.to_path_buf(),
            folder: folder_str,
            mode,
            slots: Mutex::default(),
            background: Mutex::default(),
            unready: Mutex::default(),
        }))
    }

    pub fn table(&self) -> &DerivedTable {
        &self.table
    }

    /// The reader resolution takes: the current state of one fragment.
    pub fn read(&self, path: &str, fragment: &str) -> DerivedFragment {
        let state = self.state_of(path, fragment);
        let unready = match &state {
            DerivedFragment::Pending => Some(Unready::Pending),
            _ if self.is_previous(path) => Some(Unready::Pending),
            DerivedFragment::Failed { message } => Some(Unready::Failed(message.clone())),
            DerivedFragment::Ready { .. } | DerivedFragment::Skipped { .. } => None,
        };
        if let (Some(unready), Ok(mut map)) = (unready, self.unready.lock()) {
            map.insert(path.to_string(), unready);
        }
        state
    }

    /// Whether `path` shows an earlier key's result while its re-read runs.
    pub fn is_previous(&self, path: &str) -> bool {
        self.slots.lock().ok().and_then(|s| s.get(path).cloned()).is_some_and(|slot| matches!(*slot, Slot::Previous(_)))
    }

    /// What the blocks rendered so far show: how many files render as a
    /// placeholder, and which failed. Counted from what the reader handed to
    /// resolution, not from a scan of what pages might ask for.
    pub fn unready(&self) -> UnreadyReport {
        let map = self.unready.lock().map(|m| m.clone()).unwrap_or_default();
        let mut report = UnreadyReport::default();
        for (path, unready) in map {
            match unready {
                Unready::Pending => report.pending += 1,
                Unready::Failed(message) => report.failed.push((path, message)),
            }
        }
        report
    }

    fn state_of(&self, path: &str, fragment: &str) -> DerivedFragment {
        match &*self.slot(path) {
            Slot::Pending => DerivedFragment::Pending,
            Slot::Failed(message) => DerivedFragment::Failed { message: message.clone() },
            Slot::Outcome(DeriveOutcome::Skip { message, .. }) | Slot::Previous(DeriveOutcome::Skip { message, .. }) => DerivedFragment::Skipped { message: message.clone() },
            Slot::Outcome(DeriveOutcome::Derived { fragments, facts })
            | Slot::Previous(DeriveOutcome::Derived { fragments, facts }) => match fragments.get(fragment) {
                Some(markdown) => DerivedFragment::Ready { markdown: markdown.clone(), facts: facts.clone() },
                None => DerivedFragment::Failed { message: format!("no `{fragment}` fragment was produced") },
            },
        }
    }

    fn slot(&self, path: &str) -> Arc<Slot> {
        if let Some(slot) = self.slots.lock().ok().and_then(|s| s.get(path).cloned()) {
            return slot;
        }
        let slot = Arc::new(self.compute(path));
        if let Ok(mut slots) = self.slots.lock() {
            slots.insert(path.to_string(), slot.clone());
        }
        slot
    }

    fn compute(&self, path: &str) -> Slot {
        let Some(entry) = self.registry.for_extension(&extension_of(path)) else {
            return Slot::Failed("no plugin reads this kind of file".into());
        };
        let input = self.root.join(path);
        if let Some(outcome) = derive::lookup_cached(&self.paths, entry, &input) {
            return Slot::Outcome(outcome);
        }
        match self.mode {
            DeriveMode::Adopt => match derive::remembered(entry, &input) {
                Some(Ok(outcome)) => Slot::Outcome(outcome),
                Some(Err(failure)) => Slot::Failed(failure.message),
                None => derive::lookup_previous(&self.paths, entry, &input).map_or(Slot::Pending, Slot::Previous),
            },
            DeriveMode::Await => {
                let result = derive::binary::ensure_binary(entry, None)
                    .and_then(|_| derive::derive_cached(&self.paths, entry, &input));
                match result {
                    Ok(outcome) => Slot::Outcome(outcome),
                    Err(failure) => {
                        report_failure(path, &failure);
                        Slot::Failed(failure.message)
                    }
                }
            }
        }
    }

    /// Every file the site's pages ask a derivation to read: `pages` are
    /// site-relative markdown paths.
    pub fn demands(&self, pages: &[String], graph: &ContentGraph) -> Vec<Demand> {
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut demands: Vec<Demand> = Vec::new();
        for page in pages {
            // The build's own non-waiting probe: a page still in the cloud is
            // skipped here and deferred by the render, not downloaded.
            let Ok(Some(text)) = crate::build::cloud_readiness::probe_input_to_string(&self.root.join(page)) else {
                continue;
            };
            if !text.contains("![") {
                continue;
            }
            for reference in extract_md_references(&text) {
                if !matches!(
                    reference.syntax,
                    RefSyntax::WikilinkStemEmbed | RefSyntax::WikilinkPathEmbed | RefSyntax::WikilinkAliasedEmbed { .. }
                ) {
                    continue;
                }
                let (file, Some(fragment)) = moss_core::resolve::embeds::split_embed_target(&reference.text) else { continue };
                let Some(path) = graph.resolve_path(file, page) else { continue };
                if !self.table.claims(&extension_of(&path), fragment) {
                    continue;
                }
                if let Some(&at) = index.get(&path) {
                    if !demands[at].pages.contains(page) {
                        demands[at].pages.push(page.clone());
                    }
                } else if let Some(entry) = self.registry.for_extension(&extension_of(&path)) {
                    index.insert(path.clone(), demands.len());
                    demands.push(Demand { entry: entry.clone(), path, pages: vec![page.clone()] });
                }
            }
        }
        demands
    }

    /// Get the demanded files ready for a render. `Await` reads every miss now
    /// and reports `Reading {noun} · n of m`; `Adopt` queues them for
    /// [`Self::take_background_job`]. Only a feed for parallelism: a file not
    /// demanded here is still read when the reader meets it.
    pub fn prepare(&self, demands: Vec<Demand>, reporter: Option<&dyn BuildReporter>) {
        let adopting = self.mode == DeriveMode::Adopt;
        let misses: Vec<Demand> = demands
            .into_iter()
            .filter(|d| {
                let input = self.root.join(&d.path);
                derive::lookup_cached(&self.paths, &d.entry, &input).is_none()
                    && !(adopting && derive::remembered(&d.entry, &input).is_some())
            })
            .collect();
        if misses.is_empty() {
            return;
        }
        if adopting {
            if let Ok(mut queue) = self.background.lock() {
                *queue = misses;
            }
            return;
        }
        let results = read_all(&self.paths, &self.root, &misses, reporter, &|| false, false);
        if let Ok(mut slots) = self.slots.lock() {
            for (path, result) in results {
                let slot = match result {
                    Ok(outcome) => Slot::Outcome(outcome),
                    Err(failure) => Slot::Failed(failure.message),
                };
                slots.insert(path, Arc::new(slot));
            }
        }
    }

    /// The reads `Adopt` deferred, as work for a background worker bound to
    /// the render `render` of `paths`' folder. `None` when everything was cached.
    pub fn take_background_job(&self, paths: &MossPaths, render: Option<u64>) -> Option<BackgroundDerive> {
        let jobs = std::mem::take(&mut *self.background.lock().ok()?);
        if jobs.is_empty() {
            return None;
        }
        Some(BackgroundDerive {
            paths: MossPaths::new(&self.root),
            lifecycle_paths: MossPaths::from_moss_dir(paths.root().to_path_buf()),
            root: self.root.clone(),
            folder: self.folder.clone(),
            jobs,
            render,
        })
    }
}

fn report_failure(path: &str, failure: &DeriveFailure) {
    crate::build::cli_output::log_warn_problem!(target: "derive", "could not read {path}: {}", failure.message);
}

/// The noun to show: the plugin's when every file shares one, else "files".
fn noun_of(jobs: &[Demand]) -> String {
    let nouns: HashSet<&str> = jobs.iter().map(|j| j.entry.contribution.noun.other.as_str()).collect();
    match nouns.len() {
        1 => nouns.into_iter().next().unwrap_or("files").to_string(),
        _ => "files".to_string(),
    }
}

fn report_progress(reporter: Option<&dyn BuildReporter>, noun: &str, done: usize, total: usize) {
    let message = format!("Reading {noun} \u{b7} {done} of {total}");
    match reporter {
        Some(reporter) => reporter.report(&PipelineEvent::BackgroundProgress {
            task: "derive".to_string(),
            current: done as u32,
            total: total as u32,
            message,
            completed: done == total,
            advisories: vec![],
        }),
        // No listener (a command-line build): say it where the build prints.
        None if done > 0 => crate::cli_eprintln!("{message}"),
        None => {}
    }
}

/// Read `jobs` through the cache, a budget at a time, checking `stop` between
/// groups. Each failure is reported once, here. Returns `(path, result)` for
/// every job attempted; `remember` keeps a result the cache cannot hold for the
/// rebuild that follows (see `derive::remembered`).
fn read_all(
    paths: &MossPaths,
    root: &Path,
    jobs: &[Demand],
    reporter: Option<&dyn BuildReporter>,
    stop: &(dyn Fn() -> bool + Sync),
    remember: bool,
) -> Vec<(String, Result<DeriveOutcome, DeriveFailure>)> {
    let mut installed = HashSet::new();
    let mut unavailable: HashMap<(String, String), DeriveFailure> = HashMap::new();
    for job in jobs {
        let id = (job.entry.plugin_id.clone(), job.entry.derivation_id.clone());
        if installed.insert(id.clone()) {
            if let Err(failure) = derive::binary::ensure_binary(&job.entry, None) {
                unavailable.insert(id, failure);
            }
        }
    }
    let mut results = Vec::new();
    let (runnable, blocked): (Vec<&Demand>, Vec<&Demand>) = jobs
        .iter()
        .partition(|j| !unavailable.contains_key(&(j.entry.plugin_id.clone(), j.entry.derivation_id.clone())));
    for job in blocked {
        let id = (job.entry.plugin_id.clone(), job.entry.derivation_id.clone());
        if let Some(failure) = unavailable.get(&id) {
            report_failure(&job.path, failure);
            if remember {
                derive::remember_failure(&job.entry, &root.join(&job.path), failure);
            }
            results.push((job.path.clone(), Err(failure.clone())));
        }
    }

    let noun = noun_of(jobs);
    let total = runnable.len();
    report_progress(reporter, &noun, 0, total);
    let batch: Vec<DeriveJob<'_>> =
        runnable.iter().map(|d| DeriveJob { entry: &d.entry, input: root.join(&d.path) }).collect();
    // New or changed bytes have no earlier result to show; a read repeated
    // for a changed plugin does.
    let arrived: Vec<bool> = runnable.iter().map(|d| derive::lookup_previous(paths, &d.entry, &root.join(&d.path)).is_none()).collect();
    let schedule = Arc::new(Schedule::new(root, runnable.iter().map(|d| d.pages.clone()).collect(), arrived));
    let derived = derive::derive_batch_by(paths, &batch, remember, stop, Arc::new(move |job| schedule.tier(job)), |done, total| {
        report_progress(reporter, &noun, done, total)
    });
    for (demand, result) in runnable.iter().zip(derived) {
        let Some(result) = result else { continue };
        if let Err(failure) = &result {
            report_failure(&demand.path, failure);
        }
        results.push((demand.path.clone(), result));
    }
    results
}

/// The deferred reads of one `Adopt` build, owned by a background worker.
pub struct BackgroundDerive {
    paths: MossPaths,
    /// The folder's paths as the build lifecycle knows them.
    lifecycle_paths: MossPaths,
    root: PathBuf,
    folder: String,
    jobs: Vec<Demand>,
    /// The render this read belongs to.
    render: Option<u64>,
}

impl BackgroundDerive {
    /// Read everything, stopping early if a newer render of the folder was
    /// admitted (it queues whatever is still missing) or `cancel` fires. When
    /// the reads finish and this is still the newest render, enqueue a full
    /// rebuild: it renders from the cache.
    pub fn run(self, reporter: Option<&dyn BuildReporter>, cancel: &std::sync::atomic::AtomicBool) {
        let stop = || cancel.load(Ordering::SeqCst) || !crate::build::lifecycle::is_latest_render(&self.lifecycle_paths, self.render);
        read_all(&self.paths, &self.root, &self.jobs, reporter, &stop, true);
        if stop() {
            // A stopped read sends no last count, and a listener would show it
            // as still reading. The jobs in flight have drained by now.
            if let Some(reporter) = reporter {
                reporter.report(&PipelineEvent::BackgroundProgress {
                    task: "derive".to_string(),
                    current: 0,
                    total: 0,
                    message: String::new(),
                    completed: true,
                    advisories: vec![],
                });
            }
            return;
        }
        let Some(worker) = crate::ops::watch::worker::get(&self.folder) else { return };
        log::info!(target: "derive", "derived text is ready for '{}' — enqueueing a full re-render", self.folder);
        worker.enqueue(crate::ops::watch::worker::RebuildRequest::full());
    }
}

#[cfg(test)]
#[path = "site_tests.rs"]
mod site_tests;
