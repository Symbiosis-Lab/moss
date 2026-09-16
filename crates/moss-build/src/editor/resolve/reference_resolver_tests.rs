use super::*;
use crate::build::scan::article_map::ArticleMap;

fn scratch_root() -> tempfile::TempDir {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create test-tmp base");
    tempfile::TempDir::new_in(&base).expect("tmp dir")
}

/// Fixture: a static-index folder (`app/index.html`) and a root image
/// (`photo.png`). Asserts the three primary routing paths through the
/// shared classifier: folder-index-iframe, image embed (enriched), and a
/// bare unresolved name (NotFound or Link — empty ArticleMap).
#[test]
fn batch_routes_each_kind_through_classifier() {
    let dir = scratch_root();
    let root = dir.path();
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(root.join("app/index.html"), b"<h1>x</h1>").unwrap();
    std::fs::write(root.join("photo.png"), b"\x89PNGfakebytes").unwrap();

    let targets = vec![
        RefTarget {
            text: "/app/".into(),
            is_embed: true,
        },
        RefTarget {
            text: "photo.png".into(),
            is_embed: true,
        },
        RefTarget {
            text: "missing".into(),
            is_embed: false,
        },
    ];
    let out = resolve_references_batch(&targets, "index.md", root);
    assert_eq!(out.len(), 3);

    // /app/ → FolderIndexIframe (static index.html present).
    assert_eq!(
        out[0].kind,
        ReferenceKind::FolderIndexIframe,
        "/app/ should be a static-index folder iframe, got {:?}",
        out[0].kind
    );

    // photo.png → Image, enriched (resolved.is_some, mime image/*).
    assert_eq!(out[1].kind, ReferenceKind::Image, "got {:?}", out[1].kind);
    let asset = out[1]
        .resolved
        .as_ref()
        .expect("photo.png should enrich to a ResolvedAsset");
    assert!(
        asset.mime_type.starts_with("image/"),
        "expected image/* mime, got {}",
        asset.mime_type
    );

    // missing (non-embed, empty ArticleMap) → NotFound or Link, no resolved asset.
    assert!(
        matches!(
            out[2].kind,
            ReferenceKind::NotFound | ReferenceKind::Link { .. }
        ),
        "bare missing name should be NotFound or Link, got {:?}",
        out[2].kind
    );
    assert!(out[2].resolved.is_none(), "missing should have no asset");
}

/// A folder-listing embed carries what the LAST BUILD listed there; every other
/// kind carries `None`. Proves the envelope arm is wired to `FolderFacts`, not
/// just that `FolderFacts` works in isolation.
#[test]
fn folder_listing_carries_folder_detail() {
    let dir = scratch_root();
    let root = dir.path();
    std::fs::create_dir_all(root.join("notes")).expect("mkdir");
    std::fs::write(root.join("notes/index.md"), "---\ntitle: My Notes\n---\n").expect("write");
    std::fs::write(root.join("notes/a.md"), "# a").expect("write");
    std::fs::write(root.join("photo.png"), b"\x89PNGfakebytes").expect("write");

    let mut map = ArticleMap::new();
    map.pages.insert("notes/".into(), "notes/index.md".into());
    // The title comes from the scan's record, never from re-reading the
    // index file: what the card names is what the last build published.
    map.page_titles.insert("notes/".into(), "My Notes".into());
    map.articles.insert(
        "notes/a".into(),
        crate::build::scan::article_map::ArticleInfo {
            source_path: "notes/a.md".into(),
            title: "A".into(),
            content: String::new(),
            html_content: None,
            frontmatter: std::collections::HashMap::new(),
            url_path: "notes/a".into(),
            date: None,
            tags: vec![],
            uid: None,
        },
    );
    let moss_dir = root.join(".moss");
    std::fs::create_dir_all(moss_dir.join("build")).expect("mkdir .moss/build");
    map.save(&moss_dir).expect("save article map");

    let targets = vec![
        RefTarget { text: "/notes/".into(), is_embed: true },
        RefTarget { text: "photo.png".into(), is_embed: true },
    ];
    let out = resolve_references_batch(&targets, "index.md", root);

    assert_eq!(out[0].kind, ReferenceKind::FolderListing, "got {:?}", out[0].kind);
    let folder = out[0].folder.as_ref().expect("folder listing carries facts");
    assert_eq!(folder.direct_child_count, 1);
    assert_eq!(folder.title.as_deref(), Some("My Notes"));

    assert_eq!(out[1].kind, ReferenceKind::Image);
    assert!(out[1].folder.is_none(), "only folder listings carry folder facts");

    // Following the card opens the folder's INDEX SOURCE — a file.
    let canonical = root.canonicalize().expect("canonical root");
    assert_eq!(
        out[0].resolved_path.as_deref(),
        Some(canonical.join("notes/index.md").to_string_lossy().as_ref()),
        "a folder embed follows to its index source"
    );
}

/// The slug-override case, which is the whole reason `resolved_path` goes
/// through `FolderFacts` instead of joining the target to the project root.
///
/// A folder whose index is `獎項/獎項.md` publishes at `awards/`, so an author
/// writing `![[/awards/]]` names a URL that exists and a directory that does
/// not. `<root>/awards` would be a path to nothing; the answer is the source
/// file the build recorded for that URL.
#[test]
fn folder_embed_follows_a_slug_override_to_its_real_source() {
    let dir = scratch_root();
    let root = dir.path();
    std::fs::create_dir_all(root.join("獎項")).expect("mkdir");
    std::fs::write(
        root.join("獎項/獎項.md"),
        "---\ntitle: 獎項\nslug: awards\n---\n",
    )
    .expect("write");

    let mut map = ArticleMap::new();
    // What the build recorded: the pretty URL `awards/` ← the CJK source.
    map.pages.insert("awards/".into(), "獎項/獎項.md".into());
    let moss_dir = root.join(".moss");
    std::fs::create_dir_all(moss_dir.join("build")).expect("mkdir .moss/build");
    map.save(&moss_dir).expect("save article map");

    let targets = vec![RefTarget { text: "/awards/".into(), is_embed: true }];
    let out = resolve_references_batch(&targets, "index.md", root);

    assert_eq!(out[0].kind, ReferenceKind::FolderListing, "got {:?}", out[0].kind);
    let followed = out[0]
        .resolved_path
        .as_deref()
        .expect("a known folder resolves a source to follow");
    let canonical = root.canonicalize().expect("canonical root");
    assert_eq!(
        followed,
        canonical.join("獎項/獎項.md").to_string_lossy().as_ref(),
        "must be the CJK source, not <root>/awards"
    );
    assert!(
        std::path::Path::new(followed).is_file(),
        "resolved_path is always a FILE that exists: {followed}"
    );
}

/// The ArticleMap is the LAST build's record. An index source deleted or
/// renamed since is still listed there, and offering to open it would land the
/// editor on a file that is gone — so the card must go back to offering nothing.
#[test]
fn folder_embed_whose_index_source_is_gone_has_nothing_to_follow() {
    let dir = scratch_root();
    let root = dir.path();
    std::fs::create_dir_all(root.join("notes")).expect("mkdir");
    std::fs::write(root.join("notes/a.md"), "# a").expect("write");

    let mut map = ArticleMap::new();
    // Recorded by a build that ran while notes/notes.md still existed.
    map.pages.insert("notes/".into(), "notes/notes.md".into());
    map.articles.insert(
        "notes/a".into(),
        crate::build::scan::article_map::ArticleInfo {
            source_path: "notes/a.md".into(),
            title: "A".into(),
            content: String::new(),
            html_content: None,
            frontmatter: std::collections::HashMap::new(),
            url_path: "notes/a".into(),
            date: None,
            tags: vec![],
            uid: None,
        },
    );
    let moss_dir = root.join(".moss");
    std::fs::create_dir_all(moss_dir.join("build")).expect("mkdir .moss/build");
    map.save(&moss_dir).expect("save article map");

    let targets = vec![RefTarget { text: "/notes/".into(), is_embed: true }];
    let out = resolve_references_batch(&targets, "index.md", root);

    assert_eq!(out[0].kind, ReferenceKind::FolderListing, "got {:?}", out[0].kind);
    assert_eq!(
        out[0].resolved_path, None,
        "a source the map remembers but the disk no longer has is not followable"
    );

    // …and a rename, rather than a deletion, hands over to the new home file
    // instead of vetoing it: the stale record must not outrank what is there.
    std::fs::write(root.join("notes/index.md"), "---\ntitle: Notes\n---\n").expect("write");
    let out = resolve_references_batch(&targets, "index.md", root);
    let canonical = root.canonicalize().expect("canonical root");
    assert_eq!(
        out[0].resolved_path.as_deref(),
        Some(canonical.join("notes/index.md").to_string_lossy().as_ref()),
    );
}

/// `![[Some Note]]` — a markdown transclusion — is followable for exactly the
/// reason a pdf embed is: the classifier resolved it to a source file. It was
/// the one embed kind that carried no path, so it alone could not be opened.
#[test]
fn a_markdown_transclusion_can_be_followed_to_its_source() {
    let dir = scratch_root();
    let root = dir.path();
    std::fs::create_dir_all(root.join("notes")).expect("mkdir");
    std::fs::write(root.join("notes/a.md"), "# a").expect("write");

    let targets = vec![RefTarget { text: "notes/a.md".into(), is_embed: true }];
    let out = resolve_references_batch(&targets, "index.md", root);

    assert_eq!(out[0].kind, ReferenceKind::Transclusion, "got {:?}", out[0].kind);
    let canonical = root.canonicalize().expect("canonical root");
    assert_eq!(
        out[0].resolved_path.as_deref(),
        Some(canonical.join("notes/a.md").to_string_lossy().as_ref()),
    );
}

/// Before the first build there is no ArticleMap, but `EditorFolderIndex`
/// classifies the folder from disk all the same — so the card renders, and it
/// must still know what it opens. The answer comes from the folder's own home
/// file via the build's `is_home_file` predicate.
#[test]
fn folder_embed_follows_its_home_file_when_no_build_has_run() {
    let dir = scratch_root();
    let root = dir.path();
    std::fs::create_dir_all(root.join("notes")).expect("mkdir");
    std::fs::write(root.join("notes/index.md"), "---\ntitle: Notes\n---\n").expect("write");
    std::fs::write(root.join("notes/a.md"), "# a").expect("write");
    // No .moss/ at all: ArticleMap::load falls back to default().

    let targets = vec![RefTarget { text: "/notes/".into(), is_embed: true }];
    let out = resolve_references_batch(&targets, "index.md", root);

    assert_eq!(out[0].kind, ReferenceKind::FolderListing, "got {:?}", out[0].kind);
    let canonical = root.canonicalize().expect("canonical root");
    assert_eq!(
        out[0].resolved_path.as_deref(),
        Some(canonical.join("notes/index.md").to_string_lossy().as_ref()),
        "an unbuilt folder still knows its home file"
    );
}

/// The self-named folder note is a home file too — the shape the slug-override
/// test uses, here with nothing built yet.
#[test]
fn an_unbuilt_folder_finds_its_self_named_note() {
    let dir = scratch_root();
    let root = dir.path();
    std::fs::create_dir_all(root.join("獎項")).expect("mkdir");
    std::fs::write(root.join("獎項/獎項.md"), "---\ntitle: 獎項\n---\n").expect("write");

    let targets = vec![RefTarget { text: "/獎項/".into(), is_embed: true }];
    let out = resolve_references_batch(&targets, "index.md", root);

    let canonical = root.canonicalize().expect("canonical root");
    assert_eq!(
        out[0].resolved_path.as_deref(),
        Some(canonical.join("獎項/獎項.md").to_string_lossy().as_ref()),
    );
}

/// A folder with a static `index.html` renders as an iframe, and follows to
/// that file — the card kind that was left out when the others gained a path.
#[test]
fn a_static_index_folder_follows_to_its_index_html() {
    let dir = scratch_root();
    let root = dir.path();
    std::fs::create_dir_all(root.join("app")).expect("mkdir");
    std::fs::write(root.join("app/index.html"), b"<h1>x</h1>").expect("write");

    let targets = vec![RefTarget { text: "/app/".into(), is_embed: true }];
    let out = resolve_references_batch(&targets, "index.md", root);

    assert_eq!(out[0].kind, ReferenceKind::FolderIndexIframe, "got {:?}", out[0].kind);
    let canonical = root.canonicalize().expect("canonical root");
    assert_eq!(
        out[0].resolved_path.as_deref(),
        Some(canonical.join("app/index.html").to_string_lossy().as_ref()),
    );
}

/// Which home file, when a folder has two? The build's priority order, not
/// the filesystem's: `index.md` publishes and `README.md` does not, so an
/// editor that opened README.md would be pointing at the wrong file.
#[test]
fn an_unbuilt_folder_prefers_index_over_readme() {
    let dir = scratch_root();
    let root = dir.path();
    std::fs::create_dir_all(root.join("notes")).expect("mkdir");
    std::fs::write(root.join("notes/README.md"), "# readme").expect("write");
    std::fs::write(root.join("notes/index.md"), "---\ntitle: Notes\n---\n").expect("write");

    let targets = vec![RefTarget { text: "/notes/".into(), is_embed: true }];
    let out = resolve_references_batch(&targets, "index.md", root);

    let canonical = root.canonicalize().expect("canonical root");
    assert_eq!(
        out[0].resolved_path.as_deref(),
        Some(canonical.join("notes/index.md").to_string_lossy().as_ref()),
    );
}

/// `index.htm` is a static index too — the classifier accepts both spellings,
/// so the file the card opens must be the one the classifier found.
#[test]
fn a_static_index_folder_follows_a_dot_htm_index() {
    let dir = scratch_root();
    let root = dir.path();
    std::fs::create_dir_all(root.join("app")).expect("mkdir");
    std::fs::write(root.join("app/index.htm"), b"<h1>x</h1>").expect("write");

    let targets = vec![RefTarget { text: "/app/".into(), is_embed: true }];
    let out = resolve_references_batch(&targets, "index.md", root);

    assert_eq!(out[0].kind, ReferenceKind::FolderIndexIframe, "got {:?}", out[0].kind);
    let canonical = root.canonicalize().expect("canonical root");
    assert_eq!(
        out[0].resolved_path.as_deref(),
        Some(canonical.join("app/index.htm").to_string_lossy().as_ref()),
    );
}

/// `Index.HTML` is the same page to the classifier and a different NAME to the
/// filesystem. The card must open the file that exists, not a lowercased
/// reconstruction of it.
#[test]
fn a_static_index_folder_follows_the_real_dirent_case() {
    let dir = scratch_root();
    let root = dir.path();
    std::fs::create_dir_all(root.join("app")).expect("mkdir");
    std::fs::write(root.join("app/Index.HTML"), b"<h1>x</h1>").expect("write");

    let targets = vec![RefTarget { text: "/app/".into(), is_embed: true }];
    let out = resolve_references_batch(&targets, "index.md", root);

    assert_eq!(out[0].kind, ReferenceKind::FolderIndexIframe, "got {:?}", out[0].kind);
    let canonical = root.canonicalize().expect("canonical root");
    assert_eq!(
        out[0].resolved_path.as_deref(),
        Some(canonical.join("app/Index.HTML").to_string_lossy().as_ref()),
    );
}

/// A folder whose index page is SYNTHESIZED (articles, but no index source of
/// its own) has nothing to open. `resolved_path` stays None so the editor
/// renders no follow affordance rather than one that lands nowhere.
#[test]
fn folder_embed_with_a_synthesized_index_has_nothing_to_follow() {
    let dir = scratch_root();
    let root = dir.path();
    std::fs::create_dir_all(root.join("notes")).expect("mkdir");
    std::fs::write(root.join("notes/a.md"), "# a").expect("write");

    let mut map = ArticleMap::new();
    // Note: no `pages` entry — the folder index is synthesized by the build.
    map.articles.insert(
        "notes/a".into(),
        crate::build::scan::article_map::ArticleInfo {
            source_path: "notes/a.md".into(),
            title: "A".into(),
            content: String::new(),
            html_content: None,
            frontmatter: std::collections::HashMap::new(),
            url_path: "notes/a".into(),
            date: None,
            tags: vec![],
            uid: None,
        },
    );
    let moss_dir = root.join(".moss");
    std::fs::create_dir_all(moss_dir.join("build")).expect("mkdir .moss/build");
    map.save(&moss_dir).expect("save article map");

    let targets = vec![RefTarget { text: "/notes/".into(), is_embed: true }];
    let out = resolve_references_batch(&targets, "index.md", root);

    assert_eq!(out[0].kind, ReferenceKind::FolderListing, "got {:?}", out[0].kind);
    assert!(
        out[0].folder.is_some(),
        "the card still states the count it knows"
    );
    assert!(
        out[0].resolved_path.is_none(),
        "nothing to open: {:?}",
        out[0].resolved_path
    );
}

/// The same text as an EMBED and as a LINK are separate cache entries with
/// separate kinds, so filling `resolved_path` for folder embeds cannot leak
/// into the link path (or vice versa). Locks the insulation into a test
/// instead of a comment.
#[test]
fn a_folder_as_embed_and_as_link_stay_separate() {
    let dir = scratch_root();
    let root = dir.path();
    std::fs::create_dir_all(root.join("notes")).expect("mkdir");
    std::fs::write(root.join("notes/index.md"), "---\ntitle: Notes\n---\n").expect("write");

    let mut map = ArticleMap::new();
    map.pages.insert("notes/".into(), "notes/index.md".into());
    let moss_dir = root.join(".moss");
    std::fs::create_dir_all(moss_dir.join("build")).expect("mkdir .moss/build");
    map.save(&moss_dir).expect("save article map");

    let targets = vec![
        RefTarget { text: "/notes/".into(), is_embed: true },
        RefTarget { text: "/notes/".into(), is_embed: false },
    ];
    let out = resolve_references_batch(&targets, "index.md", root);

    assert_eq!(out[0].kind, ReferenceKind::FolderListing);
    assert!(out[0].folder.is_some(), "the embed carries folder facts");

    assert!(
        matches!(out[1].kind, ReferenceKind::Link { .. }),
        "the same text as a LINK is never a folder listing, got {:?}",
        out[1].kind
    );
    assert!(out[1].folder.is_none(), "a link carries no folder facts");
}
