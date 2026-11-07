//! A plugin's hook runs are told apart by who asked for them: what a process
//! hook a build started offers reaches the reporter, and the same hook
//! started by the person writes the folder. A build's runs may write only
//! inside `.moss/` and run no program. Driven through the real manager and
//! engine, so a run is marked where moss starts it and refused where the
//! plugin's host call lands.

use crate::plugins::adapter_host::AdapterHost;
use super::PluginManager;
use crate::advisory::{Action, AppOp};
use crate::build::ports::reporter::BuildReporter;
use crate::build::progress::PipelineEvent;
use crate::plugins::types::{ProcessContext, ProjectInfo, TriggerContext};

fn project() -> (tempfile::TempDir, std::path::PathBuf) {
    project_with("folder-writer")
}

fn project_with(plugin: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let target = concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp");
    std::fs::create_dir_all(target).unwrap();
    let container = tempfile::TempDir::new_in(target).unwrap();
    let project = container.path().join("project");
    let plugin_dir = project.join(".moss/plugins").join(plugin);
    std::fs::create_dir_all(&plugin_dir).unwrap();
    for file in ["manifest.json", "main.bundle.js"] {
        let fixture = format!("{}/tests/fixtures/plugins/{plugin}/{file}", env!("CARGO_MANIFEST_DIR"));
        std::fs::copy(fixture, plugin_dir.join(file)).unwrap();
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_hook_a_build_started_writes_only_inside_moss() {
    let (_container, project) = project();
    let pp = project.to_str().unwrap();
    manager(&project).0.execute_process(&ProcessContext::for_build(pp.into(), info())).await.unwrap();

    let o = outcome(&project);
    let page = o["page"].as_str().unwrap();
    assert!(page.contains("folder-writer writes into the folder only when you start it yourself") && page.contains("notes/a.md"), "{page}");
    assert!(o["bytes"].as_str().unwrap().contains("notes/b.bin"), "{o}");
    assert_eq!(o["comments"], "ok", "its own comment data is inside .moss/: {o}");
    assert!(o["download"].as_str().unwrap().contains("when you start it yourself"), "{o}");
    assert!(o["program"].as_str().unwrap().contains("runs programs only when you start it yourself"), "{o}");
    assert!(!project.join("notes").exists() && !project.join("assets").exists());
}

/// The page emit's `listing` hook is a build's run: it may not write the folder.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listing_hook_writes_only_inside_moss() {
    let (_container, project) = project();
    let (manager, _) = manager(&project);
    let ctx = super::listing::ListingContext {
        model: super::listing::LISTING_MODEL,
        style: "cards".into(),
        page: Default::default(),
        pages: vec![],
        folders: vec![],
        terms: Default::default(),
        declarations: Default::default(),
    };

    let result = manager.execute_listing("folder-writer", &ctx).await.unwrap();

    assert!(result.html.contains("when you start it yourself"), "{}", result.html);
    assert!(!project.join("notes").exists());
}

/// A timer a build's run left behind keeps the build's reach, even when it
/// fires while the person's run of the same plugin is going; the person's run
/// itself still writes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_build_runs_leftover_timer_never_gets_the_persons_reach() {
    let (_container, project) = project_with("late-writer");
    let pp = project.to_str().unwrap();
    let (manager, _) = manager(&project);
    manager.execute_process(&ProcessContext::for_build(pp.into(), info())).await.unwrap();
    manager
        .execute_process_for_plugin("late-writer", &ProcessContext::new(pp.into(), info(), TriggerContext::SettingsManual))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert!(!project.join("notes/late.md").exists());
    assert!(project.join("notes/person.md").exists());
}

/// A promise chain a build's run left waiting on the host keeps the build's
/// reach when it resumes during the person's run: the call carries the asker
/// of the Context that made it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_build_runs_leftover_promise_never_gets_the_persons_reach() {
    let (_container, project) = project_with("detached-writer");
    let pp = project.to_str().unwrap();
    let (manager, _) = manager(&project);
    manager.execute_process(&ProcessContext::for_build(pp.into(), info())).await.unwrap();
    manager
        .execute_process_for_plugin("detached-writer", &ProcessContext::new(pp.into(), info(), TriggerContext::SettingsManual))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert!(!project.join("notes/late.md").exists());
    assert!(project.join("notes/person.md").exists());
}
