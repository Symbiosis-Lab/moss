//! Numeric, deploy-scoped telemetry. Never persist errors, URLs or source paths.
//!
//! Transfer counts describe seta uploads; plugin and dry-run records explicitly
//! mark them unavailable. Page counts reuse the seal's change set, with
//! `pages_classified` distinguishing an unknown baseline from zero changes.
//! `body_requests` and `retries` use the upload summary's accounting, excluding
//! the open and commit requests. `requests`, `requests_failed`, `sent_kb`, `resent_kb`
//! and `edge` come from the seta client's request tally and cover every request
//! of the deploy. Unentered phases have zero milliseconds.

use std::{collections::BTreeMap, future::Future, path::Path, sync::{Arc, Mutex}, time::Instant};
use crate::build::{phase::{self, PhaseTrace}, record::{self, Record}};
use super::{DeployReport, PublishError, PushResult};

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
    debug_assert!(record::REFUSALS.contains(&kind), "{kind} is not in record::REFUSALS");
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
            Self::Plugin { .. } | Self::Checked(_) => {}
        }
    }
}

pub(crate) fn target(route: &super::route::DeployRoute) -> &'static str {
    match route {
        super::route::DeployRoute::Hosted => "seta",
        super::route::DeployRoute::Prebuilt { .. } => "prebuilt",
        super::route::DeployRoute::Plugin { .. } => "plugin",
        super::route::DeployRoute::Direct { .. } => "direct",
    }
}

/// Nested entry points share one record; only the outer command appends it.
pub(crate) fn record<'a, T: Outcome + 'a, E: 'a>(
    folder: &'a Path, target: &'static str, dry_run: bool,
    body: impl Future<Output = Result<T, E>> + 'a,
) -> impl Future<Output = Result<T, E>> + 'a {
    // The build future is large; nested entry-point scopes must not copy it
    // through several task-local future states on a worker's stack.
    let body = Box::pin(body);
    async move {
        if REFUSAL.try_with(|_| ()).is_ok() { return body.await; }
        let started_at = chrono::Utc::now();
        let started = Instant::now();
        let cpu_at_start = record::usage().map(|u| u.cpu_ms);
        let refusal = Arc::new(Mutex::new(None));
        let wire = Arc::new(crate::seta::wire::WireCounters::default());
        let (result, collector) = REFUSAL.scope(refusal.clone(), crate::seta::wire::WireCounters::scope(wire.clone(), phase::with_collector(async {
            let result = body.await;
            if let Ok(value) = &result { value.observe(); }
            result
        }))).await;
        let refusal = refusal.lock().unwrap_or_else(|e| e.into_inner()).take();
        let outcome = if refusal.is_some() { "refused" } else if result.is_ok() { "ok" } else { "error" };
        let mut summary = Record::new(&record::DEPLOY);
        summary.word("outcome", outcome);
        summary.word("target", target);
        if let Some(kind) = refusal.as_ref().and_then(|r| r.as_str()) { summary.word("refusal_kind", kind); }
        summary.ms("total", i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX));
        if let (Some(start), Some(now)) = (cpu_at_start, record::usage()) { summary.count("cpu_ms", now.cpu_ms.saturating_sub(start)); }
        if let Some(mb) = record::footprint_mb() { summary.count("footprint_mb", mb); }
        let mut timings: BTreeMap<&str, u128> = record::DEPLOY_PHASES.into_iter().map(|key| (key, 0)).collect();
        let mut build_started_at = None;
        if let Ok(t) = collector.lock() {
            for &(name, n) in &t.counts { summary.count(name, n as u64); }
            for p in &t.phases { *timings.entry(p.name).or_default() += p.ms; }
            build_started_at = t.nested_started_at.map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        }
        for (name, n) in wire.counts() { summary.count(name, n); }
        // History runs inside the landing tail; report it separately from that tail.
        let history = timings["history_snapshot"];
        if let Some(after) = timings.get_mut("after_publish") { *after = after.saturating_sub(history); }
        for (name, ms) in timings.iter().filter(|(_, ms)| **ms > 0) { summary.ms(name, i64::try_from(*ms).unwrap_or(i64::MAX)); }
        let mut fields = summary.json();
        fields.insert("started_at".into(), started_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true).into());
        fields.insert("edge".into(), wire.edge().into());
        fields.insert("dry_run".into(), dry_run.into());
        fields.insert("build_started_at".into(), build_started_at.into());
        fields.insert("phases".into(), timings.iter().map(|(name, ms)| serde_json::json!({"name": name, "ms": ms})).collect());
        fields.insert("transfer_counts_available".into(), (target != "plugin" && !dry_run).into());
        let record = crate::build::phase::profile_record_of("deploy", fields.into());
        log::info!(target: "deploy", "{}", summary.line());
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
        SetaError::StorageCapExceeded { .. } => refusal("size_limit"),
        SetaError::DeploySuperseded { .. } => refusal("conflict"),
        SetaError::MassRemoval { .. } => refusal("mass_removal"),
        SetaError::Api { status: 401 | 403, .. } | SetaError::NotSignedIn { .. } => refusal("access"),
        SetaError::Api { status: 409, .. } => refusal("conflict"),
        SetaError::Api { status: 413, .. } => refusal("size_limit"),
        SetaError::Api { status: 422, .. } => refusal("incomplete_generation"),
        _ => {}
    }
}

/// Record a failed seta call in the profile and type it: a key the site does
/// not know is the sign-in refusal.
pub(crate) fn profiled_refusal(error: crate::seta::client::SetaError) -> PublishError {
    seta_error(&error);
    match error {
        crate::seta::client::SetaError::NotSignedIn { site } => PublishError::NotSignedIn { host: site },
        e => e.to_string().into(),
    }
}

pub(crate) async fn open(
    call: impl Future<Output = Result<crate::seta::deploys::DeployAnswer, crate::seta::client::SetaError>>,
) -> Result<crate::seta::deploys::DeployAnswer, crate::seta::client::SetaError> {
    // Named "sync" so deploy records stay comparable across the route change.
    let _phase = PhaseTrace::start("sync");
    let answer = call.await?;
    count("files_needed", answer.needed.len() as u64);
    Ok(answer)
}

pub(crate) async fn commit(
    call: impl Future<Output = Result<crate::seta::deploys::DeployAnswer, crate::seta::client::SetaError>>,
) -> Result<crate::seta::deploys::DeployAnswer, PublishError> {
    let _phase = PhaseTrace::start("publish");
    let answer = call.await.map_err(profiled_refusal)?;
    landed_counts(&answer);
    Ok(answer)
}

/// What a commit reports, from whichever request committed.
pub(crate) fn landed_counts(answer: &crate::seta::deploys::DeployAnswer) {
    if let Some(n) = answer.files_updated {
        count("server_files_updated", n as u64);
    }
    if let Some(n) = answer.files_removed {
        count("removed_files", n as u64);
    }
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
