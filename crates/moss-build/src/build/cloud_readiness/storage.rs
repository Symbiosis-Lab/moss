//! Complete structural reads, isolated from abandonable consumer waits.
use std::{fmt, io};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use super::Settled;

/// The storage operation that failed, kept separate from its human-readable
/// message so callers can classify the exact operation without parsing text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageOperation {
    RootMetadata,
    ReadDirOpen,
    ReadDirNext,
    EntryMetadata,
    /// `WalkDir` reports the affected path but does not say which syscall
    /// failed; keep that uncertainty explicit.
    WalkEntry,
    CacheRemove,
    CacheRead,
    CacheMkdir,
    CacheWrite,
    CachePublish,
}

/// An I/O failure with the path and operation still attached.
#[derive(Debug)]
pub struct StorageFailure {
    path: Option<PathBuf>,
    operation: StorageOperation,
    source: StorageSource,
}

#[derive(Debug)]
enum StorageSource {
    Io(io::Error),
    WalkDir(walkdir::Error),
}

impl fmt::Display for StorageSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageSource::Io(error) => write!(f, "{error}"),
            StorageSource::WalkDir(error) => write!(f, "{error}"),
        }
    }
}

impl StorageFailure {
    pub fn new(path: Option<PathBuf>, operation: StorageOperation, source: io::Error) -> Self {
        Self { path, operation, source: StorageSource::Io(source) }
    }

    /// Consume a `WalkDir` error without guessing whether its internal failure
    /// came from enumeration, metadata, or another entry operation.
    pub fn from_walkdir(error: walkdir::Error) -> Self {
        Self::from_walkdir_operation(error, StorageOperation::WalkEntry)
    }

    pub fn from_walkdir_operation(error: walkdir::Error, operation: StorageOperation) -> Self {
        let path = error.path().map(Path::to_path_buf);
        Self { path, operation, source: StorageSource::WalkDir(error) }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn operation(&self) -> StorageOperation {
        self.operation
    }

    pub fn source(&self) -> &(dyn std::error::Error + 'static) {
        match &self.source {
            StorageSource::Io(error) => error,
            StorageSource::WalkDir(error) => error,
        }
    }

    pub fn io_error(&self) -> Option<&io::Error> {
        match &self.source {
            StorageSource::Io(error) => Some(error),
            StorageSource::WalkDir(error) => error.io_error(),
        }
    }

    pub fn raw_os_error(&self) -> Option<i32> {
        self.io_error().and_then(io::Error::raw_os_error)
    }
}

impl fmt::Display for StorageFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operation = match self.operation {
            StorageOperation::RootMetadata => "reading root metadata",
            StorageOperation::ReadDirOpen => "opening directory listing",
            StorageOperation::ReadDirNext => "reading next directory entry",
            StorageOperation::EntryMetadata => "reading directory-entry metadata",
            StorageOperation::WalkEntry => "walking directory entry",
            StorageOperation::CacheRemove => "removing invalid cache record",
            StorageOperation::CacheRead => "reading current cache record",
            StorageOperation::CacheMkdir => "creating cache shard",
            StorageOperation::CacheWrite => "writing owned cache candidate",
            StorageOperation::CachePublish => "publishing verified cache candidate",
        };
        match &self.path {
            Some(path) => write!(f, "{operation} at '{}': {}", path.display(), self.source),
            None => write!(f, "{operation}: {}", self.source),
        }
    }
}

impl std::error::Error for StorageFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source())
    }
}

/// Retry only the same owned operation after a managed macOS fail-fast refusal.
/// The operation kind remains diagnostic; it never permits a weaker probe to
/// certify the required directory walk or atomic publication.
pub fn recoverable_storage_failure(failure: &StorageFailure, managed_cloud_path: bool, macos_fail_fast: bool) -> bool {
    managed_cloud_path && macos_fail_fast && failure.raw_os_error() == Some(libc::EDEADLK)
}

/// Complete one directory listing. A successful return proves that the
/// iterator reached EOF; an error after any prefix discards that prefix.
pub fn collect_directory_entries<T, I>(
    path: &Path,
    open: impl FnOnce(&Path) -> io::Result<I>,
) -> Result<Vec<T>, StorageFailure>
where
    I: Iterator<Item = io::Result<T>>,
{
    #[cfg(test)]
    test_probe(path, StorageOperation::ReadDirOpen)?;
    let mut entries = open(path).map_err(|error| {
        StorageFailure::new(Some(path.to_path_buf()), StorageOperation::ReadDirOpen, error)
    })?;
    let mut complete = Vec::new();
    loop {
        #[cfg(test)]
        if !complete.is_empty() { test_probe(path, StorageOperation::ReadDirNext)?; }
        let Some(entry) = entries.next() else { break };
        complete.push(entry.map_err(|error| {
            StorageFailure::new(Some(path.to_path_buf()), StorageOperation::ReadDirNext, error)
        })?);
    }
    Ok(complete)
}


/// What one stat walk of the source tree found. One pass feeds all three
/// verdicts: `dataless` → the cloud readers, `files`/`offline_prefixes` →
/// the drift compare, `root_unreadable`/`deadline_blown` → the unavailable
/// counter.
#[derive(Debug, Default, Clone)]
pub(crate) struct SweepSnapshot {
    /// Source files still in the cloud — the download-request set
    /// (the sweep's materialization filter, not the drift filter).
    pub dataless: Vec<PathBuf>,
    /// Drift-eligible files on disk, keyed the manifest's way.
    pub files: Vec<crate::build::watch::drift::WalkedFile>,
    /// Folder-relative keys of directories the walk could not enumerate for
    /// cloud reasons — offline subtrees, exempt from the deletion check.
    pub offline_prefixes: Vec<String>,
    /// The vault root itself would not enumerate.
    pub root_unreadable: bool,
    /// The pass ran out of deadline mid-walk. Partial results are still
    /// usable: `dataless` feeds downloads and `files` feeds a
    /// modification-only compare; what a partial pass must NOT do is claim
    /// full-tree authority (deletions, pending-set replacement).
    pub deadline_blown: bool,
    /// Files the pass looked at, and how many the watcher may watch at all
    /// (both excluding `.moss/`). Seen-but-none-watchable is a folder moss is
    /// blind to; `supervision::note_blind_folder` is what says so.
    pub files_seen: usize,
    pub files_watchable: usize,
    /// Where to pick up next pass: the last file processed before the blow.
    /// `None` when the walk reached the end. Without this, a filesystem
    /// where every pass blows at the same budget point never checks the deep
    /// half of the tree for the life of the session (the incident's file was
    /// 8 directories deep).
    pub resume: Option<String>,
}

#[derive(Debug)]
pub(crate) enum StorageValue {
    WatchTargets(Vec<crate::build::watch::scope::WatchTarget>),
    Sweep(SweepSnapshot),
    Directory { entries: Vec<(std::fs::DirEntry, std::fs::Metadata)>, hidden: u32 },
    Published(u64),
    Walk {
        entries: Vec<(walkdir::DirEntry, Option<std::fs::Metadata>)>,
        nested_boundaries: Vec<PathBuf>,
    },
}

#[derive(Debug)]
pub struct Unavailable {
    pub failure: Arc<StorageFailure>,
    pub pending: Option<Settled>,
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.failure)
    }
}

pub(crate) type Operation = Arc<dyn Fn() -> Result<StorageValue, StorageFailure> + Send + Sync>;

pub(crate) enum PublicationOutcome {
    Completed,
    PendingOptionalCache,
    Fatal(Arc<StorageFailure>),
}

/// Optional writes prove availability by completing their owned transaction.
/// Deadline and admission refusal retain the caller's complete local object;
/// neither can certify a shared publication or reclaim a native slot.
pub(crate) fn await_publication(key: OperationKey, attempt: Operation) -> PublicationOutcome {
    await_publication_revision(key, attempt, None, 0)
}

pub(crate) fn await_publication_revision(key: OperationKey, attempt: Operation, revision: Option<Arc<std::sync::atomic::AtomicU64>>, expected: u64) -> PublicationOutcome {
    await_publication_in(&super::PREFETCH, key, attempt, revision, expected)
}

pub(crate) fn await_publication_in(pool: &crate::build::cloud_prefetch::Prefetcher, key: OperationKey, attempt: Operation, revision: Option<Arc<std::sync::atomic::AtomicU64>>, expected: u64) -> PublicationOutcome {
    let path = key.path.clone();
    let Some(task) = pool.cache_publication_revision(key, attempt, revision) else {
        return PublicationOutcome::Fatal(Arc::new(StorageFailure::new(Some(path), StorageOperation::CachePublish,
            io::Error::new(io::ErrorKind::WouldBlock, "optional cache publication skipped: admission capacity exhausted"))));
    };
    let started = Instant::now();
    loop {
        if let Some(result) = &task.state.lock().unwrap_or_else(|e| e.into_inner()).result {
            return match result {
                Ok(value) if matches!(value.as_ref(), StorageValue::Published(version) if *version >= expected) => PublicationOutcome::Completed,
                Ok(_) => PublicationOutcome::PendingOptionalCache,
                Err(failure) => PublicationOutcome::Fatal(failure.clone()),
            };
        }
        if started.elapsed() >= Duration::from_millis(20) { return PublicationOutcome::PendingOptionalCache; }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The complete filtered listing can be reconstructed from its lightweight
/// policy key when native capacity becomes available. No file I/O is done
/// while constructing this owned request.
pub(crate) fn directory_operation(path: &Path, project: &Path, show_internal: bool) -> Operation {
    let root = path.to_path_buf();
    let parent_policy = path.strip_prefix(project).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
    std::sync::Arc::new(move || {
            use crate::build::cloud_readiness::{StorageFailure, StorageOperation};
            let raw = crate::build::cloud_readiness::collect_directory_entries(&root, |p| std::fs::read_dir(p))?;
            let mut entries = Vec::new();
            let mut hidden = 0;
            for entry in raw {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(reason) = crate::build::scan::classify::is_hidden_reason(&name, &parent_policy, show_internal) {
                    hidden += reason.is_curated() as u32;
                    continue;
                }
                #[cfg(test)]
                crate::build::cloud_readiness::storage::test_probe(&entry.path(), StorageOperation::EntryMetadata)?;
                let metadata = entry.metadata().map_err(|e| StorageFailure::new(Some(entry.path()), StorageOperation::EntryMetadata, e))?;
                entries.push((entry, metadata));
            }
            Ok(crate::build::cloud_readiness::storage::StorageValue::Directory { entries, hidden })
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum OperationPolicy {
    SourceWalk,
    WatchTargets,
    SweepWalk { budget: Option<Duration>, resume: Option<String> },
    CachePublication { root: PathBuf },
    EditorDirectory { project: PathBuf, show_internal: bool },
}

pub(crate) fn managed_context(path: &Path) -> bool {
    let managed = cfg!(target_os = "macos") && crate::build::cloud_provider::recovery_context(path);
    #[cfg(test)]
    let managed = managed || test_faults().roots.contains_key(path)
        || crate::build::icloud::pretend::refusal_below(&path.join("publication-probe")).is_some();
    managed
}

pub(crate) fn await_operation(
    path: &Path,
    policy: OperationPolicy,
    deadline: Duration,
    cancel: &dyn Fn() -> bool,
    on_wait: &dyn Fn(),
    attempt: Operation,
) -> Result<Arc<StorageValue>, Unavailable> {
    let managed_context = managed_context(path);
    #[cfg(test)]
    let deadline = test_faults().roots.get(path).copied().unwrap_or(deadline);
    if managed_context && !crate::platform::dataless_reads_fail_fast() {
        let operation = match policy { OperationPolicy::EditorDirectory { .. } => StorageOperation::ReadDirOpen, OperationPolicy::SourceWalk | OperationPolicy::SweepWalk { .. } => StorageOperation::WalkEntry, OperationPolicy::WatchTargets => StorageOperation::ReadDirOpen, OperationPolicy::CachePublication { .. } => StorageOperation::CachePublish };
        let waiting = Arc::new(StorageFailure::new(Some(path.to_path_buf()), operation,
            io::Error::new(io::ErrorKind::WouldBlock, "waiting for the cloud directory operation")));
        return wait_for_operation(&super::PREFETCH, path, policy, deadline, cancel, on_wait, attempt, waiting);
    }
    await_operation_in(&super::PREFETCH, path, policy, managed_context, deadline, cancel, on_wait, attempt)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct OperationKey {
    pub path: PathBuf,
    pub policy: OperationPolicy,
}

impl OperationKey {
    pub fn folder(&self) -> &Path {
        match &self.policy {
            OperationPolicy::SourceWalk | OperationPolicy::WatchTargets | OperationPolicy::SweepWalk { .. } => &self.path,
            OperationPolicy::CachePublication { root } => root,
            OperationPolicy::EditorDirectory { project, .. } => project,
        }
    }
}

#[derive(Clone)]
pub(crate) enum OperationOwner {
    Host,
    Folder(std::sync::Weak<crate::system::folder_session::FolderSession>),
}

impl OperationOwner {
    pub fn for_folder(path: &Path) -> Self {
        path.to_str().and_then(|path| crate::system::folder_session::registry().get(path))
            .map(|session| Self::Folder(Arc::downgrade(&session))).unwrap_or(Self::Host)
    }

    pub fn active(&self) -> bool {
        match self {
            Self::Host => true,
            Self::Folder(owner) => owner.upgrade().is_some_and(|session| !session.cancel.is_cancelled()),
        }
    }
}

pub(crate) struct OperationState {
    pub result: Option<Result<Arc<StorageValue>, Arc<StorageFailure>>>,
    pub started_at: Option<Instant>,
    pub attempt_revision: Option<u64>,
    pub last_failure: Option<Arc<StorageFailure>>,
    pub waiters: usize,
    pub deferred: bool,
    pub superseding: Option<OperationRequest>,
}

pub(crate) struct OperationRequest {
    pub owner: OperationOwner,
    pub attempt: Operation,
}

pub(crate) struct OperationTask {
    pub state: Mutex<OperationState>,
    pub(crate) request: OperationRequest,
    pub(crate) revision: Option<Arc<std::sync::atomic::AtomicU64>>,
}

impl OperationTask {
    pub fn new(key: &OperationKey, attempt: Operation) -> Self {
        let owner = OperationOwner::for_folder(key.folder());
        Self::from_request(OperationRequest { owner, attempt })
    }

    pub fn from_request(request: OperationRequest) -> Self {
        Self { request, revision: None, state: Mutex::new(OperationState { result: None, started_at: None, attempt_revision: None, last_failure: None, waiters: 1, deferred: false, superseding: None }) }
    }

    /// The caller's deadline never bounds a kernel call. Only these reserved
    /// workers may opt in; cancellation detaches subscribers, keeping the
    /// in-flight task deduplicated until the native call returns.
    pub fn owner_active(&self) -> bool {
        self.request.owner.active()
    }

    pub fn run(&self) -> Result<Arc<StorageValue>, Arc<StorageFailure>> {
        if !self.owner_active() {
            return Err(Arc::new(StorageFailure::new(None, StorageOperation::WalkEntry,
                io::Error::new(io::ErrorKind::Interrupted, "folder session closed"))));
        }
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.started_at = Some(Instant::now());
            state.attempt_revision = self.revision.as_ref().map(|revision| revision.load(std::sync::atomic::Ordering::Acquire));
        }
        let result = (self.request.attempt)().map(Arc::new).map_err(Arc::new);
        if let Err(failure) = &result {
            self.state.lock().unwrap_or_else(|e| e.into_inner()).last_failure = Some(failure.clone());
        }
        result
    }

}

fn await_operation_in(
    pool: &crate::build::cloud_prefetch::Prefetcher,
    path: &Path,
    policy: OperationPolicy,
    managed: bool,
    deadline: Duration,
    cancel: &dyn Fn() -> bool,
    on_wait: &dyn Fn(),
    attempt: Operation,
) -> Result<Arc<StorageValue>, Unavailable> {
    let first = match attempt() {
        Ok(value) => return Ok(Arc::new(value)),
        Err(failure) => Arc::new(failure),
    };
    if !recoverable_storage_failure(&first, managed, managed) {
        return Err(Unavailable { failure: first, pending: None });
    }
    wait_for_operation(pool, path, policy, deadline, cancel, on_wait, attempt, first)
}

fn admission_failure(pool: &crate::build::cloud_prefetch::Prefetcher, key: &OperationKey, first: Arc<StorageFailure>) -> Unavailable {
    if pool.structural_recovery_retained(key) { return Unavailable { failure: first, pending: Some(Settled::TimedOut) }; }
    Unavailable { failure: Arc::new(StorageFailure::new(Some(key.path.clone()), StorageOperation::ReadDirOpen,
        io::Error::new(io::ErrorKind::WouldBlock, "structural read admission capacity exhausted"))), pending: None }
}

fn wait_for_operation(
    pool: &crate::build::cloud_prefetch::Prefetcher,
    path: &Path,
    policy: OperationPolicy,
    deadline: Duration,
    cancel: &dyn Fn() -> bool,
    on_wait: &dyn Fn(),
    attempt: Operation,
    first: Arc<StorageFailure>,
) -> Result<Arc<StorageValue>, Unavailable> {
    on_wait();
    if cancel() {
        return Err(Unavailable { failure: first, pending: Some(Settled::Cancelled) });
    }
    let requested_at = Instant::now();
    let key = OperationKey { path: path.to_path_buf(), policy };
    let Some(mut task) = pool.structural_operation(key.clone(), attempt.clone()) else {
        return Err(admission_failure(pool, &key, first));
    };
    let started = Instant::now();
    loop {
        let mut state = task.state.lock().unwrap_or_else(|e| e.into_inner());
        if cancel() {
            state.waiters -= 1;
            return Err(Unavailable { failure: state.last_failure.clone().unwrap_or(first), pending: Some(Settled::Cancelled) });
        }
        if let Some(result) = &state.result {
            // A late subscriber cannot certify a new source candidate from a
            // walk that began before its request. Dedup lasts through the old
            // native call; the next queued operation supplies a fresh receipt.
            if state.started_at.is_none_or(|at| at < requested_at) {
                state.waiters -= 1;
                drop(state);
                let Some(fresh) = pool.structural_operation(key.clone(), attempt.clone()) else {
                    return Err(admission_failure(pool, &key, first));
                };
                task = fresh;
                continue;
            }
            let result = result.clone().map_err(|failure| Unavailable { failure, pending: None });
            state.waiters -= 1;
            return result;
        }
        if started.elapsed() >= deadline {
            state.deferred = true;
            state.waiters -= 1;
            return Err(Unavailable { failure: state.last_failure.clone().unwrap_or(first), pending: Some(Settled::TimedOut) });
        }
        drop(state);
        std::thread::sleep(Duration::from_millis(10).min(deadline.saturating_sub(started.elapsed())));
    }
}

/// Connect actual deferred read settlement to the host's existing typed bus.
/// Installed once at startup; headless hosts may leave it unset.
pub fn set_settlement_emitter(emit: crate::ops::watch::EventRelay) {
    super::PREFETCH.set_settlement_emitter(emit);
}

/// Recovery arrivals are consumed once by the folder's existing sweep.
pub(crate) fn take_arrivals(folder: &Path) -> crate::build::cloud_prefetch::StructuralArrivals {
    super::PREFETCH.take_structural_arrivals(folder)
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;

#[cfg(test)]
#[derive(Default)]
struct TestFaults {
    roots: std::collections::HashMap<PathBuf, Duration>,
    refusals: std::collections::HashMap<(PathBuf, StorageOperation), (usize, i32)>,
}

#[cfg(test)]
fn test_faults() -> std::sync::MutexGuard<'static, TestFaults> {
    static FAULTS: std::sync::LazyLock<Mutex<TestFaults>> = std::sync::LazyLock::new(|| Mutex::new(TestFaults::default()));
    FAULTS.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
pub(crate) fn test_probe(path: &Path, operation: StorageOperation) -> Result<(), StorageFailure> {
    if let Some((remaining, errno)) = test_faults().refusals.get_mut(&(path.to_path_buf(), operation)) {
        if *remaining > 0 {
            *remaining -= 1;
            return Err(StorageFailure::new(Some(path.to_path_buf()), operation, io::Error::from_raw_os_error(*errno)));
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) struct TestFault {
    root: PathBuf,
    key: (PathBuf, StorageOperation),
}

#[cfg(test)]
impl TestFault {
    pub fn install(root: &Path, path: &Path, operation: StorageOperation, count: usize, deadline: Duration) -> Self {
        Self::install_errno(root, path, operation, count, deadline, libc::EDEADLK)
    }

    pub fn install_errno(root: &Path, path: &Path, operation: StorageOperation, count: usize, deadline: Duration, errno: i32) -> Self {
        let mut faults = test_faults();
        faults.roots.insert(root.to_path_buf(), deadline);
        let key = (path.to_path_buf(), operation);
        faults.refusals.insert(key.clone(), (count, errno));
        Self { root: root.to_path_buf(), key }
    }
}

#[cfg(test)]
impl Drop for TestFault {
    fn drop(&mut self) {
        let mut faults = test_faults();
        faults.roots.remove(&self.root);
        faults.refusals.remove(&self.key);
    }
}
