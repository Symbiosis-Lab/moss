//! An image with no alt text and no caption is reported, by file and page, as
//! an advisory: it reaches the log but not the `--strict` problem count, so
//! existing sites with undescribed images keep building clean.
//!
//! Its own test binary: `log::set_logger` is process-global and one-shot.

use moss_build::build::cli_output::take_cli_problems;
use moss_build::build::markdown::{process_markdown_file, PageContext, SiteMarkdown};
use std::collections::HashMap;
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

fn warnings_for(md: &str) -> Vec<String> {
    CAPTURE.0.lock().unwrap().clear();
    let map = HashMap::new();
    process_markdown_file(
        "notes/trip.md",
        md,
        "site",
        &map,
        false,
        moss_build::i18n::Language::En,
        None,
        SiteMarkdown { implicit_figure: true, ..Default::default() },
        None,
        None,
        None,
        PageContext::default(),
    )
    .expect("parses");
    CAPTURE.0.lock().unwrap().clone()
}

#[test]
fn undescribed_images_are_named_with_their_page_and_are_not_strict_problems() {
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Warn);
    let _ = take_cli_problems();

    let bare = warnings_for("![](lake.jpg)\n");
    assert!(
        bare.iter().any(|w| w.contains("lake.jpg") && w.contains("notes/trip.md")),
        "expected a warning naming the image and the page, got: {bare:?}"
    );
    assert_eq!(take_cli_problems(), 0, "an advisory must not fail --strict");

    assert!(warnings_for("![A lake](lake.jpg)\n").iter().all(|w| !w.contains("lake.jpg")));
    assert!(warnings_for("![](lake.jpg)\n*Credit*\n").iter().all(|w| !w.contains("lake.jpg")));

    // Not an image file: a video written in the image form takes its label elsewhere.
    assert!(warnings_for("![](clip.mp4)\n").iter().all(|w| !w.contains("clip.mp4")));

    // A table cell is walked too.
    let table = warnings_for("| a |\n|---|\n| ![](cell.png) |\n");
    assert!(table.iter().any(|w| w.contains("cell.png")), "got: {table:?}");
}
