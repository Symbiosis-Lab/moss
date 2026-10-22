//! The no-arrival window, against an injected pool: a provider that answers
//! slowly is waited for, one that stops answering is reported.

use super::*;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

const DRIVE_FOLDER: &str = "/home/u/Library/CloudStorage/GoogleDrive-a@example.com/My Drive/site";

fn files(n: usize) -> Vec<PathBuf> {
    (0..n).map(|i| Path::new(DRIVE_FOLDER).join(format!("page-{i}.md"))).collect()
}

type Cloud = Arc<Mutex<HashSet<PathBuf>>>;

fn in_cloud(files: &[PathBuf]) -> (Cloud, impl Fn(&Path) -> bool + Send + Sync + 'static) {
    let cloud: Cloud = Arc::new(Mutex::new(files.iter().cloned().collect()));
    let seen = cloud.clone();
    (cloud, move |p: &Path| seen.lock().unwrap().contains(p))
}

#[test]
fn a_provider_that_refuses_every_read_is_reported_with_its_name_and_the_fix() {
    let files = files(30);
    let (_cloud, in_cloud) = in_cloud(&files);
    let pool = Prefetcher::with_materializer(
        4,
        Arc::new(|_| Err(std::io::Error::from_raw_os_error(libc::ETIMEDOUT))),
        false,
    );
    let wait = Wait::with(pool, in_cloud, Duration::from_millis(300), Duration::from_millis(10));

    let mut said = Vec::new();
    let err = bring_local(Path::new(DRIVE_FOLDER), files, &wait, "moss build", &mut |l| said.push(l.to_string()))
        .expect_err("a provider that delivers nothing must not look like success");

    assert!(err.starts_with("Google Drive is not delivering 30 of 30 cloud-only files"), "{err}");
    assert!(err.contains("page-0.md, page-1.md, page-10.md, and 27 more"), "{err}");
    assert!(err.contains("Open Google Drive so these files can download, then run `moss build` again."), "{err}");
    assert!(err.contains("refused"), "refusals are counted: {err}");
    assert!(said[0].contains("30 of 30 files are in Google Drive"), "{said:?}");
}

#[test]
fn a_slow_provider_that_keeps_delivering_is_waited_for_past_the_window() {
    let files = files(6);
    let (cloud, in_cloud) = in_cloud(&files);
    let delivered = cloud.clone();
    // One read at a time, 120ms each: the whole delivery outlasts the 400ms
    // window, but no gap between two arrivals does.
    let pool = Prefetcher::with_materializer(
        3,
        Arc::new(move |p| {
            std::thread::sleep(Duration::from_millis(120));
            delivered.lock().unwrap().remove(p);
            Ok(())
        }),
        false,
    );
    let wait = Wait::with(pool, in_cloud, Duration::from_millis(400), Duration::from_millis(10));

    let started = Instant::now();
    let outcome = await_local(Path::new(DRIVE_FOLDER), files, &wait, &mut |_, _| {});

    assert!(matches!(outcome, Outcome::Local { arrived: 6 }), "{outcome:?}");
    assert!(started.elapsed() > Duration::from_millis(400), "premise: the wait outlasted one window");
}

#[test]
fn files_already_on_disk_ask_for_nothing() {
    let (_cloud, in_cloud) = in_cloud(&[]);
    let pool = Prefetcher::with_materializer(3, Arc::new(|_| panic!("nothing was in the cloud")), false);
    let wait = Wait::with(pool, in_cloud, Duration::from_millis(50), Duration::from_millis(5));

    let outcome = await_local(Path::new(DRIVE_FOLDER), files(3), &wait, &mut |_, _| panic!("no progress to report"));

    assert!(matches!(outcome, Outcome::Local { arrived: 0 }), "{outcome:?}");
}

#[test]
fn a_build_does_not_start_while_its_sources_are_stuck_in_the_cloud() {
    let tmp = tempfile::tempdir().unwrap();
    let site = tmp.path().join("Library/CloudStorage/GoogleDrive-a@example.com/My Drive/site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.md"), "# Home").unwrap();
    let _in_cloud = icloud::pretend::evicted(&site.join("index.md"));
    let stuck = || {
        let pool = Prefetcher::with_materializer(
            3,
            Arc::new(|_| Err(std::io::Error::from_raw_os_error(libc::ETIMEDOUT))),
            false,
        );
        Wait::with(pool, icloud::is_still_in_the_cloud, Duration::from_millis(200), Duration::from_millis(10))
    };

    let err = require_sources_local(&site, &stuck, &mut |_| {}).expect_err("the build must be refused");

    assert!(err.contains("Google Drive is not delivering 1 of 1 cloud-only files"), "{err}");
    assert!(err.contains("then run `moss build` again"), "{err}");
}

#[test]
fn a_build_of_a_local_folder_asks_for_no_pool() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("index.md"), "# Home").unwrap();

    require_sources_local(tmp.path(), &|| panic!("nothing is in the cloud"), &mut |_| {}).unwrap();
}


fn listing(root: &Path) -> Vec<String> {
    let mut all: Vec<String> = walkdir::WalkDir::new(root)
        .into_iter()
        .map(|e| e.unwrap())
        .map(|e| format!("{} {}", e.path().strip_prefix(root).unwrap().display(), e.metadata().unwrap().len()))
        .collect();
    all.sort();
    all
}

fn site_in_a_drive_folder() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let site = tmp.path().join("Library/CloudStorage/GoogleDrive-a@example.com/My Drive/site");
    std::fs::create_dir_all(site.join("posts")).unwrap();
    std::fs::write(site.join("index.md"), "# Home").unwrap();
    std::fs::write(site.join("posts/one.md"), "# One").unwrap();
    (tmp, site)
}

#[test]
fn a_folder_with_nothing_in_the_cloud_is_fetched_without_a_write() {
    let (_tmp, site) = site_in_a_drive_folder();
    let before = listing(&site);
    let (mut said, mut out) = (Vec::new(), Vec::new());

    let code = fetch_folder(&site, &|| panic!("no pool is needed"), &mut |l| said.push(l.to_string()), &mut |l| out.push(l.to_string()));

    assert_eq!(code, 0, "{said:?}");
    assert!(out[0].starts_with("Nothing to fetch"), "{out:?}");
    assert_eq!(listing(&site), before, "fetch wrote into the folder");
    assert!(!site.join(".moss").exists(), "fetch must not create .moss");
}

#[test]
fn a_stalled_provider_exits_non_zero_and_names_itself() {
    let (_tmp, site) = site_in_a_drive_folder();
    let _in_cloud: Vec<_> = ["index.md", "posts/one.md"].iter().map(|f| icloud::pretend::evicted(&site.join(f))).collect();
    let before = listing(&site);
    let (mut said, mut out) = (Vec::new(), Vec::new());
    let wait = || {
        let pool = Prefetcher::with_materializer(
            3,
            Arc::new(|_| Err(std::io::Error::from_raw_os_error(libc::ETIMEDOUT))),
            false,
        );
        Wait::with(pool, icloud::is_still_in_the_cloud, Duration::from_millis(200), Duration::from_millis(10))
    };

    let code = fetch_folder(&site, &wait, &mut |l| said.push(l.to_string()), &mut |l| out.push(l.to_string()));

    assert_eq!(code, 1);
    assert!(out.is_empty(), "no success line: {out:?}");
    let report = said.join("\n");
    assert!(report.contains("Google Drive is not delivering 2 of 2 cloud-only files"), "{report}");
    assert!(report.contains("Open Google Drive so these files can download, then run `moss fetch` again."), "{report}");
    assert_eq!(listing(&site), before, "fetch wrote into the folder");
}

#[test]
fn files_the_provider_delivers_are_reported_fetched() {
    let (_tmp, site) = site_in_a_drive_folder();
    let _in_cloud = icloud::pretend::evicted(&site.join("index.md"));
    let arrived: Arc<Mutex<HashSet<PathBuf>>> = Arc::default();
    let (mut said, mut out) = (Vec::new(), Vec::new());
    let delivered = arrived.clone();
    let seen = arrived.clone();
    let wait = move || {
        let delivered = delivered.clone();
        let seen = seen.clone();
        let pool = Prefetcher::with_materializer(
            3,
            Arc::new(move |p| {
                delivered.lock().unwrap().insert(p.to_path_buf());
                Ok(())
            }),
            false,
        );
        Wait::with(
            pool,
            move |p| icloud::is_still_in_the_cloud(p) && !seen.lock().unwrap().contains(p),
            Duration::from_secs(5),
            Duration::from_millis(10),
        )
    };

    let code = fetch_folder(&site, &wait, &mut |l| said.push(l.to_string()), &mut |l| out.push(l.to_string()));

    assert_eq!(code, 0, "{said:?}");
    assert!(out[0].starts_with("Fetched 1 files"), "{out:?}");
}
