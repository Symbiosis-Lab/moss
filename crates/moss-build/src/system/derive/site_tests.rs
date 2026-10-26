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
fn a_background_read_of_a_superseded_render_asks_for_nothing() {
    let f = fixture();
    let watcher = crate::ops::watch::worker::register(&f.folder);
    let render = f.render();
    // A newer render of the folder was admitted while the read was running.
    f.render();
    f.job(Some(render)).run(None, &never());
    let asked = watcher.slot_occupied();
    crate::ops::watch::worker::deregister(&f.folder, &watcher);
    assert!(!asked, "the newer build queues whatever is still missing");
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

/// Three files in one folder, read under a plugin version that changed: `a`
/// and `b` have a result from the old version, `c` is new. `starts` reads are
/// allowed; the result says which were chosen first.
fn first_reads(pages: [&str; 3], viewed: Option<&str>, starts: usize) -> Vec<&'static str> {
    use crate::system::derive::test_support::script;
    let dir = TempDir::new().unwrap();
    let bin = script(dir.path(), "d.sh", DERIVED);
    let old = entry(&bin, "1.0.0");
    let new = entry(&bin, "2.0.0");
    let paths = MossPaths::new(dir.path());
    std::fs::write(dir.path().join("p.md"), "p").unwrap();
    let names = ["a.xyz", "b.xyz", "c.xyz"];
    for name in names {
        std::fs::write(dir.path().join(name), name).unwrap();
    }
    for name in &names[..2] {
        derive::derive_cached(&paths, &old, &dir.path().join(name)).unwrap();
    }
    if let Some(url) = viewed {
        crate::system::derive::priority::note_viewed(dir.path(), url);
    }
    let demands: Vec<Demand> =
        names.iter().zip(pages).map(|(n, p)| Demand { entry: new.clone(), path: n.to_string(), pages: vec![p.to_string()] }).collect();
    let started = std::sync::atomic::AtomicUsize::new(0);
    let stop = || started.fetch_add(1, Ordering::SeqCst) >= starts;
    let results = read_all(&paths, dir.path(), &demands, None, &stop, false);
    names.into_iter().filter(|n| results.iter().any(|(p, _)| p == n)).collect()
}

#[test]
fn a_build_reads_new_files_before_files_re_read_for_a_changed_plugin() {
    assert_eq!(first_reads(["x.md"; 3], None, 1), vec!["c.xyz"]);
}

#[test]
fn the_page_on_screen_is_read_before_new_files() {
    assert_eq!(first_reads(["x.md", "x.md", "x.md"], Some("/p"), 1), vec!["c.xyz"], "nothing on screen embeds a file");
    assert_eq!(first_reads(["x.md", "p.md", "x.md"], Some("/p"), 1), vec!["b.xyz"]);
}
