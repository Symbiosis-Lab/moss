//! What the hash index reports about a build: `[cache] hash-index: N hits, M rehashed`.
//!
//! Its own test binary, and one test, because the counts are process-wide: inside the
//! lib's test process every parallel test that touches an index moves them, and an
//! exact count is only checkable where nothing else runs.

use moss_build::build::cache::{report_hash_index_activity, FileStat, HashIndex};
use std::sync::Mutex;

struct Capture(Mutex<Vec<String>>);

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.0.lock().unwrap().push(record.args().to_string());
    }
    fn flush(&self) {}
}

static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));

fn reported() -> String {
    report_hash_index_activity();
    CAPTURE.0.lock().unwrap().pop().expect("the report printed a line")
}

#[test]
fn the_index_reports_its_hits_and_the_files_it_rehashed() {
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pic.png");
    std::fs::write(&file, b"pixels").unwrap();
    let stat = FileStat::of(&std::fs::metadata(&file).unwrap());

    assert_eq!(reported(), "[cache] hash-index: 0 hits, 0 rehashed", "nothing has touched an index yet");

    let mut index = HashIndex::new();
    // Hashed for want of an entry: one rehash. A hit and a whole-second hit answer
    // from it; a lookup that finds another instant of the file is a miss, counted as
    // neither, and so is carrying an entry forward.
    index.update("a.png".to_string(), &stat, "hash".to_string());
    assert_eq!(index.lookup("a.png", &stat), Some("hash"));
    assert_eq!(index.lookup_whole_second("a.png", stat.size, stat.mtime), Some("hash"));
    assert_eq!(index.lookup("a.png", &FileStat { size: stat.size + 1, ..stat }), None);
    let mut next = HashIndex::new();
    next.carry_forward(&index, "a.png");
    assert_eq!(reported(), "[cache] hash-index: 2 hits, 1 rehashed");

    // `resolve` is one of each: the first call hashes the file, the second is answered.
    index.resolve(&file, "pic.png").unwrap();
    index.resolve(&file, "pic.png").unwrap();
    assert_eq!(reported(), "[cache] hash-index: 1 hits, 1 rehashed");

    assert_eq!(reported(), "[cache] hash-index: 0 hits, 0 rehashed", "a report starts the count over");
}
