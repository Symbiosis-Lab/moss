use crate::resolve::embeds::expand::expand_page;
use super::*;
use crate::asset_snapshot::AssetSnapshot;
use crate::content_graph::{ContentGraph, ContentGraphBuilder};
use crate::resolve::{embeds::MarkerHandlers, resolve_content_with_derivations};

fn graph() -> ContentGraph {
    let mut b = ContentGraphBuilder::new();
    b.add_file("page.md", "page");
    b.add_file("note.md", "note");
    b.add_file("a.fake", "a");
    b.add_file("b.fake", "b");
    b.add_file("doc.pdf", "doc");
    b.build()
}

fn table() -> DerivedTable {
    let mut t = DerivedTable::new();
    t.declare("fake", "text");
    t
}

fn resolve(
    body: &str,
    reader: &DerivedReader<'_>,
) -> crate::resolve::ResolveResult {
    let table = table();
    let derivations = Derivations { table: &table, reader };
    resolve_content_with_derivations(
        "page.md",
        body,
        &graph(),
        &|p| (p == "note.md").then(|| "---\ntitle: N\n---\n# Note\n\nnote body\n".to_string()),
        &MarkerHandlers::new(),
        &AssetSnapshot::new(),
        Some(&derivations),
    )
}

fn ready(md: &str) -> DerivedFragment {
    DerivedFragment::Ready {
        markdown: md.to_string(),
        facts: DerivedFacts { title: Some("T".to_string()), description: Some("D".to_string()) },
    }
}

#[test]
fn claimed_pair_splices_the_markdown_and_records_the_dependency() {
    let r = resolve("before\n\n![[a.fake#text]]\n\nafter\n", &|_, _| ready("## Hello\n\nworld\n"));
    assert!(r.content_markdown.contains("## Hello\n\nworld"), "{}", r.content_markdown);
    assert!(!r.content_markdown.contains("moss-derived:"));
    assert!(r.embed_deps.contains(&("a.fake".to_string(), "page.md".to_string())));
    assert_eq!(r.derived_facts, DerivedFacts { title: Some("T".into()), description: Some("D".into()) });
}

#[test]
fn facts_come_from_the_first_ready_derivation_that_has_each_and_blank_ones_are_ignored() {
    let facts = |title: &str, description: &str| DerivedFragment::Ready {
        markdown: "x\n".into(),
        facts: DerivedFacts {
            title: Some(title.into()).filter(|t: &String| !t.is_empty()),
            description: Some(description.into()).filter(|t: &String| !t.is_empty()),
        },
    };
    let r = resolve("![[a.fake#text]]\n\n![[b.fake#text]]\n", &|path, _| match path {
        "a.fake" => facts("  ", "First d"),
        _ => facts("Second t", "Second d"),
    });
    assert_eq!(
        r.derived_facts,
        DerivedFacts { title: Some("Second t".into()), description: Some("First d".into()) }
    );
    let none = resolve("![[a.fake#text]]\n", &|_, _| facts("", "\t\n"));
    assert_eq!(none.derived_facts, DerivedFacts::default());
}

#[test]
fn unclaimed_pairs_are_left_for_the_viewer_dispatch() {
    let untouched = |md: &str| {
        let r = resolve(md, &|_, _| panic!("reader must not be asked"));
        assert_eq!(r.content_markdown, md);
    };
    untouched("![[doc.pdf#text]]\n");
    untouched("![[a.fake#page=3]]\n");
    untouched("![[a.fake]]\n");
}

#[test]
fn pending_skipped_and_failed_become_blocks() {
    let pending = resolve("![[a.fake#text]]\n", &|_, _| DerivedFragment::Pending).content_markdown;
    assert!(pending.contains("class=\"moss-derived moss-derived--pending\""), "{pending}");
    assert!(!pending.to_lowercase().contains("pdf"));

    let skipped = resolve("![[a.fake#text]]\n", &|_, _| DerivedFragment::Skipped {
        message: "Locked <file>".into(),
    })
    .content_markdown;
    assert!(skipped.contains("moss-derived--skipped\" role=\"note\">Locked &lt;file&gt;</div>"), "{skipped}");

    let failed = resolve("![[a.fake#text]]\n", &|_, _| DerivedFragment::Failed { message: "boom".into() })
        .content_markdown;
    assert!(failed.contains("moss-derived--skipped"));
    assert!(failed.contains("The text of this file couldn't be read."));
    assert!(!failed.contains("boom"));
}

#[test]
fn derived_text_is_not_resolved_for_embeds_again() {
    let r = resolve("![[a.fake#text]]\n", &|path, _| match path {
        "a.fake" => ready("![[b.fake#text]]\n<!-- moss-embed:note.md -->\n"),
        _ => panic!("nested derivation must not be read"),
    });
    assert!(r.content_markdown.contains("!&#91;[b.fake&#35;text]]"), "{}", r.content_markdown);
    assert!(r.content_markdown.contains("&lt;!-- moss-embed:note.md -->"));
}

#[test]
fn derived_markdown_carries_no_raw_html() {
    let payload = "<img src=x onerror=alert(1)> and AT&T\n\nUse `<b>` here:\n\n```\n<i> & <u>\n```\n\n    <s>indented</s>\n";
    let r = resolve("![[a.fake#text]]\n", &|_, _| ready(payload));
    let md = &r.content_markdown;
    assert!(md.contains("&lt;img src=x onerror=alert(1)> and AT&amp;T"), "{md}");
    assert!(md.contains("`<b>`"), "{md}");
    assert!(md.contains("```\n<i> & <u>\n```"), "{md}");
    assert!(md.contains("    <s>indented</s>"), "{md}");
}

#[test]
fn derived_markdown_does_not_trigger_moss_authoring_syntax() {
    let text = "Awarded $90,940 to Teachers & Counselors -\n\nSee [[x]] and ![[y]] and {{z}} and {++ins++} %%c%% #tag ^blk\n\n:::note\nbody\n:::\n\n## Heading\n\n`$code` stays\n";
    let r = resolve("![[a.fake#text]]\n", &|_, _| ready(text));
    let md = &r.content_markdown;
    for expected in [
        "Awarded &#36;90,940 to Teachers &amp; Counselors -",
        "&#91;[x]] and !&#91;[y]] and &#123;&#123;z}} and &#123;++ins++} &#37;&#37;c&#37;&#37; &#35;tag &#94;blk",
        "&#58;::note",
        "\n## Heading\n",
        "`$code` stays",
    ] {
        assert!(md.contains(expected), "missing {expected:?} in {md}");
    }
    assert!(!md.lines().any(|l| l.starts_with(":::")), "{md}");
}

#[test]
fn expand_escapes_derived_text_like_the_splice() {
    let table = table();
    let reader = |_: &str, _: &str| ready("<script>x</script>\n");
    let derivations = Derivations { table: &table, reader: &reader };
    let out = expand_page("page.md", "![[a.fake#text]]\n", &graph(), &|_| None, &derivations);
    assert_eq!(out.content, "&lt;script>x&lt;/script>\n\n");
}

#[test]
fn expand_replaces_ready_derived_and_note_lines_and_keeps_frontmatter() {
    let raw = "---\ntitle:   Spaced  \n---\nintro\n![[a.fake#text]]\n![[b.fake#text]]\n![[note#Note]]\ninline ![[a.fake#text]] stays\n```\n![[a.fake#text]]\n```\n";
    let table = table();
    let reader = |path: &str, _: &str| {
        if path == "a.fake" { ready("derived A\n") } else { DerivedFragment::Pending }
    };
    let derivations = Derivations { table: &table, reader: &reader };
    let out = expand_page("page.md", raw, &graph(), &|p| {
        (p == "note.md").then(|| "# Note\n\nnote body\n\n# Other\n".to_string())
    }, &derivations);
    assert!(out.content.starts_with("---\ntitle:   Spaced  \n---\n"));
    assert!(out.content.contains("intro\n\nderived A\n\n![[b.fake#text]]\n# Note\n\nnote body\ninline ![[a.fake#text]] stays\n```\n![[a.fake#text]]\n```\n"), "{}", out.content);
    let lines: Vec<usize> = out.expanded.iter().map(|e| e.line).collect();
    assert_eq!(lines, vec![5, 7]);
}

#[test]
fn expand_without_changes_returns_the_input() {
    let raw = "no embeds here\r\nsecond\r\n";
    let table = table();
    let reader = |_: &str, _: &str| DerivedFragment::Pending;
    let derivations = Derivations { table: &table, reader: &reader };
    let out = expand_page("page.md", raw, &graph(), &|_| None, &derivations);
    assert_eq!(out.content, raw);
    assert!(out.expanded.is_empty());
}

#[test]
fn a_marker_written_inside_a_code_fence_is_not_spliced() {
    let src = "```\n<!-- moss-derived:a.fake#text -->\n```\n\n<!-- moss-derived:a.fake#text -->\n";
    let r = splice_markers(src, "page.md", &Derivations { table: &table(), reader: &|_, _| ready("X\n") });
    assert_eq!(r.content.matches("<!-- moss-derived:a.fake#text -->").count(), 1, "{}", r.content);
    assert_eq!(r.deps.len(), 1);
}

#[test]
fn status_blocks_are_what_placeholder_html_writes_and_nothing_else() {
    for state in [DerivedFragment::Pending, DerivedFragment::Skipped { message: "m".into() }, DerivedFragment::Failed { message: "m".into() }] {
        let block = placeholder_html(&state).unwrap();
        assert_eq!(without_derived_markup(&format!("a\n{block}\nb\n")), "a\nb\n");
    }
    let prose = "<div class='moss-derivedish'>kept</div>\ntext\n";
    assert_eq!(without_derived_markup(prose), prose);
}

#[test]
fn a_derived_embed_sharing_its_line_is_left_as_written() {
    for body in [
        "> ![[a.fake#text]]\n",
        "- ![[a.fake#text]]\n",
        "see ![[a.fake#text]] here\n",
        "![[a.fake#text]] trailing\n",
        "1. item ![[a.fake#text]]\n",
    ] {
        let r = resolve(body, &|_, _| panic!("a non-sole embed must not be read: {body}"));
        assert_eq!(r.content_markdown, body);
        assert!(!r.content_markdown.contains("data-fragment") && r.content_markdown.contains("![[a.fake#text]]"));
        assert!(r.embed_deps.is_empty());
    }
    // A sole embed with spaces around it is still a sole embed: it is spliced.
    let r = resolve("  ![[a.fake#text]]  \n", &|_, _| ready("X\n"));
    assert!(r.content_markdown.contains("data-fragment=\"text\"") && r.content_markdown.contains('X'), "{}", r.content_markdown);
    assert!(!r.content_markdown.contains("moss-derived:"), "{}", r.content_markdown);
}

#[test]
fn escapes_the_deriver_wrote_survive_once() {
    let escaped = |md: &str| escape_derived_markdown(md);
    assert_eq!(escaped("\\$7 to \\$8"), "&#36;7 to &#36;8");
    assert_eq!(escaped("\\[[y]]"), "&#91;[y]]");
    assert_eq!(escaped("\\*stars\\*"), "\\*stars\\*");
    assert_eq!(escaped("a \\\\ b"), "a \\\\ b");
}

#[test]
fn expand_pads_derived_text_so_it_cannot_fuse_with_neighbours() {
    let table = table();
    let reader = |_: &str, _: &str| ready("derived\n");
    let derivations = Derivations { table: &table, reader: &reader };
    let out = expand_page("page.md", "above\n![[a.fake#text]]\n---\nbelow\n", &graph(), &|_| None, &derivations);
    assert_eq!(out.content, "above\n\nderived\n\n---\nbelow\n");
    let out = expand_page("page.md", "![[a.fake#text]]\n", &graph(), &|_| None, &derivations);
    assert_eq!(out.content, "derived\n\n");
}

#[test]
fn expand_writes_the_derived_title_where_the_page_inferred_it() {
    let table = table();
    let reader = |_: &str, _: &str| ready("derived\n");
    let derivations = Derivations { table: &table, reader: &reader };
    let expand = |raw: &str| expand_page("page.md", raw, &graph(), &|_| None, &derivations).content;

    assert_eq!(
        expand("---\nlayout:   wide\ninferred: [title]\n---\n![[a.fake#text]]\n"),
        "---\nlayout:   wide\ntitle: T\n---\nderived\n\n"
    );
    assert_eq!(
        expand("---\ninferred: [date, title]\ntags: [x]\n---\n![[a.fake#text]]\n"),
        "---\ninferred: [date]\ntags: [x]\ntitle: T\n---\nderived\n\n"
    );
    assert_eq!(
        expand("---\ninferred:\n  - title\n  - date\n---\n![[a.fake#text]]\n"),
        "---\ninferred:\n  - date\ntitle: T\n---\nderived\n\n"
    );
    // The description is carried the same way, and each name leaves `inferred` once written.
    assert_eq!(
        expand("---\nlayout:   wide\ninferred: [title, description]\n---\n![[a.fake#text]]\n"),
        "---\nlayout:   wide\ntitle: T\ndescription: D\n---\nderived\n\n"
    );
    assert_eq!(
        expand("---\ninferred: [description]\ntags: [x]\n---\n![[a.fake#text]]\n"),
        "---\ntags: [x]\ndescription: D\n---\nderived\n\n"
    );
    assert_eq!(
        expand("---\ndescription: Mine\ninferred: [title, description]\n---\n![[a.fake#text]]\n"),
        "---\ndescription: Mine\ninferred: [description]\ntitle: T\n---\nderived\n\n"
    );
    // A blank key counts as unset and is filled where it stands, not repeated.
    assert_eq!(
        expand("---\ndescription: \"\"\ninferred: [description]\ntags: [x]\n---\n![[a.fake#text]]\n"),
        "---\ndescription: D\ntags: [x]\n---\nderived\n\n"
    );
    assert_eq!(
        expand("---\ntitle:\ninferred: [title, description]\n---\n![[a.fake#text]]\n"),
        "---\ntitle: T\ndescription: D\n---\nderived\n\n"
    );
    // A title the author wrote, or no `inferred`, is left alone.
    let own = "---\ntitle: Mine\ninferred: [title]\n---\n![[a.fake#text]]\n";
    assert!(expand(own).starts_with("---\ntitle: Mine\ninferred: [title]\n---\n"));
    let none = "---\nlayout: x\n---\n![[a.fake#text]]\n";
    assert!(expand(none).starts_with("---\nlayout: x\n---\n"));
}

#[test]
fn ready_text_sits_in_a_wrapper_naming_its_fragment() {
    let r = resolve("before\n\n![[a.fake#text]]\n\nafter\n", &|_, _| ready("## Hello\n\nworld\n"));
    assert!(
        r.content_markdown.contains("\n<div class=\"moss-derived\" data-fragment=\"text\">\n\n## Hello\n\nworld\n\n</div><!-- /moss-derived -->\n\n"),
        "{}",
        r.content_markdown
    );
    let odd = splice_markers(
        "<!-- moss-derived:a.fake#t\"<x -->\n",
        "page.md",
        &Derivations { table: &table(), reader: &|_, _| ready("X\n") },
    );
    assert!(odd.content.contains("data-fragment=\"t&quot;&lt;x\">"), "{}", odd.content);
}

#[test]
fn wrapper_lines_are_left_out_of_plain_text_and_the_text_is_kept() {
    let spliced = resolve("a\n\n![[a.fake#text]]\n\nb\n", &|_, _| ready("kept words\n")).content_markdown;
    let plain = without_derived_markup(&spliced);
    assert!(plain.contains("kept words") && plain.contains('a') && plain.contains('b'), "{plain}");
    assert!(!plain.contains("<div") && !plain.contains("</div>") && !plain.contains("moss-derived"), "{plain}");
    // Status blocks go too; an author's own div is left alone.
    let mixed = format!("{}\n<div class='mine'>\nx\n</div>\n", placeholder_html(&DerivedFragment::Pending).unwrap());
    assert_eq!(without_derived_markup(&mixed), "<div class='mine'>\nx\n</div>\n");
    // A closing tag inside the derived text's own code block is text, not the wrapper's end.
    let fenced = resolve("![[a.fake#text]]\n", &|_, _| ready("```\n</div>\n```\n")).content_markdown;
    assert!(without_derived_markup(&fenced).contains("</div>"), "{fenced}");
}

#[test]
fn only_what_the_splice_wrote_is_removed_never_an_authors_own_html() {
    let opener = "<div class=\"moss-derived\" data-fragment=\"x\">";
    // An author's lines that look like the opener, with a plain `</div>`, are kept whole.
    let mine = format!("{opener}\nmine\n</div>\nafter\n");
    assert_eq!(without_derived_markup(&mine), mine);
    // A lone closing line of the splice's form has no opener to pair with: kept.
    let lone = "text\n</div><!-- /moss-derived -->\n";
    assert_eq!(without_derived_markup(lone), lone);
    // Next to a real wrapper, the author's lines stay and the splice's pair goes.
    let spliced = resolve("![[a.fake#text]]\n", &|_, _| ready("words\n")).content_markdown;
    let plain = without_derived_markup(&format!("{mine}{spliced}"));
    assert!(plain.starts_with(&mine) && plain.trim_end().ends_with("words") && !plain.contains("moss-derived:"), "{plain}");
    assert_eq!(plain.matches("moss-derived").count(), 1, "only the author's opener is left: {plain}");
}
