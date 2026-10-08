//! Native operation attempts share the pool's queue, bounds and dedup receipts.
use super::*;

pub(super) fn structural_reader_loop(inner: &Arc<Inner>) {
    loop {
        let (key, task) = {
            let mut q = match inner.queue.lock() { Ok(q) => q, Err(_) => return };
            loop {
                if q.shutdown { return; }
                q.editor_intents.retain(|(_, owner)| owner.active());
                while q.structural_count() < BACKGROUND_WAITING + STRUCTURAL_READERS {
                    let Some((key, owner)) = q.editor_intents.pop_front() else { break };
                    let OperationPolicy::EditorDirectory { project, show_internal } = &key.policy else { unreachable!("only editor listings retain replay intents") };
                    let attempt = crate::build::cloud_readiness::storage::directory_operation(&key.path, project, *show_internal);
                    let mut task = OperationTask::new(&key, attempt);
                    task.request.owner = owner;
                    {
                        let mut state = task.state.lock().unwrap();
                        state.waiters = 0;
                        state.deferred = true;
                    }
                    let task = Arc::new(task);
                    q.operation_pending.insert(key.clone(), task.clone());
                    q.structural.push_back((key, task, Instant::now()));
                }
                let now = Instant::now();
                if let Some(index) = q.structural.iter().position(|(_, _, ready_at)| *ready_at <= now) {
                    let (key, task, _) = q.structural.remove(index).unwrap();
                    q.structural_in_flight.insert(key.clone(), now);
                    break (key, task);
                }
                let delay = q.structural.iter().map(|(_, _, at)| at.saturating_duration_since(now)).min();
                q = match delay {
                    Some(delay) => match inner.structural_work.wait_timeout(q, delay) { Ok((q, _)) => q, Err(_) => return },
                    None => match inner.structural_work.wait(q) { Ok(q) => q, Err(_) => return },
                };
            }
        };
        let result = task.run();
        finish_operation(inner, key, task, result);
        inner.structural_work.notify_all();
    }
}

pub(super) fn finish_operation(inner: &Arc<Inner>, key: OperationKey, task: Arc<OperationTask>, result: Result<Arc<crate::build::cloud_readiness::storage::StorageValue>, Arc<crate::build::cloud_readiness::StorageFailure>>) {
        inner.done.fetch_add(1, Ordering::Relaxed);
        let mut settlement = None;
        if let Ok(mut q) = inner.queue.lock() {
            let cache = matches!(key.policy, OperationPolicy::CachePublication { .. });
            if cache {
                q.cache_in_flight.remove(&key);
                q.background_in_flight -= 1;
            } else { q.structural_in_flight.remove(&key); }
            let mut state = task.state.lock().unwrap_or_else(|e| e.into_inner());
            let newer_revision = task.revision.as_ref().is_some_and(|latest| {
                let attempted = match &result {
                    Ok(value) => match value.as_ref() { crate::build::cloud_readiness::storage::StorageValue::Published(version) => Some(*version), _ => None },
                    Err(_) => state.attempt_revision,
                };
                attempted.is_some_and(|version| version < latest.load(Ordering::Acquire))
            });
            let retry = newer_revision || result.as_ref().err().is_some_and(|failure| {
                crate::build::cloud_readiness::recoverable_storage_failure(failure, true, true)
            });
            if let Some(request) = state.superseding.take().filter(|request| request.owner.active() && !q.shutdown) {
                let fresh = Arc::new(OperationTask::from_request(request));
                {
                    let mut next = fresh.state.lock().unwrap();
                    next.waiters = 0;
                    next.deferred = true;
                }
                q.operation_pending.insert(key.clone(), fresh.clone());
                q.structural.push_back((key, fresh, Instant::now()));
                state.result = Some(result);
            } else if retry && task.owner_active() && !q.shutdown && (cache || state.waiters > 0 || state.deferred) {
                // A returned refusal releases native capacity. Keep the dedup
                // receipt and rejoin the FIFO after the central retry delay.
                let ready_at = Instant::now() + crate::build::cloud_readiness::POLL_INTERVAL;
                if cache { q.fifo.push_back(BackgroundWork::Publication(key, task.clone(), ready_at)); }
                else { q.structural.push_back((key, task.clone(), ready_at)); }
            } else {
                q.operation_pending.remove(&key);
                let complete = result.as_ref().map_or(true, |value| match value.as_ref() {
                    crate::build::cloud_readiness::storage::StorageValue::Sweep(snapshot) => !snapshot.deadline_blown && !snapshot.root_unreadable
                        && matches!(key.policy, OperationPolicy::SweepWalk { resume: None, .. }),
                    _ => true,
                });
                if !cache && complete && state.deferred && task.owner_active() {
                    use crate::types::events::SourceStructureOutcome;
                    let outcome = if result.is_ok() { SourceStructureOutcome::Ready } else { SourceStructureOutcome::Failed };
                    settlement = Some(crate::types::events::MossEvent::SourceStructureSettled {
                        folder_path: key.folder().to_string_lossy().into_owned(),
                        path: key.path.to_string_lossy().into_owned(), outcome,
                    });
                    if matches!(key.policy, OperationPolicy::SourceWalk | OperationPolicy::WatchTargets | OperationPolicy::SweepWalk { .. }) && matches!(task.request.owner, OperationOwner::Folder(_)) {
                        q.structural_arrivals.insert(key, task.request.owner.clone());
                    }
                }
                state.result = Some(result);
            }
        }
        // Host callbacks may re-enter the pool. Never invoke one while its
        // queue, task, or emitter slot is locked.
        if let Some(event) = settlement {
            let emit = inner.settlement_emitter.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if task.owner_active() {
                if let Some(emit) = emit { emit(event); }
            }
        }
    inner.work.notify_all();
    inner.structural_work.notify_all();
}
