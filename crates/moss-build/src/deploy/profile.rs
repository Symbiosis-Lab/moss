//! Numeric, deploy-scoped telemetry. Never persist errors, URLs or source paths.
//!
//! Transfer counts describe seta uploads; plugin and dry-run records explicitly
//! mark them unavailable. Page counts reuse the seal's change set, with
//! `pages_classified` distinguishing an unknown baseline from zero changes.
//! `body_requests` and `retries` use the upload summary's accounting, excluding
//! sync and commit requests. Unentered phases have zero milliseconds.

use std::{collections::BTreeMap, future::Future, path::Path, sync::{Arc, Mutex}, time::Instant};
use crate::build::phase::{self, PhaseTrace};
use super::{DeployReport, PushResult};

/// Why the deploy was refused, if it was; its counts and phases live in the
/// phase collector the record opens.
tokio::task_local! { static REFUSAL: Arc<Mutex<Option<serde_json::Value>>>; }

fn set_refusal(value: Option<serde_json::Value>) {
    let _ = REFUSAL.try_with(|r| { if let Ok(mut r) = r.lock() { *r = value; } });
}

pub(crate) fn count(name: &'static str, value: u64) {
    phase::record_value(name, value as usize);
}

pub(crate) fn begin_attempt() { set_refusal(None); }

pub(crate) fn refusal(kind: &'static str) {
    set_refusal(Some(serde_json::json!(kind)));
}

pub(crate) fn refused(kind: super::RefusalKind) {
    set_refusal(serde_json::to_value(kind).ok());
}

pub(crate) fn gate<T>(kind: &'static str, result: Result<T, String>) -> Result<T, String> {
    if result.is_err() { refusal(kind); }
    result
}

pub(crate) fn pages(set: &crate::build::manifest::change_set::ChangeSet) {
    for (name, n) in [("pages_added", set.added), ("pages_edited", set.edited),
        ("pages_deleted", set.deleted), ("pages_restyled", set.restyled)] {
        count(name, n as u64);
    }
    count("pages_classified", u64::from(set.classified));
}

pub(crate) trait Outcome {
    fn observe(&self);
}
impl Outcome for PushResult {
    fn observe(&self) {
        if matches!(self, Self::NeedsSetup) { refusal("needs_setup"); }
    }
}
impl Outcome for DeployReport {
    fn observe(&self) {
        match self {
            Self::Push(p) => p.observe(),
            Self::DryRun(d) => {
                pages(&d.change_set);
                if let Some(r) = &d.refusal { refused(r.kind); }
            }
            Self::Plugin { .. } => {}
        }
    }
}

pub(crate) fn target(route: &super::route::DeployRoute) -> &'static str {
    match route {
        super::route::DeployRoute::Hosted => "seta",
        super::route::DeployRoute::Prebuilt { .. } => "prebuilt",
        super::route::DeployRoute::Plugin { .. } => "plugin",
    }
}

/// Nested entry points share one record; only the outer command appends it.
pub(crate) fn record<'a, T: Outcome + 'a>(
    folder: &'a Path, target: &'static str, dry_run: bool,
    body: impl Future<Output = Result<T, String>> + 'a,
) -> impl Future<Output = Result<T, String>> + 'a {
    // The build future is large; nested entry-point scopes must not copy it
    // through several task-local future states on a worker's stack.
    let body = Box::pin(body);
    async move {
        if REFUSAL.try_with(|_| ()).is_ok() { return body.await; }
        let started_at = chrono::Utc::now();
        let started = Instant::now();
        let refusal = Arc::new(Mutex::new(None));
        let (result, collector) = REFUSAL.scope(refusal.clone(), phase::with_collector(async {
            let result = body.await;
            if let Ok(value) = &result { value.observe(); }
            result
        })).await;
        let refusal = refusal.lock().unwrap_or_else(|e| e.into_inner()).take();
        let mut counts: BTreeMap<&str, usize> = ["manifest_files", "files_needed", "files_uploaded",
            "bytes_uploaded", "body_requests", "retries", "chunked_files", "single_put_files",
            "final_concurrency", "achieved_bytes_per_s", "server_files_updated", "removed_files",
            "pages_added", "pages_edited", "pages_deleted", "pages_restyled", "pages_classified"]
            .into_iter().map(|key| (key, 0)).collect();
        let mut timings: BTreeMap<&str, u128> = ["build", "local_checks", "manifest", "sync",
            "upload", "publish", "after_publish", "history_snapshot"]
            .into_iter().map(|key| (key, 0)).collect();
        let mut build_started_at = None;
        if let Ok(t) = collector.lock() {
            counts.extend(t.counts.iter().copied());
            for p in &t.phases { *timings.entry(p.name).or_default() += p.ms; }
            build_started_at = t.nested_started_at.map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        }
        // History runs inside the landing tail; report it separately from that tail.
        let history = timings["history_snapshot"];
        if let Some(after) = timings.get_mut("after_publish") { *after = after.saturating_sub(history); }
        let outcome = if refusal.is_some() { "refused" } else if result.is_ok() { "ok" } else { "error" };
        let record = serde_json::json!({
            "schema": 1, "kind": "deploy", "moss_version": crate::system::app_version(),
            "started_at": started_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "total_ms": started.elapsed().as_millis(), "outcome": outcome,
            "refusal_kind": refusal, "target": target, "dry_run": dry_run,
            "build_started_at": build_started_at,
            "phases": timings.iter().map(|(name, ms)| serde_json::json!({"name": name, "ms": ms})).collect::<Vec<_>>(),
            "counts": counts,
            "transfer_counts_available": target != "plugin" && !dry_run,
        });
        log::info!(target: "deploy", "deploy.summary {}", record);
        if let Err(e) = phase::append_record(&crate::moss_paths::MossPaths::new(folder).profile(), &record) {
            log::debug!("could not append deploy profile: {e}");
        }
        result
    }
}

pub(crate) fn seta_error(error: &crate::seta::client::SetaError) {
    use crate::seta::client::SetaError;
    match error {
        SetaError::SiteLimitReached { .. } => refusal("site_limit"),
        SetaError::Api { status: 401 | 403, .. } => refusal("access"),
        SetaError::Api { status: 409, .. } => refusal("conflict"),
        SetaError::Api { status: 413, .. } => refusal("size_limit"),
        SetaError::Api { status: 422, .. } => refusal("incomplete_generation"),
        _ => {}
    }
}

pub(crate) async fn sync(
    call: impl Future<Output = Result<crate::seta::sites::SyncManifestResponse, crate::seta::client::SetaError>>,
) -> Result<crate::seta::sites::SyncManifestResponse, String> {
    let _phase = PhaseTrace::start("sync");
    let diff = call.await.map_err(|e| { seta_error(&e); e.to_string() })?;
    count("files_needed", diff.need.len() as u64);
    count("removed_files", diff.remove.len() as u64);
    Ok(diff)
}

pub(crate) async fn commit(
    call: impl Future<Output = Result<crate::seta::sites::CommitResponse, crate::seta::client::SetaError>>,
) -> Result<crate::seta::sites::CommitResponse, String> {
    let _phase = PhaseTrace::start("publish");
    let result = call.await.map_err(|e| { seta_error(&e); e.to_string() })?;
    count("server_files_updated", result.files_updated as u64);
    count("removed_files", result.files_removed as u64);
    Ok(result)
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
