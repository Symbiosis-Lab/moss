//! The app refreshes a folder's comments when the person asks (Retry, beside
//! the comment status), outside any build, through `start_background_sync`.
//! It returns at once, and the task always reports its completion: that event
//! is what the app's status line waits for.

use moss_build::build::features::sync::start_background_sync;
use moss_build::build::ports::reporter::BuildReporter;
use moss_build::build::ports::spawner::TokioSpawner;
use moss_build::build::progress::PipelineEvent;
use moss_build::config::deployment::DomainDeploymentConfig;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Each background task's (name, completed, advisory count).
#[derive(Default)]
struct Completions(Mutex<Vec<(String, bool, usize)>>);

impl BuildReporter for Completions {
    fn report(&self, event: &PipelineEvent) {
        if let PipelineEvent::BackgroundProgress { task, completed, advisories, .. } = event {
            self.0.lock().unwrap().push((task.clone(), *completed, advisories.len()));
        }
    }
}

#[tokio::test]
async fn a_refresh_on_request_reports_its_completion() {
    let site = tempfile::tempdir().unwrap();
    let reporter = Arc::new(Completions::default());
    start_background_sync(
        site.path().to_string_lossy().into_owned(),
        DomainDeploymentConfig::default(),
        reporter.clone(),
        &TokioSpawner,
    );
    for _ in 0..500 {
        if !reporter.0.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // A folder never built has nothing to sync, and still says it finished.
    assert_eq!(*reporter.0.lock().unwrap(), vec![("comment-sync".to_string(), true, 0)]);
}
