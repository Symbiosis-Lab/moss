//! A site declares its own frontmatter fields in `.moss/config.toml`, and a
//! page whose value does not fit the declared type fails a `--strict` build
//! through the real binary. All names and text are invented.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

struct Site {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn site(config: &str, pages: &[(&str, &str)]) -> Site {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("site");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", config);
    write(&root, "index.md", "---\ntitle: Home\n---\nWelcome\n");
    for (rel, text) in pages {
        write(&root, rel, text);
    }
    Site { _tmp: tmp, home, root }
}

fn build_strict(s: &Site) -> Output {
    Command::new(MOSS)
        .arg("build")
        .arg(&s.root)
        .arg("--strict")
        .env("MOSS_HOME", &s.home)
        .env_remove("MOSS_ENV")
        .output()
        .expect("spawn moss-cli")
}

const KIND: &str = "[fields.kind]\ntype = \"choice\"\nof = [\"Building\", \"Service\"]\n";

#[test]
fn a_value_outside_its_declared_choice_fails_a_strict_build_and_names_the_page() {
    let s = site(KIND, &[("Housing/court.md", "---\ntitle: Court\nkind: Buidling\n---\nbody\n")]);
    let o = build_strict(&s);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "a strict build must fail: {err}");
    assert!(
        err.contains("[Housing/court.md] frontmatter: field 'kind'") && err.contains("\"Buidling\" is not one of: Building, Service"),
        "{err}"
    );
}

#[test]
fn a_declared_value_that_fits_builds_clean_under_strict() {
    let s = site(KIND, &[("Housing/court.md", "---\ntitle: Court\nkind: Building\n---\nbody\n")]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn a_page_outside_the_declared_folders_is_not_checked() {
    let config = format!("{KIND}in = [\"Housing\"]\n");
    let s = site(&config, &[("Services/desk.md", "---\ntitle: Desk\nkind: Buidling\n---\nbody\n")]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn the_warning_still_fails_a_second_strict_build_of_an_unchanged_site() {
    let s = site(KIND, &[("Housing/court.md", "---\ntitle: Court\nkind: Buidling\n---\nbody\n")]);
    let first = build_strict(&s);
    assert!(!first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    let second = build_strict(&s);
    let err = String::from_utf8_lossy(&second.stderr);
    assert!(!second.status.success(), "a second strict build must still fail: {err}");
    assert!(err.contains("field 'kind'"), "{err}");
}

#[test]
fn a_wikilink_in_a_pages_field_is_read_as_written_and_builds_clean_under_strict() {
    let config = "[fields.highlights]\ntype = \"pages\"\n";
    let s = site(
        config,
        &[
            ("Housing/Riverside Court.md", "---\ntitle: Riverside Court\n---\nbody\n"),
            ("Housing/court.md", "---\ntitle: Court\nhighlights: [\"[[Riverside Court]]\"]\n---\nbody\n"),
            ("Housing/block.md", "---\ntitle: Block\nhighlights:\n  - \"[[Housing/Riverside Court]]\"\n---\nbody\n"),
        ],
    );
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn a_value_in_a_language_tree_is_checked_under_its_folder_scope() {
    let config = format!("{KIND}in = [\"Housing\"]\n");
    let s = site(&config, &[("de/Housing/x.md", "---\ntitle: X\nkind: Buidling\n---\nbody\n")]);
    let o = build_strict(&s);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "a strict build must fail: {err}");
    assert!(err.contains("[de/Housing/x.md] frontmatter: field 'kind'"), "{err}");
}

const PLACED: &str = r#"
[fields.kind]
type = "choice"
of = { Building = "Built" }
place = "eyebrow"
[fields.years]
type = "period"
place = "eyebrow"
[fields.figure]
type = "count"
place = "key-figure"
[fields.architect]
type = "text"
place = "facts"
label = "Architect"
[fields.district]
type = "text"
place = "subtitle"
"#;

fn built(s: &Site, rel: &str) -> String {
    std::fs::read_to_string(s.root.join(".moss/build.nosync/current").join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn build_lenient(s: &Site) -> Output {
    Command::new(MOSS)
        .arg("build")
        .arg(&s.root)
        .env("MOSS_HOME", &s.home)
        .env_remove("MOSS_ENV")
        .output()
        .expect("spawn moss-cli")
}

/// A 1×1 PNG.
const PNG: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5, 0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0x60, 0x00, 0x00, 0x00, 0x06, 0x00, 0x02, 0x30, 0x81, 0xd0, 0x2f, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82];

fn stylesheet_href(html: &str) -> String {
    let rest = html.split("rel=\"stylesheet\" href=\"").nth(1).expect("a stylesheet link");
    rest[..rest.find('"').unwrap()].trim_start_matches('/').to_string()
}

fn position(html: &str, needle: &str) -> usize {
    html.find(needle).unwrap_or_else(|| panic!("missing {needle} in {html}"))
}

#[test]
fn a_placed_field_is_drawn_around_the_title_it_belongs_to() {
    let page = "---\ntitle: Riverside Court\nkind: Building\nyears: 1927/1933\nfigure: 1,382 flats\narchitect: Anna Weber\ndistrict: Eastbank\n---\nbody\n";
    let s = site(PLACED, &[("Housing/court.md", page), ("Housing/plain.md", "---\ntitle: Plain\n---\nbody\n")]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let html = built(&s, "housing/court/index.html");
    let eyebrow = position(&html, "<p class=\"moss-eyebrow\"><span data-field=\"kind\">Built</span> <span data-field=\"years\">");
    let h1 = position(&html, "<h1 class=\"moss-article-title\">Riverside Court</h1>");
    let subtitle = position(&html, "<p class=\"moss-subtitle\" data-field=\"district\">Eastbank</p>");
    assert!(eyebrow < h1 && h1 < subtitle, "eyebrow, title, subtitle in that order: {html}");
    assert!(html.contains("<time datetime=\"1927/1933\">1927–1933</time>"), "{html}");
    assert!(html.contains("<data value=\"1382\">1,382</data> <span>flats</span>"), "{html}");
    assert!(html.contains("<div data-field=\"architect\"><dt>Architect</dt><dd>Anna Weber</dd></div>"), "{html}");
    assert!(!built(&s, "housing/plain/index.html").contains("moss-eyebrow"), "a page with no values draws nothing");
}

#[test]
fn a_german_page_reads_an_open_period_in_german() {
    let s = site(PLACED, &[("de/Housing/hof.md", "---\ntitle: Hof\nyears: 1923/..\n---\nbody\n")]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let html = built(&s, "de/housing/hof/index.html");
    assert!(html.contains("<time datetime=\"1923/..\">ab 1923</time>"), "{html}");
}

#[test]
fn the_date_and_byline_follow_the_title_and_subtitle_not_the_eyebrow() {
    let page = "---\ntitle: Riverside Court\ndate: 2026-01-05\nbyline: By Anna Weber\nkind: Building\nyears: 1927/1933\nfigure: 1,382 flats\narchitect: Anna Weber\ndistrict: Eastbank\n---\nbody\n";
    let s = site(PLACED, &[("Housing/court.md", page)]);
    assert!(build_strict(&s).status.success());
    let html = built(&s, "housing/court/index.html");
    let at = |needle: &str| position(&html, needle);
    let order = [
        at("<p class=\"moss-eyebrow\">"),
        at("<h1 class=\"moss-article-title\">"),
        at("<p class=\"moss-subtitle\""),
        at("<div class=\"date-line\">"),
        at("<div class=\"moss-byline\">"),
        at("<p class=\"moss-key-figure\""),
        at("<dl class=\"moss-facts\""),
    ];
    assert!(order.windows(2).all(|w| w[0] < w[1]), "eyebrow, title, subtitle, date, byline, figure, facts: {order:?}\n{html}");
}

#[test]
fn a_body_with_its_own_heading_gets_the_subtitle_and_facts_after_it() {
    let page = "---\ntitle: Riverside Court\nkind: Building\nfigure: 1,382 flats\narchitect: Anna Weber\ndistrict: Eastbank\n---\n# Riverside Court\n\nbody\n";
    let s = site(PLACED, &[("Housing/court.md", page)]);
    assert!(build_strict(&s).status.success());
    let html = built(&s, "housing/court/index.html");
    let h1 = position(&html, "<h1");
    assert!(!html.contains("moss-article-title"), "the body's heading is the title: {html}");
    assert!(position(&html, "<p class=\"moss-eyebrow\">") < h1, "{html}");
    assert!(h1 < position(&html, "<p class=\"moss-subtitle\"") && h1 < position(&html, "<dl class=\"moss-facts\""), "{html}");
}

#[test]
fn a_hero_page_keeps_the_subtitle_below_the_hero() {
    let page = "---\ntitle: Riverside Court\ndistrict: Eastbank\n---\n:::hero\n![](/poster.png)\n\nOverlay words\n:::\n\nbody\n";
    let s = site(PLACED, &[("Housing/court.md", page)]);
    build_lenient(&s);
    let html = built(&s, "housing/court/index.html");
    assert!(position(&html, "Overlay words") < position(&html, "<p class=\"moss-subtitle\""), "{html}");
}

#[test]
fn a_placed_value_that_does_not_fit_is_not_drawn_warns_and_fails_strict() {
    let page = "---\ntitle: Court\nyears: soon\ndistrict: Eastbank\n---\nbody\n";
    let s = site(PLACED, &[("Housing/court.md", page)]);
    let strict = build_strict(&s);
    let err = String::from_utf8_lossy(&strict.stderr);
    assert!(!strict.status.success() && err.contains("field 'years'"), "{err}");
    let lenient = build_lenient(&s);
    assert!(String::from_utf8_lossy(&lenient.stderr).contains("field 'years'"));
    let html = built(&s, "housing/court/index.html");
    assert!(html.contains("data-field=\"district\"") && !html.contains("data-field=\"years\""), "{html}");
}

#[test]
fn a_page_link_in_a_fact_reads_as_written_and_links_where_it_resolves() {
    let config = "[fields.architect]\ntype = \"page\"\nplace = \"facts\"\n[fields.partners]\ntype = \"pages\"\nplace = \"facts\"\n";
    let page = "---\ntitle: Court\narchitect: \"[[People/anna]]\"\npartners: [\"[[anna|Dr. Weber]]\", \"[[Nobody]]\"]\n---\nbody\n";
    let s = site(config, &[("People/anna.md", "---\ntitle: Dr. Anna Weber\n---\nbio\n"), ("Housing/court.md", page)]);
    build_lenient(&s);
    let html = built(&s, "housing/court/index.html");
    assert!(html.contains("<dd><a href=\"/people/anna/\">anna</a></dd>"), "{html}");
    assert!(html.contains("<dd><a href=\"/people/anna/\">Dr. Weber</a>, Nobody</dd>"), "{html}");
}

#[test]
fn a_builtin_description_placed_as_eyebrow_draws_only_the_authored_value() {
    let config = "[fields.description]\nplace = \"eyebrow\"\n";
    let page = "---\ntitle: Okabe\ndescription: Station 21 · Okabe\n---\nbody\n";
    let plain = "---\ntitle: Plain\n---\nA paragraph of body text that a derived description could be taken from.\n";
    let s = site(config, &[("Housing/okabe.md", page), ("Housing/plain.md", plain)]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let html = built(&s, "housing/okabe/index.html");
    let eyebrow = position(&html, "<p class=\"moss-eyebrow\"><span data-field=\"description\">Station 21 · Okabe</span></p>");
    let h1 = position(&html, "<h1 class=\"moss-article-title\">Okabe</h1>");
    assert!(eyebrow < h1, "{html}");
    assert!(!built(&s, "housing/plain/index.html").contains("moss-eyebrow"), "a derived description is not drawn");
}

#[test]
fn a_page_in_the_simplified_dialect_is_drawn_and_checked_like_any_other() {
    let page = "title: Court\nkind: Building\ndistrict: Eastbank\nyears: soon\n---\nbody\n";
    let s = site(PLACED, &[("Housing/court.md", page)]);
    let strict = build_strict(&s);
    let err = String::from_utf8_lossy(&strict.stderr);
    assert!(!strict.status.success() && err.contains("[Housing/court.md] frontmatter: field 'years'"), "{err}");
    build_lenient(&s);
    let html = built(&s, "housing/court/index.html");
    assert!(html.contains("<p class=\"moss-subtitle\" data-field=\"district\">Eastbank</p>"), "{html}");
    assert!(html.contains("<span data-field=\"kind\">Built</span>") && !html.contains("data-field=\"years\""), "{html}");
}

#[test]
fn title_block_after_picture_cascaded_from_a_folder_draws_the_title_block_under_the_leading_picture() {
    let config = "[fields.stage]\ntype = \"text\"\nplace = \"eyebrow\"\n";
    let home = "---\ntitle: Piers\ncascade:\n  title_block: after-picture\n---\nThe piers.\n";
    let pier = "---\ntitle: Pier Four\nstage: Pier 4\n---\n::: {.trail}\nThe harbour\n:::\n\n![A pier at dusk](dusk.png)\n\nThe text.\n";
    let note = "---\ntitle: A Note\n---\n![A pier at dusk](dusk.png)\n\nThe text.\n";
    let own = "---\ntitle: Own\ntitle_block: after-picture\n---\n![A pier at dusk](dusk.png)\n\nThe text.\n";
    let s = site(config, &[("Piers/index.md", home), ("Piers/four.md", pier), ("Notes/note.md", note), ("Notes/own.md", own)]);
    std::fs::write(s.root.join("Piers/dusk.png"), PNG).unwrap();
    std::fs::write(s.root.join("Notes/dusk.png"), PNG).unwrap();
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let html = built(&s, "piers/four/index.html");
    let (trail, figure, eyebrow, h1, text) = (position(&html, "class=\"trail\""), position(&html, "<figure"), position(&html, "<p class=\"moss-eyebrow\""), position(&html, "<h1 class=\"moss-article-title\""), position(&html, "<p>The text.</p>"));
    assert!(trail < figure && figure < eyebrow && eyebrow < h1 && h1 < text, "{html}");
    let other = built(&s, "notes/note/index.html");
    assert!(position(&other, "<h1 class=\"moss-article-title\"") < position(&other, "<figure"), "outside the folder the title stays first: {other}");
    let own = built(&s, "notes/own/index.html");
    assert!(position(&own, "<figure") < position(&own, "<h1 class=\"moss-article-title\""), "a page can set it itself: {own}");
    for page in [&html, &other, &own] {
        assert!(!page.contains("moss:body-start"));
    }
}

#[test]
fn title_block_beside_picture_sets_the_title_block_in_one_wrapper_with_the_leading_picture() {
    let home = "---\ntitle: Piers\ncascade:\n  title_block: beside-picture\n---\nThe piers.\n";
    let pier = "---\ntitle: Pier Four\n---\n![A pier at dusk](dusk.png)\n\nThe text.\n";
    let s = site("", &[("Piers/index.md", home), ("Piers/four.md", pier)]);
    std::fs::write(s.root.join("Piers/dusk.png"), PNG).unwrap();
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let html = built(&s, "piers/four/index.html");
    let (wrap, figure, h1, text) = (position(&html, "<div class=\"moss-title-beside\">"), position(&html, "<figure"), position(&html, "<h1 class=\"moss-article-title\""), position(&html, "<p>The text.</p>"));
    assert!(wrap < figure && figure < h1 && h1 < text, "{html}");
    let flat = html.replace('\n', "");
    assert!(flat.contains("</h1></div></div><p>The text.</p>"), "{html}");
    let css = std::fs::read_to_string(s.root.join(".moss/build.nosync/current").join(stylesheet_href(&html))).unwrap();
    assert!(css.contains(".moss-title-beside"), "the layout ships its stylesheet");
}

#[test]
fn every_spelling_of_a_leading_picture_places_the_title_block() {
    let spellings = [("caption", "![Dusk](dusk.png)"), ("alt-only", "![Dusk|](dusk.png)"), ("wikilink", "![[dusk.png]]"), ("bare", "![](dusk.png)")];
    for value in ["after-picture", "beside-picture"] {
        let pages: Vec<(String, String)> = spellings
            .iter()
            .map(|(name, picture)| (format!("{name}.md"), format!("---\ntitle: {name}\ntitle_block: {value}\n---\n{picture}\n\nThe text.\n")))
            .collect();
        let refs: Vec<(&str, &str)> = pages.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
        let s = site("", &refs);
        std::fs::write(s.root.join("dusk.png"), PNG).unwrap();
        let o = build_strict(&s);
        assert!(o.status.success(), "{value}: {}", String::from_utf8_lossy(&o.stderr));
        for (name, _) in spellings {
            let html = built(&s, &format!("{name}/index.html"));
            assert!(position(&html, "<img") < position(&html, "<h1 class=\"moss-article-title\""), "{value} {name}: {html}");
            assert_eq!(html.contains("moss-title-beside"), value == "beside-picture", "{value} {name}: {html}");
        }
    }
}

#[test]
fn a_page_with_title_block_but_no_leading_picture_is_a_strict_failure_naming_the_page() {
    let first = "---\ntitle: Text First\ntitle_block: beside-picture\n---\nThe text.\n\n![A pier at dusk](dusk.png)\n";
    let none = "---\ntitle: None\ntitle_block: after-picture\n---\nThe text.\n";
    let s = site("", &[("first.md", first), ("none.md", none)]);
    std::fs::write(s.root.join("dusk.png"), PNG).unwrap();
    let o = build_strict(&s);
    assert!(!o.status.success());
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(stderr.contains("`first.md`") && stderr.contains("`none.md`") && stderr.contains("title_block"), "{stderr}");
}

#[test]
fn an_unknown_title_block_value_is_a_strict_failure() {
    let s = site("", &[("a.md", "---\ntitle: A\ntitle_block: below\n---\nx\n")]);
    let o = build_strict(&s);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("title_block"), "{}", String::from_utf8_lossy(&o.stderr));
}

/// A page copied from a template still has its blank lines. A blank value is an
/// absent one for built-in and declared fields alike: strict passes and no row
/// or event line is drawn for it.
#[test]
fn a_blank_value_counts_as_absent_for_built_in_and_declared_fields() {
    let config = "[fields.architect]\ntype = \"text\"\nplace = \"facts\"\n[fields.kind]\ntype = \"choice\"\nof = [\"Building\", \"Service\"]\nplace = \"eyebrow\"\n";
    let nulls = "---\ntitle: Example Show\ndescription:\nstart:\nlocation:\narchitect:\nkind:\n---\nbody\n";
    let empties = "---\ntitle: Example Show\ndescription: \"\"\nstart: \"\"\nlocation: \"\"\narchitect: \"\"\nkind: \"\"\n---\nbody\n";
    let s = site(config, &[("nulls.md", nulls), ("empties.md", empties)]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    for page in ["nulls/index.html", "empties/index.html"] {
        let html = built(&s, page);
        for drawn in ["moss-event-meta", "moss-event-place", "data-field=", "moss-eyebrow", "<dt>"] {
            assert!(!html.contains(drawn), "{page} draws {drawn}: {html}");
        }
    }
}

#[test]
fn reader_controls_false_removes_the_text_size_control_from_a_dated_page_or_a_folder() {
    let dated = |extra: &str| format!("---\ntitle: Note\ndate: 2026-03-01\n{extra}---\nThe text.\n");
    let home = "---\ntitle: Quiet\ncascade:\n  reader_controls: false\n---\nQuiet pages.\n";
    let s = site(
        "",
        &[
            ("Plain/note.md", &dated("")),
            ("Own/note.md", &dated("reader_controls: false\n")),
            ("Quiet/index.md", home),
            ("Quiet/note.md", &dated("")),
            ("Quiet/back.md", &dated("reader_controls: true\n")),
        ],
    );
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let control = "class=\"font-trigger";
    assert!(built(&s, "plain/note/index.html").contains(control), "the control is on by default");
    assert!(!built(&s, "own/note/index.html").contains(control), "a page turns it off");
    assert!(!built(&s, "quiet/note/index.html").contains(control), "a folder turns it off for its pages");
    assert!(built(&s, "quiet/back/index.html").contains(control), "a page turns it back on");
    assert!(built(&s, "own/note/index.html").contains("class=\"date\""), "the date stays");
}

#[test]
fn contents_pictures_puts_a_body_attribute_on_that_page_only_and_an_unknown_value_is_a_strict_failure() {
    let s = site("", &[("pics.md", "---\ntitle: Pics\ncontents: pictures\n---\nx\n"), ("plain.md", "---\ntitle: Plain\n---\nx\n")]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(built(&s, "pics/index.html").contains(r#" data-contents="pictures""#));
    assert!(!built(&s, "plain/index.html").contains("data-contents"));

    let bad = site("", &[("a.md", "---\ntitle: A\ncontents: shut\n---\nx\n")]);
    let o = build_strict(&bad);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("contents"), "{}", String::from_utf8_lossy(&o.stderr));
}

const GROUP: &str = "[fields.credit_holder]\ntype = \"text\"\n\n[fields.licence]\ntype = \"text\"\n\n[fields.credit]\ntype = \"group\"\nof = [\"title\", \"date\", \"credit_holder\", \"licence\"]\nplace = \"subtitle\"\n";

const COURT: &str = "---\ntitle: Riverside Court\ndate: 1927-10-12\ncredit_holder: Anna Weber\nlicence: CC BY 4.0\n---\nbody\n";

#[test]
fn a_group_draws_its_parts_in_order_as_one_subtitle_each_keeping_its_field() {
    let s = site(GROUP, &[("Housing/court.md", COURT)]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let html = built(&s, "housing/court/index.html");
    let group = position(&html, "<p class=\"moss-subtitle moss-field-group\" data-field=\"credit\">");
    let title = position(&html, "<span data-field=\"title\">Riverside Court</span>");
    let date = position(&html, "<span data-field=\"date\"><time datetime=\"1927-10-12\">");
    let holder = position(&html, "<span data-field=\"credit_holder\">Anna Weber</span>");
    let licence = position(&html, "<span data-field=\"licence\">CC BY 4.0</span></p>");
    assert!(group < title && title < date && date < holder && holder < licence, "{html}");
    assert!(!html.contains("data-field=\"credit\"><span data-field=\"credit\""), "{html}");
}

#[test]
fn a_part_with_no_value_is_left_out_with_its_separator_and_the_rest_keep_their_order() {
    let page = "---\ntitle: Riverside Court\ncredit_holder: Anna Weber\n---\nbody\n";
    let s = site(GROUP, &[("Housing/court.md", page)]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let html = built(&s, "housing/court/index.html");
    assert!(
        html.contains("<p class=\"moss-subtitle moss-field-group\" data-field=\"credit\"><span data-field=\"title\">Riverside Court</span> <span data-field=\"credit_holder\">Anna Weber</span></p>"),
        "{html}"
    );
    assert!(!html.contains("data-field=\"licence\"") && !html.contains("data-field=\"date\""), "{html}");
}

#[test]
fn a_date_part_is_formatted_as_the_date_field_is_formatted_alone() {
    let config = "[fields.issued]\ntype = \"date\"\nplace = \"facts\"\n\n[fields.credit]\ntype = \"group\"\nof = [\"date\", \"issued\"]\nplace = \"subtitle\"\n";
    let page = "---\ntitle: Riverside Court\ndate: 1927-10-12\nissued: 1927-10-12\n---\nbody\n";
    let s = site(config, &[("Housing/court.md", page)]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let html = built(&s, "housing/court/index.html");
    let time_in = |block: &str| {
        let rest = &block[block.find("<time datetime=\"1927-10-12\">").expect("a time element") + "<time datetime=\"1927-10-12\">".len()..];
        rest[..rest.find("</time>").expect("a closed time element")].to_string()
    };
    let group = &html[html.find("data-field=\"credit\"").expect("the group")..];
    let group = &group[..group.find("</p>").expect("the group's end")];
    let row = &html[html.find("<div data-field=\"issued\">").expect("the issued row")..];
    let row = &row[..row.find("</div>").expect("the row's end")];
    assert_eq!(time_in(group), time_in(row), "{html}");
}

#[test]
fn an_invalid_group_declaration_warns_naming_it_and_is_not_drawn() {
    let invalid = [
        ("[fields.credit]\ntype = \"group\"\nof = [\"title\", \"nope\"]\nplace = \"subtitle\"\n", "`of` names \"nope\""),
        ("[fields.credit]\ntype = \"group\"\nof = [\"credit\", \"title\"]\nplace = \"subtitle\"\n", "`of` names \"credit\", which names itself"),
        (
            "[fields.a]\ntype = \"group\"\nof = [\"title\"]\n\n[fields.credit]\ntype = \"group\"\nof = [\"a\", \"title\"]\nplace = \"subtitle\"\n",
            "`of` names \"a\", which names another group",
        ),
        ("[fields.credit]\ntype = \"group\"\nof = []\nplace = \"subtitle\"\n", "[fields.credit] is a group with no `of`"),
        (
            "[fields.credit_holder]\ntype = \"text\"\n\n[fields.credit]\ntype = \"group\"\nof = [\"credit_holder\", \"credit_holder\"]\nplace = \"subtitle\"\n",
            "`of` names \"credit_holder\" twice",
        ),
        ("[fields.credit]\ntype = \"group\"\nof = [\"title\"]\n", "[fields.credit] is a group with no `place`"),
        ("[fields.date]\ntype = \"group\"\nof = [\"title\"]\nplace = \"subtitle\"\n", "[fields.date] is a moss field and cannot be redeclared"),
    ];
    for (config, warning) in invalid {
        let s = site(config, &[("Housing/court.md", COURT)]);
        let o = build_lenient(&s);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.contains(warning), "{warning} missing from {err}");
        assert!(!built(&s, "housing/court/index.html").contains("moss-field-group"), "a skipped group is not drawn: {config}");
    }
}

#[test]
fn a_page_that_writes_a_value_for_a_group_warns_and_draws_no_group_there() {
    let s = site(GROUP, &[("Housing/court.md", COURT), ("Housing/plain.md", "---\ntitle: Plain\ncredit: Someone\n---\nbody\n")]);
    let o = build_lenient(&s);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("[Housing/plain.md] frontmatter: field 'credit': a group holds no value"), "{err}");
    assert!(!built(&s, "housing/plain/index.html").contains("moss-field-group"), "the page with a value draws no group");
    assert!(built(&s, "housing/court/index.html").contains("<p class=\"moss-subtitle moss-field-group\" data-field=\"credit\">"));
}

#[test]
fn a_group_in_a_folder_scope_is_drawn_only_on_pages_in_that_folder() {
    let config = "[fields.credit_holder]\ntype = \"text\"\n\n[fields.credit]\ntype = \"group\"\nof = [\"credit_holder\"]\nplace = \"subtitle\"\nin = [\"Housing\"]\n";
    let s = site(
        config,
        &[
            ("Housing/court.md", "---\ntitle: Riverside Court\ncredit_holder: Anna Weber\n---\nbody\n"),
            ("Other/plain.md", "---\ntitle: Plain\ncredit_holder: Ben\n---\nbody\n"),
        ],
    );
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(built(&s, "housing/court/index.html").contains("<p class=\"moss-subtitle moss-field-group\" data-field=\"credit\">"));
    assert!(!built(&s, "other/plain/index.html").contains("moss-field-group"), "a page outside the scope draws no group");
}

#[test]
fn a_group_at_subtitle_is_not_repeated_by_a_subtitle_the_page_writes_with_the_same_text() {
    let config = "[fields.credit_holder]\ntype = \"text\"\n\n[fields.credit]\ntype = \"group\"\nof = [\"title\", \"credit_holder\"]\nplace = \"subtitle\"\n";
    let page = "---\ntitle: Riverside Court\nsubtitle: Riverside Court Anna Weber\ncredit_holder: Anna Weber\n---\nbody\n";
    let s = site(config, &[("Housing/court.md", page)]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let html = built(&s, "housing/court/index.html");
    assert!(html.contains("<p class=\"moss-subtitle moss-field-group\" data-field=\"credit\">"), "{html}");
    assert_eq!(html.matches("moss-subtitle").count(), 1, "one subtitle line: {html}");
}

#[test]
fn a_group_at_eyebrow_key_figure_and_facts_draws_its_parts_in_that_element() {
    let config = "[fields.credit_holder]\ntype = \"text\"\n\n[fields.licence]\ntype = \"text\"\n\n[fields.holder_line]\ntype = \"group\"\nof = [\"credit_holder\", \"licence\"]\nplace = \"eyebrow\"\n\n[fields.holder_figure]\ntype = \"group\"\nof = [\"credit_holder\"]\nplace = \"key-figure\"\n\n[fields.credit]\ntype = \"group\"\nof = [\"credit_holder\", \"licence\"]\nplace = \"facts\"\n";
    let s = site(config, &[("Housing/court.md", COURT)]);
    let o = build_strict(&s);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let html = built(&s, "housing/court/index.html");
    assert!(
        html.contains("<span class=\"moss-field-group\" data-field=\"holder_line\"><span data-field=\"credit_holder\">Anna Weber</span> <span data-field=\"licence\">CC BY 4.0</span></span>"),
        "{html}"
    );
    assert!(
        html.contains("<p class=\"moss-key-figure moss-field-group\" data-field=\"holder_figure\"><span data-field=\"credit_holder\">Anna Weber</span></p>"),
        "{html}"
    );
    assert!(
        html.contains("<div data-field=\"credit\"><dt>Credit</dt><dd class=\"moss-field-group\"><span data-field=\"credit_holder\">Anna Weber</span> <span data-field=\"licence\">CC BY 4.0</span></dd></div>"),
        "{html}"
    );
}
