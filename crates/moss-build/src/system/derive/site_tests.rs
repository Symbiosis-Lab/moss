use super::*;
use crate::system::derive::test_support::{entry, script, DERIVED};
use tempfile::TempDir;

struct Fixture {
    dir: TempDir,
    folder: String,
    demand: Demand,
    /// Held as the build's worker holds it: a folder's lifecycle record is
    /// dropped when another folder is looked up and nothing holds it.
    _lease: Box<dyn std::any::Any>,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.xyz"), "content").unwrap();
    let bin = script(dir.path(), "d.sh", DERIVED);
    let folder = dir.path().to_string_lossy().to_string();
    let demand = Demand { entry: entry(&bin, "1.0.0"), path: "a.xyz".into(), pages: vec![] };
    let _lease = Box::new(crate::build::lifecycle::detached_cache_lease(&MossPaths::new(dir.path())));
    Fixture { dir, folder, demand, _lease }
}

impl Fixture {
    fn job(&self, render: Option<u64>) -> BackgroundDerive {
        BackgroundDerive {
            paths: MossPaths::new(self.dir.path()),
            lifecycle_paths: MossPaths::new(self.dir.path()),
            root: self.dir.path().to_path_buf(),
            folder: self.folder.clone(),
            jobs: vec![self.demand.clone()],
            render,
        }
    }

    fn render(&self) -> u64 {
        crate::build::lifecycle::show_render(&MossPaths::new(self.dir.path()), true).0
    }
}

fn never() -> std::sync::atomic::AtomicBool {
    std::sync::atomic::AtomicBool::new(false)
}

#[test]
fn a_finished_background_read_asks_the_watcher_for_a_rebuild() {
    let f = fixture();
    let watcher = crate::ops::watch::worker::register(&f.folder);
    let render = f.render();
    f.job(Some(render)).run(None, &never());
    let asked = watcher.slot_occupied();
    crate::ops::watch::worker::deregister(&f.folder, &watcher);
    assert!(asked);
}

#[test]
fn a_background_read_overtaken_by_a_newer_render_still_asks_for_a_rebuild() {
    let f = fixture();
    // The newer render is admitted once the read's process is running: admitted
    // before it, the read never starts (that render queues its own), which is
    // not what this test is about. A fixed delay raced the read's start under
    // load. The sandboxed binary cannot write a signal, so the process is it.
    let bin = script(f.dir.path(), "d.sh", &format!("sleep 1\n{DERIVED}"));
    let watcher = crate::ops::watch::worker::register(&f.folder);
    let render = f.render();
    let root = f.dir.path().to_path_buf();
    let newer = std::thread::spawn(move || {
        let running = || std::process::Command::new("pgrep").arg("-f").arg(&bin).output().is_ok_and(|o| o.status.success());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !running() {
            if std::time::Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        crate::build::lifecycle::show_render(&MossPaths::new(&root), true);
        true
    });
    f.job(Some(render)).run(None, &never());
    assert!(newer.join().unwrap(), "the read's process was never seen running");
    let asked = watcher.slot_occupied();
    crate::ops::watch::worker::deregister(&f.folder, &watcher);
    assert!(asked, "the newer render may predate the finished text");
}

#[test]
fn a_cancelled_background_read_asks_for_nothing() {
    let f = fixture();
    let watcher = crate::ops::watch::worker::register(&f.folder);
    let render = f.render();
    f.job(Some(render)).run(None, &std::sync::atomic::AtomicBool::new(true));
    let asked = watcher.slot_occupied();
    crate::ops::watch::worker::deregister(&f.folder, &watcher);
    assert!(!asked);
}

#[test]
fn a_cancelled_background_read_says_it_has_stopped() {
    use crate::build::ports::reporter::BuildReporter;
    #[derive(Default)]
    struct Recording(std::sync::Mutex<Vec<PipelineEvent>>);
    impl BuildReporter for Recording {
        fn report(&self, event: &PipelineEvent) {
            self.0.lock().unwrap().push(event.clone());
        }
    }
    let f = fixture();
    let render = f.render();
    let reporter = Recording::default();
    f.job(Some(render)).run(Some(&reporter), &std::sync::atomic::AtomicBool::new(true));
    let events = reporter.0.lock().unwrap();
    let last = events.iter().rev().find_map(|e| match e {
        PipelineEvent::BackgroundProgress { task, completed, .. } if task == "derive" => Some(*completed),
        _ => None,
    });
    assert_eq!(last, Some(true), "the last derive tick closes the read");
}

/// Thirty files read under an older plugin version, then the last one (`last`)
/// under the new one, with `last` new when `arrived`, and embedded by `p.md`.
/// Reads may start until the first one finishes, so only the first picks (a
/// few, at most the budget plus any slots freed meanwhile) run. Whether `last`,
/// queued behind thirty others, was among them.
fn last_file_read_first(arrived: bool, viewed: Option<&str>, page: &str, watcher_saw_it: bool) -> bool {
    use crate::build::ports::reporter::BuildReporter;
    use crate::system::derive::test_support::script;
    #[derive(Default)]
    struct Finished(std::sync::atomic::AtomicBool);
    impl BuildReporter for Finished {
        fn report(&self, event: &PipelineEvent) {
            if matches!(event, PipelineEvent::BackgroundProgress { current, .. } if *current >= 1) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
    }
    let dir = TempDir::new().unwrap();
    let bin = script(dir.path(), "d.sh", &format!("sleep 0.3\n{DERIVED}"));
    let old = entry(&script(dir.path(), "old.sh", DERIVED), "1.0.0");
    let new = entry(&bin, "2.0.0");
    let paths = MossPaths::new(dir.path());
    std::fs::write(dir.path().join("p.md"), "p").unwrap();
    let names: Vec<String> = (0..30).map(|i| format!("f{i}.xyz")).chain(["last.xyz".to_string()]).collect();
    for name in &names {
        std::fs::write(dir.path().join(name), name).unwrap();
    }
    for name in names.iter().take(if arrived { 30 } else { 31 }) {
        derive::derive_cached(&paths, &old, &dir.path().join(name)).unwrap();
    }
    if let Some(url) = viewed {
        crate::system::derive::priority::note_viewed(dir.path(), url);
    }
    if watcher_saw_it {
        crate::system::derive::priority::note_arrived(dir.path(), &[dir.path().join("last.xyz")]);
    }
    let demands: Vec<Demand> = names
        .iter()
        .map(|n| Demand { entry: new.clone(), path: n.clone(), pages: vec![if n == "last.xyz" { page } else { "x.md" }.to_string()] })
        .collect();
    let reporter = Finished::default();
    let stop = || reporter.0.load(Ordering::SeqCst);
    let results = read_all(&paths, dir.path(), &demands, Some(&reporter), &stop, false);
    results.iter().any(|(p, _)| p == "last.xyz")
}

#[test]
fn a_build_reads_new_files_before_files_re_read_for_a_changed_plugin() {
    assert!(last_file_read_first(true, None, "p.md", false));
    assert!(!last_file_read_first(false, None, "p.md", false), "control: with no tier it waits its turn");
}

#[test]
fn the_page_on_screen_is_read_before_new_files() {
    assert!(last_file_read_first(false, Some("/p"), "p.md", false));
}

#[test]
fn a_file_the_watcher_saw_arrive_is_read_before_untouched_ones_even_with_an_earlier_result() {
    assert!(last_file_read_first(false, None, "p.md", true));
}

#[test]
fn viewing_a_folder_reads_the_files_of_the_pages_it_lists_first() {
    assert!(last_file_read_first(false, Some("/docs"), "docs/p.md", false));
    assert!(!last_file_read_first(false, Some("/other"), "docs/p.md", false), "control: another folder's page does not lead");
}
