//! A width word on an image only takes effect when the image is alone in its
//! paragraph; anywhere else the build says it was ignored.
//!
//! Its own test binary: `log::set_logger` is process-global and one-shot.

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

fn notes_for(md: &str) -> Vec<String> {
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
    CAPTURE.0.lock().unwrap().iter().filter(|w| w.contains("width word")).cloned().collect()
}

#[test]
fn a_width_word_on_an_image_inside_a_paragraph_is_reported_and_one_alone_is_not() {
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Warn);

    let inline = notes_for("The lake at dawn ![A lake|wide|](lake.jpg) before the crowds.\n");
    assert_eq!(inline.len(), 1, "{inline:?}");
    assert!(
        inline[0].contains("lake.jpg") && inline[0].contains("notes/trip.md") && inline[0].contains("'wide'") && inline[0].contains("own paragraph"),
        "{inline:?}"
    );

    let wiki = notes_for("The lake at dawn ![[lake.jpg|wide]] before the crowds.\n");
    assert_eq!(wiki.len(), 1, "{wiki:?}");
    assert!(notes_for("![[lake.jpg|wide]]\n").is_empty());

    assert!(notes_for("![A lake|wide|](lake.jpg)\n").is_empty());
    assert!(notes_for("The lake at dawn ![A lake](lake.jpg) before the crowds.\n").is_empty());
}
