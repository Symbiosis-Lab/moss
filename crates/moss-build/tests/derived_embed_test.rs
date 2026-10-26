//! `![[file.ext#fragment]]` through the whole build, with a made-up extension
//! and a shell-script deriver so nothing here knows a real format.

use std::fs;
use std::path::{Path, PathBuf};

use moss_build::system::derive::site::DeriveMode;
use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
use moss_build::cli::host::cli_host_ports;
use moss_build::vault_root::VaultRoot;

const DERIVER: &str = r###"#!/bin/sh
REQ=$(cat)
case "$REQ" in
  *locked*) printf '%s' '{"kind":"skip","reason":"locked","message":"This <file> is locked."}' ;;
  *) printf '%s' '{"kind":"derived","fragments":{"text":"## Hello\n\nworld"},"facts":{"title":"From Facts","description":"8 pages, scanned"}}' ;;
esac
"###;

struct Site {
    dir: tempfile::TempDir,
    /// Outlives each build: the seal runs on it after the build returns, as in
    /// a serving process.
    rt: tokio::runtime::Runtime,
}

impl Site {
    fn new(files: &[(&str, &str)]) -> Self {
        let site = Site { dir: tempfile::tempdir().unwrap(), rt: tokio::runtime::Runtime::new().unwrap() };
        let plugin = site.root().join(".moss/plugins/fake-reader");
        fs::create_dir_all(&plugin).unwrap();
        site.set_deriver(DERIVER);
        let manifest = serde_json::json!({
            "name": "fake-reader",
            "version": "1.0.0",
            "contributes": {"derivations": {"reader": {
                "accepts": ["fake"],
                "noun": {"one": "fake", "other": "fakes"},
                "fragments": {"text": "markdown"},
                "stub": ["#text"],
                "run": {"binary": "deriver", "artifact": [{"kind": "path", "binary": site.deriver_path()}]}
            }}}
        });
        fs::write(plugin.join("manifest.json"), manifest.to_string()).unwrap();
        for (name, body) in files {
            fs::create_dir_all(site.root().join(name).parent().unwrap()).unwrap();
            fs::write(site.root().join(name), body).unwrap();
        }
        site
    }

    fn set_version(&self, version: &str) {
        let path = self.root().join(".moss/plugins/fake-reader/manifest.json");
        let mut manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        manifest["version"] = version.into();
        fs::write(path, manifest.to_string()).unwrap();
    }

    fn root(&self) -> &Path {
        // The path the build sees; the tempdir may sit behind a symlink.
        self.dir.path()
    }

    fn deriver_path(&self) -> PathBuf {
        self.root().join(".moss/plugins/fake-reader/deriver.sh")
    }

    fn set_deriver(&self, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.deriver_path();
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Leave the deriver's bytes alone but make it unrunnable, so a build that
    /// still shows its text was answered from the cache.
    fn disable_deriver(&self) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(self.deriver_path(), fs::Permissions::from_mode(0o644)).unwrap();
    }

    /// The page as the last sealed generation holds it: what a publish would ship.
    fn sealed_page(&self, slug: &str) -> Option<String> {
        [format!("{slug}/index.html"), format!("{slug}.html")].iter().find_map(|candidate| self.sealed_file(candidate))
    }

    /// A file of the last sealed generation, by its served path.
    fn sealed_file(&self, served: &str) -> Option<String> {
        // What a publish does first: finish the materialize the seal queued.
        self.rt.block_on(moss_build::build::seal_phase::settle(&moss_build::moss_paths::MossPaths::new(self.root())));
        fs::read_to_string(self.root().join(".moss/build.nosync/current").join(served)).ok()
    }

    fn build(&self, plugins: PluginMode, mode: DeriveMode) {
        let folder = self.root().to_string_lossy().to_string();
        self.rt.block_on(run_pipeline(PipelineConfig {
            root: VaultRoot::resolve(self.root()),
            progress: moss_build::build::stdout_sink(),
            plugins,
            watch: false,
            start_server: false,
            host: cli_host_ports(&folder),
            trigger: BuildTrigger::Full,
            exits_after_build: mode == DeriveMode::Await,
            site_url_override: None,
            server_port: None,
            admission_epoch: None,
            live_port: None,
        }))
        .expect("the build must succeed");
    }

    fn page(&self, slug: &str) -> String {
        let staging = self.root().join(".moss/build.nosync/staging");
        for candidate in [format!("{slug}/index.html"), format!("{slug}.html")] {
            if let Ok(html) = fs::read_to_string(staging.join(&candidate)) {
                return html;
            }
        }
        panic!("no page `{slug}` under {}", staging.display());
    }
}

fn title_of(html: &str) -> String {
    let start = html.find("<title>").expect("a <title>") + "<title>".len();
    let end = html[start..].find("</title>").unwrap();
    html[start..start + end].to_string()
}

#[test]
fn derived_text_is_rendered_and_names_an_inferring_page() {
    let site = Site::new(&[
        ("a.fake", "bytes"),
        ("page.md", "---\ninferred: [title]\n---\n\nbefore\n\n![[a.fake#text]]\n"),
    ]);
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let html = site.page("page");
    assert!(html.contains("<h2"), "{html}");
    assert!(html.contains("Hello"), "{html}");
    assert!(html.contains("world"), "{html}");
    assert!(!html.contains("moss-derived--"), "a ready derivation leaves no status block");
    assert!(title_of(&html).contains("From Facts"), "{}", title_of(&html));
}

#[test]
fn ready_text_is_wrapped_and_still_renders_as_markdown_inside() {
    let site = Site::new(&[("a.fake", "bytes"), ("page.md", "before\n\n![[a.fake#text]]\n\nafter\n")]);
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let html = site.page("page");
    let open = html.find("<div class=\"moss-derived\" data-fragment=\"text\"").unwrap_or_else(|| panic!("no wrapper in {html}"));
    let heading = html[open..].find("<h2").map(|at| open + at).expect("the heading renders inside the wrapper");
    let close = html[heading..].find("</div>").map(|at| heading + at).expect("the wrapper closes after its text");
    assert!(html[heading..close].contains("Hello") && html[heading..close].contains("world"), "{html}");
    assert!(!html.contains("## Hello"), "the heading must not be left as source text: {html}");
}

fn meta_description(html: &str) -> String {
    html.split("name=\"description\" content=\"").nth(1).and_then(|rest| rest.split('"').next()).unwrap_or("").to_string()
}

/// A folder of two pages, one of them embedding derived text, built to a
/// listing at `docs/` and to `llms.txt`.
fn listed_site() -> Site {
    Site::new(&[
        ("docs/a.fake", "bytes"),
        ("docs/item.md", "---\ninferred: [title]\n---\n\n![[a.fake#text]]\n"),
        ("docs/other.md", "---\ntitle: Other\n---\n\nplain words\n"),
    ])
}

#[test]
fn a_listing_card_shows_the_derived_words_and_no_wrapper_markup() {
    let site = listed_site();
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let listing = site.page("docs");
    assert!(listing.contains("From Facts"), "the listing names the derived page: {listing}");
    assert!(listing.contains("world"), "the card shows the derived words: {listing}");
    assert!(!listing.contains("moss-derived") && !listing.contains("&lt;div"), "{listing}");
    let page = site.page("docs/item");
    let meta = meta_description(&page);
    assert!(meta.contains("world") && !meta.contains("div"), "meta description {meta:?} in {page}");
}

#[test]
fn an_inferred_description_fills_the_meta_description_and_the_listing_card() {
    let site = Site::new(&[
        ("docs/a.fake", "bytes"),
        ("docs/item.md", "---\ninferred: [title, description]\n---\n\n![[a.fake#text]]\n"),
        ("docs/other.md", "---\ntitle: Other\n---\n\nplain words\n"),
    ]);
    site.build(PluginMode::Blocking, DeriveMode::Await);
    assert_eq!(meta_description(&site.page("docs/item")), "8 pages, scanned");
    let listing = site.page("docs");
    assert!(listing.contains("8 pages, scanned"), "the card shows the derived description: {listing}");
    assert!(listing.contains("plain words"), "other cards are unaffected: {listing}");
}

#[test]
fn a_written_description_beats_the_inferred_one() {
    let site = Site::new(&[
        ("docs/a.fake", "bytes"),
        ("docs/item.md", "---\ndescription: Mine\ninferred: [title, description]\n---\n\n![[a.fake#text]]\n"),
        ("docs/other.md", "---\ntitle: Other\n---\n\nplain words\n"),
    ]);
    site.build(PluginMode::Blocking, DeriveMode::Await);
    assert_eq!(meta_description(&site.page("docs/item")), "Mine");
    let listing = site.page("docs");
    assert!(listing.contains("Mine") && !listing.contains("8 pages, scanned"), "{listing}");
}

#[test]
fn without_description_in_inferred_the_description_stays_the_page_text() {
    let site = listed_site();
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let meta = meta_description(&site.page("docs/item"));
    assert!(meta.contains("world") && !meta.contains("scanned"), "{meta}");
    assert!(!site.page("docs").contains("8 pages, scanned"));
}

#[test]
fn llms_txt_carries_the_derived_words_and_no_wrapper_markup() {
    let site = listed_site();
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let llms = site.sealed_file("llms.txt").expect("llms.txt in the sealed generation");
    assert!(llms.contains("world"), "{llms}");
    assert!(!llms.contains("<div") && !llms.contains("</div>") && !llms.contains("moss-derived"), "{llms}");
}

#[test]
fn a_written_title_beats_the_inferred_one() {
    let site = Site::new(&[
        ("a.fake", "bytes"),
        ("page.md", "---\ntitle: Mine\ninferred: [title]\n---\n\n![[a.fake#text]]\n"),
    ]);
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let title = title_of(&site.page("page"));
    assert!(title.contains("Mine") && !title.contains("From Facts"), "{title}");
}

#[test]
fn without_inferred_the_title_stays_the_filename() {
    let site = Site::new(&[("a.fake", "bytes"), ("page.md", "![[a.fake#text]]\n")]);
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let title = title_of(&site.page("page"));
    assert!(!title.contains("From Facts"), "{title}");
}

#[test]
fn a_skipped_file_shows_its_message_in_a_note() {
    let site = Site::new(&[("locked.fake", "bytes"), ("page.md", "![[locked.fake#text]]\n")]);
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let html = site.page("page");
    assert!(html.contains("moss-derived--skipped"), "{html}");
    assert!(html.contains("This &lt;file&gt; is locked."), "{html}");
}

#[test]
fn a_second_build_reads_the_cache_not_the_deriver() {
    let site = Site::new(&[("a.fake", "bytes"), ("page.md", "![[a.fake#text]]\n")]);
    site.build(PluginMode::Blocking, DeriveMode::Await);
    site.disable_deriver();
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let html = site.page("page");
    assert!(html.contains("Hello"), "{html}");
}

#[test]
fn a_failed_read_shows_a_generic_note_and_is_not_retried_in_a_loop() {
    let site = Site::new(&[("a.fake", "bytes"), ("page.md", "![[a.fake#text]]\n")]);
    site.set_deriver("#!/bin/sh\ncat >/dev/null\necho 'not json'\n");
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let html = site.page("page");
    assert!(html.contains("moss-derived--skipped"), "{html}");
    assert!(html.contains("couldn&#x27;t be read") || html.contains("couldn't be read"), "{html}");
}

#[test]
fn derived_text_cannot_inject_html_into_the_page() {
    let site = Site::new(&[("a.fake", "bytes"), ("page.md", "![[a.fake#text]]\n")]);
    site.set_deriver(
        "#!/bin/sh\ncat >/dev/null\nprintf '%s' '{\"kind\":\"derived\",\"fragments\":{\"text\":\"<img src=x onerror=alert(1)>\"},\"facts\":{}}'\n",
    );
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let html = site.page("page");
    assert!(!html.contains("<img src=x onerror"), "{html}");
    assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"), "{html}");
}

/// A deriver that waits `delay` seconds and then answers with `answer`.
fn slow_deriver(delay: u32, answer: &str) -> String {
    format!("#!/bin/sh\ncat >/dev/null\nsleep {delay}\n{answer}\n")
}

const GOOD_ANSWER: &str = r###"printf '%s' '{"kind":"derived","fragments":{"text":"## Hello\n\nworld"},"facts":{}}'"###;

fn wait_for(what: &str, secs: u64, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn adopt_seals_the_placeholder_while_the_read_runs_then_asks_for_a_rebuild() {
    let site = Site::new(&[("a.fake", "bytes"), ("page.md", "![[a.fake#text]]\n")]);
    site.set_deriver(&slow_deriver(6, GOOD_ANSWER));
    let folder = site.root().to_string_lossy().to_string();
    let watcher = moss_build::ops::watch::worker::register(&folder);

    site.build(PluginMode::NonBlocking, DeriveMode::Adopt);
    assert!(site.page("page").contains("moss-derived--pending"));
    assert_eq!(moss_build::system::build_records::records().pending_derivations(&folder), Some(1));

    // The generation a publish would ship seals with the placeholder in it
    // while the read is still running; it does not wait for the read.
    wait_for("the placeholder generation to seal", 20, || {
        site.sealed_page("page").is_some_and(|html| html.contains("moss-derived--pending"))
    });
    assert!(!watcher.slot_occupied(), "the read finished before the generation sealed");
    assert!(
        moss_build::deploy::refuse_publish(&folder).is_err(),
        "a pending block must hold back a publish"
    );

    wait_for("the background read to ask for a rebuild", 30, || watcher.slot_occupied());
    // The rebuild the watcher would run: the text comes from the cache.
    watcher.take();
    site.build(PluginMode::NonBlocking, DeriveMode::Adopt);
    let second = site.page("page");
    assert!(second.contains("Hello") && !second.contains("moss-derived--pending"), "{second}");
    assert_eq!(moss_build::system::build_records::records().pending_derivations(&folder), Some(0));
    moss_build::ops::watch::worker::deregister(&folder, &watcher);
}

const NEW_ANSWER: &str = r###"printf '%s' '{"kind":"derived","fragments":{"text":"## Fresh\n\nnewer"},"facts":{}}'"###;

/// A site derived once under version 1.0.0, then re-keyed by a version bump
/// with a deriver that answers `NEW_ANSWER` after a delay.
fn rekeyed_site(page_bytes_after: Option<&str>) -> Site {
    let site = Site::new(&[("a.fake", "bytes"), ("page.md", "![[a.fake#text]]\n")]);
    site.build(PluginMode::Blocking, DeriveMode::Await);
    assert!(site.page("page").contains("world"));
    site.set_deriver(&slow_deriver(4, NEW_ANSWER));
    site.set_version("1.1.0");
    if let Some(bytes) = page_bytes_after {
        fs::write(site.root().join("a.fake"), bytes).unwrap();
    }
    site
}

#[test]
fn a_re_keyed_source_keeps_its_previous_text_until_the_new_read_lands() {
    let site = rekeyed_site(None);
    let folder = site.root().to_string_lossy().to_string();
    let watcher = moss_build::ops::watch::worker::register(&folder);

    site.build(PluginMode::NonBlocking, DeriveMode::Adopt);
    let html = site.page("page");
    assert!(html.contains("world") && !html.contains("moss-derived--pending"), "{html}");
    // The re-read is still owed: the preview counts it and a publish is held back.
    assert_eq!(moss_build::system::build_records::records().pending_derivations(&folder), Some(1));
    assert!(moss_build::deploy::refuse_publish(&folder).is_err(), "an older result must not publish silently");

    wait_for("the re-read to ask for a rebuild", 30, || watcher.slot_occupied());
    watcher.take();
    site.build(PluginMode::NonBlocking, DeriveMode::Adopt);
    let html = site.page("page");
    assert!(html.contains("newer") && !html.contains("world"), "{html}");
    assert_eq!(moss_build::system::build_records::records().pending_derivations(&folder), Some(0));
    moss_build::ops::watch::worker::deregister(&folder, &watcher);
}

#[test]
fn a_re_keyed_source_whose_bytes_changed_gets_no_older_text() {
    let site = rekeyed_site(Some("other bytes"));
    let folder = site.root().to_string_lossy().to_string();
    let watcher = moss_build::ops::watch::worker::register(&folder);
    site.build(PluginMode::NonBlocking, DeriveMode::Adopt);
    let html = site.page("page");
    assert!(html.contains("moss-derived--pending") && !html.contains("world"), "{html}");
    moss_build::ops::watch::worker::deregister(&folder, &watcher);
}

#[test]
fn a_failed_background_read_is_shown_once_and_does_not_loop() {
    let site = Site::new(&[("a.fake", "bytes"), ("page.md", "![[a.fake#text]]\n")]);
    site.set_deriver(&slow_deriver(0, "echo 'not json'"));
    let folder = site.root().to_string_lossy().to_string();
    let watcher = moss_build::ops::watch::worker::register(&folder);

    site.build(PluginMode::NonBlocking, DeriveMode::Adopt);
    wait_for("the background read to ask for a rebuild", 30, || watcher.slot_occupied());
    watcher.take();

    site.build(PluginMode::NonBlocking, DeriveMode::Adopt);
    let html = site.page("page");
    assert!(html.contains("moss-derived--skipped") && !html.contains("moss-derived--pending"), "{html}");
    assert_eq!(moss_build::system::build_records::records().pending_derivations(&folder), Some(0));

    // Were the failure forgotten, this build would start the read again and
    // ask for yet another rebuild, forever.
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert!(!watcher.slot_occupied(), "the failed read must not be started again and ask for another rebuild");
    moss_build::ops::watch::worker::deregister(&folder, &watcher);
}

#[test]
fn a_publish_retries_a_read_that_failed_while_previewing() {
    let site = Site::new(&[("a.fake", "bytes"), ("page.md", "![[a.fake#text]]\n")]);
    let flag = site.root().join("fail-now");
    fs::write(&flag, "").unwrap();
    // One script whose bytes never change: it fails only while the flag exists.
    site.set_deriver(&format!(
        "#!/bin/sh\ncat >/dev/null\nif [ -f '{}' ]; then echo 'timed out' >&2; exit 1; fi\n{GOOD_ANSWER}\n",
        flag.display()
    ));
    let folder = site.root().to_string_lossy().to_string();
    let watcher = moss_build::ops::watch::worker::register(&folder);
    site.build(PluginMode::NonBlocking, DeriveMode::Adopt);
    wait_for("the background read to ask for a rebuild", 30, || watcher.slot_occupied());
    moss_build::ops::watch::worker::deregister(&folder, &watcher);

    // The environment recovers; the same long-lived process now builds to publish.
    fs::remove_file(&flag).unwrap();
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let html = site.page("page");
    assert!(html.contains("Hello") && !html.contains("moss-derived--"), "{html}");
}

#[test]
fn with_plugins_off_the_embed_is_left_to_the_viewer() {
    let site = Site::new(&[("a.fake", "bytes"), ("page.md", "![[a.fake#text]]\n")]);
    site.build(PluginMode::Skip, DeriveMode::Await);
    let html = site.page("page");
    assert!(!html.contains("Hello") && !html.contains("moss-derived"), "{html}");
}

#[test]
fn expand_derives_missing_text_and_writes_it_into_the_page() {
    let page = "---\ntitle:   Kept  \n---\nintro\n![[a.fake#text]]\n![[locked.fake#text]]\n![[note#Part]]\n";
    let site = Site::new(&[
        ("a.fake", "bytes"),
        // Different bytes: the cache is keyed by content, so equal bytes would share a result.
        ("locked.fake", "locked bytes"),
        ("note.md", "# Part\n\nfrom the note\n\n# Other\n\nnot this\n"),
        ("page.md", page),
    ]);
    let target = site.root().join("page.md");

    let dry = moss_build::cli::expand::expand_in(&target, site.root(), true).unwrap();
    assert!(dry.last().unwrap().starts_with("would expand 2 embeds in 1 page"), "{dry:?}");
    assert_eq!(fs::read_to_string(&target).unwrap(), page, "--dry-run must not write");

    let done = moss_build::cli::expand::expand_in(&target, site.root(), false).unwrap();
    assert!(done.last().unwrap().starts_with("expanded 2 embeds"), "{done:?}");
    let written = fs::read_to_string(&target).unwrap();
    assert_eq!(
        written,
        "---\ntitle:   Kept  \n---\nintro\n\n## Hello\n\nworld\n\n![[locked.fake#text]]\n# Part\n\nfrom the note\n",
    );

    let again = moss_build::cli::expand::expand_in(&target, site.root(), false).unwrap();
    assert_eq!(again, vec!["nothing to expand".to_string()]);
}

const AWKWARD_TEXT: &str = "<img src=x onerror=alert(1)> and <b>bold</b>\n\n\
Awarded $90,940 and \\$7 \u{2026} \\$8\n\n\
%%c%% {++x++} {--y--} [[x]] \\[[y]] ![[z]] {{y}} ^blk\n\n\
:::note\nbody\n:::\n\n\
#1 priority, C++, x^2, AT&T, 5 <= 6\n";

fn awkward_site() -> Site {
    let site = Site::new(&[("a.fake", "bytes"), ("page.md", "above\n\n![[a.fake#text]]\n---\nbelow\n")]);
    let answer = serde_json::json!({"kind": "derived", "fragments": {"text": AWKWARD_TEXT}, "facts": {}});
    fs::write(site.root().join("answer.json"), answer.to_string()).unwrap();
    site.set_deriver(&format!("#!/bin/sh\ncat >/dev/null\ncat '{}'\n", site.root().join("answer.json").display()));
    site
}

/// The article of the built page, without the head, the site chrome or the
/// source line numbers (which differ once an embed is written out as text).
fn article(html: &str) -> String {
    let start = html.find("<article").expect("an article");
    let body = &html[start..html.find("</article>").expect("the end of the article")];
    let mut out = String::new();
    let mut rest = body;
    while let Some(at) = rest.find(" data-source-line=\"") {
        out.push_str(&rest[..at]);
        let value = &rest[at + " data-source-line=\"".len()..];
        rest = &value[value.find('"').unwrap() + 1..];
    }
    out + rest
}

#[test]
fn derived_text_renders_as_plain_text_and_ordinary_prose_is_untouched() {
    let site = awkward_site();
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let html = site.page("page");
    let body = &article(&html);
    for expected in [
        "&lt;img src=x onerror=alert(1)&gt; and &lt;b&gt;bold&lt;/b&gt;",
        "Awarded $90,940 and $7 \u{2026} $8",
        "%%c%% {++x++} {--y--} [[x]] [[y]] ![[z]] {{y}} ^blk",
        ":::note",
        "#1 priority, C++, x^2, AT&amp;T, 5 &lt;= 6",
    ] {
        assert!(body.contains(expected), "missing {expected:?} in {body}");
    }
    for live in ["<img", "<b>", "<ins", "<del", "<a ", "math", "<aside", "class=\"tag"] {
        assert!(!body.contains(live), "{live:?} is live in {body}");
    }
}

#[test]
fn an_expanded_page_renders_as_the_embed_did() {
    let site = awkward_site();
    site.build(PluginMode::Blocking, DeriveMode::Await);
    let before = site.page("page");

    let target = site.root().join("page.md");
    moss_build::cli::expand::expand_in(&target, site.root(), false).unwrap();
    assert!(!fs::read_to_string(&target).unwrap().contains("![[a.fake"));
    site.build(PluginMode::Blocking, DeriveMode::Await);
    // Only the wrapper's own two lines differ from the written-out text.
    let unwrapped = article(&before)
        .replacen("<div class=\"moss-derived\" data-fragment=\"text\">\n", "", 1)
        .replacen("</div><!-- /moss-derived -->\n", "", 1);
    assert_eq!(article(&site.page("page")), unwrapped);
}

#[test]
fn an_embed_sharing_its_line_renders_as_before_and_leaks_no_marker() {
    let site = Site::new(&[
        ("a.fake", "bytes"),
        ("page.md", "> ![[a.fake#text]]\n\n- item ![[a.fake#text]]\n\nsee ![[a.fake#text]] now\n"),
    ]);
    site.build(PluginMode::Blocking, DeriveMode::Await);
    // The page may carry the picture viewer, which opens with its own comment.
    let html = site.page("page").replace("<!-- Lightbox -->", "");
    assert!(!html.contains("moss-derived:") && !html.contains("<!--"), "{html}");
}
