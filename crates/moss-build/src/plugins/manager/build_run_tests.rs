//! A plugin's hook runs are told apart by who asked for them: what a process
//! hook a build started offers reaches the reporter, and the same hook
//! started by the person writes the folder. Driven through the real manager
//! and engine, so a run is marked where moss starts it.

use crate::plugins::adapter_host::AdapterHost;
use super::PluginManager;
use crate::advisory::{Action, AppOp};
use crate::build::ports::reporter::BuildReporter;
use crate::build::progress::PipelineEvent;
use crate::plugins::types::{ProcessContext, ProjectInfo, TriggerContext};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/plugins/folder-writer");

fn project() -> (tempfile::TempDir, std::path::PathBuf) {
    let target = concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp");
    std::fs::create_dir_all(target).unwrap();
    let container = tempfile::TempDir::new_in(target).unwrap();
    let project = container.path().join("project");
    let plugin_dir = project.join(".moss/plugins/folder-writer");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    for file in ["manifest.json", "main.bundle.js"] {
        std::fs::copy(format!("{FIXTURE}/{file}"), plugin_dir.join(file)).unwrap();
    }
    (container, project)
}

fn info() -> ProjectInfo {
    ProjectInfo { total_files: 0, homepage_file: None, folder_name: None, site_name: None, lang: "en".into() }
}

/// The offer ticks the manager reported, as `(task, what, action)`; a tick
/// that clears the offer is `(task, "", Action::None)`.
#[derive(Default)]
struct Offers(std::sync::Mutex<Vec<(String, String, Action)>>);

impl BuildReporter for Offers {
    fn report(&self, event: &PipelineEvent) {
        if let PipelineEvent::BackgroundProgress { task, advisories, .. } = event {
            if !task.starts_with("plugin-offer-") {
                return;
            }
            let mut offers = self.0.lock().unwrap();
            match advisories.as_slice() {
                [] => offers.push((task.clone(), String::new(), Action::None)),
                found => offers.extend(found.iter().map(|a| (task.clone(), a.what.clone(), a.action.clone()))),
            }
        }
    }
}

fn manager(project: &std::path::Path) -> (PluginManager, std::sync::Arc<Offers>) {
    let offers = std::sync::Arc::new(Offers::default());
    let manager = PluginManager::new(project.to_str().unwrap(), AdapterHost::headless(offers.clone())).unwrap();
    (manager, offers)
}

fn outcome(project: &std::path::Path) -> serde_json::Value {
    let raw = std::fs::read_to_string(project.join(".moss/plugins/folder-writer/outcome.json"))
        .expect("the hook records its outcome in its own storage");
    serde_json::from_str(&raw).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_hook_a_build_started_offers_what_it_found() {
    let (_container, project) = project();
    let pp = project.to_str().unwrap();
    let (manager, offers) = manager(&project);
    manager.execute_process(&ProcessContext::for_build(pp.into(), info())).await.unwrap();

    let offers = offers.0.lock().unwrap();
    let [(task, what, Action::InApp { op: AppOp::RunPluginProcess, args, .. })] = offers.as_slice() else {
        panic!("one Sync offer: {offers:?}")
    };
    assert_eq!((task.as_str(), what.as_str()), ("plugin-offer-folder-writer", "2 new notes"));
    assert_eq!(args["plugin"], "folder-writer", "Sync runs the plugin that offered");
    assert_eq!(args.as_object().unwrap().len(), 1, "the app names the plugin from its manifest: {args}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_same_hook_started_by_the_person_writes_the_folder() {
    let (_container, project) = project();
    let pp = project.to_str().unwrap();
    let (manager, offers) = manager(&project);
    manager
        .execute_process_for_plugin("folder-writer", &ProcessContext::new(pp.into(), info(), TriggerContext::SettingsManual))
        .await
        .unwrap();

    let o = outcome(&project);
    assert_eq!(o["page"], "ok", "{o}");
    assert_eq!(o["bytes"], "ok", "{o}");
    assert_eq!(std::fs::read_to_string(project.join("notes/a.md")).unwrap(), "# a");
    for refused_elsewhere in ["download", "program"] {
        assert!(!o[refused_elsewhere].as_str().unwrap().contains("when you start it yourself"), "{o}");
    }
    assert_eq!(
        *offers.0.lock().unwrap(),
        vec![("plugin-offer-folder-writer".to_string(), String::new(), Action::None)],
        "the person's run clears the offer instead of making one",
    );
}

/// A person's run that starts while a build's run of the same hook is going
/// waits for it and then runs with its own reach, instead of sharing the
/// build's result.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_persons_run_never_takes_a_builds_result() {
    let (_container, project) = project();
    let pp = project.to_str().unwrap().to_string();
    let manager = std::sync::Arc::new(manager(&project).0);
    let build = tokio::spawn({
        let (manager, pp) = (manager.clone(), pp.clone());
        async move { manager.execute_process(&ProcessContext::for_build(pp, info())).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    manager
        .execute_process_for_plugin("folder-writer", &ProcessContext::new(pp, info(), TriggerContext::SettingsManual))
        .await
        .unwrap();
    build.await.unwrap().unwrap();

    assert_eq!(std::fs::read_to_string(project.join("notes/a.md")).unwrap(), "# a");
}

/// A build skipped because the person's run is busy says nothing about the
/// offer, so the offer that run is answering is not cleared under it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_skipped_build_run_leaves_the_offer_alone() {
    let (_container, project) = project();
    let pp = project.to_str().unwrap();
    let (manager, offers) = manager(&project);
    let persons_run = manager.host.state.hooks.begin_person_run("folder-writer").await;

    manager.execute_process(&ProcessContext::for_build(pp.into(), info())).await.unwrap();
    drop(persons_run);

    assert!(offers.0.lock().unwrap().is_empty(), "{:?}", offers.0.lock().unwrap());
    assert!(!project.join(".moss/plugins/folder-writer/outcome.json").exists(), "the build's run was skipped");
}
