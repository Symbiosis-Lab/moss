//! Every public path a site with every generator on serves, pinned.
//!
//! A path the build writes is a published URL: feeds, sitemaps and cards are
//! stored by readers and machines outside the site. The publish gate only
//! guards the author's own pages, so a generator that stops serving one of
//! its paths is caught here. A path in the list that the build no longer
//! serves fails until `feeds::redirect_table::FORMER_PATHS` keeps it answering;
//! a new path fails until it is added to the list, deliberately:
//!
//! ```sh
//! SERVED_PATHS=overwrite cargo test -p moss-build --test served_path_inventory_test
//! ```
//!
//! Overwriting only adds: a line leaves the list by a hand edit a reviewer
//! sees, never as a side effect of regenerating it.
//!
//! A video's ladder files are left out: they exist only where ffmpeg does,
//! and their names are pinned by `hls_ladder_shape_test`.

#[path = "support/site.rs"]
mod site;

use site::Site;
use std::collections::BTreeSet;

const LIST: &str = "tests/served_paths.txt";

/// A one-pixel PNG, so the picture gets its derivatives.
const PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d,
    0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

const PAGES: &[(&str, &str)] = &[
    ("index.md", "---\ntitle: River Notes\n---\n\nThe water came up past the second step.\n"),
    ("about.md", "---\ntitle: About\n---\n\nA small site about the river.\n"),
    ("notes/spring.md", "---\ntitle: Spring\ntags: [flood]\n---\n\nFlood, day three.\n\n![Bank](bank.png)\n"),
    ("talk.md", "---\ntitle: Talk\nstart: 2099-03-01 19:30\n---\n\nAn evening by the water.\n"),
];

const CONFIG: &str = "domain = \"example.com\"\nai_answers = true\nai_training = false\nselection_tools = true\n";

/// A content hash in a file name is part of the file, not of its address.
fn without_hash(path: &str) -> String {
    let mut out = Vec::new();
    for part in path.split('.') {
        let hashed = part.len() >= 8 && part.bytes().all(|b| b.is_ascii_hexdigit());
        out.push(if hashed { "HASH" } else { part });
    }
    out.join(".")
}

fn served(site: &Site) -> BTreeSet<String> {
    let staging = site.output("");
    walkdir::WalkDir::new(&staging)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(&staging).unwrap().to_string_lossy().replace('\\', "/"))
        .filter(|p| moss_build::build::served_path::is_public_address(p))
        .map(|p| without_hash(&p))
        .collect()
}

#[test]
fn a_site_with_every_generator_on_serves_the_listed_paths() {
    let site = Site::with_config(CONFIG, PAGES);
    std::fs::write(site.root.join("notes/bank.png"), PIXEL_PNG).unwrap();
    site.rebuild();
    let now = served(&site);
    let list = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(LIST);
    let listed: BTreeSet<String> =
        std::fs::read_to_string(&list).unwrap().lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
    if std::env::var("SERVED_PATHS").as_deref() == Ok("overwrite") {
        std::fs::write(&list, listed.union(&now).map(|p| format!("{p}\n")).collect::<String>()).unwrap();
        return;
    }
    let gone: Vec<_> = listed.difference(&now).collect();
    assert!(
        gone.is_empty(),
        "the build no longer serves {gone:?}: links and subscriptions to them break. Keep each answering \
         with an entry in `feeds::redirect_table::FORMER_PATHS`."
    );
    let added: Vec<_> = now.difference(&listed).collect();
    assert!(added.is_empty(), "new public paths {added:?}: add them to {LIST} (SERVED_PATHS=overwrite).");
}
