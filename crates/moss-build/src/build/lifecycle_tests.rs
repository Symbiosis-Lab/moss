use super::*;
use crate::build::ship::next_promotion_epoch;

/// A vault under target/test-tmp with `generations/<gen>/<page>/index.html`
/// for every `(gen, page)`, and staging created empty.
fn vault(gens: &[(&str, &str)]) -> (tempfile::TempDir, MossPaths) {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::TempDir::new_in(&base).unwrap();
    let mp = MossPaths::new(tmp.path());
    for (gen, page) in gens {
        let dir = mp.generation_dir(gen).join(page);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), format!("<html>{gen} {page}</html>")).unwrap();
    }
    std::fs::create_dir_all(mp.staging_dir()).unwrap();
    (tmp, mp)
}

fn stage_page(mp: &MossPaths, page: &str) {
    let dir = mp.staging_dir().join(page);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), format!("<html>staged {page}</html>")).unwrap();
}

async fn serve(cell: &ServedCell, port: u16) -> (u16, tokio::sync::oneshot::Sender<()>) {
    crate::ops::serve::start_server(crate::ops::serve::ServeConfig::new(cell.clone(), port))
        .await
        .expect("server starts")
}

fn status(port: u16, path: &str) -> u16 {
    let url = format!("http://localhost:{port}/{path}");
    match ureq::get(&url).timeout(std::time::Duration::from_secs(5)).call() {
        Ok(r) => r.status(),
        Err(ureq::Error::Status(code, _)) => code,
        Err(e) => panic!("transport error fetching {path}: {e}"),
    }
}

fn empty_cell() -> ServedCell {
    Arc::new(RwLock::new(PathBuf::new()))
}

/// A withheld render leaves the preview where the park put it, and the next
/// build may not sweep while the withheld build's workers still write staging.
///
/// Render 1 is shown and promoted; build 2 parks on `current` and renders a
/// page only it has, but could not read its sources. Showing it anyway would
/// flip the outcome. Build 3 then starts while build 2's background workers are
/// still running, and a sweep then would unlink what they are writing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_withheld_render_is_never_shown_and_its_workers_block_the_next_sweep() {
    let (_tmp, mp) = vault(&[("g1", "in-g1")]);
    let _record = lock_for(&mp);
    let cell = empty_cell();
    adopt_server(&mp, &cell);
    let (r1, _) = show_render(&mp, true);
    assert!(promote(&mp, next_promotion_epoch(), Some(r1), "g1").unwrap());

    let build2 = cache_write_lease(&mp);
    assert!(park_for_rebuild(&mp, true, Default::default()).is_some(), "render 1 is on current, so build 2 may sweep");
    assert_eq!(read(&cell), mp.current_ptr());
    stage_page(&mp, "only-in-render-2");
    let (_, announced) = show_render(&mp, false);

    assert_eq!(read(&cell), mp.current_ptr(), "a withheld render must not be shown");
    assert_eq!(snapshot(&mp).0, Some(r1));
    assert_eq!(announced, Some(mp.current_ptr()), "and the served tree announced is current");
    let (port, stop) = serve(&cell, 63100).await;
    assert_eq!(status(port, "only-in-render-2/"), 404);
    assert_eq!(status(port, "in-g1/"), 200);
    let _ = stop.send(());

    let build3 = cache_write_lease(&mp);
    assert!(park_for_rebuild(&mp, true, Default::default()).is_none(), "build 2's workers are still writing staging");
    assert_eq!(read(&cell), mp.current_ptr());
    drop(build2);
    assert!(park_for_rebuild(&mp, true, Default::default()).is_some(), "and once they have joined, build 3 may sweep");
    drop(build3);
}

/// Render numbers are compared only when this process minted both.
///
/// (a) A cold start adopts `current`, and a park there permits without moving.
/// (b) A cell already on staging when this record was made — a dead process's
///     staging, a `current` repaired after adoption — shows a render nobody
///     here numbered, so the park must neither permit nor move: `current` may
///     be older than what is on screen.
/// (c) 404c's double cold start: a second adoption a second after the first
///     render leaves the cell on that render, and the second build, finding
///     nothing promoted yet, does not park.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn render_numbers_from_before_this_record_are_never_compared() {
    // (a)
    let (_tmp, mp) = vault(&[("g1", "old")]);
    let _record = lock_for(&mp);
    mp.set_current_ptr("g1").unwrap();
    stage_page(&mp, "fresh");
    let cell = empty_cell();
    adopt_server(&mp, &cell);
    assert_eq!(read(&cell), mp.current_ptr());
    assert_eq!(snapshot(&mp).0, None);
    assert!(park_for_rebuild(&mp, false, Default::default()).is_some());
    assert_eq!(read(&cell), mp.current_ptr());

    // (b)
    let (_tmp_b, mp_b) = vault(&[("g1", "old")]);
    let _record_b = lock_for(&mp_b);
    mp_b.set_current_ptr("g1").unwrap();
    stage_page(&mp_b, "fresh");
    let cell_b: ServedCell = Arc::new(RwLock::new(mp_b.staging_dir()));
    adopt_server(&mp_b, &cell_b);
    assert!(park_for_rebuild(&mp_b, false, Default::default()).is_none(), "an unnumbered render on staging is no licence");
    assert_eq!(read(&cell_b), mp_b.staging_dir());
    let (port, stop) = serve(&cell_b, 63200).await;
    assert_eq!(status(port, "fresh/"), 200);
    let _ = stop.send(());

    // (c)
    let (_tmp_c, mp_c) = vault(&[("g1", "old")]);
    let _record_c = lock_for(&mp_c);
    mp_c.set_current_ptr("g1").unwrap();
    let cell_c = empty_cell();
    adopt_server(&mp_c, &cell_c);
    show_render(&mp_c, true);
    adopt_server(&mp_c, &cell_c);
    assert_eq!(read(&cell_c), mp_c.staging_dir(), "the second build must not pull the preview off render 1");
    assert!(park_for_rebuild(&mp_c, false, Default::default()).is_none());
    assert_eq!(read(&cell_c), mp_c.staging_dir());
}

/// A record something still holds survives a folder switch; an idle one is
/// dropped, and the folder comes back with nothing to compare.
#[test]
fn a_folder_switch_evicts_only_idle_records() {
    let (_a_tmp, a) = vault(&[]);
    let (_b_tmp, b) = vault(&[]);
    let (_c_tmp, c) = vault(&[]);

    let lease = cache_write_lease(&a);
    let (first, _) = show_render(&a, false);
    lock_for(&b);
    let (second, _) = show_render(&a, false);
    assert_eq!(second, first + 1, "a leased record must survive a switch to another folder");
    assert_eq!(snapshot(&a).2, 1);

    drop(lease);
    lock_for(&c);
    assert_eq!(show_render(&a, false).0, 1, "an idle record is evicted, so the folder starts over");
    assert_eq!(snapshot(&a), (None, None, 0));
}

/// A seal stuck in `await_completion` holds its lease for as long as it is
/// stuck (925 s in one field log). Nothing a later build does may wait on it.
#[test]
fn a_stuck_seal_blocks_no_later_build() {
    let (_tmp, mp) = vault(&[]);
    let _record = lock_for(&mp);
    let stuck = cache_write_lease(&mp);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let root = mp.project_root().to_path_buf();
    std::thread::spawn(move || {
        let mp = MossPaths::new(&root);
        let lease = cache_write_lease(&mp);
        let permit = park_for_rebuild(&mp, true, Default::default());
        let _ = done_tx.send(permit.is_none());
        drop(lease);
    });
    let refused = done_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("a lease and a park must return while another build's lease is open");
    assert!(refused, "and the park must refuse a sweep");
    drop(stuck);
}

/// A running cache GC is the one thing a lease waits for, and only for the
/// length of the walk: a writer that started mid-sweep would store blobs the
/// sweep has already judged unreferenced.
#[test]
fn a_lease_waits_out_a_running_cache_gc_and_only_that() {
    let (_tmp, mp) = vault(&[]);
    let _record = lock_for(&mp);
    let token = try_begin_cache_gc(&mp).expect("nothing is writing");
    assert!(try_begin_cache_gc(&mp).is_err(), "one sweep at a time");
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let root = mp.project_root().to_path_buf();
    std::thread::spawn(move || {
        let lease = cache_write_lease(&MossPaths::new(&root));
        let _ = done_tx.send(());
        drop(lease);
    });
    assert!(
        done_rx.recv_timeout(std::time::Duration::from_millis(200)).is_err(),
        "a lease must not start while a sweep is walking the store"
    );
    drop(token);
    done_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("and must start as soon as the sweep ends");
}

/// Seal tails finish in worker-completion order, so build N's can land after
/// build N+1's. The epoch refuses it, a retried tail is refused the same way,
/// and one folder's epochs never refuse another's.
#[test]
fn promotion_refuses_older_and_repeated_epochs_per_folder() {
    let (_tmp, mp) = vault(&[("genN", "a"), ("genN1", "a")]);
    let _record = lock_for(&mp);
    let epoch_n = next_promotion_epoch();
    let epoch_n1 = next_promotion_epoch();

    assert!(promote(&mp, epoch_n1, None, "genN1").unwrap());
    assert!(!promote(&mp, epoch_n, None, "genN").unwrap(), "a tail from an older build must not promote");
    assert!(!promote(&mp, epoch_n1, None, "genN1").unwrap(), "nor a repeat of the epoch on current");
    assert_eq!(mp.current_generation_id().unwrap(), "genN1");

    let (_other_tmp, other) = vault(&[("g", "a")]);
    let _other_record = lock_for(&other);
    assert!(promote(&other, epoch_n, None, "g").unwrap(), "another folder's newer epoch refuses nothing here");
}
